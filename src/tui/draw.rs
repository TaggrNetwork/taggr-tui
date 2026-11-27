//! All rendering. Look and feel follow organiser and mailbox: ANSI colours only,
//! rounded panes and popups, key hints in the bottom line.

use super::form::FieldKind;
use super::page::{ACCENT, Kind, Ln, accent, dim, head, key_style};
use super::{App, Focus, Nav, Popup};
use crate::config::IconSet;
use ratatui::Frame;
use ratatui::buffer::{Buffer, CellWidth};
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Padding, Paragraph, Wrap};
use ratatui_image::StatefulImage;

const NARROW: u16 = 90;
const SPIN: [&str; 4] = ["⠋", "⠙", "⠸", "⠴"];

fn nav_icon(app: &App, n: &Nav) -> &'static str {
    if app.cfg.icons != IconSet::Nerd {
        return "";
    }
    match n {
        Nav::Go(k, _) => match k {
            Kind::Home => "\u{f015}",
            Kind::Hot => "\u{f06d}",
            Kind::New => "\u{f017}",
            Kind::Bookmarks => "\u{f02e}",
            Kind::Tags(_) => "\u{f292}",
            Kind::Realm(_) => "\u{f0c0}",
            Kind::Realms => "\u{f0ac}",
            Kind::Inbox => "\u{f01c}",
            Kind::User(_) => "\u{f007}",
            Kind::Journal(_) => "\u{f02d}",
            Kind::Wallet => "\u{f555}",
            Kind::Invites => "\u{f0e0}",
            Kind::Settings => "\u{f013}",
            Kind::Welcome => "\u{f4fc}",
            Kind::Search(_) => "\u{f002}",
            Kind::Proposals => "\u{f0e3}",
            Kind::Tokens => "\u{f51e}",
            Kind::Dashboard => "\u{f080}",
            Kind::Distribution => "\u{f1fe}",
            Kind::Domains => "\u{f0ac}",
            Kind::Links => "\u{f0c1}",
            Kind::Whitepaper => "\u{f15c}",
            _ => "",
        },
        Nav::SignIn => "\u{f090}",
        Nav::SignOut => "\u{f08b}",
        _ => "",
    }
}

/// Drops emoji variation selectors and joiners: terminals disagree on their width,
/// which leaves stray characters behind.
fn plain_width(s: &str) -> String {
    if s.contains(['\u{FE0F}', '\u{200D}']) {
        s.replace(['\u{FE0F}', '\u{200D}'], "")
    } else {
        s.to_string()
    }
}

fn pane(title: Line<'static>, focused: bool) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .padding(Padding::horizontal(1))
        .border_type(BorderType::Rounded)
        .border_style(if focused {
            Style::default().fg(ACCENT)
        } else {
            dim()
        })
        .title(title)
}

fn selected() -> Style {
    Style::default()
        .bg(ACCENT)
        .fg(Color::Black)
        .add_modifier(Modifier::BOLD)
}

fn width(s: &str) -> usize {
    Span::raw(s).width()
}

fn fit(s: &str, w: usize) -> String {
    if width(s) <= w {
        return format!("{s}{}", " ".repeat(w - width(s)));
    }
    let mut out = String::new();
    for c in s.chars() {
        if width(&out) + width(&c.to_string()) + 1 > w {
            break;
        }
        out.push(c);
    }
    format!("{out}…{}", " ".repeat(w.saturating_sub(width(&out) + 1)))
}

pub fn draw(f: &mut Frame, app: &mut App) {
    app.hits = Default::default();
    let area = f.area();
    let [body, bottom] = Layout::vertical([Constraint::Min(3), Constraint::Length(1)]).areas(area);
    let narrow = body.width < NARROW;
    let side_w = (body.width / 6).clamp(20, 28);
    if narrow && app.focus == Focus::Nav {
        draw_nav(f, app, body);
    } else if narrow {
        draw_page(f, app, body);
    } else {
        let [l, r] =
            Layout::horizontal([Constraint::Length(side_w), Constraint::Min(30)]).areas(body);
        draw_nav(f, app, l);
        draw_page(f, app, r);
    }
    draw_status(f, app, bottom);
    paint_selection(f.buffer_mut(), app);
    if app.popup.is_some() {
        draw_popup(f, app, area);
    }
}

/// Highlight the mouse selection and keep its text in `App::selected_text`: the cells
/// from `from` to `to` in reading order, inside the page pane only. Frames and comment
/// guides at either end of a row are left out, and rows of only frame are skipped.
fn paint_selection(buf: &mut Buffer, app: &mut App) {
    let Some(s) = app.selection.as_ref().filter(|s| s.dragged) else {
        return;
    };
    let (a, b) = if (s.from.y, s.from.x) <= (s.to.y, s.to.x) {
        (s.from, s.to)
    } else {
        (s.to, s.from)
    };
    let mut rows: Vec<String> = Vec::new();
    for y in a.y..=b.y {
        let x0 = if y == a.y { a.x } else { s.area.x };
        let x1 = if y == b.y { b.x } else { s.area.right() - 1 };
        let mut cells = Vec::new();
        let mut x = x0;
        while x <= x1 && buf.area.contains(Position::new(x, y)) {
            let cell = &buf[(x, y)];
            cells.push((x, cell.symbol().to_string()));
            x += cell.cell_width().max(1);
        }
        let content = |c: &(u16, String)| !is_decoration(&c.1);
        let (Some(first), Some(last)) = (
            cells.iter().position(content),
            cells.iter().rposition(content),
        ) else {
            // an empty line inside a frame or between comments separates paragraphs;
            // top and bottom borders are dropped
            let inside = !cells.is_empty()
                && cells
                    .iter()
                    .all(|c| c.1.chars().all(|ch| ch.is_whitespace() || ch == '│'));
            if inside {
                rows.push(String::new());
            }
            continue;
        };
        let mut row = String::new();
        for (x, sym) in &cells[first..=last] {
            row.push_str(sym);
            buf[(*x, y)].modifier.insert(Modifier::REVERSED);
        }
        rows.push(row);
    }
    // no blank lines at either end
    let first = rows.iter().position(|r| !r.is_empty());
    let end = rows
        .iter()
        .rposition(|r| !r.is_empty())
        .map_or(0, |i| i + 1);
    app.selected_text = first.map(|i| rows[i..end].join("\n")).unwrap_or_default();
}

/// A cell that only draws: blank, a frame or guide line, or a half-block picture.
fn is_decoration(sym: &str) -> bool {
    sym.chars()
        .all(|c| c.is_whitespace() || ('\u{2500}'..='\u{259F}').contains(&c))
}

fn draw_nav(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::Nav;
    let title = match &app.user {
        Some(u) => format!(" @{} · {} cr ", u.name, super::page::isep(u.cycles)),
        None if !app.principal.is_empty() => " new user ".into(),
        None => " Taggr ".into(),
    };
    let block = pane(Line::styled(title, head()), focused);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let (w, h) = (inner.width as usize, inner.height as usize);
    if app.nav_sel < app.nav_off {
        app.nav_off = app.nav_sel;
    } else if h > 0 && app.nav_sel >= app.nav_off + h {
        app.nav_off = app.nav_sel + 1 - h;
    }
    app.hits.nav = inner;
    let open = app.pages.first().map(|p| p.kind.clone());
    let mut lines = Vec::new();
    for (i, n) in app.nav.iter().enumerate().skip(app.nav_off).take(h) {
        let ic = nav_icon(app, n);
        let ic = if ic.is_empty() {
            String::new()
        } else {
            format!("{ic} ")
        };
        let line = match n {
            Nav::Spacer => Line::raw(""),
            Nav::Header(t) => Line::styled(fit(t, w), dim().add_modifier(Modifier::BOLD)),
            Nav::Go(k, label) => {
                let is_open = open.as_ref() == Some(k);
                let mut st = if is_open { accent() } else { Style::default() };
                if matches!(k, Kind::Inbox) && label.contains('(') {
                    st = st.fg(Color::Yellow).add_modifier(Modifier::BOLD);
                }
                Line::styled(fit(&format!(" {ic}{label}"), w), st)
            }
            Nav::SignIn => Line::styled(
                fit(&format!(" {ic}Sign in"), w),
                Style::default().fg(Color::Green),
            ),
            Nav::SignOut => Line::styled(fit(&format!(" {ic}Sign out"), w), dim()),
        };
        let sel = i == app.nav_sel && focused;
        lines.push(if sel { line.style(selected()) } else { line });
    }
    f.render_widget(Paragraph::new(lines), inner);
}

fn draw_page(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::Page;
    let Some(mut page) = app.pages.pop() else {
        f.render_widget(pane(Line::raw(""), focused), area);
        return;
    };
    let mut title = vec![Span::styled(format!(" {} ", page.title), head())];
    if !app.pages.is_empty() {
        title.insert(0, Span::styled(format!(" ‹ {}", app.pages.len()), dim()));
    }
    if page.loading {
        let t = (std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
            / 150) as usize;
        title.push(Span::styled(format!("{} ", SPIN[t % 4]), accent()));
    }
    let block = pane(Line::from(title), focused);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let w = inner.width as usize;
    if page.built != Some(inner.width) {
        let (entries, tabs) = super::view::build(app, &page, w);
        page.entries = entries;
        page.tab_row = tabs;
        page.built = Some(inner.width);
        page.restore_sel();
        let total = page.total();
        if page.off > total.saturating_sub(1) {
            page.off = total.saturating_sub(inner.height as usize);
        }
    }
    app.hits.page = inner;
    let h = inner.height as usize;
    let mut y = 0usize; // line index from the top of the page
    let mut imgs = Vec::new();
    for (i, e) in page.entries.iter().enumerate() {
        let eh = e.height();
        if y + eh <= page.off {
            y += eh;
            continue;
        }
        if y >= page.off + h {
            break;
        }
        let sel = i == page.sel && e.target.is_some();
        let top = y.max(page.off) - page.off;
        let vis = (y + eh).min(page.off + h) - y.max(page.off);
        app.hits.entries.push((i, inner.y + top as u16, vis as u16));
        if page.tab_row.as_ref().is_some_and(|t| t.0 == i) && y >= page.off {
            app.hits.tabs = Some((inner.y + top as u16, inner.x));
        }
        // a selected post card shows by its frame colour
        let fs = super::page::frame_style();
        let card = e.lines.first().is_some_and(|l| {
            matches!(l, Ln::Text(t) if t.spans.iter().any(|s| s.style == fs && s.content.starts_with('╭')))
        });
        let lit = sel && card;
        let frame_span = |s: &Span<'static>| -> Span<'static> {
            if !lit || s.style != fs {
                return Span::styled(plain_width(&s.content), s.style);
            }
            let fg = if focused { ACCENT } else { Color::Gray };
            Span::styled(
                s.content.clone(),
                Style::default().fg(fg).add_modifier(Modifier::BOLD),
            )
        };
        // other selected entries are highlighted rows, as in mailbox and organiser
        let row_sel = sel && !card;
        let row_style = if focused {
            Style::default().fg(Color::Black).bg(ACCENT)
        } else {
            Style::default().add_modifier(Modifier::REVERSED)
        };
        for l in &e.lines {
            let lh = match l {
                Ln::Text(_) => 1,
                Ln::Image { rows, .. } => *rows as usize,
            };
            if y + lh <= page.off || y >= page.off + h {
                y += lh;
                continue;
            }
            let row = inner.y + (y.max(page.off) - page.off) as u16;
            match l {
                Ln::Text(t) => {
                    // spans inherit the line's own style (`Line::styled`)
                    let sp: Vec<Span> = t
                        .spans
                        .iter()
                        .map(|s| {
                            let s = Span::styled(s.content.clone(), t.style.patch(s.style));
                            if row_sel {
                                Span::styled(plain_width(&s.content), s.style.patch(row_style))
                            } else {
                                frame_span(&s)
                            }
                        })
                        .collect();
                    let mut line = Line::from(sp);
                    if row_sel {
                        let pad = (inner.width as usize).saturating_sub(line.width());
                        line.spans.push(Span::styled(" ".repeat(pad), row_style));
                    }
                    f.render_widget(
                        Paragraph::new(line),
                        Rect::new(inner.x, row, inner.width, 1),
                    );
                }
                Ln::Image {
                    url,
                    rows,
                    alt,
                    width,
                    left,
                    right,
                } => {
                    let fully = y >= page.off && y + lh <= page.off + h;
                    let deco = |spans: &[Span<'static>]| -> Line<'static> {
                        Line::from(spans.iter().map(frame_span).collect::<Vec<_>>())
                    };
                    let lw: u16 = left.iter().map(|s| s.width() as u16).sum();
                    let ix = inner.x + lw;
                    let l = deco(left);
                    let r = deco(right);
                    for k in 0..lh {
                        let ry = y + k;
                        if ry >= page.off && ry < page.off + h {
                            let rr = inner.y + (ry - page.off) as u16;
                            f.render_widget(
                                Paragraph::new(l.clone()),
                                Rect::new(inner.x, rr, lw, 1),
                            );
                            let rx = ix + width;
                            if rx < inner.x + inner.width {
                                f.render_widget(
                                    Paragraph::new(r.clone()),
                                    Rect::new(rx, rr, inner.x + inner.width - rx, 1),
                                );
                            }
                        }
                    }
                    if fully && !app.images.failed(url) {
                        imgs.push((url.clone(), Rect::new(ix, row, *width, *rows), alt.clone()));
                    } else {
                        let label = if app.images.failed(url) {
                            format!("[picture {alt} (not loaded)]")
                        } else {
                            format!("[picture {alt}]")
                        };
                        f.render_widget(
                            Paragraph::new(Line::styled(label, dim())),
                            Rect::new(ix, row, *width, 1),
                        );
                    }
                }
            }
            y += lh;
        }
    }
    let total = page.total();
    if total > h && h > 0 {
        let pos = format!(" {}% ", ((page.off + h).min(total) * 100 / total.max(1)));
        let r = Rect::new(
            area.x + area.width.saturating_sub(pos.len() as u16 + 2),
            area.y + area.height - 1,
            pos.len() as u16,
            1,
        );
        f.render_widget(Paragraph::new(Span::styled(pos, dim())), r);
    }
    for (url, r, alt) in imgs {
        let before = app.images.queue.len();
        match app.images.get(&url) {
            Some(proto) => f.render_stateful_widget(StatefulImage::default(), r, proto),
            None => {
                f.render_widget(
                    Paragraph::new(Line::styled(format!("[loading picture {alt}]"), dim())),
                    Rect::new(r.x, r.y, r.width, 1),
                );
            }
        }
        if app.images.queue.len() > before {
            page.dirty();
        }
    }
    app.pages.push(page);
}

/// Keys for what is focused and selected, most useful first.
fn hints(app: &App) -> Vec<(String, String)> {
    use super::page::Target;
    let k = |a: &str, b: &str| (a.to_string(), b.to_string());
    if app.focus == Focus::Nav {
        return vec![
            k("j k", "move"),
            k("Enter", "open"),
            k("Tab", "page"),
            k("/", "search"),
            k("c", "new post"),
            k("?", "help"),
            k("q", "quit"),
        ];
    }
    let signed = app.user.is_some();
    let page = app.pages.last();
    let mut v = vec![];
    match page.and_then(|p| p.selected_target()) {
        Some(Target::Post(id)) => {
            let it = page.and_then(|p| p.post(*id));
            let mine = it.is_some_and(|i| i.post.user == app.me());
            let open_here = matches!(page.map(|p| &p.kind), Some(Kind::Post(cur)) if cur == id);
            if !open_here {
                v.push(k("Enter", "open"));
            }
            if signed {
                v.extend([k("r", "reply"), k("+", "react"), k("b", "bookmark")]);
                if mine {
                    v.extend([k("e", "edit"), k("D", "delete")]);
                } else {
                    v.push(k("t", "tip"));
                }
                v.push(k("P", "repost"));
                if it.is_some_and(|i| i.post.poll().is_some()) {
                    v.push(k("v", "vote"));
                }
            }
            if it.is_some_and(|i| !i.post.files.is_empty()) {
                v.push(k("I", "pictures"));
            }
            if it.is_some_and(|i| crate::pgp::find_block(&i.post.body).is_some()) {
                let what = if app.decrypted.contains_key(id) {
                    "hide plain text"
                } else {
                    "decrypt"
                };
                v.push(k("x", what));
            }
            v.extend([k("o", "links"), k("i", "info")]);
            if !signed {
                v.push(k("", "sign in (sidebar) to reply and react"));
            }
        }
        Some(Target::User(_) | Target::UserId(_)) => {
            v.push(k("Enter", "profile"));
            if signed {
                v.extend([
                    k("f", "follow"),
                    k("M", "mute"),
                    k("B", "block"),
                    k("$", "credits"),
                ]);
            }
        }
        Some(Target::Realm(_)) => {
            v.push(k("Enter", "open realm"));
            if signed {
                v.push(k("J", "join/leave"));
            }
        }
        Some(Target::Notification(_)) => v.extend([
            k("Enter", "open"),
            k("d", "dismiss"),
            k("D", "clear all"),
            k("w", "unwatch"),
        ]),
        Some(Target::Listed(..)) => v.push(k("x", "remove")),
        Some(Target::Invite(_)) => v.push(k("Enter", "edit invite")),
        Some(Target::Domain(_)) => v.push(k("Enter", "edit/open")),
        Some(Target::Tx(_)) => v.push(k("Enter", "details")),
        Some(Target::More) => v.push(k("Enter", "load more")),
        Some(Target::Run(_)) => v.push(k("Enter", "choose")),
        Some(Target::Tag(_) | Target::Page(..)) => v.push(k("Enter", "open")),
        None => {}
    }
    if let Some(t) = page.and_then(|p| p.tab_row.as_ref()) {
        v.push(k(&format!("1-{} [ ]", t.1.len()), "tab"));
    }
    if page.is_some_and(|p| {
        p.entries
            .iter()
            .filter(|e| e.target.is_some())
            .nth(1)
            .is_some()
    }) {
        v.push(k("j k", "move"));
    }
    v.extend([k("Esc", "back"), k("?", "help")]);
    v
}

fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    let left = match &app.status {
        Some((m, true, _)) => Line::styled(format!(" {m}"), Style::default().fg(Color::Red)),
        Some((m, false, _)) => Line::styled(format!(" {m}"), Style::default().fg(Color::Green)),
        None => {
            let mut spans = vec![Span::raw(" ")];
            for (k, d) in hints(app) {
                spans.push(Span::styled(k, key_style()));
                spans.push(Span::styled(format!(" {d}  "), dim()));
            }
            Line::from(spans)
        }
    };
    f.render_widget(Paragraph::new(left), area);
    if app.busy > 0 {
        let t = (std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
            / 150) as usize;
        f.render_widget(
            Paragraph::new(Line::styled(format!("{} ", SPIN[t % 4]), accent()).right_aligned()),
            area,
        );
    }
}

/// A row of key hints: key, then what it does.
fn key_line(keys: &[(&str, String)]) -> Line<'static> {
    let mut spans = vec![];
    for (k, d) in keys {
        spans.push(Span::styled(k.to_string(), key_style()));
        spans.push(Span::styled(format!(" {d}  "), dim()));
    }
    Line::from(spans)
}

fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width.saturating_sub(2));
    let h = h.min(area.height.saturating_sub(2));
    Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    )
}

fn popup_block(title: &str) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(ACCENT))
        .padding(Padding::horizontal(1))
        .title(Line::styled(format!(" {title} "), accent()))
}

fn wrap_lines(s: &str, w: usize, st: Style) -> Vec<Line<'static>> {
    crate::markdown::wrap(vec![Span::styled(s.to_string(), st)], w, &[], &[])
}

fn draw_popup(f: &mut Frame, app: &mut App, area: Rect) {
    let Some(p) = app.popup.as_ref() else { return };
    match p {
        Popup::Help => draw_help(f, area),
        Popup::Confirm(text, _) => {
            let w = 64usize;
            let mut lines = wrap_lines(text, w - 4, Style::default());
            lines.push(Line::raw(""));
            lines.push(Line::from(vec![
                Span::styled("y", key_style()),
                Span::styled(" yes   ", dim()),
                Span::styled("n", key_style()),
                Span::styled(" no", dim()),
            ]));
            let r = centered(area, w as u16, lines.len() as u16 + 2);
            f.render_widget(Clear, r);
            f.render_widget(Paragraph::new(lines).block(popup_block("Confirm")), r);
        }
        Popup::Menu(m) => {
            let w = m
                .items
                .iter()
                .map(|(l, _)| width(l) + 6)
                .max()
                .unwrap_or(20)
                .clamp(30, area.width as usize - 4);
            let h = (m.items.len() + 2).min(area.height as usize - 2);
            let r = centered(area, w as u16, h as u16);
            let inner_h = h.saturating_sub(2);
            let off = if m.sel >= inner_h {
                m.sel + 1 - inner_h
            } else {
                0
            };
            let lines: Vec<Line> = m
                .items
                .iter()
                .enumerate()
                .skip(off)
                .take(inner_h)
                .map(|(i, (l, _))| {
                    let num = if i < 10 {
                        format!("{} ", (i + 1) % 10)
                    } else {
                        "  ".into()
                    };
                    let line = Line::from(vec![Span::styled(num, dim()), Span::raw(fit(l, w - 6))]);
                    if i == m.sel {
                        line.style(selected())
                    } else {
                        line
                    }
                })
                .collect();
            f.render_widget(Clear, r);
            f.render_widget(Paragraph::new(lines).block(popup_block(&m.title)), r);
        }
        Popup::Form(form) => {
            let w = 76usize;
            let iw = w - 4;
            let mut lines: Vec<Line> = Vec::new();
            for n in &form.notes {
                lines.extend(wrap_lines(n, iw, dim()));
                lines.push(Line::raw(""));
            }
            let mut cursor = None;
            for (i, fl) in form.fields.iter().enumerate() {
                let sel = i == form.sel;
                lines.push(Line::styled(
                    fl.label.clone(),
                    if sel { accent() } else { dim() },
                ));
                let shown = fl.shown();
                let editable = matches!(fl.kind, FieldKind::Text | FieldKind::Secret);
                let (vis, cur_col) = if editable {
                    // keep the cursor visible in long values
                    let chars: Vec<char> = shown.chars().collect();
                    let start = fl.cur.saturating_sub(iw.saturating_sub(4));
                    let s: String = chars.iter().skip(start).take(iw - 2).collect();
                    (s, fl.cur - start)
                } else {
                    (fit(&shown, iw - 2), 0)
                };
                let st = if sel {
                    Style::default().add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                lines.push(Line::from(vec![
                    Span::styled("▸ ", if sel { accent() } else { dim() }),
                    Span::styled(vis, st),
                ]));
                if sel && editable {
                    cursor = Some((lines.len() - 1, cur_col + 2));
                }
            }
            lines.push(Line::raw(""));
            if let Some(e) = &form.error {
                lines.extend(wrap_lines(e, iw, Style::default().fg(Color::Red)));
            }
            // only the keys that do something on the selected field
            let n = form.fields.len();
            let kind = form.fields.get(form.sel).map(|f| &f.kind);
            let mut keys = vec![];
            if n > 1 {
                keys.push(("Tab", "next".to_string()));
            }
            match kind {
                Some(FieldKind::Bool) => keys.push(("Space", "toggle".into())),
                Some(FieldKind::Choice(..)) => keys.push(("← →", "change".into())),
                _ => {}
            }
            match kind {
                Some(FieldKind::Long) => keys.push(("Enter", "edit".into())),
                _ if form.sel + 1 < n => keys.push(("Enter", "next".into())),
                _ => {}
            }
            keys.extend([("^S", form.submit.clone()), ("Esc", "cancel".into())]);
            lines.push(key_line(&keys));
            let h = (lines.len() as u16 + 2).min(area.height - 2);
            let r = centered(area, w as u16, h);
            f.render_widget(Clear, r);
            let off = cursor
                .map(|c| (c.0 + 3).saturating_sub(h as usize - 2))
                .unwrap_or(0) as u16;
            f.render_widget(
                Paragraph::new(lines)
                    .scroll((off, 0))
                    .block(popup_block(&form.title)),
                r,
            );
            if let Some((row, col)) = cursor {
                let y = r.y + 1 + row as u16 - off.min(row as u16);
                if y < r.y + r.height - 1 {
                    f.set_cursor_position(Position::new(r.x + 2 + col as u16, y));
                }
            }
        }
        Popup::Send(d, scroll) => {
            let w = (area.width - 4).min(100);
            let iw = w as usize - 4;
            let mut lines = vec![];
            let mut info = vec![];
            if let Some(r) = &d.realm {
                info.push(format!("realm /{r}"));
            }
            info.push(format!("~{} credits", super::compose::cost(app, d)));
            let imgs = super::compose::local_images(&d.body).len();
            if imgs > 0 {
                info.push(format!("{imgs} pictures to upload"));
            }
            if let Some(to) = &d.encrypted {
                info.push(format!("encrypted to {}", to.join(", ")));
            } else if d.plain_quote {
                info.push("quotes decrypted text in the clear".into());
            }
            lines.push(Line::styled(info.join(" · "), dim()));
            lines.push(Line::raw(""));
            if let Some(p) = &d.poll {
                for (i, o) in p.options.iter().enumerate() {
                    lines.push(Line::raw(format!("  {}. {o}", i + 1)));
                }
                lines.push(Line::styled(
                    format!("  poll open for {}h", p.deadline),
                    dim(),
                ));
                lines.push(Line::raw(""));
            }
            let r = if d.encrypted.is_some() {
                // ciphertext as it will be posted, not as Markdown
                let raw = d.body.lines().map(|l| Line::styled(l.to_string(), dim()));
                lines.extend(raw);
                Default::default()
            } else {
                crate::markdown::render(&d.body, iw)
            };
            for b in r.blocks {
                lines.push(match b {
                    crate::markdown::Block::Line(l) => l,
                    crate::markdown::Block::Image(id, alt) => {
                        Line::styled(format!("[picture {alt} {id}]"), dim())
                    }
                });
            }
            let h = area.height - 2;
            let r = centered(area, w, h);
            let body_h = h.saturating_sub(4);
            let max = (lines.len() as u16).saturating_sub(body_h);
            let s = (*scroll).min(max);
            f.render_widget(Clear, r);
            let block = popup_block(&d.title());
            let inner = block.inner(r);
            f.render_widget(block, r);
            let [b, k] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(inner);
            f.render_widget(Paragraph::new(lines).scroll((s, 0)), b);
            let mut keys = vec![("y", "post".to_string()), ("e", "edit".into())];
            if super::compose::encrypt_blocker(d).is_none() {
                keys.push(("x", "encrypt".into()));
            }
            keys.extend([("Esc", "keep for later".into()), ("D", "discard".into())]);
            if max > 0 {
                keys.push(("j k", "scroll".into()));
            }
            f.render_widget(Paragraph::new(key_line(&keys)), k);
        }
        Popup::Text(title, lines, scroll) => {
            let w = (area.width - 4).min(84);
            let h = (lines.len() as u16 + 2).min(area.height - 2);
            let r = centered(area, w, h);
            let max = (lines.len() as u16).saturating_sub(h - 2);
            f.render_widget(Clear, r);
            f.render_widget(
                Paragraph::new(lines.clone())
                    .scroll(((*scroll).min(max), 0))
                    .wrap(Wrap { trim: false })
                    .block(popup_block(title)),
                r,
            );
        }
    }
}

fn draw_help(f: &mut Frame, area: Rect) {
    let keys: &[(&str, &str)] = &[
        ("Tab", "switch between sidebar and page"),
        ("j k  ↓ ↑", "previous / next post or item"),
        ("g G  PgUp PgDn  Space", "top / bottom / page"),
        (
            "Enter",
            "open: post, user, realm, tag, notification, button",
        ),
        (
            "Esc  h  ←",
            "back (to the sidebar on the first page; → returns)",
        ),
        (
            "1-9  [ ]",
            "switch tabs (realms, profiles, proposals); tabs are clickable",
        ),
        ("s", "reload the page"),
        ("/", "search (on Realms: search realms)"),
        ("c  C", "new post / new poll (in the shown realm)"),
        ("", ""),
        ("r", "post: reply"),
        ("+", "post: react (cost and reward shown)"),
        ("b w H p", "post: bookmark · watch · hide · pin to profile"),
        ("t  P  v", "post: tip · repost · vote in poll"),
        ("e  D", "post: edit · delete (your posts)"),
        ("x", "post: decrypt PGP message · send prompt: encrypt"),
        (
            "i  o  O",
            "post: info and versions · links · open on the web",
        ),
        ("m", "post: remove from realm (realm controllers)"),
        ("A Z X", "proposal: accept · reject · cancel"),
        ("", ""),
        (
            "f M B $ !",
            "user: follow · mute · block · send credits · report",
        ),
        ("A Z", "reported user: agree · disagree (stalwarts)"),
        ("J  E  M", "realm: join/leave · edit · mute"),
        ("f  M", "tag feed: follow · mute tag"),
        ("d D w", "inbox: dismiss · clear all · unwatch"),
        ("n", "proposals: new · realms: create"),
        ("q  Ctrl-C", "quit"),
        (
            "mouse",
            "click selects · click again opens · wheel scrolls · drag copies",
        ),
        ("", ""),
        (
            "",
            "Posts are written in $EDITOR. Top-level posts start with header lines",
        ),
        (
            "",
            "(Realm:, and for polls Deadline: and Option:) then an empty line.",
        ),
        (
            "",
            "Pictures: ![alt](/path/to/file.png) uploads to your storage.",
        ),
    ];
    let lines: Vec<Line> = keys
        .iter()
        .map(|(k, d)| {
            Line::from(vec![
                Span::styled(format!("{k:<22}"), key_style()),
                Span::raw(*d),
            ])
        })
        .collect();
    let r = centered(area, 92, lines.len() as u16 + 2);
    f.render_widget(Clear, r);
    f.render_widget(Paragraph::new(lines).block(popup_block("Keys")), r);
}
