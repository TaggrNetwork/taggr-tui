//! All rendering. Look and feel follow organiser and mailbox: ANSI colours only,
//! rounded panes and popups, key hints in the bottom line.

use super::form::FieldKind;
use super::page::{ACCENT, Kind, Ln, accent, dim, head, key_style};
use super::{App, Focus, Nav, Popup};
use crate::config::IconSet;
use ratatui::Frame;
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
            Kind::Ethereum => "\u{f42e}",
            Kind::Recovery => "\u{f1cd}",
            Kind::Links => "\u{f0c1}",
            Kind::Whitepaper => "\u{f15c}",
            _ => "",
        },
        Nav::SignIn => "\u{f090}",
        Nav::SignOut => "\u{f08b}",
        _ => "",
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
    if app.popup.is_some() {
        draw_popup(f, app, area);
    }
}

fn draw_nav(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::Nav;
    let title = match &app.user {
        Some(u) => format!(" @{} ", u.name),
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
    // two columns for the selection bar
    let w = inner.width.saturating_sub(2) as usize;
    if page.built != Some(inner.width) {
        page.entries = super::view::build(app, &page, w);
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
        let bar = if sel && focused {
            Span::styled("▌ ", Style::default().fg(ACCENT))
        } else if sel {
            Span::styled("▏ ", dim())
        } else {
            Span::raw("  ")
        };
        for l in &e.lines {
            let lh = match l {
                Ln::Text(_) => 1,
                Ln::Image(_, r, _) => *r as usize,
            };
            if y + lh <= page.off || y >= page.off + h {
                y += lh;
                continue;
            }
            let row = inner.y + (y.max(page.off) - page.off) as u16;
            match l {
                Ln::Text(t) => {
                    let mut sp = vec![bar.clone()];
                    sp.extend(t.spans.iter().cloned());
                    let mut line = Line::from(sp);
                    // one-line selectable rows get a full highlight
                    if sel && focused && e.lines.len() == 1 && e.target.is_some() {
                        line = line.style(Style::default().add_modifier(Modifier::REVERSED));
                        line.spans[0] = Span::styled("▌ ", Style::default().fg(ACCENT));
                    }
                    f.render_widget(
                        Paragraph::new(line),
                        Rect::new(inner.x, row, inner.width, 1),
                    );
                }
                Ln::Image(url, rows, alt) => {
                    let fully = y >= page.off && y + lh <= page.off + h;
                    for r in 0..lh {
                        let ry = y + r;
                        if ry >= page.off && ry < page.off + h {
                            let rr = inner.y + (ry - page.off) as u16;
                            f.render_widget(
                                Paragraph::new(Line::from(bar.clone())),
                                Rect::new(inner.x, rr, 2, 1),
                            );
                        }
                    }
                    if fully && !app.images.failed(url) {
                        imgs.push((
                            url.clone(),
                            Rect::new(inner.x + 2, row, w as u16, *rows),
                            alt.clone(),
                        ));
                    } else {
                        let label = if app.images.failed(url) {
                            format!("[picture {alt} (not loaded)]")
                        } else {
                            format!("[picture {alt}]")
                        };
                        f.render_widget(
                            Paragraph::new(Line::styled(label, dim())),
                            Rect::new(inner.x + 2, row, w as u16, 1),
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

fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    let left = match &app.status {
        Some((m, true, _)) => Line::styled(format!(" {m}"), Style::default().fg(Color::Red)),
        Some((m, false, _)) => Line::styled(format!(" {m}"), Style::default().fg(Color::Green)),
        None => {
            let kind = app.pages.last().map(|p| &p.kind);
            let post_sel = app.sel_post_id().is_some() || matches!(kind, Some(Kind::Post(_)));
            let keys: Vec<(&str, &str)> = if app.focus == Focus::Nav {
                vec![
                    ("j k", "move"),
                    ("Enter", "open"),
                    ("Tab", "page"),
                    ("/", "search"),
                    ("c", "post"),
                    ("?", "help"),
                    ("q", "quit"),
                ]
            } else if post_sel && app.user.is_some() {
                vec![
                    ("Enter", "open"),
                    ("Esc", "back"),
                    ("r", "reply"),
                    ("+", "react"),
                    ("b", "bookmark"),
                    ("o", "links"),
                    ("c", "post"),
                    ("[ ]", "tab"),
                    ("?", "help"),
                ]
            } else {
                vec![
                    ("j k", "move"),
                    ("Enter", "open"),
                    ("Esc", "back"),
                    ("Tab", "sidebar"),
                    ("s", "reload"),
                    ("/", "search"),
                    ("?", "help"),
                    ("q", "quit"),
                ]
            };
            let mut spans = vec![Span::raw(" ")];
            for (k, d) in keys {
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
    } else if let Some(u) = &app.user {
        let s = format!(
            "{} credits · {} rewards ",
            super::page::isep(u.cycles),
            super::page::isep(u.rewards)
        );
        if app.status.is_none() && area.width as usize > 100 {
            f.render_widget(Paragraph::new(Line::styled(s, dim()).right_aligned()), area);
        }
    }
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
            lines.push(Line::from(vec![
                Span::styled("Tab", key_style()),
                Span::styled(" next  ", dim()),
                Span::styled("Space", key_style()),
                Span::styled(" toggle  ", dim()),
                Span::styled("Enter", key_style()),
                Span::styled(format!(" next/{}  ", form.submit), dim()),
                Span::styled("^S", key_style()),
                Span::styled(format!(" {}  ", form.submit), dim()),
                Span::styled("Esc", key_style()),
                Span::styled(" cancel", dim()),
            ]));
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
            let r = crate::markdown::render(&d.body, iw);
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
            let keys = Line::from(vec![
                Span::styled("y", key_style()),
                Span::styled(" post  ", dim()),
                Span::styled("e", key_style()),
                Span::styled(" edit  ", dim()),
                Span::styled("Esc", key_style()),
                Span::styled(" keep for later  ", dim()),
                Span::styled("D", key_style()),
                Span::styled(" discard  ", dim()),
                Span::styled("j k", key_style()),
                Span::styled(" scroll", dim()),
            ]);
            f.render_widget(Paragraph::new(keys), k);
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
        ("j k  ↓ ↑", "select (long posts scroll first)"),
        ("g G  PgUp PgDn  Space", "top / bottom / page"),
        (
            "Enter  l",
            "open: post, user, realm, tag, notification, button",
        ),
        ("Esc  h", "back (to the sidebar on the first page)"),
        ("[ ]", "switch tabs (feeds, profiles, realms, proposals)"),
        ("s", "reload the page"),
        ("/", "search (in All realms: search realms)"),
        ("c  C", "new post / new poll (in the shown realm)"),
        ("", ""),
        ("r", "post: reply"),
        ("+", "post: react (cost and reward shown)"),
        ("b w H p", "post: bookmark · watch · hide · pin to profile"),
        ("t  P  v", "post: tip · repost · vote in poll"),
        ("e  D", "post: edit · delete (your posts)"),
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
        ("mouse", "click selects · click again opens · wheel scrolls"),
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
