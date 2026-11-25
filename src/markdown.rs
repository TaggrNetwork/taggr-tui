//! Markdown → styled terminal lines, with Taggr's extras: `@user`, `#tag`, `$TOKEN`
//! and `/REALM` tokens, `/blob/<id>` pictures and the four-newline cut.

use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

/// Where a post's "more" part starts.
pub const CUT: &str = "\n\n\n\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    Url(String),
    User(String),
    Tag(String),
    Realm(String),
    Post(u64),
}

impl Link {
    pub fn label(&self) -> String {
        match self {
            Link::Url(u) => u.clone(),
            Link::User(u) => format!("@{u}"),
            Link::Tag(t) => format!("#{t}"),
            Link::Realm(r) => format!("/{r}"),
            Link::Post(p) => format!("post #{p}"),
        }
    }
}

pub enum Block {
    Line(Line<'static>),
    /// A picture: `/blob/<id>` id or external URL, and its alt text.
    Image(String, String),
}

#[derive(Default)]
pub struct Rendered {
    pub blocks: Vec<Block>,
    pub links: Vec<Link>,
}

pub const LINK: Color = Color::Blue;
pub const TOKEN: Color = Color::Cyan;
pub const CODE: Color = Color::Yellow;

/// Turns an in-app href (`#/user/x`, `/#/post/1`, `https://…/#/thread/2`) or a URL into a link.
pub fn parse_href(href: &str) -> Link {
    let route = href.find("#/").map(|i| &href[i + 2..]).filter(|_| {
        href.starts_with('#')
            || href.starts_with("/#")
            || href.contains(".icp")
            || href.contains("taggr")
    });
    if let Some(route) = route {
        let mut it = route.split('/');
        let (h, p) = (it.next().unwrap_or(""), it.next().unwrap_or(""));
        match h {
            "user" | "journal" if !p.is_empty() => return Link::User(p.to_string()),
            "feed" if !p.is_empty() => return Link::Tag(p.to_string()),
            "realm" if !p.is_empty() => return Link::Realm(p.to_uppercase()),
            "post" | "thread" => {
                if let Ok(id) = p.parse() {
                    return Link::Post(id);
                }
            }
            _ => {}
        }
    }
    Link::Url(href.to_string())
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric()
}

/// Splits plain text into spans, styling Taggr tokens (see `content.tsx` `linkTokenExp`).
fn tokens(text: &str, base: Style, links: &mut Vec<Link>) -> Vec<Span<'static>> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut out = Vec::new();
    let mut last = 0;
    let mut i = 0;
    while i < chars.len() {
        let (pos, c) = chars[i];
        let prev_ok = i == 0 || {
            let p = chars[i - 1].1;
            p.is_whitespace() || p == '('
        };
        let starts = matches!(c, '/' | '#' | '@')
            || (c == '$' && chars.get(i + 1).is_some_and(|(_, n)| n.is_alphabetic()));
        if prev_ok && starts {
            let mut j = i + 1;
            while j < chars.len() && (is_word(chars[j].1) || matches!(chars[j].1, '-' | '_' | '.'))
            {
                j += 1;
            }
            // must end with a letter or digit
            while j > i + 1 && !is_word(chars[j - 1].1) {
                j -= 1;
            }
            if j > i + 1 {
                let end = chars.get(j).map(|c| c.0).unwrap_or(text.len());
                if pos > last {
                    out.push(Span::styled(text[last..pos].to_string(), base));
                }
                let tok = &text[pos..end];
                let name = tok[1..].to_string();
                links.push(match c {
                    '@' => Link::User(name),
                    '/' => Link::Realm(name.to_uppercase()),
                    _ => Link::Tag(name),
                });
                out.push(Span::styled(tok.to_string(), base.fg(TOKEN)));
                last = end;
                i = j;
                continue;
            }
        }
        i += 1;
    }
    if last < text.len() {
        out.push(Span::styled(text[last..].to_string(), base));
    }
    out
}

fn width(s: &str) -> usize {
    Span::raw(s).width()
}

/// Greedy word wrap of styled spans to `w` columns; `first`/`rest` prefix the lines.
pub fn wrap(
    spans: Vec<Span<'static>>,
    w: usize,
    first: &[Span<'static>],
    rest: &[Span<'static>],
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let mut cur: Vec<Span<'static>> = first.to_vec();
    let mut cur_w: usize = first.iter().map(|s| s.width()).sum();
    let mut at_start = true;
    let rest_w: usize = rest.iter().map(|s| s.width()).sum();
    let w = w.max(rest_w + 8);
    for span in spans {
        let style = span.style;
        // split into words and whitespace / explicit newlines
        let mut word = String::new();
        let flush = |word: &mut String,
                     cur: &mut Vec<Span<'static>>,
                     cur_w: &mut usize,
                     lines: &mut Vec<Line<'static>>,
                     at_start: &mut bool| {
            if word.is_empty() {
                return;
            }
            let ww = width(word);
            let is_space = word.chars().all(|c| c == ' ');
            if *cur_w + ww > w && !*at_start {
                if is_space {
                    word.clear();
                    return;
                }
                lines.push(finish(std::mem::take(cur)));
                *cur = rest.to_vec();
                *cur_w = rest_w;
            }
            if is_space && *at_start && *cur_w == rest_w && !lines.is_empty() {
                word.clear();
                return;
            }
            // hard-break words longer than the line
            let mut piece = String::new();
            for c in word.chars() {
                let cw = width(&c.to_string());
                if *cur_w + width(&piece) + cw > w && !piece.is_empty() {
                    cur.push(Span::styled(std::mem::take(&mut piece), style));
                    lines.push(finish(std::mem::take(cur)));
                    *cur = rest.to_vec();
                    *cur_w = rest_w;
                }
                piece.push(c);
            }
            *cur_w += width(&piece);
            cur.push(Span::styled(piece, style));
            *at_start = false;
            word.clear();
        };
        let mut space = None;
        for c in span.content.chars() {
            if c == '\n' {
                flush(&mut word, &mut cur, &mut cur_w, &mut lines, &mut at_start);
                lines.push(finish(std::mem::take(&mut cur)));
                cur = rest.to_vec();
                cur_w = rest_w;
                at_start = true;
                space = None;
                continue;
            }
            let is_sp = c == ' ';
            if space.is_some_and(|s| s != is_sp) {
                flush(&mut word, &mut cur, &mut cur_w, &mut lines, &mut at_start);
            }
            space = Some(is_sp);
            word.push(c);
        }
        flush(&mut word, &mut cur, &mut cur_w, &mut lines, &mut at_start);
    }
    if cur.len() > rest.len() || lines.is_empty() || !at_start {
        lines.push(finish(cur));
    }
    lines
}

/// A wrapped line without trailing blanks.
fn finish(mut spans: Vec<Span<'static>>) -> Line<'static> {
    while spans.len() > 1
        && spans
            .last()
            .is_some_and(|l| l.content.trim_end_matches(' ').is_empty())
    {
        spans.pop();
    }
    if let Some(last) = spans.last_mut() {
        let t = last.content.trim_end_matches(' ').to_string();
        if t.len() != last.content.len() {
            last.content = t.into();
        }
    }
    Line::from(spans)
}

struct List {
    next: Option<u64>,
}

struct R<'a> {
    w: usize,
    out: &'a mut Rendered,
    spans: Vec<Span<'static>>,
    styles: Vec<Style>,
    /// Prefix stack: (first line, following lines) per nesting level.
    quote: usize,
    lists: Vec<List>,
    item_first: bool,
    in_code: Option<String>,
    code: String,
    link: Option<String>,
    image: Option<(String, String)>,
    table: Option<Table>,
    heading: bool,
}

#[derive(Default)]
struct Table {
    aligns: Vec<Alignment>,
    rows: Vec<Vec<Vec<Span<'static>>>>,
    cur: Vec<Vec<Span<'static>>>,
    head_rows: usize,
}

impl R<'_> {
    fn style(&self) -> Style {
        self.styles.last().copied().unwrap_or_default()
    }
    fn push_style(&mut self, f: impl Fn(Style) -> Style) {
        let s = f(self.style());
        self.styles.push(s);
    }
    fn prefixes(&mut self) -> (Vec<Span<'static>>, Vec<Span<'static>>) {
        let dim = Style::default().fg(Color::DarkGray);
        let mut first = Vec::new();
        let mut rest = Vec::new();
        for _ in 0..self.quote {
            first.push(Span::styled("│ ", dim));
            rest.push(Span::styled("│ ", dim));
        }
        let depth = self.lists.len();
        if depth > 0 {
            let indent = "  ".repeat(depth - 1);
            let bullet = match self.lists.last().and_then(|l| l.next) {
                Some(n) => format!("{n}. "),
                None => "• ".to_string(),
            };
            let pad = " ".repeat(width(&bullet));
            if self.item_first {
                first.push(Span::raw(indent.clone()));
                first.push(Span::styled(bullet, Style::default().fg(Color::DarkGray)));
                self.item_first = false;
                if let Some(l) = self.lists.last_mut()
                    && let Some(n) = l.next.as_mut()
                {
                    *n += 1;
                }
            } else {
                first.push(Span::raw(format!("{indent}{pad}")));
            }
            rest.push(Span::raw(format!("{indent}{pad}")));
        }
        (first, rest)
    }
    fn flush(&mut self) {
        if self.spans.is_empty() {
            return;
        }
        let spans = std::mem::take(&mut self.spans);
        let (first, rest) = self.prefixes();
        for l in wrap(spans, self.w, &first, &rest) {
            self.out.blocks.push(Block::Line(l));
        }
    }
    fn blank(&mut self) {
        if matches!(self.out.blocks.last(), Some(Block::Line(l)) if l.width() > 0)
            || matches!(self.out.blocks.last(), Some(Block::Image(..)))
        {
            if self.quote > 0 {
                let dim = Style::default().fg(Color::DarkGray);
                let l = Line::from(vec![Span::styled(
                    "│ ".repeat(self.quote).trim_end().to_string(),
                    dim,
                )]);
                self.out.blocks.push(Block::Line(l));
            } else {
                self.out.blocks.push(Block::Line(Line::raw("")));
            }
        }
    }
    fn text(&mut self, t: &str) {
        if let Some((_, alt)) = self.image.as_mut() {
            alt.push_str(t);
            return;
        }
        if let Some(table) = self.table.as_mut() {
            let st = self.styles.last().copied().unwrap_or_default();
            if let Some(cell) = table.cur.last_mut() {
                cell.push(Span::styled(t.to_string(), st));
            }
            return;
        }
        let st = self.style();
        if self.link.is_some() || self.heading {
            self.spans.push(Span::styled(t.to_string(), st));
        } else {
            let sp = tokens(t, st, &mut self.out.links);
            self.spans.extend(sp);
        }
    }
    fn table_end(&mut self) {
        let Some(t) = self.table.take() else { return };
        let cols = t.rows.iter().map(|r| r.len()).max().unwrap_or(0);
        if cols == 0 {
            return;
        }
        let mut widths = vec![0usize; cols];
        for r in &t.rows {
            for (i, c) in r.iter().enumerate() {
                widths[i] = widths[i].max(c.iter().map(|s| s.width()).sum());
            }
        }
        // shrink the widest columns until the table fits
        let sep = 3 * (cols - 1);
        while widths.iter().sum::<usize>() + sep > self.w && widths.iter().any(|w| *w > 6) {
            let i = (0..cols).max_by_key(|i| widths[*i]).unwrap_or(0);
            widths[i] -= 1;
        }
        let dim = Style::default().fg(Color::DarkGray);
        for (ri, r) in t.rows.iter().enumerate() {
            let mut spans = Vec::new();
            for (i, cw) in widths.iter().enumerate() {
                if i > 0 {
                    spans.push(Span::styled(" │ ", dim));
                }
                let cell: Vec<Span> = r.get(i).cloned().unwrap_or_default();
                let text: String = cell.iter().map(|s| s.content.as_ref()).collect();
                let st = cell.first().map(|s| s.style).unwrap_or_default();
                let st = if ri < t.head_rows {
                    st.add_modifier(Modifier::BOLD)
                } else {
                    st
                };
                let cut = fit(&text, *cw);
                let pad = cw.saturating_sub(width(&cut));
                let right = matches!(t.aligns.get(i), Some(Alignment::Right));
                if right {
                    spans.push(Span::raw(" ".repeat(pad)));
                }
                spans.push(Span::styled(cut, st));
                if !right {
                    spans.push(Span::raw(" ".repeat(pad)));
                }
            }
            self.out.blocks.push(Block::Line(Line::from(spans)));
            if ri + 1 == t.head_rows {
                let rule: Vec<String> = widths.iter().map(|w| "─".repeat(*w)).collect();
                self.out
                    .blocks
                    .push(Block::Line(Line::styled(rule.join("─┼─"), dim)));
            }
        }
    }
}

fn fit(s: &str, w: usize) -> String {
    if width(s) <= w {
        return s.to_string();
    }
    let mut out = String::new();
    for c in s.chars() {
        if width(&out) + width(&c.to_string()) + 1 > w {
            break;
        }
        out.push(c);
    }
    out.push('…');
    out
}

/// Renders `md` to lines at most `w` columns wide.
pub fn render(md: &str, w: usize) -> Rendered {
    let mut out = Rendered::default();
    let md = wrap_pgp(md);
    let opts = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let mut r = R {
        w: w.max(10),
        out: &mut out,
        spans: Vec::new(),
        styles: vec![Style::default()],
        quote: 0,
        lists: Vec::new(),
        item_first: false,
        in_code: None,
        code: String::new(),
        link: None,
        image: None,
        table: None,
        heading: false,
    };
    for ev in Parser::new_ext(&md, opts) {
        match ev {
            Event::Start(tag) => match tag {
                Tag::Paragraph => {
                    if r.lists.is_empty() || !r.item_first {
                        r.flush();
                    }
                }
                Tag::Heading { level, .. } => {
                    r.flush();
                    r.blank();
                    r.heading = true;
                    let s = Style::default().add_modifier(Modifier::BOLD);
                    let s = if level == HeadingLevel::H1 { s.fg(Color::Cyan) } else { s };
                    r.styles.push(s);
                }
                Tag::BlockQuote(_) => {
                    r.flush();
                    r.blank();
                    r.quote += 1;
                    r.push_style(|s| s.add_modifier(Modifier::ITALIC));
                }
                Tag::CodeBlock(kind) => {
                    r.flush();
                    r.blank();
                    r.in_code = Some(match kind {
                        CodeBlockKind::Fenced(l) => l.to_string(),
                        CodeBlockKind::Indented => String::new(),
                    });
                    r.code.clear();
                }
                Tag::List(start) => {
                    r.flush();
                    if r.lists.is_empty() {
                        r.blank();
                    }
                    r.lists.push(List { next: start });
                }
                Tag::Item => {
                    r.flush();
                    r.item_first = true;
                }
                Tag::Emphasis => r.push_style(|s| s.add_modifier(Modifier::ITALIC)),
                Tag::Strong => r.push_style(|s| s.add_modifier(Modifier::BOLD)),
                Tag::Strikethrough => r.push_style(|s| s.add_modifier(Modifier::CROSSED_OUT)),
                Tag::Link { dest_url, .. } => {
                    r.out.links.push(parse_href(&dest_url));
                    r.link = Some(dest_url.to_string());
                    r.push_style(|s| s.fg(LINK).add_modifier(Modifier::UNDERLINED));
                }
                Tag::Image { dest_url, .. } => {
                    let id = dest_url.strip_prefix("/blob/").unwrap_or(&dest_url).to_string();
                    r.image = Some((id, String::new()));
                }
                Tag::Table(aligns) => {
                    r.flush();
                    r.blank();
                    r.table = Some(Table { aligns, ..Default::default() });
                }
                Tag::TableHead => {
                    if let Some(t) = r.table.as_mut() {
                        t.cur.clear();
                    }
                }
                Tag::TableRow => {
                    if let Some(t) = r.table.as_mut() {
                        t.cur.clear();
                    }
                }
                Tag::TableCell => {
                    if let Some(t) = r.table.as_mut() {
                        t.cur.push(Vec::new());
                    }
                }
                _ => {}
            },
            Event::End(end) => match end {
                TagEnd::Paragraph => {
                    r.flush();
                    if r.lists.is_empty() {
                        r.blank();
                    }
                }
                TagEnd::Heading(_) => {
                    r.flush();
                    r.styles.pop();
                    r.heading = false;
                    r.blank();
                }
                TagEnd::BlockQuote(_) => {
                    r.flush();
                    r.quote -= 1;
                    r.styles.pop();
                    r.blank();
                }
                TagEnd::CodeBlock => {
                    let lang = r.in_code.take().unwrap_or_default();
                    let code = std::mem::take(&mut r.code);
                    let st = Style::default().fg(CODE);
                    let (first, _) = r.prefixes();
                    if !lang.is_empty() {
                        r.out.blocks.push(Block::Line(Line::styled(format!("  {lang}"), Style::default().fg(Color::DarkGray))));
                    }
                    for l in code.trim_end_matches('\n').lines() {
                        let mut sp = first.clone();
                        sp.push(Span::styled(format!("  {}", fit(l, r.w.saturating_sub(2))), st));
                        r.out.blocks.push(Block::Line(Line::from(sp)));
                    }
                    r.blank();
                }
                TagEnd::List(_) => {
                    r.flush();
                    r.lists.pop();
                    if r.lists.is_empty() {
                        r.blank();
                    }
                }
                TagEnd::Item => r.flush(),
                TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => {
                    r.styles.pop();
                }
                TagEnd::Link => {
                    r.styles.pop();
                    r.link = None;
                }
                TagEnd::Image => {
                    if let Some((id, alt)) = r.image.take() {
                        r.flush();
                        r.out.blocks.push(Block::Image(id, alt));
                    }
                }
                TagEnd::TableHead => {
                    if let Some(t) = r.table.as_mut() {
                        let row = std::mem::take(&mut t.cur);
                        t.rows.push(row);
                        t.head_rows = t.rows.len();
                    }
                }
                TagEnd::TableRow => {
                    if let Some(t) = r.table.as_mut() {
                        let row = std::mem::take(&mut t.cur);
                        t.rows.push(row);
                    }
                }
                TagEnd::Table => {
                    r.table_end();
                    r.blank();
                }
                _ => {}
            },
            Event::Text(t) => {
                if r.in_code.is_some() {
                    r.code.push_str(&t);
                } else {
                    r.text(&t);
                }
            }
            Event::Code(t) => {
                if let Some(table) = r.table.as_mut() {
                    if let Some(cell) = table.cur.last_mut() {
                        cell.push(Span::styled(t.to_string(), Style::default().fg(CODE)));
                    }
                } else {
                    r.spans.push(Span::styled(t.to_string(), Style::default().fg(CODE)));
                }
            }
            Event::SoftBreak => r.text(" "),
            Event::HardBreak => r.spans.push(Span::raw("\n")),
            Event::Rule => {
                r.flush();
                let w = r.w;
                r.out.blocks.push(Block::Line(Line::styled("─".repeat(w), Style::default().fg(Color::DarkGray))));
                r.blank();
            }
            Event::TaskListMarker(done) => r.spans.push(Span::raw(if done { "[x] " } else { "[ ] " })),
            Event::Html(h) | Event::InlineHtml(h)
                // show HTML as text (the web app strips most of it)
                if r.in_code.is_none() => {
                    let t = h.trim_end_matches('\n').to_string();
                    if !t.starts_with("<details") && !t.starts_with("</details") && !t.starts_with("<summary") {
                        r.text(&t);
                    }
                }
            _ => {}
        }
    }
    r.flush();
    while matches!(out.blocks.last(), Some(Block::Line(l)) if l.width() == 0) {
        out.blocks.pop();
    }
    out.links.dedup();
    out
}

fn wrap_pgp(md: &str) -> String {
    const B: &str = "-----BEGIN PGP MESSAGE-----";
    const E: &str = "-----END PGP MESSAGE-----";
    if !md.contains(B) {
        return md.to_string();
    }
    md.replace(B, &format!("```pgp\n{B}"))
        .replace(E, &format!("{E}\n```"))
}

/// Plain text of the first non-empty line of `md`, for list rows.
pub fn first_line(md: &str) -> String {
    for b in render(md, 10_000).blocks {
        if let Block::Line(l) = b {
            let t: String = l.spans.iter().map(|s| s.content.as_ref()).collect();
            if !t.trim().is_empty() {
                return t.trim().to_string();
            }
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(r: &Rendered) -> Vec<String> {
        r.blocks
            .iter()
            .map(|b| match b {
                Block::Line(l) => l.spans.iter().map(|s| s.content.as_ref()).collect(),
                Block::Image(id, _) => format!("[img {id}]"),
            })
            .collect()
    }

    #[test]
    fn tokens_are_links() {
        let r = render("hi @alice see #rust and /DAO, $TAGGR (not a/b)", 80);
        assert_eq!(
            r.links,
            vec![
                Link::User("alice".into()),
                Link::Tag("rust".into()),
                Link::Realm("DAO".into()),
                Link::Tag("TAGGR".into())
            ]
        );
    }

    #[test]
    fn internal_links_parse() {
        assert_eq!(parse_href("#/post/12"), Link::Post(12));
        assert_eq!(parse_href("/#/thread/7"), Link::Post(7));
        assert_eq!(parse_href("#/user/bob"), Link::User("bob".into()));
        assert_eq!(
            parse_href("https://x.org"),
            Link::Url("https://x.org".into())
        );
    }

    #[test]
    fn wraps_and_lists() {
        let r = render(
            "one two three four five six\n\n- a\n- b\n\n![p](/blob/abc)",
            12,
        );
        let t = text(&r);
        assert_eq!(t[0], "one two");
        assert!(t.contains(&"• a".to_string()));
        assert_eq!(t.last().unwrap(), "[img abc]");
    }

    #[test]
    fn table_renders() {
        let r = render("|a|b|\n|-|-|\n|1|22|", 40);
        let t = text(&r);
        assert_eq!(t[0], "a │ b ");
        assert_eq!(t[2], "1 │ 22");
    }
}
