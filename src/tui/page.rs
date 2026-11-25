//! Pages: what each one loads and how it turns into selectable entries.

use super::{Action, App};
use crate::api::{self, Item, PostId, Realm, User};
use crate::markdown::{self, Block};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use serde_json::Value;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

pub const DIM: Color = Color::DarkGray;
pub const ACCENT: Color = Color::Cyan;
pub const HEAD: Color = Color::Blue;
pub const WARN: Color = Color::Yellow;
pub const GOOD: Color = Color::Green;
pub const BAD: Color = Color::Red;
/// Lines of a post shown in feeds before "…".
const PREVIEW: usize = 12;

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Kind {
    Home,
    Hot,
    New,
    Bookmarks,
    Tags(Vec<String>),
    Realm(String),
    Realms,
    Inbox,
    User(String),
    Journal(String),
    Post(PostId),
    Wallet,
    Invites,
    Settings,
    Welcome,
    Search(String),
    Proposals,
    Tokens,
    /// Ledger transactions of a principal (empty: all) and subaccount (hex).
    Transactions(String, String),
    Dashboard,
    Distribution,
    Domains,
    Ethereum,
    Recovery,
    Links,
    Whitepaper,
    /// A list of users (followers, follows, members…).
    Users(Vec<u64>),
    Reposts(PostId),
}

#[derive(Clone)]
pub enum Target {
    Post(PostId),
    User(String),
    UserId(u64),
    Realm(String),
    Tag(String),
    Page(Kind, String),
    Run(Rc<RefCell<Option<Action>>>),
    Notification(u64),
    Invite(String),
    Domain(String),
    Tx(u32),
    /// Entry of a settings list to remove (kind, value).
    Listed(&'static str, String),
    More,
}

pub fn run(f: impl FnOnce(&mut App) + 'static) -> Target {
    Target::Run(Rc::new(RefCell::new(Some(Box::new(f)))))
}

pub enum Ln {
    Text(Line<'static>),
    /// Picture: url, rows it takes, alt.
    Image(String, u16, String),
}

pub struct Entry {
    pub lines: Vec<Ln>,
    pub target: Option<Target>,
    /// Stable identity to keep the selection across rebuilds.
    pub key: String,
}

impl Entry {
    pub fn height(&self) -> usize {
        self.lines
            .iter()
            .map(|l| match l {
                Ln::Text(_) => 1,
                Ln::Image(_, h, _) => *h as usize,
            })
            .sum()
    }
}

/// Everything a page loaded, by kind.
#[derive(Default)]
pub struct Data {
    pub user: Option<User>,
    pub realm: Option<Realm>,
    pub realms: Vec<(String, Realm)>,
    pub results: Vec<api::SearchResult>,
    pub invites: Vec<(String, api::Invite)>,
    pub txs: Vec<(u32, api::Transaction)>,
    pub json: Value,
    pub json2: Value,
    pub text: String,
    /// Comment depth in a thread (post page).
    pub depth: HashMap<PostId, usize>,
    /// Ancestors of the open post, root first.
    pub parents: Vec<Item>,
    /// Posts embedded by reposts, by id.
    pub embedded: HashMap<PostId, Item>,
    pub domains: BTreeMap<String, api::DomainConfig>,
    pub numbers: BTreeMap<String, u64>,
    pub tokens: Vec<WalletToken>,
    pub auction: Option<(api::Auction, String)>,
    pub eth: Option<(String, api::EthConfig, Vec<api::EthRecipe>)>,
    pub strings: Vec<String>,
    pub loaded: bool,
}

pub struct Page {
    /// Unique per opened page, so late replies find (only) their page.
    pub id: u64,
    pub kind: Kind,
    pub title: String,
    pub items: Vec<Item>,
    pub data: Data,
    pub tab: usize,
    pub pageno: usize,
    pub offset: PostId,
    pub more: bool,
    pub loading: bool,
    pub entries: Vec<Entry>,
    pub built: Option<u16>,
    pub sel: usize,
    pub sel_key: String,
    /// First visible line.
    pub off: usize,
    /// Filter or search text of the page (realms search, search query).
    pub query: String,
}

impl Page {
    pub fn new(kind: Kind, title: &str) -> Page {
        let query = match &kind {
            Kind::Search(q) => q.clone(),
            _ => String::new(),
        };
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        Page {
            id: NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            kind,
            title: title.to_string(),
            items: Vec::new(),
            data: Data::default(),
            tab: 0,
            pageno: 0,
            offset: 0,
            more: false,
            loading: false,
            entries: Vec::new(),
            built: None,
            sel: 0,
            sel_key: String::new(),
            off: 0,
            query,
        }
    }

    pub fn dirty(&mut self) {
        self.built = None;
    }

    pub fn post(&self, id: PostId) -> Option<&Item> {
        self.items
            .iter()
            .chain(self.data.parents.iter())
            .find(|i| i.post.id == id)
            .or_else(|| self.data.embedded.get(&id))
    }

    pub fn selected_target(&self) -> Option<&Target> {
        self.entries.get(self.sel)?.target.as_ref()
    }

    pub fn select(&mut self, i: usize) {
        if let Some(e) = self.entries.get(i) {
            self.sel = i;
            self.sel_key = e.key.clone();
        }
    }

    fn top_of(&self, i: usize) -> usize {
        self.entries[..i.min(self.entries.len())]
            .iter()
            .map(|e| e.height())
            .sum()
    }

    pub fn total(&self) -> usize {
        self.entries.iter().map(|e| e.height()).sum()
    }

    /// Moves the selection by `d` selectable entries; a tall entry scrolls first.
    pub fn step(&mut self, d: isize, view_h: usize) {
        if self.entries.is_empty() {
            return;
        }
        let top = self.top_of(self.sel);
        let h = self.entries[self.sel].height();
        if d > 0 && top + h > self.off + view_h && h > view_h / 2 {
            self.off = (self.off + view_h / 2).min(top + h - view_h.min(top + h));
            return;
        }
        if d < 0 && top < self.off && h > view_h / 2 {
            self.off = self.off.saturating_sub(view_h / 2).max(top);
            return;
        }
        let mut i = self.sel as isize;
        loop {
            i += d.signum();
            if i < 0 || i as usize >= self.entries.len() {
                // nothing selectable that way: just scroll
                if d > 0 {
                    let max = self.total().saturating_sub(view_h);
                    self.off = (self.off + view_h / 2).min(max);
                } else {
                    self.off = self.off.saturating_sub(view_h / 2);
                }
                return;
            }
            if self.entries[i as usize].target.is_some() {
                self.select(i as usize);
                self.follow(view_h, d < 0);
                return;
            }
        }
    }

    /// Keeps the selection on screen; going up shows its start.
    pub fn follow(&mut self, view_h: usize, up: bool) {
        if self.entries.is_empty() {
            return;
        }
        let top = self.top_of(self.sel);
        let h = self.entries[self.sel].height();
        // show the headers above the first selectable entry
        let first_sel = self
            .entries
            .iter()
            .position(|e| e.target.is_some())
            .unwrap_or(0);
        if self.sel == first_sel && top < view_h {
            self.off = 0;
            return;
        }
        if top < self.off || up && top < self.off + 1 {
            self.off = top;
        } else if top + h.min(view_h) > self.off + view_h {
            self.off = top + h.min(view_h) - view_h;
        }
    }

    pub fn scroll(&mut self, d: isize) {
        let max = self.total().saturating_sub(1);
        self.off = (self.off as isize + d).clamp(0, max as isize) as usize;
    }

    pub fn first(&mut self) {
        if let Some(i) = self.entries.iter().position(|e| e.target.is_some()) {
            self.select(i);
        }
        self.off = 0;
    }

    pub fn last(&mut self, view_h: usize) {
        if let Some(i) = self.entries.iter().rposition(|e| e.target.is_some()) {
            self.select(i);
        }
        self.off = self.total().saturating_sub(view_h);
    }

    /// Rebuilt entries: put the selection back on the same thing.
    pub fn restore_sel(&mut self) {
        if let Some(i) = self
            .entries
            .iter()
            .position(|e| !e.key.is_empty() && e.key == self.sel_key)
        {
            self.sel = i;
        } else if self
            .entries
            .get(self.sel)
            .is_none_or(|e| e.target.is_none())
        {
            let from = self.sel.min(self.entries.len().saturating_sub(1));
            let next = self
                .entries
                .iter()
                .skip(from)
                .position(|e| e.target.is_some())
                .map(|i| i + from);
            let prev = self
                .entries
                .iter()
                .take(from)
                .rposition(|e| e.target.is_some());
            if let Some(i) = next.or(prev) {
                self.sel = i;
            }
        }
        if let Some(e) = self.entries.get(self.sel) {
            self.sel_key = e.key.clone();
        }
    }
}

// ---- formatting helpers --------------------------------------------------------

pub fn ago(ns: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let s = now.saturating_sub(ns) / 1_000_000_000;
    match s {
        0..60 => format!("{s}s"),
        60..3600 => format!("{}m", s / 60),
        3600..86400 => format!("{}h", s / 3600),
        86400..7776000 => format!("{}d", s / 86400),
        _ => date(ns),
    }
}

pub fn date(ns: u64) -> String {
    chrono::DateTime::from_timestamp((ns / 1_000_000_000) as i64, 0)
        .map(|d| {
            d.with_timezone(&chrono::Local)
                .format("%-d %b %Y")
                .to_string()
        })
        .unwrap_or_default()
}

pub fn datetime(ns: u64) -> String {
    chrono::DateTime::from_timestamp((ns / 1_000_000_000) as i64, 0)
        .map(|d| {
            d.with_timezone(&chrono::Local)
                .format("%-d %b %Y %H:%M")
                .to_string()
        })
        .unwrap_or_default()
}

/// Base units to a decimal string with `dec` decimals, thousands separated.
pub fn amount(v: u64, dec: u32) -> String {
    let base = 10u64.pow(dec);
    let int = sep(v / base);
    if dec == 0 {
        return int;
    }
    let frac = format!("{:0w$}", v % base, w = dec as usize);
    let frac = frac.trim_end_matches('0');
    if frac.is_empty() {
        int
    } else {
        format!("{int}.{frac}")
    }
}

pub fn sep(v: u64) -> String {
    let s = v.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

pub fn isep(v: i64) -> String {
    if v < 0 {
        format!("-{}", sep(v.unsigned_abs()))
    } else {
        sep(v as u64)
    }
}

pub fn icp(e8s: u64) -> String {
    amount(e8s, 8)
}

pub fn dim() -> Style {
    Style::default().fg(DIM)
}
pub fn head() -> Style {
    Style::default().fg(HEAD).add_modifier(Modifier::BOLD)
}
pub fn accent() -> Style {
    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
}
pub fn key_style() -> Style {
    Style::default()
        .fg(Color::Magenta)
        .add_modifier(Modifier::BOLD)
}

/// Builder for entries.
pub struct Out<'a> {
    pub e: Vec<Entry>,
    pub w: usize,
    pub app: &'a App,
}

impl<'a> Out<'a> {
    pub fn text(&mut self, l: Line<'static>) {
        self.e.push(Entry {
            lines: vec![Ln::Text(l)],
            target: None,
            key: String::new(),
        });
    }
    pub fn blank(&mut self) {
        self.text(Line::raw(""));
    }
    pub fn heading(&mut self, s: &str) {
        if !self.e.is_empty() {
            self.blank();
        }
        self.text(Line::styled(s.to_string(), head()));
    }
    /// `label: value` row.
    pub fn kv(&mut self, k: &str, v: impl Into<String>) {
        let first = [Span::styled(format!("{k:<24}"), dim())];
        let rest = [Span::raw(" ".repeat(24))];
        let lines = markdown::wrap(vec![Span::raw(v.into())], self.w, &first, &rest);
        self.e.push(Entry {
            lines: lines.into_iter().map(Ln::Text).collect(),
            target: None,
            key: String::new(),
        });
    }
    pub fn md(&mut self, md: &str) {
        let r = markdown::render(md, self.w);
        let lines = blocks(self.app, r.blocks, self.w);
        if !lines.is_empty() {
            self.e.push(Entry {
                lines,
                target: None,
                key: String::new(),
            });
        }
    }
    pub fn note(&mut self, s: &str) {
        for l in markdown::wrap(vec![Span::styled(s.to_string(), dim())], self.w, &[], &[]) {
            self.text(l);
        }
    }
    /// Selectable row.
    pub fn row(&mut self, key: impl Into<String>, l: Line<'static>, t: Target) {
        self.e.push(Entry {
            lines: vec![Ln::Text(l)],
            target: Some(t),
            key: key.into(),
        });
    }
    pub fn rows(&mut self, key: impl Into<String>, lines: Vec<Line<'static>>, t: Target) {
        self.e.push(Entry {
            lines: lines.into_iter().map(Ln::Text).collect(),
            target: Some(t),
            key: key.into(),
        });
    }
    /// A selectable action row: `▸ label` running `f`.
    pub fn button(&mut self, label: &str, hint: &str, f: impl FnOnce(&mut App) + 'static) {
        let mut sp = vec![Span::styled(format!("▸ {label}"), accent())];
        if !hint.is_empty() {
            sp.push(Span::styled(format!("  {hint}"), dim()));
        }
        self.e.push(Entry {
            lines: vec![Ln::Text(Line::from(sp))],
            target: Some(run(f)),
            key: format!("btn:{label}"),
        });
    }
    pub fn user_link(&mut self, id: u64, extra: &str) {
        let name = self.app.name(id);
        self.row(
            format!("u:{id}"),
            Line::from(vec![
                Span::styled(format!("@{name}"), Style::default().fg(ACCENT)),
                Span::styled(format!(" {extra}"), dim()),
            ]),
            Target::UserId(id),
        );
    }
}

/// Markdown blocks to page lines; pictures become image lines when enabled.
pub fn blocks(app: &App, blocks: Vec<Block>, w: usize) -> Vec<Ln> {
    blocks_with(app, blocks, w, &BTreeMap::new())
}

fn blocks_with(
    app: &App,
    blocks: Vec<Block>,
    w: usize,
    urls: &BTreeMap<String, String>,
) -> Vec<Ln> {
    blocks
        .into_iter()
        .map(|b| match b {
            Block::Line(l) => Ln::Text(l),
            Block::Image(id, alt) => {
                let url = urls.get(&id).cloned().unwrap_or(id);
                if app.images.enabled() && url.starts_with("http") {
                    Ln::Image(url.clone(), app.images.rows(&url, w as u16), alt)
                } else {
                    Ln::Text(Line::styled(format!("[picture {alt}]"), dim()))
                }
            }
        })
        .collect()
}

/// The one-line head of a post: author, realm, age, comments, reactions.
pub fn post_head(app: &App, it: &Item, depth: usize) -> Line<'static> {
    let p = &it.post;
    let mut sp = Vec::new();
    if depth > 0 {
        sp.push(Span::styled("  ".repeat(depth - 1) + "↳ ", dim()));
    }
    let me = app.me() == p.user;
    sp.push(Span::styled(
        format!("@{}", it.meta.author_name),
        if me {
            Style::default().fg(GOOD).add_modifier(Modifier::BOLD)
        } else {
            accent()
        },
    ));
    if let Some(r) = &p.realm {
        sp.push(Span::styled(format!("  /{r}"), Style::default().fg(WARN)));
    }
    sp.push(Span::styled(format!("  {}", ago(p.timestamp)), dim()));
    if p.parent.is_some() && depth == 0 {
        sp.push(Span::styled("  reply", dim()));
    }
    if !p.children.is_empty() || p.tree_size > 0 {
        sp.push(Span::styled(
            format!("  ↩ {}", p.tree_size.max(p.children.len() as u64)),
            dim(),
        ));
    }
    let reactions = reactions_line(p);
    if !reactions.is_empty() {
        sp.push(Span::raw(format!("  {reactions}")));
    }
    if !p.tips.is_empty() {
        sp.push(Span::styled(
            format!("  ¤{}", p.tips.len()),
            Style::default().fg(WARN),
        ));
    }
    if let Some(u) = &app.user {
        if u.bookmarks.contains(&p.id) {
            sp.push(Span::styled("  ★", Style::default().fg(WARN)));
        }
        if p.watchers.contains(&u.id) {
            sp.push(Span::styled("  ◉", dim()));
        }
        if p.hidden_for.contains(&u.id) {
            sp.push(Span::styled("  hidden", dim()));
        }
    }
    if !p.patches.is_empty() {
        sp.push(Span::styled("  edited", dim()));
    }
    if it.meta.nsfw {
        sp.push(Span::styled("  NSFW", Style::default().fg(BAD)));
    }
    Line::from(sp)
}

pub fn reaction_icon(id: u16) -> &'static str {
    match id {
        1 => "❌",
        10 => "❤",
        11 => "👍",
        12 => "😢",
        50 => "🔥",
        51 => "😂",
        52 => "💯",
        53 => "🚀",
        100 => "⭐",
        101 => "🏴‍☠️",
        _ => "?",
    }
}

fn reactions_line(p: &api::Post) -> String {
    p.reactions
        .iter()
        .filter(|(_, u)| !u.is_empty())
        .map(|(id, u)| format!("{}{}", reaction_icon(*id), u.len()))
        .collect::<Vec<_>>()
        .join(" ")
}

/// A post as an entry: head, body (preview or full), poll, repost.
pub fn post_entry(app: &App, page: &Page, it: &Item, w: usize, full: bool, depth: usize) -> Entry {
    let p = &it.post;
    let indent = if depth > 0 { 2 * depth } else { 0 };
    let bw = w.saturating_sub(indent).max(20);
    let mut lines = vec![Ln::Text(post_head(app, it, depth))];
    let pad = |l: Line<'static>| -> Line<'static> {
        if indent == 0 {
            return l;
        }
        let mut sp = vec![Span::raw(" ".repeat(indent))];
        sp.extend(l.spans);
        Line::from(sp)
    };
    if p.deleted() {
        lines.push(Ln::Text(pad(Line::styled("[deleted]", dim()))));
    } else if it.meta.viewer_blocked {
        lines.push(Ln::Text(pad(Line::styled(
            "[you are blocked by the author]",
            dim(),
        ))));
    } else if p.hidden_for.contains(&app.me()) && !full {
        lines.push(Ln::Text(pad(Line::styled("[hidden by you]", dim()))));
    } else {
        let body = if full {
            p.body.replacen(markdown::CUT, "\n\n---\n\n", 1)
        } else {
            p.body.split(markdown::CUT).next().unwrap_or("").to_string()
        };
        let r = markdown::render(&body, bw);
        let urls = p.images(app.api.mainnet);
        let mut body = blocks_with(app, r.blocks, bw, &urls);
        let cut = !full && (body.len() > PREVIEW || p.body.contains(markdown::CUT));
        if !full && body.len() > PREVIEW {
            body.truncate(PREVIEW);
        }
        for l in body {
            lines.push(match l {
                Ln::Text(t) => Ln::Text(pad(t)),
                img => img,
            });
        }
        if cut {
            lines.push(Ln::Text(pad(Line::styled("… (Enter: read on)", dim()))));
        }
    }
    if let Some(poll) = p.poll() {
        lines.extend(
            poll_lines(app, p, poll, bw)
                .into_iter()
                .map(|l| Ln::Text(pad(l))),
        );
    }
    if let Some(api::Extension::Repost(id)) = &p.extension {
        match page.data.embedded.get(id) {
            Some(inner) => {
                let mut l = vec![Ln::Text(pad(Line::styled(
                    format!("┌ repost of #{id}"),
                    dim(),
                )))];
                let e = post_entry(app, page, inner, bw.saturating_sub(2), false, 0);
                for x in e.lines {
                    l.push(match x {
                        Ln::Text(t) => {
                            let mut sp =
                                vec![Span::raw(" ".repeat(indent)), Span::styled("│ ", dim())];
                            sp.extend(t.spans);
                            Ln::Text(Line::from(sp))
                        }
                        img => img,
                    });
                }
                lines.extend(l);
            }
            None => lines.push(Ln::Text(pad(Line::styled(
                format!("[repost of #{id}]"),
                dim(),
            )))),
        }
    }
    if let Some(api::Extension::Proposal(id)) = &p.extension {
        lines.push(Ln::Text(pad(Line::styled(
            format!("[proposal #{id}: open the post to vote]"),
            Style::default().fg(WARN),
        ))));
    }
    Entry {
        lines,
        target: Some(Target::Post(p.id)),
        key: format!("p:{}", p.id),
    }
}

fn poll_lines(app: &App, p: &api::Post, poll: &api::Poll, w: usize) -> Vec<Line<'static>> {
    let mut out = vec![Line::raw("")];
    let total: usize = poll.votes.values().map(|v| v.len()).sum();
    let mine = poll
        .votes
        .iter()
        .find(|(_, v)| v.contains(&app.me()))
        .map(|(k, _)| *k);
    let deadline = p.timestamp + poll.deadline * 3_600_000_000_000;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let open = now < deadline;
    for (i, o) in poll.options.iter().enumerate() {
        let n = poll.votes.get(&(i as u16)).map(|v| v.len()).unwrap_or(0);
        let pct = (n * 100).checked_div(total).unwrap_or(0);
        let bar_w = 12;
        let filled = pct * bar_w / 100;
        let mark = if mine == Some(i as u16) { "●" } else { " " };
        let label = format!("{mark} {}. {}", i + 1, o);
        let label = if label.chars().count() > w.saturating_sub(24) {
            label.chars().take(w.saturating_sub(25)).collect::<String>() + "…"
        } else {
            label
        };
        let mut sp = vec![Span::raw(label)];
        if !open || mine.is_some() || p.user == app.me() {
            sp.push(Span::styled(
                format!("  {}{} ", "█".repeat(filled), "░".repeat(bar_w - filled)),
                Style::default().fg(ACCENT),
            ));
            sp.push(Span::styled(format!("{pct}% ({n})"), dim()));
        }
        out.push(Line::from(sp));
    }
    let tokens: u64 = poll.weighted_by_tokens.values().sum();
    let mut info = format!("{total} votes · ");
    if open {
        info += &format!(
            "closes in {}h · v: vote",
            (deadline - now) / 3_600_000_000_000
        );
    } else {
        info += "closed";
    }
    if tokens > 0 {
        let dec = app.backend.token_decimals;
        let best = poll.weighted_by_tokens.iter().max_by_key(|(_, v)| **v);
        if let Some((k, v)) = best {
            info += &format!(" · token-weighted lead: {} ({})", k + 1, amount(*v, dec));
        }
    }
    out.push(Line::styled(info, dim()));
    out
}

// ---- loading -----------------------------------------------------------------------

/// Loads one page of a feed.
type FeedFn = Box<
    dyn FnOnce(
        std::sync::Arc<api::Api>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<Vec<Item>>> + Send>,
    >,
>;
/// Token stats, the auction and the latest transactions.
type TokensData = (
    Value,
    Option<(api::Auction, String)>,
    Vec<(u32, api::Transaction)>,
);
/// Ancestors, comments, comment depths, reposted posts, the proposal.
type ThreadData = (
    Vec<Item>,
    Vec<Item>,
    HashMap<PostId, usize>,
    Vec<Item>,
    Option<Value>,
);
/// ICRC token in the wallet: ledger, symbol, decimals, fee, balance.
pub type WalletToken = (String, String, u32, u64, u64);

/// Posts per feed page.
pub fn page_size(app: &App) -> usize {
    if app.backend.feed_page_size > 0 {
        app.backend.feed_page_size
    } else {
        30
    }
}

impl App {
    /// Loads the shown page (`more`: next page of a paged list).
    pub fn load(&mut self, more: bool) {
        let size = page_size(self);
        let Some(p) = self.pages.last_mut() else {
            return;
        };
        let kind = p.kind.clone();
        let (pageno, offset) = if more {
            (p.pageno + 1, p.offset)
        } else {
            (0, 0)
        };
        let tab = p.tab;
        let query = p.query.clone();
        p.loading = true;
        let bookmarks = self
            .user
            .as_ref()
            .map(|u| u.bookmarks.clone())
            .unwrap_or_default();
        let principal = self.principal.clone();
        let wallet_tokens = self
            .user
            .as_ref()
            .map(|u| u.wallet_tokens.clone())
            .unwrap_or_default();

        let pid = p.id;
        let check =
            move |app: &mut App| -> Option<usize> { app.pages.iter().position(|p| p.id == pid) };
        let feed = |app: &mut App, f: FeedFn| {
            app.bg(
                move |api| {
                    let fut = f(api.clone());
                    async move {
                        let items = fut.await?;
                        // posts embedded by reposts
                        let ids: Vec<PostId> = items
                            .iter()
                            .filter_map(|i| match &i.post.extension {
                                Some(api::Extension::Repost(id)) => Some(*id),
                                _ => None,
                            })
                            .collect();
                        let emb = api.posts(&ids).await.unwrap_or_default();
                        Ok((items, emb))
                    }
                },
                move |app, (items, emb): (Vec<Item>, Vec<Item>)| {
                    let Some(i) = check(app) else { return };
                    let ids: Vec<u64> = items
                        .iter()
                        .chain(emb.iter())
                        .map(|i| i.post.user)
                        .collect();
                    let p = &mut app.pages[i];
                    p.loading = false;
                    p.data.loaded = true;
                    p.more = items.len() >= size;
                    if more {
                        p.pageno = pageno;
                        p.items.extend(items);
                    } else {
                        p.pageno = 0;
                        p.offset = items.first().map(|i| i.post.id).unwrap_or(0);
                        p.items = items;
                    }
                    for e in emb {
                        p.data.embedded.insert(e.post.id, e);
                    }
                    p.dirty();
                    app.want_names(ids);
                },
            );
        };
        macro_rules! feed {
            (|$api:ident| $body:expr) => {
                feed(
                    self,
                    Box::new(move |$api: std::sync::Arc<api::Api>| Box::pin(async move { $body })),
                )
            };
        }
        match kind {
            Kind::Home => feed!(|api| api.personal_feed(pageno, offset).await),
            Kind::Hot => feed!(|api| api.hot_posts("", pageno, offset).await),
            Kind::New => feed!(|api| api.last_posts("", pageno, offset).await),
            Kind::Tags(tags) => {
                feed!(|api| api.posts_by_tags("", &tags, pageno, offset).await)
            }
            Kind::Bookmarks => {
                let ids: Vec<PostId> = bookmarks
                    .iter()
                    .skip(pageno * size)
                    .take(size)
                    .copied()
                    .collect();
                feed!(|api| api.posts(&ids).await)
            }
            Kind::Journal(h) => {
                let h2 = h.clone();
                feed!(|api| api.user_feed("journal", &h2, pageno, offset).await);
                if !more {
                    self.load_profile(pid, h.clone());
                }
            }
            Kind::Proposals => {
                let filter = [
                    "ALL",
                    "RELEASE",
                    "ICP TRANSFER",
                    "REALM CONTROLLER",
                    "FUNDING",
                    "REWARDS",
                ][tab % 6];
                feed!(|api| api.proposals(pageno, filter).await)
            }
            Kind::Realm(r) => {
                let r2 = r.clone();
                if tab == 0 {
                    feed!(|api| api.last_posts(&r2, pageno, offset).await)
                } else {
                    feed!(|api| api.hot_posts(&r2, pageno, offset).await)
                }
                if !more {
                    self.bg(
                        move |api| async move { api.realms(&[r]).await },
                        move |app, rs: Vec<Realm>| {
                            let Some(i) = check(app) else { return };
                            let ids: Vec<u64> =
                                rs.iter().flat_map(|r| r.controllers.clone()).collect();
                            app.pages[i].data.realm = rs.into_iter().next();
                            app.pages[i].dirty();
                            app.want_names(ids);
                        },
                    );
                }
            }
            Kind::User(h) => {
                let method = ["user_posts", "user_tags", "rewarded_posts"][tab % 3];
                let h2 = h.clone();
                feed!(|api| api.user_feed(method, &h2, pageno, offset).await);
                if !more {
                    self.load_profile(pid, h);
                }
            }
            Kind::Post(id) => self.load_thread(pid, id),
            Kind::Reposts(id) => {
                self.bg(
                    move |api| async move {
                        let p = api.posts(&[id]).await?;
                        let ids = p
                            .first()
                            .map(|i| i.post.reposts.clone())
                            .unwrap_or_default();
                        api.posts(&ids).await
                    },
                    move |app, items: Vec<Item>| {
                        let Some(i) = check(app) else { return };
                        let ids = items.iter().map(|i| i.post.user).collect();
                        let p = &mut app.pages[i];
                        p.items = items;
                        p.loading = false;
                        p.data.loaded = true;
                        p.dirty();
                        app.want_names(ids);
                    },
                );
            }
            Kind::Realms => {
                let order = ["popularity", "last_update", "name"][tab % 3];
                let q = query.clone();
                self.bg(
                    move |api| async move {
                        if q.is_empty() {
                            api.all_realms(order, pageno).await
                        } else {
                            api.realm_search(order, &q).await
                        }
                    },
                    move |app, rs: Vec<(String, Realm)>| {
                        let Some(i) = check(app) else { return };
                        let p = &mut app.pages[i];
                        p.more = rs.len() >= 20 && p.query.is_empty();
                        p.loading = false;
                        p.data.loaded = true;
                        if more {
                            p.pageno = pageno;
                            p.data.realms.extend(rs);
                        } else {
                            p.data.realms = rs;
                        }
                        p.dirty();
                    },
                );
            }
            Kind::Inbox => {
                self.load_user();
                if let Some(p) = self.page() {
                    p.loading = false;
                    p.data.loaded = true;
                    p.dirty();
                }
                // the posts the notifications point to
                let ids: Vec<PostId> = self
                    .user
                    .as_ref()
                    .map(|u| {
                        u.notifications
                            .values()
                            .filter_map(|(n, _)| match n {
                                api::Notification::NewPost(_, id) => Some(*id),
                                api::Notification::WatchedPostEntries(id, _) => Some(*id),
                                api::Notification::Conditional(_, api::Predicate::Proposal(id)) => {
                                    Some(*id)
                                }
                                api::Notification::Conditional(
                                    _,
                                    api::Predicate::ReportOpen(id),
                                ) => Some(*id),
                                _ => None,
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                self.bg(
                    move |api| async move { api.posts(&ids).await },
                    move |app, items: Vec<Item>| {
                        let Some(i) = check(app) else { return };
                        let ids = items.iter().map(|i| i.post.user).collect();
                        app.pages[i].items = items;
                        app.pages[i].dirty();
                        app.want_names(ids);
                    },
                );
            }
            Kind::Search(q) => {
                let q = if query.is_empty() { q } else { query };
                if q.is_empty() {
                    if let Some(p) = self.page() {
                        p.loading = false;
                    }
                    return;
                }
                self.bg(
                    move |api| async move { api.search(&q).await },
                    move |app, rs: Vec<api::SearchResult>| {
                        let Some(i) = check(app) else { return };
                        let ids = rs
                            .iter()
                            .map(|r| if r.result == "user" { r.id } else { r.user_id })
                            .collect();
                        let p = &mut app.pages[i];
                        p.data.results = rs;
                        p.loading = false;
                        p.data.loaded = true;
                        p.sel = 0;
                        p.sel_key.clear();
                        p.dirty();
                        app.want_names(ids);
                    },
                );
            }
            Kind::Users(ids) => {
                if let Some(p) = self.page() {
                    p.loading = false;
                    p.data.loaded = true;
                    p.dirty();
                }
                self.want_names(ids);
            }
            Kind::Invites => {
                self.bg(
                    |api| async move { api.invites().await },
                    move |app, inv: Vec<(String, api::Invite)>| {
                        let Some(i) = check(app) else { return };
                        let ids = inv
                            .iter()
                            .flat_map(|(_, i)| i.joined_user_ids.clone())
                            .collect();
                        let p = &mut app.pages[i];
                        p.data.invites = inv;
                        p.loading = false;
                        p.data.loaded = true;
                        p.dirty();
                        app.want_names(ids);
                    },
                );
            }
            Kind::Wallet => self.load_wallet(pid, principal, wallet_tokens),
            Kind::Settings => {
                self.load_user();
                let ids: Vec<u64> = self
                    .user
                    .as_ref()
                    .map(|u| {
                        u.filters
                            .users
                            .iter()
                            .chain(u.blacklist.iter())
                            .copied()
                            .collect()
                    })
                    .unwrap_or_default();
                self.want_names(ids);
                let bucket = self.user.as_ref().and_then(|u| u.bucket.clone());
                self.bg(
                    move |api| async move {
                        let pending = api.principal_change_target().await.ok().flatten();
                        let status = match bucket {
                            Some(b) => crate::storage::status(&api, &b).await.ok(),
                            None => None,
                        };
                        Ok((pending, status))
                    },
                    move |app, (pending, status): (Option<String>, Option<Value>)| {
                        let Some(i) = check(app) else { return };
                        let p = &mut app.pages[i];
                        p.data.text = pending.unwrap_or_default();
                        p.data.json = status.unwrap_or_default();
                        p.loading = false;
                        p.data.loaded = true;
                        p.dirty();
                    },
                );
            }
            Kind::Welcome => {
                if let Some(p) = self.page() {
                    p.loading = false;
                    p.data.loaded = true;
                    p.dirty();
                }
            }
            Kind::Tokens => {
                self.bg(
                    |api| async move {
                        let stats = api.token_stats().await?;
                        let auction = api.auction().await.ok();
                        let txs = api
                            .transactions(0, "2vxsx-fae", "")
                            .await
                            .unwrap_or_default();
                        Ok((stats, auction, txs))
                    },
                    move |app, (stats, auction, txs): TokensData| {
                        let Some(i) = check(app) else { return };
                        let mut ids: Vec<u64> = stats["balances"]
                            .as_array()
                            .map(|a| a.iter().filter_map(|b| b[2].as_u64()).collect())
                            .unwrap_or_default();
                        if let Some((a, _)) = &auction {
                            ids.extend(a.bids.iter().map(|b| b.user));
                        }
                        let p = &mut app.pages[i];
                        p.data.json = stats;
                        p.data.auction = auction;
                        p.data.txs = txs;
                        p.loading = false;
                        p.data.loaded = true;
                        p.dirty();
                        app.want_names(ids);
                    },
                );
            }
            Kind::Transactions(owner, sub) => {
                self.bg(
                    move |api| async move {
                        let o = if owner.is_empty() {
                            "2vxsx-fae".to_string()
                        } else {
                            owner
                        };
                        api.transactions(pageno, &o, &sub).await
                    },
                    move |app, txs: Vec<(u32, api::Transaction)>| {
                        let Some(i) = check(app) else { return };
                        let p = &mut app.pages[i];
                        p.more = txs.len() >= size;
                        if more {
                            p.pageno = pageno;
                            p.data.txs.extend(txs);
                        } else {
                            p.data.txs = txs;
                        }
                        p.loading = false;
                        p.data.loaded = true;
                        p.dirty();
                    },
                );
            }
            Kind::Dashboard => {
                self.bg(
                    |api| async move { Ok((api.stats().await?, api.logs().await.unwrap_or_default())) },
                    move |app, (stats, logs): (Value, Vec<Value>)| {
                        let Some(i) = check(app) else { return };
                        let mut ids: Vec<u64> = stats["stalwarts"]
                            .as_array()
                            .map(|a| a.iter().filter_map(|v| v.as_u64()).collect())
                            .unwrap_or_default();
                        if let Some(a) = stats["weekly_karma_leaders"].as_array() {
                            ids.extend(a.iter().filter_map(|v| v[0].as_u64()));
                        }
                        let p = &mut app.pages[i];
                        p.data.json = stats;
                        p.data.json2 = Value::Array(logs);
                        p.loading = false;
                        p.data.loaded = true;
                        p.dirty();
                        app.want_names(ids);
                    },
                );
            }
            Kind::Distribution => {
                self.bg(
                    |api| async move { api.distribution().await },
                    move |app, d: Vec<Value>| {
                        let Some(i) = check(app) else { return };
                        let p = &mut app.pages[i];
                        p.data.json = Value::Array(d);
                        p.loading = false;
                        p.data.loaded = true;
                        p.dirty();
                    },
                );
            }
            Kind::Domains => {
                self.bg(
                    |api| async move { api.domains().await },
                    move |app, d: BTreeMap<String, api::DomainConfig>| {
                        let Some(i) = check(app) else { return };
                        let ids = d
                            .values()
                            .filter_map(|c| match &c.sub_config {
                                api::DomainSub::Journal(u) => Some(*u),
                                _ => None,
                            })
                            .chain(d.values().filter_map(|c| c.owner))
                            .collect();
                        let p = &mut app.pages[i];
                        p.data.domains = d;
                        p.loading = false;
                        p.data.loaded = true;
                        p.dirty();
                        app.want_names(ids);
                    },
                );
            }
            Kind::Ethereum => {
                self.bg(
                    |api| async move { api.eth().await },
                    move |app, e: (String, api::EthConfig, Vec<api::EthRecipe>)| {
                        let Some(i) = check(app) else { return };
                        let p = &mut app.pages[i];
                        p.data.eth = Some(e);
                        p.loading = false;
                        p.data.loaded = true;
                        p.dirty();
                    },
                );
            }
            Kind::Recovery => {
                self.bg(
                    |api| async move { api.recovery_state().await },
                    move |app, (status, supporters): (String, Vec<String>)| {
                        let Some(i) = check(app) else { return };
                        let p = &mut app.pages[i];
                        p.data.text = status;
                        p.data.strings = supporters;
                        p.loading = false;
                        p.data.loaded = true;
                        p.dirty();
                    },
                );
            }
            Kind::Links => {
                if let Some(p) = self.page() {
                    p.loading = false;
                    p.data.loaded = true;
                    p.dirty();
                }
            }
            Kind::Whitepaper => {
                let url = if self.cfg.mainnet() {
                    format!("https://{}.icp0.io/WHITEPAPER.md", self.cfg.canister)
                } else {
                    format!("http://{}.localhost:8080/WHITEPAPER.md", self.cfg.canister)
                };
                self.bg(
                    move |_| async move {
                        let stats = reqwest::get(url).await?.error_for_status()?.text().await?;
                        Ok(stats)
                    },
                    move |app, text: String| {
                        let Some(i) = check(app) else { return };
                        let filled = fill_whitepaper(app, &text);
                        let p = &mut app.pages[i];
                        p.data.text = filled;
                        p.loading = false;
                        p.data.loaded = true;
                        p.dirty();
                    },
                );
                // stats for the placeholders
                self.bg(
                    |api| async move { api.stats().await },
                    move |app, s: Value| {
                        let Some(i) = check(app) else { return };
                        app.pages[i].data.json = s;
                    },
                );
            }
        }
    }

    fn load_profile(&mut self, pid: u64, handle: String) {
        self.bg(
            move |api| async move { api.user(Some(&handle)).await },
            move |app, u: Option<User>| {
                let Some(i) = app.pages.iter().position(|p| p.id == pid) else {
                    return;
                };
                let Some(u) = u else {
                    app.error("user not found");
                    return;
                };
                let mut ids = u.followers.clone();
                ids.extend(u.followees.iter());
                if let Some(r) = &u.report {
                    ids.push(r.reporter);
                }
                ids.extend(u.invited_by);
                let p = &mut app.pages[i];
                p.data.user = Some(u);
                p.dirty();
                app.want_names(ids);
            },
        );
    }

    fn load_thread(&mut self, pid: u64, id: PostId) {
        self.bg(
            move |api| async move {
                let chain = api.thread(id).await?;
                // all comments, breadth first
                let mut depths = HashMap::new();
                let mut items: Vec<Item> = Vec::new();
                let mut level: Vec<PostId> = chain
                    .last()
                    .map(|i| i.post.children.clone())
                    .unwrap_or_default();
                let mut d = 1;
                while !level.is_empty() && items.len() < 2000 {
                    let mut next = Vec::new();
                    for chunk in level.chunks(200) {
                        for it in api.posts(chunk).await? {
                            depths.insert(it.post.id, d);
                            next.extend(it.post.children.iter().copied());
                            items.push(it);
                        }
                    }
                    level = next;
                    d += 1;
                }
                let mut emb = Vec::new();
                for it in chain.iter().chain(items.iter()) {
                    if let Some(api::Extension::Repost(r)) = &it.post.extension {
                        emb.push(*r);
                    }
                }
                let emb = api.posts(&emb).await.unwrap_or_default();
                let proposal = match chain.last().and_then(|i| i.post.extension.clone()) {
                    Some(api::Extension::Proposal(pid)) => api.proposal(pid).await.ok(),
                    _ => None,
                };
                Ok((chain, items, depths, emb, proposal))
            },
            move |app, (mut chain, items, depths, emb, proposal): ThreadData| {
                let Some(pi) = app.pages.iter().position(|p| p.id == pid) else {
                    return;
                };
                let mut ids: Vec<u64> = chain
                    .iter()
                    .chain(items.iter())
                    .map(|i| i.post.user)
                    .collect();
                if let Some(pr) = &proposal {
                    ids.extend(
                        pr["bulletins"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|b| b[0].as_u64()),
                    );
                    ids.extend(pr["proposer"].as_u64());
                }
                for it in chain.iter().chain(items.iter()) {
                    ids.extend(it.post.tips.iter().map(|t| t.0));
                    ids.extend(it.post.reactions.values().flatten());
                }
                let p = &mut app.pages[pi];
                let Some(main) = chain.pop() else {
                    p.loading = false;
                    app.error("post not found");
                    return;
                };
                // comments in tree order
                let mut by_parent: HashMap<PostId, Vec<Item>> = HashMap::new();
                for it in items {
                    by_parent
                        .entry(it.post.parent.unwrap_or(0))
                        .or_default()
                        .push(it);
                }
                let mut ordered = vec![main.clone()];
                fn walk(id: PostId, by: &mut HashMap<PostId, Vec<Item>>, out: &mut Vec<Item>) {
                    if let Some(mut kids) = by.remove(&id) {
                        kids.sort_by_key(|k| k.post.id);
                        for k in kids {
                            let kid = k.post.id;
                            out.push(k);
                            walk(kid, by, out);
                        }
                    }
                }
                walk(main.post.id, &mut by_parent, &mut ordered);
                p.items = ordered;
                p.data.parents = chain;
                p.data.depth = depths;
                p.data.json = proposal.unwrap_or_default();
                for e in emb {
                    p.data.embedded.insert(e.post.id, e);
                }
                p.loading = false;
                p.data.loaded = true;
                p.dirty();
                app.want_names(ids);
            },
        );
    }

    fn load_wallet(&mut self, pid: u64, principal: String, tokens: Vec<String>) {
        self.bg(
            move |api| async move {
                let me = candid::Principal::from_text(&principal)?;
                let icp = api.icp_balance(me).await.ok();
                let mut list = Vec::new();
                for t in tokens {
                    let Ok(c) = candid::Principal::from_text(&t) else {
                        continue;
                    };
                    match api.icrc_meta(&c).await {
                        Ok(m) => {
                            let bal = api.icrc_balance(&c, me).await.unwrap_or(0);
                            list.push((t, m.symbol, m.decimals, m.fee, bal));
                        }
                        Err(_) => list.push((t, "?".into(), 0, 0, 0)),
                    }
                }
                Ok((icp, list))
            },
            move |app, (icp, list): (Option<u64>, Vec<WalletToken>)| {
                let Some(i) = app.pages.iter().position(|p| p.id == pid) else {
                    return;
                };
                let p = &mut app.pages[i];
                if let Some(v) = icp {
                    p.data.numbers.insert("icp".into(), v);
                }
                p.data.tokens = list;
                p.loading = false;
                p.data.loaded = true;
                p.dirty();
            },
        );
        self.load_user();
    }
}

/// Replaces the whitepaper's `$placeholders` like the web app does.
fn fill_whitepaper(app: &App, text: &str) -> String {
    let c = &app.backend;
    let dec = c.token_decimals;
    let mut out = String::new();
    let mut rest = text;
    while let Some(i) = rest.find('$') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        let n = after
            .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
            .unwrap_or(after.len());
        let key = &after[..n];
        let v = match key {
            "" => None,
            "maximum_supply" => Some(sep(c.maximum_supply / 10u64.pow(dec))),
            "usd_per_xdr" => Some("1.33".into()),
            "proposal_escrow_amount_usd" => {
                Some(format!("{:.0}", c.proposal_escrow_amount_xdr as f64 * 1.33))
            }
            "fee" => Some(amount(c.transaction_fee, dec)),
            k if k.starts_with("weekly_auction_size_tokens")
                || k.starts_with("random_reward_amount") =>
            {
                c.rest
                    .get(k)
                    .and_then(|v| v.as_u64())
                    .or((k == "weekly_auction_size_tokens").then_some(c.weekly_auction_size_tokens))
                    .map(|v| sep(v / 10u64.pow(dec)))
            }
            "active_user_share_for_minting_promille" => c
                .rest
                .get(key)
                .and_then(|v| v.as_u64())
                .map(|v| format!("{}", v as f64 / 10.0)),
            "canister_id" => Some(app.cfg.canister.clone()),
            "token_symbol" => Some(c.token_symbol.clone()),
            "name" => Some(c.name.clone()),
            k => c.rest.get(k).map(|v| match v {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            }),
        };
        match v {
            Some(v) => out.push_str(&v),
            None => {
                out.push('$');
                out.push_str(key);
            }
        }
        rest = &after[n..];
    }
    out.push_str(rest);
    out
}
