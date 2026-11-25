use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use image::DynamicImage;
use tokio::sync::mpsc::UnboundedSender;

use crate::api::models::{PostItem, SearchResult, User};
use crate::api::{Client, now_ns};
use crate::config::{Config, Paths};
use crate::identity;
use crate::media;

use super::images::ImageStore;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FeedTab {
    Latest,
    Hot,
    Personal,
}

impl FeedTab {
    pub fn title(self) -> &'static str {
        match self {
            FeedTab::Latest => "latest",
            FeedTab::Hot => "hot",
            FeedTab::Personal => "personal",
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum Screen {
    Login,
    Timeline,
    Reader,
    Profile,
    Search,
}

pub enum ComposeKind {
    New,
    Reply(u64),
    Repost(u64),
    Poll,
}

pub struct Compose {
    pub kind: ComposeKind,
    pub body: String,
    pub options: String,
    pub deadline: u64,
    pub focus_options: bool,
}

impl Compose {
    pub fn kind_is_poll(&self) -> bool {
        matches!(self.kind, ComposeKind::Poll)
    }
}

pub enum PromptAction {
    Search,
    Report(u64),
    Tip(u64),
    VotePoll(u64),
}

pub struct Prompt {
    pub label: String,
    pub value: String,
    pub action: PromptAction,
}

pub struct Reader {
    pub posts: Vec<PostItem>,
    pub sel: usize,
    pub scroll: u16,
}

pub struct ProfileState {
    pub user: User,
    pub posts: Vec<PostItem>,
    pub selected: usize,
}

pub struct SearchState {
    pub query: String,
    pub results: Vec<SearchResult>,
    pub selected: usize,
}

pub enum Msg {
    Key(KeyEvent),
    Resize,
    Tick,
    Feed {
        req_id: u64,
        items: Vec<PostItem>,
        append: bool,
    },
    Thread {
        req_id: u64,
        items: Vec<PostItem>,
    },
    User(Option<User>),
    Profile {
        user: User,
        posts: Vec<PostItem>,
    },
    Search(Vec<SearchResult>),
    ImageReady {
        key: String,
        image: DynamicImage,
    },
    ImageFailed(String),
    Reload,
    Toast(String),
    Error(String),
}

pub struct App {
    pub client: Arc<Client>,
    pub http: reqwest::Client,
    pub paths: Paths,
    pub config: Config,
    pub tx: UnboundedSender<Msg>,

    pub logged_in: bool,
    pub me: Option<User>,
    pub principal: String,

    pub screen: Screen,
    pub tab: FeedTab,
    pub posts: Vec<PostItem>,
    pub selected: usize,
    pub page: usize,
    pub offset: u64,
    pub loading: bool,
    pub end: bool,
    pub req_id: u64,

    pub reader: Option<Reader>,
    pub profile: Option<ProfileState>,
    pub search: Option<SearchState>,
    pub compose: Option<Compose>,
    pub prompt: Option<Prompt>,
    pub help: bool,

    pub input: String,
    pub toast: Option<String>,
    pub toast_at: Option<std::time::Instant>,
    pub now_ns: u64,
    pub images: ImageStore,
    pub quit: bool,
}

impl App {
    pub fn new(
        client: Arc<Client>,
        paths: Paths,
        config: Config,
        tx: UnboundedSender<Msg>,
        images: ImageStore,
        logged_in: bool,
        principal: String,
    ) -> Self {
        let http = reqwest::Client::builder()
            .user_agent("taggr-tui")
            .build()
            .unwrap_or_default();
        let screen = Screen::Timeline;
        Self {
            client,
            http,
            paths,
            config,
            tx,
            logged_in,
            me: None,
            principal,
            screen,
            tab: FeedTab::Latest,
            posts: Vec::new(),
            selected: 0,
            page: 0,
            offset: 0,
            loading: false,
            end: false,
            req_id: 0,
            reader: None,
            profile: None,
            search: None,
            compose: None,
            prompt: None,
            help: false,
            input: String::new(),
            toast: None,
            toast_at: None,
            now_ns: now_ns(),
            images,
            quit: false,
        }
    }

    pub fn start(&mut self) {
        self.load_feed(false);
        if self.logged_in {
            self.load_me();
        }
    }

    fn require_login(&mut self) -> bool {
        if self.logged_in {
            true
        } else {
            self.toast("sign in required — press L");
            false
        }
    }

    fn spawn<F>(&self, fut: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        tokio::spawn(fut);
    }

    fn toast(&mut self, msg: impl Into<String>) {
        self.toast = Some(msg.into());
        self.toast_at = Some(std::time::Instant::now());
    }

    // -------------------- loading --------------------

    pub fn load_me(&mut self) {
        let client = self.client.clone();
        let tx = self.tx.clone();
        self.spawn(async move {
            match client.user(None).await {
                Ok(user) => {
                    let _ = tx.send(Msg::User(user));
                }
                Err(e) => {
                    let _ = tx.send(Msg::Error(format!("loading user: {e}")));
                }
            }
        });
    }

    pub fn load_feed(&mut self, append: bool) {
        self.req_id += 1;
        let req_id = self.req_id;
        self.loading = true;
        if !append {
            self.page = 0;
            self.offset = 0;
            self.end = false;
        }
        let page = self.page;
        let offset = self.offset;
        let tab = self.tab;
        let client = self.client.clone();
        let tx = self.tx.clone();
        self.spawn(async move {
            let result = match tab {
                FeedTab::Latest => client.latest_posts("", page, offset).await,
                FeedTab::Hot => client.hot_posts("", page, offset).await,
                FeedTab::Personal => client.personal_feed(page, offset).await,
            };
            match result {
                Ok(items) => {
                    let _ = tx.send(Msg::Feed {
                        req_id,
                        items,
                        append,
                    });
                }
                Err(e) => {
                    let _ = tx.send(Msg::Error(format!("loading feed: {e}")));
                }
            }
        });
    }

    fn load_thread(&mut self, id: u64) {
        self.req_id += 1;
        let req_id = self.req_id;
        let client = self.client.clone();
        let tx = self.tx.clone();
        self.spawn(async move {
            match client.thread(id).await {
                Ok(items) => {
                    let _ = tx.send(Msg::Thread { req_id, items });
                }
                Err(e) => {
                    let _ = tx.send(Msg::Error(format!("loading thread: {e}")));
                }
            }
        });
    }

    fn load_profile(&mut self, handle: String) {
        let client = self.client.clone();
        let tx = self.tx.clone();
        self.spawn(async move {
            let user = match client.user(Some(&handle)).await {
                Ok(u) => u,
                Err(e) => {
                    let _ = tx.send(Msg::Error(format!("loading profile: {e}")));
                    return;
                }
            };
            let Some(user) = user else {
                let _ = tx.send(Msg::Error("user not found".into()));
                return;
            };
            let posts = client.user_posts(&handle, 0, 0).await.unwrap_or_default();
            let _ = tx.send(Msg::Profile { user, posts });
        });
    }

    fn run_search(&mut self, query: String) {
        let client = self.client.clone();
        let tx = self.tx.clone();
        self.spawn(async move {
            match client.search(&query).await {
                Ok(results) => {
                    let _ = tx.send(Msg::Search(results));
                }
                Err(e) => {
                    let _ = tx.send(Msg::Error(format!("search: {e}")));
                }
            }
        });
    }

    pub fn request_image(&mut self, key: String, bucket: String, offset: u64, len: u64) {
        if !self.images.enabled()
            || self.images.has(&key)
            || self.images.is_pending(&key)
            || self.images.is_failed(&key)
        {
            return;
        }
        self.images.mark_pending(&key);
        let http = self.http.clone();
        let client = self.client.clone();
        let cache_dir = self.paths.image_cache();
        let tx = self.tx.clone();
        self.spawn(async move {
            let r = media::ImageRef {
                blob_id: String::new(),
                bucket,
                offset,
                len,
            };
            match media::load_or_fetch(&http, &client, &cache_dir, &r).await {
                Ok(image) => {
                    let _ = tx.send(Msg::ImageReady { key, image });
                }
                Err(_) => {
                    let _ = tx.send(Msg::ImageFailed(key));
                }
            }
        });
    }

    // -------------------- event handling --------------------

    pub fn handle(&mut self, msg: Msg) {
        match msg {
            Msg::Tick => {
                self.now_ns = now_ns();
                if let Some(at) = self.toast_at
                    && at.elapsed().as_secs() >= 4
                {
                    self.toast = None;
                    self.toast_at = None;
                }
            }
            Msg::Resize => {}
            Msg::Key(key) => self.handle_key(key),
            Msg::Feed {
                req_id,
                items,
                append,
            } => {
                if req_id != self.req_id {
                    return;
                }
                self.loading = false;
                if items.len() < crate::config::FEED_PAGE_SIZE {
                    self.end = true;
                }
                if append {
                    self.posts.extend(items);
                } else {
                    self.posts = items;
                    self.selected = 0;
                }
                if let Some(first) = self.posts.first()
                    && !append
                {
                    self.offset = first.id();
                }
            }
            Msg::Thread { req_id, items } => {
                if req_id != self.req_id {
                    return;
                }
                let sel = 0;
                self.reader = Some(Reader {
                    posts: items,
                    sel,
                    scroll: 0,
                });
                self.screen = Screen::Reader;
            }
            Msg::User(user) => {
                let was_logged_in = self.logged_in;
                if let Some(u) = &user {
                    self.principal = u.principal.clone();
                }
                self.me = user;
                if self.me.is_some() {
                    self.logged_in = true;
                    self.screen = Screen::Timeline;
                    if !was_logged_in {
                        self.load_feed(false);
                    }
                } else {
                    self.logged_in = false;
                    if was_logged_in {
                        // Stored identity has no account: fall back to anonymous browsing.
                        self.toast("stored identity has no account — browsing anonymously");
                    } else {
                        self.toast("no account found for this identity");
                    }
                }
            }
            Msg::Profile { user, posts } => {
                self.profile = Some(ProfileState {
                    user,
                    posts,
                    selected: 0,
                });
                self.screen = Screen::Profile;
            }
            Msg::Search(results) => {
                self.search = Some(SearchState {
                    query: self.input.clone(),
                    results,
                    selected: 0,
                });
                self.screen = Screen::Search;
            }
            Msg::ImageReady { key, image } => {
                self.images.insert(&key, image);
            }
            Msg::ImageFailed(key) => {
                self.images.mark_failed(&key);
            }
            Msg::Reload => {
                if self.screen == Screen::Reader {
                    if let Some(id) = self.reader_post_id() {
                        self.load_thread(id);
                    }
                } else {
                    self.load_feed(false);
                }
            }
            Msg::Toast(t) => self.toast(t),
            Msg::Error(e) => self.toast(format!("error: {e}")),
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
        if self.help {
            self.help = false;
            return;
        }
        if self.compose.is_some() {
            self.handle_compose_key(key);
            return;
        }
        if self.prompt.is_some() {
            self.handle_prompt_key(key);
            return;
        }
        match self.screen {
            Screen::Login => self.handle_login_key(key),
            Screen::Timeline => self.handle_timeline_key(key),
            Screen::Reader => self.handle_reader_key(key),
            Screen::Profile => self.handle_profile_key(key),
            Screen::Search => self.handle_search_key(key),
        }
    }

    fn handle_login_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter => {
                let phrase = self.input.trim().to_string();
                if phrase.is_empty() {
                    return;
                }
                let seed = identity::derive_seed(&phrase);
                let id = identity::identity_from_seed(&seed);
                self.principal = identity::principal_of(&id);
                let path = self.paths.identity_file();
                // Persist the derived key, not the phrase.
                let _ = identity::write_seed(&path, &seed);
                let client = self.client.clone();
                let tx = self.tx.clone();
                self.input.clear();
                self.toast("signing in...");
                self.spawn(async move {
                    client.ic.set_identity(id).await;
                    match client.user(None).await {
                        Ok(user) => {
                            let _ = tx.send(Msg::User(user));
                        }
                        Err(e) => {
                            let _ = tx.send(Msg::Error(format!("login: {e}")));
                        }
                    }
                });
            }
            KeyCode::Backspace => {
                self.input.pop();
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.input.push(c);
            }
            KeyCode::Esc => {
                self.input.clear();
                self.screen = Screen::Timeline;
            }
            _ => {}
        }
    }

    fn handle_timeline_key(&mut self, key: KeyEvent) {
        let count = self.posts.len();
        match key.code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('?') => self.help = true,
            KeyCode::Char('j') | KeyCode::Down => {
                if count > 0 {
                    self.selected = (self.selected + 1).min(count - 1);
                    if self.selected + 3 >= count && !self.end && !self.loading {
                        self.page += 1;
                        self.load_feed(true);
                    }
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.selected = self.selected.saturating_sub(1);
            }
            KeyCode::Char('g') | KeyCode::Home => self.selected = 0,
            KeyCode::Char('G') | KeyCode::End => {
                if count > 0 {
                    self.selected = count - 1;
                }
            }
            KeyCode::Char('R') => self.load_feed(false),
            KeyCode::Char('L') => {
                if !self.logged_in {
                    self.input.clear();
                    self.screen = Screen::Login;
                }
            }
            KeyCode::Tab | KeyCode::Char(']') => {
                self.tab = match self.tab {
                    FeedTab::Latest => FeedTab::Hot,
                    FeedTab::Hot => FeedTab::Personal,
                    FeedTab::Personal => FeedTab::Latest,
                };
                self.load_feed(false);
            }
            KeyCode::BackTab | KeyCode::Char('[') => {
                self.tab = match self.tab {
                    FeedTab::Latest => FeedTab::Personal,
                    FeedTab::Hot => FeedTab::Latest,
                    FeedTab::Personal => FeedTab::Hot,
                };
                self.load_feed(false);
            }
            KeyCode::Char('/') => {
                self.input.clear();
                self.prompt = Some(Prompt {
                    label: "search".into(),
                    value: String::new(),
                    action: PromptAction::Search,
                });
            }
            KeyCode::Char('n') => {
                if self.require_login() {
                    self.compose = Some(Compose {
                        kind: ComposeKind::New,
                        body: String::new(),
                        options: String::new(),
                        deadline: 24,
                        focus_options: false,
                    });
                }
            }
            KeyCode::Char('u') => {
                if self.require_login() {
                    self.load_profile(String::new());
                }
            }
            KeyCode::Enter => {
                if let Some(item) = self.posts.get(self.selected) {
                    let id = item.id();
                    self.load_thread(id);
                }
            }
            KeyCode::Char('r') => {
                if let Some(item) = self.posts.get(self.selected) {
                    self.open_reply(item.id());
                }
            }
            KeyCode::Char('p') => {
                if let Some(item) = self.posts.get(self.selected) {
                    self.repost(item.id(), String::new());
                }
            }
            KeyCode::Char('b') => self.action_toggle("bookmark"),
            KeyCode::Char('f') => self.action_toggle("follow"),
            KeyCode::Char('h') => self.action_toggle("hide"),
            KeyCode::Char('P') => self.action_pin(),
            KeyCode::Char('!') => self.open_report(),
            KeyCode::Char('t') => self.open_tip(),
            KeyCode::Char(c) if c.is_ascii_digit() => {
                if let Some(item) = self.posts.get(self.selected) {
                    let id = item.id();
                    self.react(id, c);
                }
            }
            _ => {}
        }
    }

    fn handle_reader_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                self.screen = Screen::Timeline;
                self.reader = None;
            }
            KeyCode::Char('?') => self.help = true,
            KeyCode::Char('j') | KeyCode::Down => {
                if let Some(r) = &mut self.reader {
                    r.scroll = r.scroll.saturating_add(1);
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if let Some(r) = &mut self.reader {
                    r.scroll = r.scroll.saturating_sub(1);
                }
            }
            KeyCode::Char('J') => {
                if let Some(r) = &mut self.reader
                    && r.sel + 1 < r.posts.len()
                {
                    r.sel += 1;
                    r.scroll = 0;
                }
            }
            KeyCode::Char('K') => {
                if let Some(r) = &mut self.reader {
                    r.sel = r.sel.saturating_sub(1);
                    r.scroll = 0;
                }
            }
            KeyCode::Char('d') | KeyCode::PageDown => {
                if let Some(r) = &mut self.reader {
                    r.scroll = r.scroll.saturating_add(10);
                }
            }
            KeyCode::Char('u') | KeyCode::PageUp => {
                if let Some(r) = &mut self.reader {
                    r.scroll = r.scroll.saturating_sub(10);
                }
            }
            KeyCode::Char('r') => {
                if let Some(id) = self.reader_post_id() {
                    self.open_reply(id);
                }
            }
            KeyCode::Char('p') => {
                if let Some(id) = self.reader_post_id() {
                    self.repost(id, String::new());
                }
            }
            KeyCode::Char('b') => self.action_toggle("bookmark"),
            KeyCode::Char('f') => self.action_toggle("follow"),
            KeyCode::Char('h') => self.action_toggle("hide"),
            KeyCode::Char('P') => self.action_pin(),
            KeyCode::Char('!') => self.open_report(),
            KeyCode::Char('t') => self.open_tip(),
            KeyCode::Char('v') => self.open_poll_vote(),
            KeyCode::Char(c) if c.is_ascii_digit() => {
                if let Some(item) = self.reader_current() {
                    let id = item.post.id;
                    self.react(id, c);
                }
            }
            _ => {}
        }
    }

    fn handle_profile_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                self.screen = Screen::Timeline;
                self.profile = None;
            }
            KeyCode::Char('?') => self.help = true,
            KeyCode::Char('j') | KeyCode::Down => {
                if let Some(p) = &mut self.profile
                    && !p.posts.is_empty()
                {
                    p.selected = (p.selected + 1).min(p.posts.len() - 1);
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if let Some(p) = &mut self.profile {
                    p.selected = p.selected.saturating_sub(1);
                }
            }
            KeyCode::Enter => {
                if let Some(p) = &self.profile
                    && let Some(item) = p.posts.get(p.selected)
                {
                    let id = item.id();
                    self.load_thread(id);
                }
            }
            _ => {}
        }
    }

    fn handle_search_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                self.screen = Screen::Timeline;
                self.search = None;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if let Some(s) = &mut self.search
                    && !s.results.is_empty()
                {
                    s.selected = (s.selected + 1).min(s.results.len() - 1);
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if let Some(s) = &mut self.search {
                    s.selected = s.selected.saturating_sub(1);
                }
            }
            KeyCode::Enter => {
                if let Some(s) = &self.search
                    && let Some(r) = s.results.get(s.selected)
                {
                    let id = r.id;
                    self.load_thread(id);
                }
            }
            _ => {}
        }
    }

    fn handle_compose_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => self.compose = None,
            KeyCode::Enter => {
                if let Some(c) = &mut self.compose {
                    if c.focus_options {
                        c.options.push('\n');
                    } else {
                        c.body.push('\n');
                    }
                }
            }
            KeyCode::Tab | KeyCode::BackTab => {
                if let Some(c) = &mut self.compose {
                    c.focus_options = !c.focus_options;
                }
            }
            KeyCode::Backspace => {
                if let Some(c) = &mut self.compose {
                    if c.focus_options {
                        c.options.pop();
                    } else {
                        c.body.pop();
                    }
                }
            }
            KeyCode::Char(ch) if ctrl && (ch == 's' || ch == 'S') => {
                self.submit_compose();
            }
            KeyCode::Char(ch) if !ctrl => {
                if let Some(c) = &mut self.compose {
                    if c.focus_options {
                        c.options.push(ch);
                    } else {
                        c.body.push(ch);
                    }
                }
            }
            _ => {}
        }
    }

    fn handle_prompt_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.prompt = None,
            KeyCode::Enter => self.submit_prompt(),
            KeyCode::Backspace => {
                if let Some(p) = &mut self.prompt {
                    p.value.pop();
                }
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(p) = &mut self.prompt {
                    p.value.push(c);
                }
            }
            _ => {}
        }
    }

    // -------------------- actions --------------------

    fn reader_current(&self) -> Option<PostItem> {
        self.reader
            .as_ref()
            .and_then(|r| r.posts.get(r.sel).cloned())
    }

    fn reader_post_id(&self) -> Option<u64> {
        self.reader_current().map(|p| p.post.id)
    }

    fn open_reply(&mut self, id: u64) {
        if !self.require_login() {
            return;
        }
        self.compose = Some(Compose {
            kind: ComposeKind::Reply(id),
            body: String::new(),
            options: String::new(),
            deadline: 24,
            focus_options: false,
        });
    }

    fn open_report(&mut self) {
        if !self.require_login() {
            return;
        }
        if let Some(id) = self.action_target() {
            self.prompt = Some(Prompt {
                label: format!("report reason for #{id}"),
                value: String::new(),
                action: PromptAction::Report(id),
            });
        }
    }

    fn open_tip(&mut self) {
        if !self.require_login() {
            return;
        }
        if let Some(id) = self.action_target() {
            self.prompt = Some(Prompt {
                label: format!("tip credits for #{id}"),
                value: String::new(),
                action: PromptAction::Tip(id),
            });
        }
    }

    fn open_poll_vote(&mut self) {
        if !self.require_login() {
            return;
        }
        if let Some(item) = self.action_target_item() {
            if let Some(crate::api::models::Extension::Poll(_)) = item.post.extension {
                self.prompt = Some(Prompt {
                    label: format!("poll option number for #{}", item.post.id),
                    value: String::new(),
                    action: PromptAction::VotePoll(item.post.id),
                });
            } else {
                self.toast("no poll on this post");
            }
        }
    }

    fn action_target_item(&self) -> Option<PostItem> {
        match self.screen {
            Screen::Reader => self.reader_current(),
            _ => self.posts.get(self.selected).cloned(),
        }
    }

    fn action_target(&self) -> Option<u64> {
        self.action_target_item().map(|i| i.post.id)
    }

    fn react(&mut self, post_id: u64, digit: char) {
        if !self.require_login() {
            return;
        }
        let code = reaction_for_digit(digit);
        let client = self.client.clone();
        let tx = self.tx.clone();
        self.spawn(async move {
            match client.react(post_id, code).await {
                Ok(()) => {
                    let _ = tx.send(Msg::Toast(format!("reacted {}", reaction_icon(code))));
                }
                Err(e) => {
                    let _ = tx.send(Msg::Error(format!("react: {e}")));
                }
            }
        });
    }

    fn repost(&mut self, post_id: u64, body: String) {
        if !self.require_login() {
            return;
        }
        let client = self.client.clone();
        let tx = self.tx.clone();
        self.toast("reposting...");
        self.spawn(async move {
            match client.create_repost(post_id, &body, None).await {
                Ok(_) => {
                    let _ = tx.send(Msg::Toast("reposted".into()));
                    let _ = tx.send(Msg::Reload);
                }
                Err(e) => {
                    let _ = tx.send(Msg::Error(format!("repost: {e}")));
                }
            }
        });
    }

    fn action_pin(&mut self) {
        if !self.require_login() {
            return;
        }
        let Some(id) = self.action_target() else {
            return;
        };
        let client = self.client.clone();
        let tx = self.tx.clone();
        self.spawn(async move {
            match client.toggle_pinned_post(id).await {
                Ok(()) => {
                    let _ = tx.send(Msg::Toast("pin toggled".into()));
                }
                Err(e) => {
                    let _ = tx.send(Msg::Error(format!("pin: {e}")));
                }
            }
        });
    }

    fn action_toggle(&mut self, kind: &str) {
        if !self.require_login() {
            return;
        }
        let Some(id) = self.action_target() else {
            return;
        };
        let client = self.client.clone();
        let tx = self.tx.clone();
        let kind = kind.to_string();
        self.spawn(async move {
            let result = match kind.as_str() {
                "bookmark" => client
                    .toggle_bookmark(id)
                    .await
                    .map(|_| "bookmark toggled".to_string()),
                "follow" => client
                    .toggle_following_post(id)
                    .await
                    .map(|_| "following toggled".to_string()),
                "hide" => client
                    .toggle_hide_post(id)
                    .await
                    .map(|_| "hidden toggled".to_string()),
                _ => Ok("done".to_string()),
            };
            match result {
                Ok(msg) => {
                    let _ = tx.send(Msg::Toast(msg));
                }
                Err(e) => {
                    let _ = tx.send(Msg::Error(format!("{kind}: {e}")));
                }
            }
        });
    }

    fn submit_compose(&mut self) {
        let Some(c) = self.compose.take() else {
            return;
        };
        let client = self.client.clone();
        let tx = self.tx.clone();
        let body = c.body;
        let options: Vec<String> = c
            .options
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();
        self.spawn(async move {
            let result = match c.kind {
                ComposeKind::New => client.add_post(&body, vec![], None, None, None).await,
                ComposeKind::Reply(parent) => {
                    client
                        .add_post(&body, vec![], Some(parent), None, None)
                        .await
                }
                ComposeKind::Repost(id) => client.create_repost(id, &body, None).await,
                ComposeKind::Poll => {
                    if options.len() < 2 {
                        let _ = tx.send(Msg::Error("poll needs at least 2 options".into()));
                        return;
                    }
                    client.create_poll(&body, options, c.deadline, None).await
                }
            };
            match result {
                Ok(id) => {
                    let _ = tx.send(Msg::Toast(format!("posted #{id}")));
                    let _ = tx.send(Msg::Reload);
                }
                Err(e) => {
                    let _ = tx.send(Msg::Error(format!("post: {e}")));
                }
            }
        });
    }

    fn submit_prompt(&mut self) {
        let Some(p) = self.prompt.take() else {
            return;
        };
        let client = self.client.clone();
        let tx = self.tx.clone();
        match p.action {
            PromptAction::Search => {
                self.input = p.value.clone();
                self.run_search(p.value);
            }
            PromptAction::Report(id) => {
                self.spawn(async move {
                    match client.report(id, &p.value).await {
                        Ok(()) => {
                            let _ = tx.send(Msg::Toast("reported".into()));
                        }
                        Err(e) => {
                            let _ = tx.send(Msg::Error(format!("report: {e}")));
                        }
                    }
                });
            }
            PromptAction::Tip(id) => {
                let amount: u64 = p.value.trim().parse().unwrap_or(0);
                self.spawn(async move {
                    match client.tip(id, amount).await {
                        Ok(()) => {
                            let _ = tx.send(Msg::Toast(format!("tipped {amount}")));
                        }
                        Err(e) => {
                            let _ = tx.send(Msg::Error(format!("tip: {e}")));
                        }
                    }
                });
            }
            PromptAction::VotePoll(id) => {
                let option: u16 = p.value.trim().parse().unwrap_or(0);
                self.spawn(async move {
                    match client.vote_on_poll(id, option, false).await {
                        Ok(()) => {
                            let _ = tx.send(Msg::Toast("voted".into()));
                        }
                        Err(e) => {
                            let _ = tx.send(Msg::Error(format!("vote: {e}")));
                        }
                    }
                });
            }
        }
    }
}

fn reaction_for_digit(digit: char) -> u16 {
    match digit {
        '1' => 10,
        '2' => 11,
        '3' => 12,
        '4' => 50,
        '5' => 51,
        '6' => 52,
        '7' => 53,
        '8' => 100,
        '9' => 101,
        '0' => 1,
        _ => 10,
    }
}

fn reaction_icon(code: u16) -> &'static str {
    crate::api::models::reaction_icon(code)
}
