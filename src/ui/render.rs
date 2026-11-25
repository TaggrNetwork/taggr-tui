use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};
use ratatui_image::protocol::StatefulProtocol;
use ratatui_image::{Resize, StatefulImage};

use crate::api::models::{Extension, PostItem};

use super::app::{App, ComposeKind, Screen};

pub fn draw(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(f.area());
    let body = chunks[0];
    let status = chunks[1];

    match app.screen {
        Screen::Login => draw_login(f, app, body),
        Screen::Timeline => draw_timeline(f, app, body),
        Screen::Reader => draw_reader(f, app, body),
        Screen::Profile => draw_profile(f, app, body),
        Screen::Search => draw_search(f, app, body),
    }
    draw_status(f, app, status);

    if app.help {
        draw_help(f, body);
    }
    if app.compose.is_some() {
        draw_compose(f, app, body);
    }
    if app.prompt.is_some() {
        draw_prompt(f, app, body);
    }
}

fn draw_login(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(35),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(1),
        ])
        .split(area);
    let title = Paragraph::new(format!("taggr-tui  ·  {}", app.config.canister_id))
        .alignment(Alignment::Center)
        .style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );
    f.render_widget(title, chunks[0]);

    let input = Paragraph::new(app.input.as_str()).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" seed phrase "),
    );
    f.render_widget(input, chunks[1]);
    let hint = Paragraph::new("Enter: sign in    Esc: back to browsing")
        .alignment(Alignment::Center)
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(hint, chunks[2]);
}

fn draw_timeline(f: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(area);
    let tabs = Line::from(vec![
        tab_span("latest", app.tab.title() == "latest"),
        Span::raw("  "),
        tab_span("hot", app.tab.title() == "hot"),
        Span::raw("  "),
        tab_span("personal", app.tab.title() == "personal"),
        Span::raw("   "),
        Span::styled(
            format!("[{}]", app.posts.len()),
            Style::default().fg(Color::DarkGray),
        ),
    ]);
    f.render_widget(Paragraph::new(tabs), chunks[0]);

    let items: Vec<ListItem> = app
        .posts
        .iter()
        .map(|item| ListItem::new(post_line(item, app.now_ns)))
        .collect();
    let mut state = ListState::default();
    state.select(Some(app.selected));
    let list = List::new(items)
        .highlight_style(
            Style::default()
                .bg(Color::Indexed(236))
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");
    f.render_stateful_widget(list, chunks[1], &mut state);
}

enum ReaderElement {
    Text(String, Style),
    Image {
        key: String,
        bucket: String,
        offset: u64,
        len: u64,
    },
}

fn draw_reader(f: &mut Frame, app: &mut App, area: Rect) {
    let item = app
        .reader
        .as_ref()
        .and_then(|r| r.posts.get(r.sel))
        .cloned();
    let Some(item) = item else {
        return;
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" #{} ", item.post.id));
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let elements = build_reader_elements(app, &item, inner.width);

    // Trigger fetches for every image referenced in the post.
    for el in &elements {
        if let ReaderElement::Image {
            key,
            bucket,
            offset,
            len,
        } = el
        {
            app.request_image(key.clone(), bucket.clone(), *offset, *len);
        }
    }

    let scroll = app.reader.as_ref().map(|r| r.scroll).unwrap_or(0) as usize;
    let bottom = inner.y + inner.height;
    let mut y = inner.y;
    let mut skipped = 0usize;
    for el in &elements {
        let h = match el {
            ReaderElement::Text(_, _) => 1,
            ReaderElement::Image { key, .. } => app.images.rows_for(key, inner.width),
        };
        if y >= bottom {
            break;
        }
        if skipped < scroll {
            skipped += h as usize;
            continue;
        }
        let rect = Rect {
            x: inner.x,
            y,
            width: inner.width,
            height: h.min(bottom - y),
        };
        match el {
            ReaderElement::Text(text, style) => {
                f.render_widget(
                    Paragraph::new(Line::from(Span::styled(text.clone(), *style))),
                    rect,
                );
            }
            ReaderElement::Image { key, .. } => {
                if let Some(proto) = app.images.get(key) {
                    f.render_stateful_widget(
                        StatefulImage::<StatefulProtocol>::new().resize(Resize::Fit(None)),
                        rect,
                        proto,
                    );
                } else if app.images.is_failed(key) {
                    f.render_widget(
                        Paragraph::new("image failed")
                            .alignment(Alignment::Center)
                            .style(Style::default().fg(Color::Red)),
                        rect,
                    );
                } else {
                    f.render_widget(
                        Paragraph::new("loading image…")
                            .alignment(Alignment::Center)
                            .style(Style::default().fg(Color::DarkGray)),
                        rect,
                    );
                }
            }
        }
        y += h;
    }
}

fn draw_profile(f: &mut Frame, app: &mut App, area: Rect) {
    let Some(profile) = &app.profile else {
        return;
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(5), Constraint::Min(1)])
        .split(area);
    let header = Text::from(vec![
        Line::from(Span::styled(
            format!("@{}  (#{})", profile.user.name, profile.user.id),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(format!(
            "followers {}   followees {}   posts {}   balance {}",
            profile.user.followers.len(),
            profile.user.followees.len(),
            profile.user.num_posts,
            profile.user.balance
        )),
        Line::from(profile.user.about.clone()),
    ]);
    f.render_widget(
        Paragraph::new(header).block(Block::default().borders(Borders::ALL).title(" profile ")),
        chunks[0],
    );

    let items: Vec<ListItem> = profile
        .posts
        .iter()
        .map(|item| ListItem::new(post_line(item, app.now_ns)))
        .collect();
    let mut state = ListState::default();
    state.select(Some(profile.selected));
    f.render_stateful_widget(
        List::new(items)
            .highlight_style(
                Style::default()
                    .bg(Color::Indexed(236))
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("> "),
        chunks[1],
        &mut state,
    );
}

fn draw_search(f: &mut Frame, app: &mut App, area: Rect) {
    let Some(search) = &app.search else {
        return;
    };
    let items: Vec<ListItem> = search
        .results
        .iter()
        .map(|r| {
            ListItem::new(Line::from(vec![
                Span::styled(format!("#{} ", r.id), Style::default().fg(Color::DarkGray)),
                Span::raw(r.result.clone()),
            ]))
        })
        .collect();
    let mut state = ListState::default();
    state.select(Some(search.selected));
    f.render_stateful_widget(
        List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!(" search: {} ", search.query)),
            )
            .highlight_style(
                Style::default()
                    .bg(Color::Indexed(236))
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("> "),
        area,
        &mut state,
    );
}

fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    let hints = match app.screen {
        Screen::Login => "enter: sign in  esc: quit",
        Screen::Timeline => {
            "j/k: move  enter: open  n: new  r: reply  p: repost  b/f/h: bookmark/follow/hide  P: pin  1-0: react  tab: feed  /: search  u: profile  L: sign in  ?: help  q: quit"
        }
        Screen::Reader => {
            "j/k: scroll  d/u: page  J/K: post  r: reply  p: repost  b/f/h/P  t: tip  v: vote  !: report  1-0: react  q: back"
        }
        Screen::Profile => "j/k: move  enter: open  q: back",
        Screen::Search => "j/k: move  enter: open  q: back",
    };
    let left = if let Some(toast) = &app.toast {
        Span::styled(format!(" {toast} "), Style::default().fg(Color::Yellow))
    } else {
        Span::styled(format!(" {hints} "), Style::default().fg(Color::DarkGray))
    };
    let right = if app.loading {
        Span::styled(" loading… ", Style::default().fg(Color::Cyan))
    } else {
        match &app.me {
            Some(u) => Span::styled(format!("@{} ", u.name), Style::default().fg(Color::Green)),
            None => Span::styled(
                "anonymous · L sign in ",
                Style::default().fg(Color::DarkGray),
            ),
        }
    };
    let line = Line::from(vec![left, Span::raw(" "), right]);
    f.render_widget(Paragraph::new(line), area);
}

fn draw_help(f: &mut Frame, area: Rect) {
    let popup = centered_rect(70, 80, area);
    f.render_widget(Clear, popup);
    let text = vec![
        Line::from("taggr-tui keys"),
        Line::from(""),
        Line::from("timeline:  j/k move · enter open · tab switch feed · R refresh"),
        Line::from("           n new · r reply · p repost · u own profile · / search"),
        Line::from("reader:    j/k scroll · d/u page · J/K select post · images inline"),
        Line::from("           r reply · p repost · t tip · v vote poll · ! report"),
        Line::from(
            "anywhere:  1-0 react · b bookmark · f follow · h hide · P pin · L sign in · ? help · q quit",
        ),
        Line::from("writes need sign-in; browsing is anonymous."),
        Line::from("compose:   type · tab switch body/options · ctrl-s submit · esc cancel"),
    ];
    let para = Paragraph::new(text)
        .block(Block::default().borders(Borders::ALL).title(" help "))
        .wrap(Wrap { trim: false });
    f.render_widget(para, popup);
}

fn draw_compose(f: &mut Frame, app: &App, area: Rect) {
    let Some(c) = &app.compose else {
        return;
    };
    let popup = centered_rect(80, 70, area);
    f.render_widget(Clear, popup);
    let title = match &c.kind {
        ComposeKind::New => " new post ".to_string(),
        ComposeKind::Reply(id) => format!(" reply to #{id} "),
        ComposeKind::Repost(id) => format!(" repost of #{id} "),
        ComposeKind::Poll => " new poll ".to_string(),
    };
    let body_label = if c.focus_options { "body" } else { "body*" };
    let mut text = vec![Line::from(Span::styled(
        format!("[{body_label}]  tab: switch  ctrl-s: submit  esc: cancel"),
        Style::default().fg(Color::DarkGray),
    ))];
    for l in c.body.lines() {
        text.push(Line::from(l.to_string()));
    }
    if c.kind_is_poll() {
        text.push(Line::from(""));
        let opt_label = if c.focus_options {
            "options* (one per line)"
        } else {
            "options (one per line)"
        };
        text.push(Line::from(Span::styled(
            opt_label.to_string(),
            Style::default().fg(Color::DarkGray),
        )));
        for l in c.options.lines() {
            text.push(Line::from(l.to_string()));
        }
    }
    let para = Paragraph::new(text)
        .block(Block::default().borders(Borders::ALL).title(title))
        .wrap(Wrap { trim: false });
    f.render_widget(para, popup);
}

fn draw_prompt(f: &mut Frame, app: &App, area: Rect) {
    let Some(p) = &app.prompt else {
        return;
    };
    let popup = centered_rect(70, 20, area);
    f.render_widget(Clear, popup);
    let input = Paragraph::new(p.value.as_str()).block(
        Block::default()
            .borders(Borders::ALL)
            .title(p.label.clone()),
    );
    f.render_widget(input, popup);
}

fn build_reader_elements(app: &App, item: &PostItem, width: u16) -> Vec<ReaderElement> {
    let mut els = Vec::new();
    let refs = crate::media::image_refs(&item.post);
    let by_id: std::collections::HashMap<&str, &crate::media::ImageRef> =
        refs.iter().map(|r| (r.blob_id.as_str(), r)).collect();

    let realm = match &item.post.realm {
        Some(r) => format!("  ·  {r}"),
        None => String::new(),
    };
    push_wrapped(
        &mut els,
        &format!(
            "@{}  ·  {} ago{}",
            item.author,
            fmt_age(item.post.age_secs(app.now_ns)),
            realm
        ),
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
        width,
    );
    push_wrapped(&mut els, "", Style::default(), width);

    for line in item.post.body.lines() {
        if let Some(id) = blob_id_of(line)
            && let Some(r) = by_id.get(id.as_str())
        {
            els.push(ReaderElement::Image {
                key: r.cache_key(),
                bucket: r.bucket.clone(),
                offset: r.offset,
                len: r.len,
            });
            continue;
        }
        push_wrapped(&mut els, line, Style::default(), width);
    }

    if let Some(Extension::Poll(poll)) = &item.post.extension {
        let total: usize = poll.votes.values().map(|v| v.len()).sum();
        push_wrapped(&mut els, "", Style::default(), width);
        push_wrapped(
            &mut els,
            &format!("poll · {} votes · deadline {}h", total, poll.deadline),
            Style::default().fg(Color::Yellow),
            width,
        );
        for (i, opt) in poll.options.iter().enumerate() {
            let votes = poll.votes.get(&(i as u16)).map(|v| v.len()).unwrap_or(0);
            let pct = (votes * 100).checked_div(total).unwrap_or(0);
            push_wrapped(
                &mut els,
                &format!("  [{i}] {opt}  {votes} ({pct}%)"),
                Style::default(),
                width,
            );
        }
    }

    if let Some(Extension::Repost(id)) = &item.post.extension {
        push_wrapped(
            &mut els,
            &format!("repost of #{id}"),
            Style::default().fg(Color::Blue),
            width,
        );
    }

    let reactions = crate::api::models::reaction_set()
        .into_iter()
        .filter_map(|(code, icon)| {
            let n = item.post.reaction_count(code);
            if n > 0 {
                Some(format!("{icon}{n}"))
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join("  ");
    push_wrapped(&mut els, "", Style::default(), width);
    push_wrapped(
        &mut els,
        &format!("{}   ·   {} replies", reactions, item.post.children.len()),
        Style::default().fg(Color::DarkGray),
        width,
    );

    if let Some(reader) = &app.reader
        && reader.posts.len() > 1
    {
        push_wrapped(&mut els, "", Style::default(), width);
        push_wrapped(
            &mut els,
            "── thread ──",
            Style::default().fg(Color::DarkGray),
            width,
        );
        for (i, p) in reader.posts.iter().enumerate() {
            let marker = if i == reader.sel { "▶" } else { " " };
            let style = if i == reader.sel {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            };
            push_wrapped(
                &mut els,
                &format!(
                    "{marker} #{} @{}  {}",
                    p.post.id,
                    p.author,
                    p.post.body.lines().next().unwrap_or("")
                ),
                style,
                width,
            );
        }
    }

    els
}

fn push_wrapped(els: &mut Vec<ReaderElement>, text: &str, style: Style, width: u16) {
    for line in wrap_text(text, width) {
        els.push(ReaderElement::Text(line, style));
    }
}

fn wrap_text(text: &str, width: u16) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }
    textwrap::wrap(text, width.max(1) as usize)
        .into_iter()
        .map(|c| c.into_owned())
        .collect()
}

/// Extract the blob id from a `/blob/<id>` reference in a markdown line.
fn blob_id_of(line: &str) -> Option<String> {
    let idx = line.find("/blob/")?;
    let rest = &line[idx + "/blob/".len()..];
    let end = rest.find([')', ' ', ']', '?']).unwrap_or(rest.len());
    let id = &rest[..end];
    if id.is_empty() {
        None
    } else {
        Some(id.to_string())
    }
}

fn post_line(item: &PostItem, now_ns: u64) -> Line<'static> {
    let img = if crate::media::has_images(&item.post) {
        " 📷"
    } else {
        ""
    };
    let nsfw = if item.nsfw { " NSFW" } else { "" };
    let snippet: String = item
        .post
        .body
        .lines()
        .next()
        .unwrap_or("")
        .chars()
        .take(120)
        .collect();
    Line::from(vec![
        Span::styled(
            format!("#{:<7} ", item.post.id),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(
            format!("@{:<16}", truncate(&item.author, 16)),
            Style::default().fg(Color::Cyan),
        ),
        Span::styled(
            format!("{:>6} ", fmt_age(item.post.age_secs(now_ns))),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(
            format!("♥{:<4}", item.post.total_reactions()),
            Style::default().fg(Color::Red),
        ),
        Span::styled(
            format!("↩{:<4}", item.post.children.len()),
            Style::default().fg(Color::Blue),
        ),
        Span::styled(img, Style::default().fg(Color::Magenta)),
        Span::styled(nsfw, Style::default().fg(Color::Red)),
        Span::raw(format!(" {snippet}")),
    ])
}

fn tab_span(name: &str, active: bool) -> Span<'static> {
    if active {
        Span::styled(
            format!(" {name} "),
            Style::default()
                .bg(Color::Cyan)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(format!(" {name} "), Style::default().fg(Color::DarkGray))
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}

fn fmt_age(secs: u64) -> String {
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else if secs < 86_400 {
        format!("{}h", secs / 3600)
    } else {
        format!("{}d", secs / 86_400)
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::Client;
    use crate::api::models::{Meta, Post, PostItem};
    use crate::config::{Config, Paths};
    use crate::ic::Ic;
    use crate::ui::app::{App, Reader, Screen};
    use crate::ui::images::ImageStore;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui_image::picker::Picker;
    use std::sync::Arc;

    fn sample_post(id: u64) -> PostItem {
        let post: Post = serde_json::from_value(serde_json::json!({
            "id": id,
            "body": "hello world\n![pic](/blob/abc123)\nsecond line",
            "user": 1,
            "timestamp": 0,
            "children": [],
            "parent": null,
            "tags": [],
            "reactions": {"10": [1, 2]},
            "patches": [],
            "files": {"abc123@bucket": [0, 10]},
            "tips": [],
            "extension": null,
            "realm": null,
            "hashes": [],
            "reposts": [],
            "encrypted": false,
            "hidden_for": []
        }))
        .unwrap();
        let meta: Meta =
            serde_json::from_value(serde_json::json!({"author_name": "alice"})).unwrap();
        PostItem::from_pair((post, meta))
    }

    #[tokio::test]
    async fn renders_timeline_and_reader() {
        let ic = Arc::new(
            Ic::new("https://ic0.app", "6qfxa-ryaaa-aaaai-qbhsq-cai", None)
                .await
                .unwrap(),
        );
        let client = Arc::new(Client::new(ic, "taggr.link".into(), "raw.icp0.io".into()));
        let paths = Paths {
            config_dir: std::env::temp_dir(),
            cache_dir: std::env::temp_dir(),
        };
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut images = ImageStore::new(Picker::halfblocks(), true);
        // Preload the referenced blob so the inline image path renders a real protocol.
        images.insert("bucket_0_10", image::DynamicImage::new_rgb8(8, 8));
        let mut app = App::new(
            client,
            paths,
            Config::default(),
            tx,
            images,
            true,
            "principal".into(),
        );
        app.screen = Screen::Timeline;
        app.posts = vec![sample_post(1), sample_post(2)];

        let mut term = Terminal::new(TestBackend::new(120, 40)).unwrap();
        term.draw(|f| draw(f, &mut app)).unwrap();

        app.screen = Screen::Reader;
        app.reader = Some(Reader {
            posts: app.posts.clone(),
            sel: 0,
            scroll: 0,
        });
        term.draw(|f| draw(f, &mut app)).unwrap();
    }
}
