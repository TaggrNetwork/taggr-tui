//! App state, background calls, navigation and the event loop.

mod actions;
mod compose;
mod draw;
mod form;
mod images;
mod page;
mod view;

use crate::api::{Api, Config as Backend, PostId, User};
use crate::config::{self, Config};
use crate::ic::Ic;
use crate::identity;
use anyhow::Result;
pub use form::{Field, Form};
pub use page::{Kind, Page, Target};
use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::crossterm::execute;
use ratatui::layout::{Position, Rect};
use std::collections::BTreeMap;
use std::future::Future;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

/// How long a status message replaces the key hints.
const STATUS: Duration = Duration::from_secs(5);
/// How often the signed-in user (inbox, balances) is reloaded.
const RELOAD_USER: Duration = Duration::from_secs(60);

/// A change applied to the app once a background call finishes.
pub type Done = Box<dyn FnOnce(&mut App) + Send>;
/// Something to do when a menu item or confirmation is chosen.
pub type Action = Box<dyn FnOnce(&mut App)>;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Nav,
    Page,
}

/// Sidebar rows.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Nav {
    Header(&'static str),
    Spacer,
    Go(Kind, String),
    SignIn,
    SignOut,
}

pub enum Popup {
    Help,
    Form(Form),
    Menu(Menu),
    Confirm(String, Option<Action>),
    /// The edited post, waiting for y post / e edit / d discard.
    Send(compose::Draft, u16),
    /// Text to read (version of a post, invoice details…), scrollable.
    Text(String, Vec<ratatui::text::Line<'static>>, u16),
}

pub struct Menu {
    pub title: String,
    pub items: Vec<(String, Option<Action>)>,
    pub sel: usize,
}

impl Menu {
    pub fn new(title: impl Into<String>) -> Menu {
        Menu {
            title: title.into(),
            items: Vec::new(),
            sel: 0,
        }
    }
    pub fn add(mut self, label: impl Into<String>, f: impl FnOnce(&mut App) + 'static) -> Menu {
        self.items.push((label.into(), Some(Box::new(f))));
        self
    }
}

#[derive(Default)]
pub struct Hits {
    pub nav: Rect,
    pub page: Rect,
    /// First line index of each visible entry and its screen row, for clicks.
    pub entries: Vec<(usize, u16, u16)>,
    /// Screen row and first column of the tab bar.
    pub tabs: Option<(u16, u16)>,
}

pub struct App {
    pub cfg: Config,
    pub api: Arc<Api>,
    rt: tokio::runtime::Runtime,
    tx: Sender<Done>,
    rx: Receiver<Done>,
    /// Calls in flight (spinner).
    pub busy: usize,
    pub backend: Backend,
    /// Signed-in principal (empty: anonymous).
    pub principal: String,
    /// Signed-in user; None while anonymous or not registered.
    pub user: Option<User>,
    pub users: BTreeMap<u64, String>,
    pub nav: Vec<Nav>,
    pub nav_sel: usize,
    pub nav_off: usize,
    /// Page stack; the last one is shown.
    pub pages: Vec<Page>,
    pub focus: Focus,
    pub popup: Option<Popup>,
    pub status: Option<(String, bool, Instant)>,
    pub hits: Hits,
    pub images: images::Images,
    /// Posts whose pictures are shown.
    pub expanded: std::collections::HashSet<PostId>,
    last_user_load: Instant,
    /// The signed-in user was loaded at least once.
    user_loaded: bool,
    /// A post to compose in $EDITOR on the next loop turn.
    edit: Option<compose::Draft>,
    /// A long form field to edit in $EDITOR on the next loop turn.
    long_edit: Option<form::LongEdit>,
    /// The backend config as sent, for the whitepaper placeholders.
    pub backend_raw: serde_json::Value,
    quit: bool,
}

impl App {
    fn new(cfg: Config, api: Api, principal: String, images: images::Images) -> App {
        let (tx, rx) = channel();
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("tokio runtime");
        let mut app = App {
            cfg,
            api: Arc::new(api),
            rt,
            tx,
            rx,
            busy: 0,
            backend: Backend::default(),
            principal,
            user: None,
            users: BTreeMap::new(),
            nav: Vec::new(),
            nav_sel: 0,
            nav_off: 0,
            pages: Vec::new(),
            focus: Focus::Page,
            popup: None,
            status: None,
            hits: Hits::default(),
            images,
            expanded: Default::default(),
            last_user_load: Instant::now(),
            user_loaded: false,
            edit: None,
            long_edit: None,
            backend_raw: serde_json::Value::Null,
            quit: false,
        };
        app.build_nav();
        app
    }

    pub fn info(&mut self, s: impl Into<String>) {
        self.status = Some((s.into(), false, Instant::now()));
    }

    pub fn error(&mut self, s: impl std::fmt::Display) {
        self.status = Some((s.to_string(), true, Instant::now()));
    }

    /// Requires a registered user; says why not otherwise.
    pub fn need_user(&mut self) -> bool {
        if self.user.is_some() {
            return true;
        }
        if self.principal.is_empty() {
            self.error("sign in first (sidebar: Sign in)");
        } else {
            self.error("this principal has no account yet (sidebar: Welcome)");
        }
        false
    }

    pub fn me(&self) -> u64 {
        self.user.as_ref().map(|u| u.id).unwrap_or(u64::MAX)
    }

    /// Runs `fut` in the background; `then` gets its value on the UI thread, errors go to the status line.
    pub fn bg<T, F>(
        &mut self,
        fut: impl FnOnce(Arc<Api>) -> F,
        then: impl FnOnce(&mut App, T) + Send + 'static,
    ) where
        T: Send + 'static,
        F: Future<Output = Result<T>> + Send + 'static,
    {
        self.busy += 1;
        let tx = self.tx.clone();
        let f = fut(self.api.clone());
        self.rt.spawn(async move {
            let r = f.await;
            let done: Done = Box::new(move |app: &mut App| {
                app.busy = app.busy.saturating_sub(1);
                match r {
                    Ok(v) => then(app, v),
                    Err(e) => {
                        // a failed load leaves no page spinning
                        for p in &mut app.pages {
                            if p.loading {
                                p.loading = false;
                                p.data.loaded = true;
                                p.dirty();
                            }
                        }
                        app.error(short_error(&format!("{e:#}")));
                    }
                }
            });
            let _ = tx.send(done);
        });
    }

    /// Fire an update, report `ok` on success and reload the page and user.
    pub fn act<F>(&mut self, fut: impl FnOnce(Arc<Api>) -> F, ok: impl Into<String>)
    where
        F: Future<Output = Result<serde_json::Value>> + Send + 'static,
    {
        let ok = ok.into();
        self.info("working…");
        self.bg(fut, move |app, _| {
            app.info(ok);
            app.load_user();
            app.reload();
        });
    }

    fn start(&mut self) {
        self.bg(
            |api| async move { api.config_raw().await },
            |app, c: serde_json::Value| {
                app.backend = serde_json::from_value(c.clone()).unwrap_or_default();
                app.backend_raw = c;
                for p in &mut app.pages {
                    p.dirty();
                }
            },
        );
        if !self.principal.is_empty() {
            self.load_user();
        }
        self.open_root(Kind::New, "New");
        if let Some(i) = self
            .nav
            .iter()
            .position(|n| matches!(n, Nav::Go(Kind::New, _)))
        {
            self.nav_sel = i;
        }
    }

    pub fn load_user(&mut self) {
        if self.principal.is_empty() {
            return;
        }
        self.last_user_load = Instant::now();
        self.bg(
            |api| async move { api.user(None).await },
            |app, u: Option<User>| {
                let first = app.user.is_none() && u.is_some();
                if let Some(u) = &u {
                    app.users.insert(u.id, u.name.clone());
                }
                let unregistered = u.is_none() && !app.user_loaded;
                app.user_loaded = true;
                app.user = u;
                app.build_nav();
                if first {
                    app.reload();
                } else if unregistered && app.pages.len() == 1 {
                    // a stored key without an account: start where it can register
                    app.open_root(Kind::Welcome, "Welcome");
                }
            },
        );
    }

    /// Ensures user names for `ids` are known (filled in when they arrive).
    pub fn want_names(&mut self, ids: Vec<u64>) {
        let ids: Vec<u64> = ids
            .into_iter()
            .filter(|i| !self.users.contains_key(i))
            .collect();
        if ids.is_empty() {
            return;
        }
        for i in &ids {
            self.users.insert(*i, String::new());
        }
        self.bg(
            move |api| async move { api.users_data(&ids).await },
            |app, m| {
                app.users.extend(m);
                for p in &mut app.pages {
                    p.dirty();
                }
            },
        );
    }

    pub fn name(&self, id: u64) -> String {
        match self.users.get(&id) {
            Some(n) if !n.is_empty() => n.clone(),
            _ => format!("#{id}"),
        }
    }

    // ---- sidebar --------------------------------------------------------------

    pub fn build_nav(&mut self) {
        let cur = self.nav.get(self.nav_sel).cloned();
        let mut n = vec![Nav::Header("Feeds")];
        if self.user.is_some() {
            n.push(Nav::Go(Kind::Home, "Home".into()));
        }
        n.push(Nav::Go(Kind::New, "New".into()));
        n.push(Nav::Go(Kind::Hot, "Hot".into()));
        if let Some(u) = &self.user {
            n.push(Nav::Go(Kind::Bookmarks, "Bookmarks".into()));
            for f in &u.feeds {
                n.push(Nav::Go(
                    Kind::Tags(f.clone()),
                    f.iter()
                        .map(|t| format!("#{t}"))
                        .collect::<Vec<_>>()
                        .join(" "),
                ));
            }
        }
        n.push(Nav::Spacer);
        n.push(Nav::Header("Realms"));
        if let Some(u) = &self.user {
            let mut r = u.realms.clone();
            r.sort();
            for id in r {
                n.push(Nav::Go(Kind::Realm(id.clone()), format!("/{id}")));
            }
        }
        n.push(Nav::Go(Kind::Realms, "All realms".into()));
        n.push(Nav::Spacer);
        match &self.user {
            Some(u) => {
                n.push(Nav::Header("Me"));
                let unread = u.notifications.values().filter(|(_, read)| !read).count();
                let inbox = if unread > 0 {
                    format!("Inbox ({unread})")
                } else {
                    "Inbox".into()
                };
                n.push(Nav::Go(Kind::Inbox, inbox));
                n.push(Nav::Go(Kind::User(u.name.clone()), "Profile".into()));
                n.push(Nav::Go(Kind::Journal(u.name.clone()), "Journal".into()));
                n.push(Nav::Go(Kind::Wallet, "Wallet".into()));
                n.push(Nav::Go(Kind::Invites, "Invites".into()));
                n.push(Nav::Go(Kind::Settings, "Settings".into()));
                n.push(Nav::Spacer);
            }
            None if !self.principal.is_empty() => {
                n.push(Nav::Header("Me"));
                n.push(Nav::Go(Kind::Welcome, "Welcome".into()));
                n.push(Nav::Spacer);
            }
            None => {}
        }
        n.push(Nav::Header("Taggr"));
        for (k, l) in [
            (Kind::Search(String::new()), "Search"),
            (Kind::Proposals, "Proposals"),
            (Kind::Tokens, "Tokens"),
            (Kind::Dashboard, "Dashboard"),
            (Kind::Distribution, "Distribution"),
            (Kind::Domains, "Domains"),
            (Kind::Links, "Links"),
            (Kind::Whitepaper, "Whitepaper"),
        ] {
            n.push(Nav::Go(k, l.into()));
        }
        n.push(Nav::Spacer);
        n.push(if self.principal.is_empty() {
            Nav::SignIn
        } else {
            Nav::SignOut
        });
        self.nav = n;
        // keep the selection on the same row
        self.nav_sel = cur
            .and_then(|c| {
                self.nav.iter().position(|x| match (x, &c) {
                    (Nav::Go(a, _), Nav::Go(b, _)) => a == b,
                    _ => *x == c,
                })
            })
            .unwrap_or(self.nav_sel)
            .min(self.nav.len().saturating_sub(1));
        if !self.nav_selectable(self.nav_sel) {
            self.move_nav(1);
        }
    }

    fn nav_selectable(&self, i: usize) -> bool {
        !matches!(self.nav.get(i), Some(Nav::Header(_) | Nav::Spacer) | None)
    }

    fn move_nav(&mut self, d: isize) {
        let mut i = self.nav_sel as isize;
        loop {
            i += d.signum();
            if i < 0 || i as usize >= self.nav.len() {
                return;
            }
            if self.nav_selectable(i as usize) {
                self.nav_sel = i as usize;
                return;
            }
        }
    }

    fn nav_go(&mut self) {
        match self.nav.get(self.nav_sel).cloned() {
            Some(Nav::Go(k, label)) => {
                let label = label.split(" (").next().unwrap_or(&label).to_string();
                self.open_root(k, &label);
                self.focus = Focus::Page;
            }
            Some(Nav::SignIn) => self.sign_in_form(),
            Some(Nav::SignOut) => {
                self.popup = Some(Popup::Confirm(
                    "Sign out? The stored key is deleted; you need the seed phrase to sign in again.".into(),
                    Some(Box::new(|app: &mut App| app.sign_out())),
                ));
            }
            _ => {}
        }
    }

    // ---- pages ---------------------------------------------------------------

    pub fn page(&mut self) -> Option<&mut Page> {
        self.pages.last_mut()
    }

    pub fn open_root(&mut self, kind: Kind, title: &str) {
        self.pages.clear();
        self.focus = Focus::Page;
        self.push(kind, title);
    }

    pub fn push(&mut self, kind: Kind, title: &str) {
        if let Kind::Search(q) = &kind
            && q.is_empty()
        {
            self.pages.push(Page::new(kind.clone(), title));
            self.search_prompt();
            return;
        }
        self.pages.push(Page::new(kind, title));
        self.load(false);
    }

    pub fn back(&mut self) {
        if self.pages.len() > 1 {
            self.pages.pop();
            if let Some(p) = self.pages.last_mut() {
                p.dirty();
            }
        } else {
            self.focus = Focus::Nav;
        }
    }

    /// Reloads the shown page, keeping its selection.
    pub fn reload(&mut self) {
        self.load(false);
    }

    pub fn open_target(&mut self, t: Target) {
        match t {
            Target::Post(id) => self.push(Kind::Post(id), &format!("Post #{id}")),
            Target::User(name) => self.push(Kind::User(name.clone()), &format!("@{name}")),
            Target::UserId(id) => {
                let n = self.name(id);
                let n = if n.starts_with('#') {
                    id.to_string()
                } else {
                    n
                };
                self.push(Kind::User(n.clone()), &format!("@{n}"))
            }
            Target::Realm(r) => self.push(Kind::Realm(r.clone()), &format!("/{r}")),
            Target::Tag(t) => self.push(Kind::Tags(vec![t.clone()]), &format!("#{t}")),
            Target::Page(k, title) => self.push(k, &title),
            Target::Run(f) => f(self),
            _ => {}
        }
    }

    pub fn open_url(&mut self, u: &str) {
        let r = std::process::Command::new("xdg-open")
            .arg(u)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        match r {
            Ok(_) => self.info(format!("opened {u}")),
            Err(e) => self.error(format!("xdg-open: {e}")),
        }
    }

    pub fn web_url(&self, route: &str) -> String {
        let host = if self.cfg.mainnet() {
            if self.cfg.domain.contains('.') {
                self.cfg.domain.clone()
            } else {
                format!("{}.icp0.io", self.cfg.canister)
            }
        } else {
            format!(
                "{}.{}",
                self.cfg.canister,
                self.cfg.local_host().unwrap_or_default()
            )
        };
        let scheme = if self.cfg.mainnet() { "https" } else { "http" };
        format!("{scheme}://{host}/#/{route}")
    }

    // ---- sign in -----------------------------------------------------------------

    fn sign_in_form(&mut self) {
        let f = Form::new("Sign in with seed phrase", "sign in")
            .note("The seed phrase is the one used on the web (Seed Phrase sign-in). A new phrase creates a new principal; register it under Welcome. Only the derived key is stored, in a 0600 file.")
            .field(Field::secret("Seed phrase", ""))
            .on_submit(|app, v| {
                let phrase = v[0].clone();
                if phrase.is_empty() {
                    return Err("enter a seed phrase".into());
                }
                app.sign_in(&phrase);
                Ok(())
            });
        self.popup = Some(Popup::Form(f));
    }

    fn sign_in(&mut self, phrase: &str) {
        self.info("deriving key…");
        let seed = identity::derive_seed(phrase);
        let cfg = self.cfg.clone();
        let tx = self.tx.clone();
        self.busy += 1;
        self.rt.spawn(async move {
            let id = identity::identity_from_seed(&seed);
            let principal = identity::principal_of(&id);
            let api = Ic::new(&cfg.url, &cfg.canister, Some(id)).await;
            let done: Done = Box::new(move |app: &mut App| {
                app.busy -= 1;
                match api {
                    Ok(ic) => {
                        if let Err(e) = identity::write_seed(&config::identity_file(), &seed) {
                            app.error(format!("signed in, but the key could not be stored: {e}"));
                        } else {
                            app.info(format!("signed in as {principal}"));
                        }
                        app.api = Arc::new(Api {
                            ic,
                            domain: app.cfg.domain.clone(),
                            local: app.cfg.local_host(),
                        });
                        app.principal = principal;
                        app.user = None;
                        app.build_nav();
                        app.bg(
                            |api| async move { api.user(None).await },
                            |app, u: Option<User>| {
                                let registered = u.is_some();
                                app.user = u;
                                app.build_nav();
                                if registered {
                                    app.open_root(Kind::Home, "Home");
                                } else {
                                    app.open_root(Kind::Welcome, "Welcome");
                                }
                            },
                        );
                    }
                    Err(e) => app.error(e),
                }
            });
            let _ = tx.send(done);
        });
    }

    fn sign_out(&mut self) {
        let _ = std::fs::remove_file(config::identity_file());
        let cfg = self.cfg.clone();
        let tx = self.tx.clone();
        self.rt.spawn(async move {
            let ic = Ic::new(&cfg.url, &cfg.canister, None).await;
            let done: Done = Box::new(move |app: &mut App| match ic {
                Ok(ic) => {
                    app.api = Arc::new(Api {
                        ic,
                        domain: app.cfg.domain.clone(),
                        local: app.cfg.local_host(),
                    });
                    app.principal.clear();
                    app.user = None;
                    app.build_nav();
                    app.open_root(Kind::New, "New");
                    app.info("signed out");
                }
                Err(e) => app.error(e),
            });
            let _ = tx.send(done);
        });
    }

    // ---- loop ------------------------------------------------------------------

    fn tick(&mut self) {
        while let Ok(done) = self.rx.try_recv() {
            done(self);
        }
        if let Some((_, _, t)) = &self.status
            && t.elapsed() > STATUS
            && self.busy == 0
        {
            self.status = None;
        }
        for url in std::mem::take(&mut self.images.queue) {
            let tx = self.tx.clone();
            self.rt.spawn(async move {
                let img = images::fetch(url.clone()).await;
                let done: Done = Box::new(move |app: &mut App| {
                    app.images.insert(url, img);
                    for p in &mut app.pages {
                        p.dirty();
                    }
                });
                let _ = tx.send(done);
            });
        }
        if self.user.is_some() && self.last_user_load.elapsed() > RELOAD_USER {
            self.load_user();
        }
    }

    fn on_key(&mut self, k: KeyEvent) {
        if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }
        if let Some(p) = self.popup.take() {
            self.popup_key(p, k);
            return;
        }
        if self
            .status
            .as_ref()
            .is_some_and(|s| !s.1 || s.2.elapsed() > Duration::from_millis(300))
            && self.busy == 0
        {
            self.status = None;
        }
        match k.code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('?') => self.popup = Some(Popup::Help),
            KeyCode::Tab | KeyCode::BackTab => {
                self.focus = if self.focus == Focus::Nav {
                    Focus::Page
                } else {
                    Focus::Nav
                };
            }
            _ if self.focus == Focus::Nav => self.nav_key(k),
            _ => self.page_key(k),
        }
    }

    fn nav_key(&mut self, k: KeyEvent) {
        match k.code {
            KeyCode::Char('j') | KeyCode::Down => self.move_nav(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_nav(-1),
            KeyCode::Char('g') | KeyCode::Home => {
                self.nav_sel = 0;
                self.move_nav(1);
            }
            KeyCode::Char('G') | KeyCode::End => {
                self.nav_sel = self.nav.len();
                self.move_nav(-1);
            }
            KeyCode::Enter => self.nav_go(),
            KeyCode::Char('l') | KeyCode::Right => self.focus = Focus::Page,
            KeyCode::Esc | KeyCode::Char('h') | KeyCode::Left => self.focus = Focus::Page,
            _ => self.global_key(k),
        }
    }

    fn on_mouse(&mut self, m: MouseEvent) {
        let pos = Position::new(m.column, m.row);
        match m.kind {
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                let d: isize = if m.kind == MouseEventKind::ScrollDown {
                    3
                } else {
                    -3
                };
                if self.hits.nav.contains(pos) {
                    self.move_nav(d.signum());
                } else if let Some(p) = self.page() {
                    p.scroll(d);
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if self.popup.is_some() {
                    return;
                }
                if self.hits.nav.contains(pos) {
                    let i = self.nav_off + (m.row - self.hits.nav.y) as usize;
                    if self.nav_selectable(i) {
                        let again = self.nav_sel == i && self.focus == Focus::Nav;
                        self.nav_sel = i;
                        self.focus = Focus::Nav;
                        if again || true {
                            self.nav_go();
                        }
                    }
                } else if self.hits.page.contains(pos) {
                    self.focus = Focus::Page;
                    if let Some((row, x0)) = self.hits.tabs
                        && m.row == row
                    {
                        let col = m.column.saturating_sub(x0) as usize;
                        let hit = self
                            .pages
                            .last()
                            .and_then(|p| p.tab_row.as_ref())
                            .and_then(|t| t.1.iter().position(|(a, b)| col >= *a && col < *b));
                        if let Some(i) = hit
                            && let Some(p) = self.page()
                        {
                            p.tab = i;
                            self.tab_changed();
                        }
                        return;
                    }
                    let hit = self
                        .hits
                        .entries
                        .iter()
                        .find(|(_, top, h)| m.row >= *top && m.row < top + h)
                        .map(|e| e.0);
                    if let Some(i) = hit {
                        let open = self.page().map(|p| p.sel == i).unwrap_or(false);
                        if let Some(p) = self.page() {
                            p.select(i);
                        }
                        if open {
                            self.enter();
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn popup_key(&mut self, p: Popup, k: KeyEvent) {
        match p {
            Popup::Help => {}
            Popup::Confirm(text, action) => match k.code {
                KeyCode::Char('y') | KeyCode::Enter => {
                    if let Some(a) = action {
                        a(self);
                    }
                }
                KeyCode::Char('n') | KeyCode::Esc | KeyCode::Char('q') => {}
                _ => self.popup = Some(Popup::Confirm(text, action)),
            },
            Popup::Menu(mut m) => match k.code {
                KeyCode::Esc | KeyCode::Char('q') => {}
                KeyCode::Char('j') | KeyCode::Down => {
                    m.sel = (m.sel + 1).min(m.items.len().saturating_sub(1));
                    self.popup = Some(Popup::Menu(m));
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    m.sel = m.sel.saturating_sub(1);
                    self.popup = Some(Popup::Menu(m));
                }
                KeyCode::Enter | KeyCode::Char('l') => {
                    if let Some(a) = m.items.get_mut(m.sel).and_then(|i| i.1.take()) {
                        a(self);
                    }
                }
                KeyCode::Char(c) if c.is_ascii_digit() => {
                    let i = if c == '0' {
                        9
                    } else {
                        c as usize - '1' as usize
                    };
                    if let Some(a) = m.items.get_mut(i).and_then(|i| i.1.take()) {
                        a(self);
                    } else {
                        self.popup = Some(Popup::Menu(m));
                    }
                }
                _ => self.popup = Some(Popup::Menu(m)),
            },
            Popup::Form(f) => form::key(self, f, k),
            Popup::Send(d, scroll) => compose::send_key(self, d, scroll, k),
            Popup::Text(title, lines, scroll) => match k.code {
                KeyCode::Esc | KeyCode::Char('q') | KeyCode::Enter => {}
                KeyCode::Char('j') | KeyCode::Down => {
                    self.popup = Some(Popup::Text(title, lines, scroll + 1))
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.popup = Some(Popup::Text(title, lines, scroll.saturating_sub(1)))
                }
                KeyCode::PageDown | KeyCode::Char(' ') => {
                    self.popup = Some(Popup::Text(title, lines, scroll + 15))
                }
                KeyCode::PageUp => {
                    self.popup = Some(Popup::Text(title, lines, scroll.saturating_sub(15)))
                }
                _ => self.popup = Some(Popup::Text(title, lines, scroll)),
            },
        }
    }

    pub fn confirm(&mut self, text: impl Into<String>, f: impl FnOnce(&mut App) + 'static) {
        self.popup = Some(Popup::Confirm(text.into(), Some(Box::new(f))));
    }

    pub fn text_popup(&mut self, title: impl Into<String>, md: &str) {
        let r = crate::markdown::render(md, 76);
        let lines = r
            .blocks
            .into_iter()
            .map(|b| match b {
                crate::markdown::Block::Line(l) => l,
                crate::markdown::Block::Image(_, alt) => {
                    ratatui::text::Line::raw(format!("[picture {alt}]"))
                }
            })
            .collect();
        self.popup = Some(Popup::Text(title.into(), lines, 0));
    }

    pub fn sel_post_id(&self) -> Option<PostId> {
        match self.pages.last()?.selected_target()? {
            Target::Post(id) => Some(*id),
            _ => None,
        }
    }
}

/// IC rejections are long; keep the message the canister gave.
fn short_error(e: &str) -> String {
    let e = match e.find("reject message ") {
        Some(i) => &e[i + 15..],
        None => e,
    };
    let e = match e.find(": ") {
        Some(i) if e.starts_with("Error from Canister") => &e[i + 2..],
        _ => e,
    };
    e.replace('\n', " ")
}

fn run_editor(terminal: &mut ratatui::DefaultTerminal, path: &std::path::Path) -> Result<()> {
    let _ = execute!(std::io::stdout(), DisableMouseCapture);
    ratatui::restore();
    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "vi".into());
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("{editor} \"$1\""))
        .arg("sh")
        .arg(path)
        .status();
    *terminal = ratatui::init();
    let _ = execute!(std::io::stdout(), EnableMouseCapture);
    terminal.clear()?;
    let st = status?;
    if !st.success() {
        anyhow::bail!("editor exited with {st}");
    }
    Ok(())
}

pub fn run(cfg: Config) -> Result<()> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let id_file = config::identity_file();
    let (identity, principal) = match identity::read_seed(&id_file) {
        Ok(seed) => {
            let id = identity::identity_from_seed(&seed);
            let p = identity::principal_of(&id);
            (Some(id), p)
        }
        Err(_) => (None, String::new()),
    };
    let ic = rt.block_on(Ic::new(&cfg.url, &cfg.canister, identity))?;
    drop(rt);
    let api = Api {
        ic,
        domain: cfg.domain.clone(),
        local: cfg.local_host(),
    };

    let mut terminal = ratatui::init();
    execute!(std::io::stdout(), EnableMouseCapture)?;
    let imgs = images::Images::new(&cfg);
    let mut app = App::new(cfg, api, principal, imgs);
    app.start();
    let res = (|| -> Result<()> {
        while !app.quit {
            terminal.draw(|f| draw::draw(f, &mut app))?;
            if event::poll(Duration::from_millis(100))? {
                match event::read()? {
                    Event::Key(k) if k.kind == KeyEventKind::Press => app.on_key(k),
                    Event::Mouse(m) => app.on_mouse(m),
                    _ => {}
                }
            }
            if let Some(mut le) = app.long_edit.take() {
                match run_editor(&mut terminal, &le.file) {
                    Ok(()) => {
                        let v = std::fs::read_to_string(&le.file).unwrap_or_default();
                        le.form.fields[le.field].value = v.trim_end().to_string();
                    }
                    Err(e) => le.form.error = Some(format!("editor: {e}")),
                }
                let _ = std::fs::remove_file(&le.file);
                app.popup = Some(Popup::Form(le.form));
            }
            if let Some(d) = app.edit.take() {
                match run_editor(&mut terminal, &d.file) {
                    Ok(()) => compose::edited(&mut app, d),
                    Err(e) => app.error(format!("editor: {e}")),
                }
            }
            app.tick();
        }
        Ok(())
    })();
    let _ = execute!(std::io::stdout(), DisableMouseCapture);
    ratatui::restore();
    res
}
