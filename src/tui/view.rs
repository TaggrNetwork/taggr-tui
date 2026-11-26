//! Turns a loaded page into entries for the current width.

use super::App;
use super::page::*;
use crate::api::{self, Item};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use serde_json::Value;
use std::rc::Rc;

/// Entries plus where the tab bar is (for mouse clicks).
pub type Built = (Vec<Entry>, Option<(usize, Vec<(usize, usize)>)>);

fn act(f: impl Fn(&mut App) + 'static) -> Action2 {
    Rc::new(f)
}

fn more(o: &mut Out) {
    o.entry(Entry {
        lines: vec![Ln::Text(Line::styled("▸ load more", accent()))],
        target: Some(Target::More),
        key: "more".into(),
    });
}

/// Posts separated by rules.
fn posts(o: &mut Out, p: &Page) {
    for it in &p.items {
        let e = post_card(o.app, p, it, o.w, false);
        o.entry(e);
    }
    if p.more {
        o.rule();
        more(o);
    }
    if p.items.is_empty() && p.data.loaded {
        o.text(Line::styled("nothing here", dim()));
    }
}

fn tok(app: &App, v: u64) -> String {
    format!(
        "{} {}",
        amount(v, app.backend.token_decimals),
        app.backend.token_symbol
    )
}

pub fn build(app: &App, p: &Page, w: usize) -> Built {
    let mut o = Out::new(app, w);
    let o = &mut o;
    match &p.kind {
        Kind::Home | Kind::Hot | Kind::New | Kind::Bookmarks | Kind::Reposts(_) => posts(o, p),
        Kind::Tags(tags) => {
            if app.user.is_some() {
                let following = app
                    .user
                    .as_ref()
                    .is_some_and(|u| u.feeds.iter().any(|f| f == tags));
                let tags2 = tags.clone();
                let tag = tags.first().cloned().unwrap_or_default();
                o.actions(
                    "tags",
                    vec![
                        (
                            if following {
                                "unfollow feed (f)"
                            } else {
                                "follow feed (f)"
                            }
                            .into(),
                            act(move |app| app.toggle_feed(tags2.clone())),
                        ),
                        (
                            "mute tag (M)".into(),
                            act(move |app| app.toggle_filter("tag", tag.clone())),
                        ),
                    ],
                );
                o.rule();
            }
            posts(o, p)
        }
        Kind::Realm(id) => realm(o, p, id),
        Kind::Realms => realms(o, p),
        Kind::User(_) => user(o, p),
        Kind::Journal(h) => {
            match &p.data.user {
                Some(u) => {
                    o.heading(&format!("{}'s journal", u.name));
                    if !u.about.is_empty() {
                        o.md(&u.about);
                    }
                }
                None => o.heading(&format!("{h}'s journal")),
            }
            o.rule();
            posts(o, p)
        }
        Kind::Post(_) => thread(o, p),
        Kind::Inbox => inbox(o, p),
        Kind::Search(_) => search(o, p),
        Kind::Users(ids) => {
            let rows = ids
                .iter()
                .map(|id| {
                    (
                        format!("u:{id}"),
                        vec![(format!("@{}", app.name(*id)), Style::default().fg(ACCENT))],
                        Some(Target::UserId(*id)),
                    )
                })
                .collect();
            o.table(&[("user", false)], rows);
        }
        Kind::Proposals => {
            o.tab_bar(
                &[
                    "All",
                    "Release",
                    "ICP transfer",
                    "Realm controller",
                    "Funding",
                    "Rewards",
                ],
                p.tab,
            );
            if app.user.is_some() {
                o.actions(
                    "prop",
                    vec![(
                        "new proposal (n)".into(),
                        act(|app| app.new_proposal_menu()),
                    )],
                );
            }
            o.rule();
            posts(o, p)
        }
        Kind::Wallet => wallet(o, p),
        Kind::Invites => invites(o, p),
        Kind::Settings => settings(o, p),
        Kind::Welcome => welcome(o, p),
        Kind::Tokens => tokens(o, p),
        Kind::Transactions(owner, _) => {
            o.heading(&if owner.is_empty() {
                "All transactions".to_string()
            } else {
                format!("Transactions of {owner}")
            });
            txs(o, &p.data.txs);
            if p.more {
                more(o);
            }
        }
        Kind::Dashboard => dashboard(o, p),
        Kind::Distribution => distribution(o, p),
        Kind::Domains => domains(o, p),
        Kind::Links => links(o),
        Kind::Whitepaper => {
            if p.data.text.is_empty() {
                o.text(Line::styled("loading…", dim()));
            } else {
                o.md(&p.data.text);
            }
        }
    }
    let tabs = o.tabs.take();
    (o.finish(), tabs)
}

fn realm(o: &mut Out, p: &Page, id: &str) {
    let app = o.app;
    o.heading(&format!("/{id}"));
    if let Some(r) = &p.data.realm {
        if !r.description.is_empty() {
            o.md(r
                .description
                .split(crate::markdown::CUT)
                .next()
                .unwrap_or(""));
        }
        o.kv("Members", sep(r.num_members));
        o.kv("Posts", sep(r.num_posts));
        o.kv("Created", date(r.created));
        o.kv("Last update", ago(r.last_update));
        o.kv("Revenue", format!("{} credits", sep(r.revenue)));
        o.kv(
            "Clean-up penalty",
            format!("{} credits", sep(r.cleanup_penalty)),
        );
        o.kv("Adult content", if r.adult_content { "yes" } else { "no" });
        o.kv(
            "Comments filtering",
            if r.comments_filtering { "on" } else { "off" },
        );
        let f = &r.filter;
        if f.age_days > 0 || f.safe || f.balance > 0 || f.num_followers > 0 {
            o.kv(
                "Access",
                format!(
                    "age ≥ {}d, balance ≥ {}, followers ≥ {}{}",
                    f.age_days,
                    amount(f.balance, app.backend.token_decimals),
                    f.num_followers,
                    if f.safe { ", no open reports" } else { "" }
                ),
            );
        }
        o.users("ctl", "Controllers", &r.controllers, 2);
        o.users("wl", "Whitelist", &r.whitelist, 2);
    }
    let mut acts: Vec<(String, Action2)> = vec![];
    if let Some(r) = &p.data.realm
        && r.description.contains(crate::markdown::CUT)
    {
        let d = r.description.clone();
        acts.push((
            "rules".into(),
            act(move |app| app.text_popup("Realm description", &d)),
        ));
    }
    if let Some(u) = &app.user {
        let member = u.realms.iter().any(|r| r == id);
        let ctl = p
            .data
            .realm
            .as_ref()
            .is_some_and(|r| r.controllers.contains(&u.id));
        let (a, b, c, d) = (
            id.to_string(),
            id.to_string(),
            id.to_string(),
            id.to_string(),
        );
        acts.push((
            if member { "leave (J)" } else { "join (J)" }.into(),
            act(move |app| app.toggle_membership(a.clone())),
        ));
        acts.push((
            "new post here (c)".into(),
            act(move |app| app.new_post(Some(b.clone()))),
        ));
        acts.push((
            "mute (M)".into(),
            act(move |app| app.toggle_filter("realm", c.clone())),
        ));
        if ctl {
            acts.push((
                "edit (E)".into(),
                act(move |app| app.realm_form(Some(d.clone()))),
            ));
        }
    }
    o.actions("realm", acts);
    o.blank();
    o.tab_bar(&["New", "Hot"], p.tab);
    o.rule();
    posts(o, p);
}

fn realms(o: &mut Out, p: &Page) {
    // Signed in, the first tab is the user's own realms.
    let mine = o.app.user.is_some() && p.tab == 0;
    if o.app.user.is_some() {
        o.tab_bar(&["Mine", "Popularity", "Last update", "Name"], p.tab);
    } else {
        o.tab_bar(&["Popularity", "Last update", "Name"], p.tab);
    }
    let mut acts: Vec<(String, Action2)> =
        vec![("search (/)".into(), act(|app| app.realm_search_prompt()))];
    if !p.query.is_empty() {
        acts.push((
            format!("show all, not only “{}” (Esc)", p.query),
            act(|app| {
                if let Some(p) = app.page() {
                    p.query.clear();
                }
                app.reload();
            }),
        ));
    }
    if o.app.user.is_some() {
        acts.push(("create a realm (n)".into(), act(|app| app.realm_form(None))));
    }
    o.actions("realms", acts);
    o.blank();
    let me = o.app.user.as_ref();
    let is_member = |id: &String| me.is_some_and(|u| u.realms.contains(id));
    if mine && p.data.loaded && !p.data.realms.iter().any(|(id, _)| is_member(id)) {
        o.text(Line::styled(
            if p.query.is_empty() {
                "You have not joined any realms yet: find some under Popularity (2)."
            } else {
                "None of your realms match."
            },
            dim(),
        ));
        return;
    }
    let rows = p
        .data
        .realms
        .iter()
        // a realm just left drops out of Mine once the user reloads
        .filter(|(id, _)| !mine || is_member(id))
        .map(|(id, r)| {
            let member = is_member(id);
            (
                format!("r:{id}"),
                vec![
                    (
                        format!("/{id}"),
                        Style::default().fg(WARN).add_modifier(Modifier::BOLD),
                    ),
                    (
                        if member { "✓".into() } else { String::new() },
                        Style::default().fg(GOOD),
                    ),
                    (sep(r.num_members), Style::default()),
                    (sep(r.num_posts), Style::default()),
                    (ago(r.last_update), dim()),
                    (
                        crate::markdown::first_line(&r.description),
                        Style::default(),
                    ),
                ],
                Some(Target::Realm(id.clone())),
            )
        })
        .collect();
    o.table(
        &[
            ("realm", false),
            ("", false),
            ("members", true),
            ("posts", true),
            ("updated", true),
            ("description", false),
        ],
        rows,
    );
    if p.more {
        more(o);
    }
}

fn user(o: &mut Out, p: &Page) {
    let app = o.app;
    let Some(u) = &p.data.user else {
        o.text(Line::styled("loading…", dim()));
        return;
    };
    let mut l = vec![Span::styled(format!("@{}", u.name), accent())];
    let mut label = |t: &str, st: Style| l.push(Span::styled(format!("  {t}"), st));
    if u.timestamp < 1_672_500_000_000_000_000 {
        label("OG", Style::default().fg(WARN));
    }
    if u.stalwart {
        label("Stalwart", Style::default().fg(BAD));
    }
    if u.active_weeks > 12 {
        label("Frequenter", Style::default().fg(HEAD));
    }
    if app
        .user
        .as_ref()
        .is_some_and(|me| me.id != u.id && u.followees.contains(&me.id))
    {
        label("Follows you", Style::default().fg(GOOD));
    }
    let weeks = app
        .backend
        .rest
        .get("voting_power_activity_weeks")
        .and_then(|v| v.as_u64())
        .unwrap_or(4);
    if now_ns().saturating_sub(u.last_activity) / 86_400_000_000_000 > weeks * 7 {
        label("Inactive", dim());
    }
    if u.deactivated {
        label("deactivated", dim());
    }
    o.text(Line::from(l));
    if !u.previous_names.is_empty() {
        o.note(&format!("aka {}", u.previous_names.join(", ")));
    }
    if !u.about.is_empty() {
        o.md(&u.about);
    }
    if let Some(r) = &u.report
        && !r.closed
    {
        let mut inner = vec![Ln::Text(Line::styled(
            "This user was REPORTED.",
            Style::default().fg(BAD).add_modifier(Modifier::BOLD),
        ))];
        inner.extend(blocks(
            app,
            crate::markdown::render(&r.reason, o.w.saturating_sub(4)).blocks,
            o.w,
        ));
        inner.push(Ln::Text(Line::styled(
            format!(
                "reporter @{} · confirmed by {} · rejected by {}",
                app.name(r.reporter),
                r.confirmed_by.len(),
                r.rejected_by.len()
            ),
            dim(),
        )));
        let lines = boxed("report", inner, o.w, Style::default().fg(BAD));
        o.entry(Entry {
            lines,
            target: None,
            key: String::new(),
        });
        if app.user.as_ref().is_some_and(|me| me.stalwart) {
            let (a, b) = (u.id, u.id);
            o.actions(
                "report",
                vec![
                    ("agree (A)".into(), act(move |app| app.vote_report(a, true))),
                    (
                        "disagree (Z)".into(),
                        act(move |app| app.vote_report(b, false)),
                    ),
                ],
            );
        }
    }
    if let Some(me) = &app.user
        && u.blacklist.contains(&me.id)
    {
        o.text(Line::styled(
            "This user has blocked you.",
            Style::default().fg(BAD),
        ));
    }
    o.blank();
    o.kv("Tokens", tok(app, u.balance));
    if u.cold_balance > 0 {
        o.kv("Cold wallet", tok(app, u.cold_balance));
    }
    o.kv("Rewards", isep(u.rewards));
    o.kv("Credits", isep(u.cycles));
    o.kv("Joined", date(u.timestamp));
    o.kv("Last activity", ago(u.last_activity));
    o.kv("Active weeks", u.active_weeks.to_string());
    o.kv("Posts", sep(u.num_posts));
    if let Some(i) = u.invited_by {
        o.kv("Invited by", format!("@{}", app.name(i)));
    }
    if let Some(pgp) = u.settings.get("pgp").filter(|s| !s.is_empty()) {
        o.kv(
            "PGP key",
            pgp.lines()
                .find(|l| !l.starts_with("-----") && !l.is_empty())
                .unwrap_or(pgp)
                .to_string(),
        );
    }
    if !u.feeds.is_empty() {
        let f: Vec<String> = u
            .feeds
            .iter()
            .map(|f| {
                f.iter()
                    .map(|t| format!("#{t}"))
                    .collect::<Vec<_>>()
                    .join("+")
            })
            .collect();
        o.kv("Follows feeds", f.join(" "));
    }
    if !u.realms.is_empty() {
        o.kv(
            "Realms",
            u.realms
                .iter()
                .map(|r| format!("/{r}"))
                .collect::<Vec<_>>()
                .join(" "),
        );
    }
    if !u.controlled_realms.is_empty() {
        o.kv(
            "Controls",
            u.controlled_realms
                .iter()
                .map(|r| format!("/{r}"))
                .collect::<Vec<_>>()
                .join(" "),
        );
    }
    o.users("follows", "Follows", &u.followees, 2);
    o.users("followers", "Followers", &u.followers, 2);
    let mut acts: Vec<(String, Action2)> = vec![];
    let (principal, name) = (u.principal.clone(), u.name.clone());
    acts.push((
        "transactions".into(),
        act(move |app| {
            app.push(
                Kind::Transactions(principal.clone(), String::new()),
                &format!("@{name} transactions"),
            )
        }),
    ));
    let name = u.name.clone();
    acts.push((
        "journal".into(),
        act(move |app| app.push(Kind::Journal(name.clone()), &format!("{name}'s journal"))),
    ));
    if !u.accounting.is_empty() {
        let acc = u.accounting.clone();
        acts.push((
            format!("accounting ({})", acc.len()),
            act(move |app| {
                let mut md = String::from("| when | type | delta | log |\n|-|-|-:|-|\n");
                for (t, kind, d, log) in &acc {
                    md += &format!(
                        "| {} | {} | {} | {} |\n",
                        ago(*t),
                        kind,
                        d,
                        log.replace('|', "/")
                    );
                }
                app.text_popup("Rewards and credits accounting", &md);
            }),
        ));
    }
    if let Some(me) = &app.user
        && me.id != u.id
    {
        let id = u.id;
        let follows = me.followees.contains(&id);
        let muted = me.filters.users.contains(&id);
        let blocked = me.blacklist.contains(&id);
        acts.push((
            if follows {
                "unfollow (f)"
            } else {
                "follow (f)"
            }
            .into(),
            act(move |app| app.toggle_follow(id)),
        ));
        acts.push((
            if muted { "unmute (M)" } else { "mute (M)" }.into(),
            act(move |app| app.toggle_filter("user", id.to_string())),
        ));
        acts.push((
            if blocked { "unblock (B)" } else { "block (B)" }.into(),
            act(move |app| app.toggle_block(id)),
        ));
        acts.push((
            "send credits ($)".into(),
            act(move |app| app.send_credits_form(id)),
        ));
        acts.push(("report (!)".into(), act(move |app| app.report_form(id))));
    }
    o.actions("user", acts);
    o.blank();
    o.tab_bar(&["Posts", "Mentions", "Rewarded"], p.tab);
    o.rule();
    posts(o, p);
}

fn post_actions(o: &mut Out, it: &Item) {
    let app = o.app;
    let id = it.post.id;
    let mine = it.post.user == app.me();
    let mut a: Vec<(String, Action2)> = vec![];
    if app.user.is_some() {
        a.push(("reply (r)".into(), act(move |app| app.reply(id))));
        a.push(("react (+)".into(), act(move |app| app.react_menu(id))));
        let bm = app.user.as_ref().is_some_and(|u| u.bookmarks.contains(&id));
        a.push((
            if bm { "unbookmark (b)" } else { "bookmark (b)" }.into(),
            act(move |app| app.toggle_bookmark(id)),
        ));
        let w = it.post.watchers.contains(&app.me());
        a.push((
            if w { "unwatch (w)" } else { "watch (w)" }.into(),
            act(move |app| app.toggle_watch(id)),
        ));
        a.push(("repost (P)".into(), act(move |app| app.repost(id))));
        if it.post.poll().is_some() {
            a.push(("vote (v)".into(), act(move |app| app.poll_vote(id))));
        }
        if mine {
            a.push(("edit (e)".into(), act(move |app| app.edit_post(id))));
            a.push(("pin (p)".into(), act(move |app| app.toggle_pin(id))));
            a.push(("delete (D)".into(), act(move |app| app.delete_post(id))));
        } else {
            a.push(("tip (t)".into(), act(move |app| app.tip_form(id))));
            a.push(("hide (H)".into(), act(move |app| app.toggle_hide(id))));
        }
    }
    if !it.post.files.is_empty() {
        a.push((
            "pictures (I)".into(),
            act(move |app| app.toggle_pictures(id)),
        ));
    }
    a.push(("links (o)".into(), act(move |app| app.links_menu(id))));
    a.push((
        "info & versions (i)".into(),
        act(move |app| app.post_info(id)),
    ));
    o.actions(&format!("post{id}"), a);
}

fn thread(o: &mut Out, p: &Page) {
    let app = o.app;
    for it in &p.data.parents {
        let l = Line::from(vec![
            Span::styled("↑ ", dim()),
            Span::styled(format!("@{} ", it.meta.author_name), accent()),
            Span::raw(fit(
                &crate::markdown::first_line(&it.post.body),
                o.w.saturating_sub(it.meta.author_name.len() + 4),
            )),
        ]);
        o.row(format!("p:{}", it.post.id), l, Target::Post(it.post.id));
    }
    let Some(main) = p.items.first() else {
        if p.loading {
            o.text(Line::styled("loading…", dim()));
        }
        return;
    };
    if !p.data.parents.is_empty() {
        o.rule();
    }
    let e = post_card(app, p, main, o.w, true);
    o.entry(e);
    post_footer(o, main);
    post_actions(o, main);
    if !p.data.json.is_null() {
        proposal(o, &p.data.json);
    }
    let n = p.items.len() - 1;
    if n > 0 {
        o.heading(&format!("{n} comment{}", if n == 1 { "" } else { "s" }));
    }
    for it in p.items.iter().skip(1) {
        let d = p.data.depth.get(&it.post.id).copied().unwrap_or(1).min(8);
        // a guide line between comments keeps the tree readable
        o.text(Line::styled("│ ".repeat(d.saturating_sub(1)), dim()));
        let e = post_entry(app, p, it, o.w, true, d.saturating_sub(1));
        o.entry(e);
    }
}

fn post_footer(o: &mut Out, it: &Item) {
    let app = o.app;
    let post = &it.post;
    let mut info = vec![datetime(post.timestamp)];
    if !post.patches.is_empty() {
        info.push(format!("{} versions", post.patches.len() + 1));
    }
    if !post.tips.is_empty() {
        let total: u64 = post.tips.iter().map(|t| t.1).sum();
        info.push(format!("tips: {}", tok(app, total)));
    }
    o.note(&info.join(" · "));
    if !post.reposts.is_empty() {
        o.row(
            "reposts",
            Line::styled(format!("▸ {} reposts", post.reposts.len()), accent()),
            Target::Page(Kind::Reposts(post.id), format!("reposts of #{}", post.id)),
        );
    }
}

fn proposal(o: &mut Out, pr: &Value) {
    let app = o.app;
    let dec = app.backend.token_decimals;
    o.heading(&format!("Proposal #{}", pr["id"]));
    let status = pr["status"].as_str().unwrap_or("?").to_string();
    o.kv("Status", status.clone());
    o.kv(
        "Proposer",
        format!("@{}", app.name(pr["proposer"].as_u64().unwrap_or(0))),
    );
    let payload = &pr["payload"];
    if let Some(r) = payload.get("Release") {
        o.kv("Type", "Release");
        o.kv("Commit", r["commit"].as_str().unwrap_or("").to_string());
        if let Some(h) = r["hash"].as_str().filter(|h| !h.is_empty()) {
            o.kv("Build hash", h.to_string());
        }
    } else if let Some(a) = payload.get("AddRealmController") {
        o.kv("Type", "Add realm controller");
        o.kv("Realm", format!("/{}", a[0].as_str().unwrap_or("")));
        o.kv(
            "New controller",
            format!("@{}", app.name(a[1].as_u64().unwrap_or(0))),
        );
    } else if let Some(t) = payload.get("ICPTransfer") {
        o.kv("Type", "ICP transfer");
        o.kv("Receiver", api::account_hex(&t[0]));
        o.kv(
            "Amount",
            format!("{} ICP", icp(t[1]["e8s"].as_u64().unwrap_or(0))),
        );
    } else if let Some(f) = payload.get("Funding") {
        o.kv("Type", "Funding");
        o.kv("Receiver", f[0].as_str().unwrap_or("").to_string());
        o.kv("Amount", tok(app, f[1].as_u64().unwrap_or(0)));
    } else if let Some(r) = payload.get("Rewards") {
        o.kv("Type", "Rewards");
        o.kv("Receiver", r["receiver"].as_str().unwrap_or("").to_string());
        if status == "Executed" {
            o.kv("Minted", tok(app, r["minted"].as_u64().unwrap_or(0)));
        }
    }
    let vp = pr["voting_power"].as_u64().unwrap_or(0).max(1);
    let (mut yes, mut no, mut voted) = (0u64, 0u64, false);
    let (mut yes_ids, mut no_ids) = (vec![], vec![]);
    for v in pr["bulletins"].as_array().into_iter().flatten() {
        let uid = v[0].as_u64().unwrap_or(0);
        voted |= uid == app.me();
        if v[1].as_bool() == Some(true) {
            yes += v[2].as_u64().unwrap_or(0);
            yes_ids.push(uid);
        } else {
            no += v[2].as_u64().unwrap_or(0);
            no_ids.push(uid);
        }
    }
    o.kv("Voting power", amount(vp, dec));
    o.kv(
        "Accepted",
        format!("{} ({}%)", amount(yes, dec), yes * 100 / vp),
    );
    o.kv(
        "Rejected",
        format!("{} ({}%)", amount(no, dec), no * 100 / vp),
    );
    o.users("yes", "✓ accepted by", &yes_ids, 3);
    o.users("no", "✗ rejected by", &no_ids, 3);
    if status == "Open" && app.user.is_some() {
        let mut a: Vec<(String, Action2)> = vec![];
        if !voted {
            a.push(("accept (A)".into(), act(|app| app.vote_proposal(true))));
            a.push(("reject (Z)".into(), act(|app| app.vote_proposal(false))));
        }
        if pr["proposer"].as_u64() == Some(app.me()) {
            a.push(("cancel (X)".into(), act(|app| app.cancel_proposal())));
        }
        o.actions("proposal", a);
    }
}

fn inbox(o: &mut Out, p: &Page) {
    let app = o.app;
    let Some(u) = &app.user else { return };
    let mut fresh: Vec<_> = u
        .notifications
        .iter()
        .filter(|(_, (_, read))| !read)
        .collect();
    fresh.reverse();
    let mut old: Vec<_> = u
        .notifications
        .iter()
        .filter(|(_, (_, read))| *read)
        .collect();
    old.reverse();
    o.actions(
        "inbox",
        vec![
            (
                "clear all (D)".into(),
                act(|app| {
                    let ids: Vec<u64> = app
                        .user
                        .as_ref()
                        .map(|u| u.notifications.keys().copied().collect())
                        .unwrap_or_default();
                    app.confirm("Clear all notifications?", move |app| {
                        app.dismiss(ids.clone(), true)
                    });
                }),
            ),
            (
                if p.tab == 0 {
                    format!("show archive ({})", old.len())
                } else {
                    "hide archive".into()
                },
                act(|app| {
                    if let Some(p) = app.page() {
                        p.tab = 1 - p.tab.min(1);
                        p.dirty();
                    }
                }),
            ),
        ],
    );
    o.rule();
    if fresh.is_empty() {
        o.text(Line::styled("Inbox zero achieved! 🚀", head()));
    }
    let mut lists = vec![(fresh, false)];
    if p.tab == 1 {
        lists.push((old, true));
    }
    for (list, archived) in lists {
        if archived && !list.is_empty() {
            o.heading("Archive");
        }
        for (id, (n, _)) in list {
            let (msg, post) = match n {
                api::Notification::Generic(m) => (m.clone(), None),
                api::Notification::NewPost(m, pid) => (m.clone(), Some(*pid)),
                api::Notification::Conditional(m, pred) => (
                    m.clone(),
                    match pred {
                        api::Predicate::ReportOpen(i) | api::Predicate::Proposal(i) => Some(*i),
                        api::Predicate::UserReportOpen(_) => None,
                    },
                ),
                api::Notification::WatchedPostEntries(pid, ids) => (
                    format!(
                        "`{}` new thread update(s) on a watched post: {}",
                        ids.len(),
                        ids.iter()
                            .map(|i| format!("#{i}"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    Some(*pid),
                ),
            };
            let r = crate::markdown::render(&msg, o.w.saturating_sub(2));
            let mut lines = indent(blocks(app, r.blocks, o.w), &[Span::raw("  ")]);
            if !archived && let Some(Ln::Text(l)) = lines.first_mut() {
                l.spans[0] = Span::styled("● ", Style::default().fg(WARN));
            }
            if let Some(pid) = post
                && let Some(it) = p.items.iter().find(|i| i.post.id == pid)
            {
                let e = post_entry(app, p, it, o.w.saturating_sub(4), false, 0);
                let inner: Vec<Ln> = e.lines.into_iter().take(6).collect();
                lines.extend(indent(inner, &[Span::raw("  "), Span::styled("▏ ", dim())]));
            }
            o.entry(Entry {
                lines,
                target: Some(Target::Notification(*id)),
                key: format!("n:{id}"),
            });
            o.rule();
        }
    }
}

fn search(o: &mut Out, p: &Page) {
    o.actions(
        "search",
        vec![("new search (/)".into(), act(|app| app.search_prompt()))],
    );
    o.note("Terms: @user, #tag, /realm or words.");
    if !p.query.is_empty() {
        o.heading(&format!("Results for “{}”", p.query));
    }
    let rows = p
        .data
        .results
        .iter()
        .map(|r| {
            let (label, t) = match r.result.as_str() {
                "user" => (format!("@{}", o.app.name(r.id)), Target::UserId(r.id)),
                "tag" => (format!("#{}", r.relevant), Target::Tag(r.relevant.clone())),
                "realm" => (
                    format!("/{}", r.generic_id),
                    Target::Realm(r.generic_id.clone()),
                ),
                _ => (
                    format!("#{} @{}", r.id, o.app.name(r.user_id)),
                    Target::Post(r.id),
                ),
            };
            let rel = if r.result == "tag" {
                String::new()
            } else {
                r.relevant.replace('\n', " ")
            };
            (
                format!("s:{}:{}:{}", r.result, r.id, r.generic_id),
                vec![
                    (r.result.clone(), dim()),
                    (label, accent()),
                    (rel, Style::default()),
                ],
                Some(t),
            )
        })
        .collect();
    if !p.data.results.is_empty() {
        o.table(&[("kind", false), ("match", false), ("", false)], rows);
    } else if p.data.loaded {
        o.text(Line::styled("no results", dim()));
    }
}

fn wallet(o: &mut Out, p: &Page) {
    let app = o.app;
    let Some(u) = &app.user else { return };
    if u.cycles <= 200 {
        o.text(Line::styled(
            "You are low on credits! Transfer ICP to your account below and mint credits.",
            Style::default().fg(WARN),
        ));
    }
    o.kv("Principal", u.principal.clone());
    o.kv("ICP account", u.account.clone());
    o.heading("ICP");
    o.kv(
        "Balance",
        p.data
            .numbers
            .get("icp")
            .map(|v| format!("{} ICP", icp(*v)))
            .unwrap_or_else(|| "…".into()),
    );
    o.kv("Rewards", format!("{} points", isep(u.rewards)));
    if u.treasury_e8s > 0 {
        o.kv("Treasury", format!("{} ICP", icp(u.treasury_e8s)));
    }
    o.actions(
        "icp",
        vec![
            ("send ICP".into(), act(|app| app.send_icp_form())),
            ("withdraw rewards".into(), act(|app| app.withdraw_rewards())),
        ],
    );
    o.heading("Credits");
    o.kv("Available", isep(u.cycles));
    o.actions(
        "credits",
        vec![("mint credits".into(), act(|app| app.mint_credits_form()))],
    );
    o.heading(&app.backend.token_symbol.clone());
    o.kv("Wallet", tok(app, u.balance));
    if let Some(c) = &u.cold_wallet {
        o.kv("Cold wallet", format!("{} ({c})", tok(app, u.cold_balance)));
    }
    let principal = u.principal.clone();
    let mut a: Vec<(String, Action2)> = vec![
        (
            "send tokens".into(),
            act(|app| {
                let sym = app.backend.token_symbol.clone();
                let (dec, fee) = (app.backend.token_decimals, app.backend.transaction_fee);
                let c = app.cfg.canister.clone();
                app.send_tokens_form(c, sym, dec, fee);
            }),
        ),
        (
            "transactions".into(),
            act(move |app| {
                app.push(
                    Kind::Transactions(principal.clone(), String::new()),
                    "My transactions",
                )
            }),
        ),
    ];
    if u.cold_wallet.is_some() {
        a.push((
            "unlink cold wallet".into(),
            act(|app| app.unlink_cold_wallet()),
        ));
    }
    o.actions("tokens", a);
    if u.cold_wallet.is_none() {
        o.note("Linking a cold wallet needs the Plug browser extension: use the web app.");
    }
    if u.settings.get("icrcWallet").is_some_and(|s| s == "true") {
        o.heading("ICRC tokens");
        let rows = p
            .data
            .tokens
            .iter()
            .map(|(c, sym, d, fee, bal)| {
                let (c2, sym2, d2, fee2, c3) = (c.clone(), sym.clone(), *d, *fee, c.clone());
                (
                    format!("tok:{c}"),
                    vec![
                        (sym.clone(), accent()),
                        (amount(*bal, *d), Style::default()),
                        (c.clone(), dim()),
                    ],
                    Some(run(move |app: &mut App| {
                        let (c2, sym2, c3) = (c2.clone(), sym2.clone(), c3.clone());
                        app.popup = Some(super::Popup::Menu(
                            super::Menu::new(sym2.clone())
                                .add("Send", move |app| app.send_tokens_form(c2, sym2, d2, fee2))
                                .add("Remove from wallet", move |app| app.remove_wallet_token(c3)),
                        ))
                    })),
                )
            })
            .collect();
        o.table(
            &[("token", false), ("balance", true), ("ledger", false)],
            rows,
        );
        o.actions(
            "icrc",
            vec![("add token".into(), act(|app| app.add_wallet_token_form()))],
        );
    }
}

fn invites(o: &mut Out, p: &Page) {
    let app = o.app;
    o.note(&format!(
        "Invite people by giving them a code; the credits you put in are transferred to each new user (at least {} credits needed).",
        app.backend.min_credits_for_inviting
    ));
    o.actions(
        "inv",
        vec![("create invite".into(), act(|app| app.invite_form(None)))],
    );
    o.heading("Your invites");
    if p.data.invites.is_empty() && p.data.loaded {
        o.text(Line::styled("none", dim()));
        return;
    }
    let rows = p
        .data
        .invites
        .iter()
        .map(|(code, inv)| {
            let joined: Vec<String> = inv
                .joined_user_ids
                .iter()
                .map(|i| format!("@{}", app.name(*i)))
                .collect();
            (
                format!("inv:{code}"),
                vec![
                    (code.clone(), accent()),
                    (isep(inv.credits), Style::default()),
                    (isep(inv.credits_per_user), Style::default()),
                    (
                        inv.realm_id
                            .clone()
                            .map(|r| format!("/{r}"))
                            .unwrap_or_default(),
                        Style::default().fg(WARN),
                    ),
                    (joined.join(" "), dim()),
                ],
                Some(Target::Invite(code.clone())),
            )
        })
        .collect();
    o.table(
        &[
            ("code", false),
            ("credits left", true),
            ("per user", true),
            ("realm", false),
            ("joined", false),
        ],
        rows,
    );
    o.note(&format!(
        "Enter edits an invite. Invite links: {}",
        app.web_url("welcome/<code>")
    ));
}

fn settings(o: &mut Out, p: &Page) {
    let app = o.app;
    let Some(u) = &app.user else { return };
    o.heading("Profile");
    o.kv("Name", u.name.clone());
    o.kv("Usage mode", u.mode.clone());
    o.kv("Governance", if u.governance { "yes" } else { "no" });
    o.kv(
        "PGP key",
        if u.settings.get("pgp").is_some_and(|s| !s.is_empty()) {
            "set"
        } else {
            "—"
        },
    );
    o.kv(
        "ICRC tokens in wallet",
        u.settings
            .get("icrcWallet")
            .cloned()
            .unwrap_or("false".into()),
    );
    o.kv("About", u.about.clone());
    o.actions(
        "profile",
        vec![("edit profile".into(), act(|app| app.profile_form()))],
    );
    o.heading("Noise filter");
    o.note("Posts and comments from users not passing this filter are hidden from you, and they cannot reply to your posts.");
    let f = &u.filters.noise;
    o.kv("Min. account age", format!("{} days", f.age_days));
    o.kv(
        "Min. token balance",
        amount(f.balance, app.backend.token_decimals),
    );
    o.kv("Min. followers", f.num_followers.to_string());
    o.kv("Non-controversial only", if f.safe { "yes" } else { "no" });
    o.kv(
        "Followed, other realms",
        if u.show_posts_in_realms {
            "shown"
        } else {
            "hidden"
        },
    );
    o.actions(
        "filter",
        vec![("edit filter".into(), act(|app| app.filter_form()))],
    );
    let lists: [(&str, &'static str, Vec<String>); 4] = [
        (
            "Muted users",
            "user",
            u.filters.users.iter().map(|i| i.to_string()).collect(),
        ),
        (
            "Blocked users",
            "blacklist",
            u.blacklist.iter().map(|i| i.to_string()).collect(),
        ),
        ("Muted tags", "tag", u.filters.tags.clone()),
        ("Muted realms", "realm", u.filters.realms.clone()),
    ];
    for (title, kind, items) in lists {
        if items.is_empty() {
            continue;
        }
        o.heading(&format!("{title} · x: remove"));
        let rows = items
            .into_iter()
            .map(|v| {
                let label = match kind {
                    "user" | "blacklist" => format!("@{}", app.name(v.parse().unwrap_or(0))),
                    "tag" => format!("#{v}"),
                    _ => format!("/{v}"),
                };
                (
                    format!("{kind}:{v}"),
                    vec![(label, Style::default())],
                    Some(Target::Listed(kind, v)),
                )
            })
            .collect();
        o.table(&[("", false)], rows);
    }
    o.heading("Storage");
    match &u.bucket {
        Some(b) => {
            o.kv("Storage canister", b.clone());
            let s = &p.data.json;
            let mut a: Vec<(String, Action2)> =
                vec![("top up".into(), act(|app| app.top_up_form()))];
            if !s.is_null() {
                o.kv("Status", s["status"].as_str().unwrap_or("?").to_string());
                o.kv("Cycles", sep(s["cycles"].as_u64().unwrap_or(0)));
                o.kv(
                    "Memory",
                    format!("{} MB", s["memory"].as_u64().unwrap_or(0) / 1_000_000),
                );
                if let Some(d) = s["days"].as_u64() {
                    o.kv("Runs for", format!("~{d} days"));
                }
                if let Some(c) = s["controllers"].as_array() {
                    o.kv(
                        "Controllers",
                        c.iter()
                            .filter_map(|v| v.as_str())
                            .collect::<Vec<_>>()
                            .join(", "),
                    );
                }
                if s["stale"].as_bool() == Some(true) {
                    o.text(Line::styled(
                        "The storage code is outdated.",
                        Style::default().fg(WARN),
                    ));
                    a.push(("upgrade storage".into(), act(|app| app.upgrade_bucket())));
                }
                let bh = s["controllers"].as_array().is_some_and(|c| {
                    c.iter()
                        .any(|v| v.as_str() == Some(crate::storage::BLACKHOLE))
                });
                if !bh {
                    a.push((
                        "add blackhole controller".into(),
                        act(|app| app.add_blackhole()),
                    ));
                }
            }
            o.actions("storage", a);
        }
        None => {
            o.note("Pictures are stored in your own storage canister (a “bucket”), paid with ICP from your wallet.");
            o.actions(
                "storage",
                vec![("create storage".into(), act(|app| app.create_bucket_form()))],
            );
        }
    }
    o.heading("Account");
    o.note("Suspending encrypts all your posts with a password; reactivating decrypts them.");
    if !p.data.text.is_empty() {
        o.note(&format!(
            "Principal change requested: sign in with {} to confirm it.",
            p.data.text
        ));
    }
    o.actions(
        "account",
        vec![
            (
                if u.deactivated {
                    "activate"
                } else {
                    "deactivate"
                }
                .into(),
                act(|app| app.crypt_form()),
            ),
            (
                "change principal".into(),
                act(|app| app.principal_change_form()),
            ),
        ],
    );
}

fn welcome(o: &mut Out, _p: &Page) {
    let app = o.app;
    o.heading("New user detected");
    o.kv("Your principal", app.principal.clone());
    o.note("This principal has no account yet. Join with an invite code, or pay for credits with ICP, then pick a name.");
    o.button("Join with an invite", "", |app: &mut App| {
        app.join_form(String::new())
    });
    o.button(
        "Mint credits with ICP",
        "shows where to send ICP",
        |app: &mut App| app.welcome_mint(),
    );
    o.button("Register (after paying)", "", |app: &mut App| {
        app.join_form(String::new())
    });
    o.button(
        "Confirm principal change",
        "if you requested one from your old principal",
        |app: &mut App| app.confirm_principal_change(),
    );
}

fn tokens(o: &mut Out, p: &Page) {
    let app = o.app;
    let s = &p.data.json;
    let dec = app.backend.token_decimals;
    let n = |k: &str| s[k].as_u64().unwrap_or(0);
    if s.is_null() {
        o.text(Line::styled("loading…", dim()));
        return;
    }
    o.heading(&format!("{} token", app.backend.token_symbol));
    o.kv("Maximum supply", amount(app.backend.maximum_supply, dec));
    o.kv("Circulating supply", amount(n("circulating_supply"), dec));
    o.kv("Holders", sep(n("holders")));
    o.kv("Held by users", amount(n("held_by_users"), dec));
    o.kv("Active voting power", amount(n("active_users_vp"), dec));
    o.kv("Nakamoto coefficient", sep(n("nakamoto_coefficient")));
    o.kv("Fees burned", amount(n("fees_burned"), dec));
    o.kv("Bridged out", amount(n("bridged_out"), dec));
    o.kv("Volume 24h", amount(n("volume_day"), dec));
    o.kv("Volume 7d", amount(n("volume_week"), dec));
    o.kv("ICP per 1 XDR", icp(n("e8s_for_one_xdr")));
    o.kv("ICP per 1k credits", icp(n("e8s_revenue_per_1k")));
    o.kv("Last weekly chores", ago(n("last_weekly_chores")));
    if let Some((a, account)) = &p.data.auction {
        o.heading("Upcoming auction");
        o.kv("Tokens", amount(a.amount, dec));
        o.kv("Your deposit account", account.clone());
        o.note("Bids are paid in ICP from your wallet; tokens are sold at the next weekly chores to the highest bids.");
        if app.user.is_some() {
            let mut acts: Vec<(String, Action2)> = vec![("bid".into(), act(|app| app.bid_form()))];
            if a.bids.iter().any(|b| b.user == app.me()) {
                acts.push(("cancel my bid".into(), act(|app| app.cancel_bid())));
            }
            o.actions("auction", acts);
        }
        if !a.bids.is_empty() {
            let rows = a
                .bids
                .iter()
                .map(|b| {
                    (
                        format!("bid:{}", b.user),
                        vec![
                            (format!("@{}", app.name(b.user)), accent()),
                            (amount(b.amount, dec), Style::default()),
                            (icp(b.e8s_per_token * 10u64.pow(dec)), Style::default()),
                            (ago(b.timestamp), dim()),
                        ],
                        Some(Target::UserId(b.user)),
                    )
                })
                .collect();
            o.table(
                &[
                    ("bidder", false),
                    ("tokens", true),
                    ("ICP per token", true),
                    ("when", true),
                ],
                rows,
            );
        }
    }
    o.heading("Top token holders");
    if let Some(b) = s["balances"].as_array() {
        let circ = n("circulating_supply").max(1);
        let rows = b
            .iter()
            .take(100)
            .enumerate()
            .map(|(i, v)| {
                let bal = v[1].as_u64().unwrap_or(0);
                let uid = v[2].as_u64();
                let name = uid
                    .map(|u| format!("@{}", app.name(u)))
                    .unwrap_or_else(|| "—".into());
                let active = v[3].as_bool().unwrap_or(false);
                let owner = v[0]["owner"].as_str().unwrap_or("").to_string();
                (
                    format!("h:{owner}"),
                    vec![
                        ((i + 1).to_string(), dim()),
                        (name.clone(), if active { accent() } else { dim() }),
                        (amount(bal, dec), Style::default()),
                        (format!("{:.2}%", bal as f64 * 100.0 / circ as f64), dim()),
                        (owner.clone(), dim()),
                    ],
                    Some(Target::Page(
                        Kind::Transactions(owner, String::new()),
                        format!("transactions of {name}"),
                    )),
                )
            })
            .collect();
        o.table(
            &[
                ("#", true),
                ("holder", false),
                ("balance", true),
                ("share", true),
                ("principal", false),
            ],
            rows,
        );
    }
    o.heading("Latest transactions");
    txs(o, &p.data.txs);
    o.row(
        "alltx",
        Line::styled("▸ all transactions", accent()),
        Target::Page(
            Kind::Transactions(String::new(), String::new()),
            "Transactions".into(),
        ),
    );
}

fn txs(o: &mut Out, list: &[(u32, api::Transaction)]) {
    let app = o.app;
    let dec = app.backend.token_decimals;
    let who = |a: &api::Account| -> String {
        if a.owner == app.cfg.canister || a.owner.is_empty() {
            "minting".into()
        } else {
            a.owner.clone()
        }
    };
    let rows = list
        .iter()
        .map(|(id, t)| {
            (
                format!("tx:{id}"),
                vec![
                    (id.to_string(), dim()),
                    (ago(t.timestamp), dim()),
                    (who(&t.from), Style::default()),
                    (who(&t.to), Style::default()),
                    (amount(t.amount, dec), accent()),
                ],
                Some(Target::Tx(*id)),
            )
        })
        .collect();
    o.table(
        &[
            ("id", true),
            ("when", true),
            ("from", false),
            ("to", false),
            ("amount", true),
        ],
        rows,
    );
}

fn dashboard(o: &mut Out, p: &Page) {
    let app = o.app;
    let s = &p.data.json;
    if s.is_null() {
        o.text(Line::styled("loading…", dim()));
        return;
    }
    let n = |k: &str| s[k].as_u64().unwrap_or(0);
    let dec = app.backend.token_decimals;
    o.heading("Users and content");
    o.kv("Users", sep(n("users")));
    o.kv("Active users", sep(n("active_users")));
    o.kv("Online", sep(n("users_online")));
    o.kv("Invited", sep(n("invited_users")));
    o.kv("In bootcamp", sep(n("bootcamp_users")));
    o.kv("Posts", sep(n("posts")));
    o.kv("Comments", sep(n("comments")));
    o.kv("Realms", sep(n("realms")));
    o.heading("Economy");
    o.kv("Credits", sep(n("credits")));
    o.kv("Credits burned", sep(n("burned_credits")));
    o.kv("Circulating supply", amount(n("circulating_supply"), dec));
    o.kv(
        "Rewards shared",
        format!("{} ICP", icp(n("total_rewards_shared"))),
    );
    o.kv(
        "Revenue shared",
        format!("{} ICP", icp(n("total_revenue_shared"))),
    );
    o.kv("ICP per 1 XDR", icp(n("e8s_for_one_xdr")));
    if let Some(v) = s["vesting_tokens_of_x"].as_array() {
        o.kv(
            "Founder vesting",
            format!(
                "{} of {}",
                amount(v[0].as_u64().unwrap_or(0), dec),
                amount(v[1].as_u64().unwrap_or(0), dec)
            ),
        );
    }
    o.heading("Canister");
    o.kv(
        "Cycles",
        format!("{} T", n("canister_cycle_balance") / 1_000_000_000_000),
    );
    o.kv(
        "Cycle burn",
        format!("{} B/day", n("canister_cycle_burn") / 1_000_000_000),
    );
    o.kv("State size", format!("{} MB", n("state_size") / 1_000_000));
    o.kv("Last weekly chores", ago(n("last_weekly_chores")));
    let lr = &s["last_release"];
    if !lr.is_null() {
        o.kv(
            "Last release",
            format!(
                "{} ({})",
                lr["commit"]
                    .as_str()
                    .unwrap_or("")
                    .chars()
                    .take(10)
                    .collect::<String>(),
                ago(lr["timestamp"].as_u64().unwrap_or(0))
            ),
        );
    }
    o.kv(
        "Canister id",
        s["canister_id"].as_str().unwrap_or("").to_string(),
    );
    o.kv(
        "Treasury account",
        s["account"].as_str().unwrap_or("").to_string(),
    );
    if let Some(pid) = lr["post_id"].as_u64() {
        o.row(
            "rel",
            Line::styled("▸ release proposal", accent()),
            Target::Post(pid),
        );
    }
    o.blank();
    let stalwarts: Vec<u64> = s["stalwarts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_u64())
        .collect();
    o.users("stalwarts", "Stalwarts", &stalwarts, 3);
    if let Some(l) = s["weekly_karma_leaders"].as_array() {
        o.heading("Weekly reward leaders");
        let rows = l
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let id = v[0].as_u64().unwrap_or(0);
                (
                    format!("k:{id}"),
                    vec![
                        ((i + 1).to_string(), dim()),
                        (format!("@{}", app.name(id)), accent()),
                        (sep(v[1].as_u64().unwrap_or(0)), Style::default()),
                    ],
                    Some(Target::UserId(id)),
                )
            })
            .collect();
        o.table(&[("#", true), ("user", false), ("rewards", true)], rows);
    }
    o.heading("Logs");
    if let Some(l) = p.data.json2.as_array() {
        let mut l: Vec<&Value> = l.iter().collect();
        l.sort_by_key(|e| std::cmp::Reverse(e["timestamp"].as_u64().unwrap_or(0)));
        let rows = l
            .iter()
            .take(300)
            .enumerate()
            .map(|(i, e)| {
                let lvl = e["level"].as_str().unwrap_or("").to_string();
                let st = match lvl.as_str() {
                    "ERROR" => Style::default().fg(BAD),
                    "WARN" => Style::default().fg(WARN),
                    _ => dim(),
                };
                let msg = crate::markdown::first_line(e["message"].as_str().unwrap_or(""));
                let full = e["message"].as_str().unwrap_or("").to_string();
                (
                    format!("log:{i}"),
                    vec![
                        (datetime(e["timestamp"].as_u64().unwrap_or(0)), dim()),
                        (lvl, st),
                        (msg, Style::default()),
                    ],
                    Some(run(move |app: &mut App| app.text_popup("Log entry", &full))),
                )
            })
            .collect();
        o.table(
            &[("time", false), ("level", false), ("message", false)],
            rows,
        );
    }
}

fn distribution(o: &mut Out, p: &Page) {
    o.heading("Distribution");
    o.note("Reports of the last weekly distributions of rewards and minted tokens.");
    if o.app.user.is_some() {
        o.actions(
            "dist",
            vec![(
                "vote to delay the weekly chores".into(),
                act(|app| app.delay_chores()),
            )],
        );
    }
    if let Some(list) = p.data.json.as_array() {
        for s in list.iter().rev() {
            o.heading(s["title"].as_str().unwrap_or(""));
            o.md(s["description"].as_str().unwrap_or(""));
            let items: Vec<String> = s["items"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();
            // items like "0.6 to @user": two columns
            let rows = items
                .iter()
                .enumerate()
                .map(|(i, it)| {
                    let (a, b) = it.split_once(" to ").unwrap_or(("", it));
                    (
                        format!("d:{i}"),
                        vec![(a.to_string(), Style::default()), (b.to_string(), accent())],
                        None,
                    )
                })
                .collect();
            if !items.is_empty() {
                o.table(&[("amount", true), ("to", false)], rows);
            }
        }
    }
}

fn domains(o: &mut Out, p: &Page) {
    let app = o.app;
    o.heading("Domains");
    o.note("Each domain serves this frontend with its own realm configuration. Owners decide which realms show. Enter on your domain edits it.");
    if app.user.is_some() {
        o.actions(
            "dom",
            vec![("add your domain".into(), act(|app| app.domain_form(None)))],
        );
    }
    let rows = p
        .data
        .domains
        .iter()
        .map(|(name, c)| {
            let cfg = match &c.sub_config {
                api::DomainSub::Journal(u) => format!("journal of @{}", app.name(*u)),
                api::DomainSub::WhiteListedRealms(r) => format!(
                    "only {}",
                    r.iter()
                        .map(|r| format!("/{r}"))
                        .collect::<Vec<_>>()
                        .join(" ")
                ),
                api::DomainSub::BlackListedRealms(r) if r.is_empty() => "all realms".into(),
                api::DomainSub::BlackListedRealms(r) => format!(
                    "all but {}",
                    r.iter()
                        .map(|r| format!("/{r}"))
                        .collect::<Vec<_>>()
                        .join(" ")
                ),
            };
            let mut nm = name.clone();
            if name == &app.cfg.domain {
                nm += " (this client)";
            }
            let owner = c
                .owner
                .map(|o| format!("@{}", app.name(o)))
                .unwrap_or_else(|| "DAO".into());
            let st = if c.owner == Some(app.me()) {
                Style::default().fg(GOOD)
            } else {
                accent()
            };
            (
                format!("d:{name}"),
                vec![(nm, st), (owner, Style::default()), (cfg, dim())],
                Some(Target::Domain(name.clone())),
            )
        })
        .collect();
    o.table(
        &[("domain", false), ("owner", false), ("content", false)],
        rows,
    );
}

pub const LINKS: &str = "## Price listings\n\n- [CoinGecko](https://www.coingecko.com/en/coins/taggr)\n- [ICP Coins](https://icpcoins.com/#/token/TAGGR)\n- [ICPSwap](https://app.icpswap.com/info-swap/token/details/6qfxa-ryaaa-aaaai-qbhsq-cai)\n\n\
## Community-maintained resources\n\n- [#mntyetti and #finn feed](#/feed/mntyetti+finn) by @MntYetti\n- [OpenChat community](https://oc.app/community/zbg63-qqaaa-aaaar-atika-cai)\n\n\
## Development\n\n- [user #0](#/user/0)\n- [user #935](#/user/935)\n";

fn links(o: &mut Out) {
    for line in LINKS.lines() {
        if let Some(h) = line.strip_prefix("## ") {
            o.heading(h);
        } else if line.starts_with("- ") {
            let r = crate::markdown::render(line, o.w);
            let label: String = r
                .blocks
                .iter()
                .filter_map(|b| match b {
                    crate::markdown::Block::Line(l) => Some(
                        l.spans
                            .iter()
                            .map(|s| s.content.as_ref())
                            .collect::<String>(),
                    ),
                    _ => None,
                })
                .collect();
            if let Some(l) = r.links.into_iter().next() {
                let key = format!("link:{}", l.label());
                o.row(
                    key,
                    Line::raw(label),
                    run(move |app: &mut App| app.follow_link(l.clone())),
                );
            }
        }
    }
}
