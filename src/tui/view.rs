//! Turns a loaded page into entries for the current width.

use super::App;
use super::page::*;
use crate::api::{self, Item};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use serde_json::Value;

fn tabs(names: &[&str], cur: usize) -> Line<'static> {
    let mut sp = vec![];
    for (i, n) in names.iter().enumerate() {
        let st = if i == cur {
            Style::default()
                .fg(ACCENT)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            dim()
        };
        sp.push(Span::styled(n.to_string(), st));
        sp.push(Span::raw("   "));
    }
    sp.push(Span::styled("[ ] switch", dim()));
    Line::from(sp)
}

fn posts(o: &mut Out, p: &Page) {
    for it in &p.items {
        o.e.push(post_entry(o.app, p, it, o.w, false, 0));
        o.blank();
    }
    if p.more {
        o.e.push(Entry {
            lines: vec![Ln::Text(Line::styled("▸ load more", accent()))],
            target: Some(Target::More),
            key: "more".into(),
        });
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

pub fn build(app: &App, p: &Page, w: usize) -> Vec<Entry> {
    let mut o = Out {
        e: Vec::new(),
        w,
        app,
    };
    let o = &mut o;
    match &p.kind {
        Kind::Home | Kind::Hot | Kind::New | Kind::Bookmarks | Kind::Reposts(_) => posts(o, p),
        Kind::Tags(tags) => {
            if let Some(u) = &app.user {
                let following = u.feeds.iter().any(|f| f == tags);
                o.note(if following {
                    "following this feed · f: unfollow · M: mute tag"
                } else {
                    "f: follow this feed · M: mute tag"
                });
                o.blank();
            }
            posts(o, p)
        }
        Kind::Realm(id) => realm(o, p, id),
        Kind::Realms => realms(o, p),
        Kind::User(_) => user(o, p),
        Kind::Journal(h) => {
            if let Some(u) = &p.data.user {
                o.text(Line::styled(format!("{}'s journal", u.name), head()));
                if !u.about.is_empty() {
                    o.md(&u.about);
                }
                o.blank();
            } else {
                o.text(Line::styled(format!("{h}'s journal"), head()));
            }
            posts(o, p)
        }
        Kind::Post(_) => thread(o, p),
        Kind::Inbox => inbox(o, p),
        Kind::Search(_) => search(o, p),
        Kind::Users(ids) => {
            for id in ids {
                o.user_link(*id, "");
            }
        }
        Kind::Proposals => {
            o.text(tabs(
                &[
                    "All",
                    "Release",
                    "ICP transfer",
                    "Realm controller",
                    "Funding",
                    "Rewards",
                ],
                p.tab,
            ));
            o.note("n: new proposal");
            o.blank();
            posts(o, p)
        }
        Kind::Wallet => wallet(o, p),
        Kind::Invites => invites(o, p),
        Kind::Settings => settings(o, p),
        Kind::Welcome => welcome(o, p),
        Kind::Tokens => tokens(o, p),
        Kind::Transactions(owner, _) => {
            o.text(Line::styled(
                if owner.is_empty() {
                    "All transactions".to_string()
                } else {
                    format!("Transactions of {owner}")
                },
                head(),
            ));
            o.blank();
            txs(o, p, &p.data.txs);
            if p.more {
                o.e.push(Entry {
                    lines: vec![Ln::Text(Line::styled("▸ load more", accent()))],
                    target: Some(Target::More),
                    key: "more".into(),
                });
            }
        }
        Kind::Dashboard => dashboard(o, p),
        Kind::Distribution => distribution(o, p),
        Kind::Domains => domains(o, p),
        Kind::Ethereum => ethereum(o, p),
        Kind::Recovery => recovery(o, p),
        Kind::Links => links(o),
        Kind::Whitepaper => {
            if p.data.text.is_empty() {
                o.text(Line::styled("loading…", dim()));
            } else {
                o.md(&p.data.text);
            }
        }
    }
    std::mem::take(&mut o.e)
}

fn realm(o: &mut Out, p: &Page, id: &str) {
    let app = o.app;
    o.text(Line::styled(
        format!("/{id}"),
        Style::default().fg(WARN).add_modifier(Modifier::BOLD),
    ));
    if let Some(r) = &p.data.realm {
        if !r.description.is_empty() {
            let d = r
                .description
                .split(crate::markdown::CUT)
                .next()
                .unwrap_or("");
            o.md(d);
        }
        o.blank();
        o.kv("Members", sep(r.num_members));
        o.kv("Posts", sep(r.num_posts));
        o.kv("Created", date(r.created));
        o.kv("Last update", ago(r.last_update));
        o.kv("Revenue", format!("{} credits", sep(r.revenue)));
        o.kv(
            "Clean-up penalty",
            format!("{} credits", sep(r.cleanup_penalty)),
        );
        if r.adult_content {
            o.kv("Adult content", "yes");
        }
        if r.comments_filtering {
            o.kv("Comments filtering", "on");
        }
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
        if !r.whitelist.is_empty() {
            o.kv("Whitelist", format!("{} users", r.whitelist.len()));
        }
        o.text(Line::styled("Controllers", dim()));
        for c in &r.controllers {
            o.user_link(*c, "");
        }
        if r.description.contains(crate::markdown::CUT) {
            o.button("Realm rules / full description", "", {
                let d = r.description.clone();
                move |app: &mut App| app.text_popup("Realm description", &d)
            });
        }
    }
    if let Some(u) = &app.user {
        let member = u.realms.iter().any(|r| r == id);
        let ctl = p
            .data
            .realm
            .as_ref()
            .is_some_and(|r| r.controllers.contains(&u.id));
        let mut hint = vec![
            if member { "J: leave" } else { "J: join" },
            "c: new post here",
            "M: mute realm",
        ];
        if ctl {
            hint.push("E: edit realm");
        }
        o.blank();
        o.note(&hint.join(" · "));
    }
    o.blank();
    o.text(tabs(&["New", "Hot"], p.tab));
    o.blank();
    posts(o, p);
}

fn realms(o: &mut Out, p: &Page) {
    o.text(tabs(&["Popularity", "Last update", "Name"], p.tab));
    let mut note = "/: search realms".to_string();
    if !p.query.is_empty() {
        note = format!("matching “{}” · Esc: all", p.query);
    }
    if o.app.user.is_some() {
        note += " · n: create a realm";
    }
    o.note(&note);
    o.blank();
    let me = o.app.user.as_ref();
    for (id, r) in &p.data.realms {
        let member = me.is_some_and(|u| u.realms.contains(id));
        let desc = crate::markdown::first_line(&r.description);
        let mut l1 = vec![Span::styled(
            format!("/{id}"),
            Style::default().fg(WARN).add_modifier(Modifier::BOLD),
        )];
        if member {
            l1.push(Span::styled("  member", Style::default().fg(GOOD)));
        }
        l1.push(Span::styled(
            format!(
                "  {} members · {} posts · {}",
                sep(r.num_members),
                sep(r.num_posts),
                ago(r.last_update)
            ),
            dim(),
        ));
        let l2 = Line::raw(format!("  {}", fit(&desc, o.w.saturating_sub(2))));
        o.rows(
            format!("r:{id}"),
            vec![Line::from(l1), l2],
            Target::Realm(id.clone()),
        );
    }
    if p.more {
        o.e.push(Entry {
            lines: vec![Ln::Text(Line::styled("▸ load more", accent()))],
            target: Some(Target::More),
            key: "more".into(),
        });
    }
}

pub fn fit(s: &str, w: usize) -> String {
    if s.chars().count() <= w {
        return s.to_string();
    }
    s.chars().take(w.saturating_sub(1)).collect::<String>() + "…"
}

fn user(o: &mut Out, p: &Page) {
    let app = o.app;
    let Some(u) = &p.data.user else {
        o.text(Line::styled("loading…", dim()));
        return;
    };
    let mut l = vec![Span::styled(format!("@{}", u.name), accent())];
    if u.stalwart {
        l.push(Span::styled("  Stalwart", Style::default().fg(BAD)));
    }
    if u.deactivated {
        l.push(Span::styled("  deactivated", dim()));
    }
    if u.id < 100 {
        l.push(Span::styled("  OG", Style::default().fg(WARN)));
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
        o.blank();
        o.text(Line::styled(
            "This user was REPORTED.",
            Style::default().fg(BAD).add_modifier(Modifier::BOLD),
        ));
        o.md(&r.reason);
        o.note(&format!(
            "reporter @{} · confirmed by {} · rejected by {}",
            app.name(r.reporter),
            r.confirmed_by.len(),
            r.rejected_by.len()
        ));
        if app.user.as_ref().is_some_and(|me| me.stalwart) {
            o.note("Stalwarts: A agree (confirm) · Z disagree (reject)");
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
            fit(
                pgp.lines()
                    .find(|l| !l.starts_with("-----") && !l.is_empty())
                    .unwrap_or(pgp),
                40,
            ),
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
        o.kv("Follows feeds", f.join(", "));
    }
    let realms = u.realms.clone();
    if !realms.is_empty() {
        o.kv(
            "Realms",
            realms
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
    o.row(
        "follows",
        Line::from(vec![
            Span::styled(format!("{:<24}", "Follows"), dim()),
            Span::styled(format!("{} users ▸", u.followees.len()), accent()),
        ]),
        Target::Page(
            Kind::Users(u.followees.clone()),
            format!("@{} follows", u.name),
        ),
    );
    o.row(
        "followers",
        Line::from(vec![
            Span::styled(format!("{:<24}", "Followers"), dim()),
            Span::styled(format!("{} users ▸", u.followers.len()), accent()),
        ]),
        Target::Page(
            Kind::Users(u.followers.clone()),
            format!("followers of @{}", u.name),
        ),
    );
    o.row(
        "txs",
        Line::from(vec![
            Span::styled(format!("{:<24}", "Transactions"), dim()),
            Span::styled("▸", accent()),
        ]),
        Target::Page(
            Kind::Transactions(u.principal.clone(), String::new()),
            format!("@{} transactions", u.name),
        ),
    );
    o.row(
        "journal",
        Line::from(vec![
            Span::styled(format!("{:<24}", "Journal"), dim()),
            Span::styled("▸", accent()),
        ]),
        Target::Page(
            Kind::Journal(u.name.clone()),
            format!("{}'s journal", u.name),
        ),
    );
    if !u.accounting.is_empty() {
        let acc = u.accounting.clone();
        o.row(
            "accounting",
            Line::from(vec![
                Span::styled(format!("{:<24}", "Accounting"), dim()),
                Span::styled(format!("{} entries ▸", acc.len()), accent()),
            ]),
            run(move |app: &mut App| {
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
        );
    }
    if let Some(me) = &app.user
        && me.id != u.id
    {
        let follows = me.followees.contains(&u.id);
        let muted = me.filters.users.contains(&u.id);
        let blocked = me.blacklist.contains(&u.id);
        o.blank();
        o.note(&format!(
            "f: {} · M: {} · B: {} · $: send credits · !: report",
            if follows { "unfollow" } else { "follow" },
            if muted { "unmute" } else { "mute" },
            if blocked { "unblock" } else { "block" },
        ));
    }
    o.blank();
    o.text(tabs(&["Posts", "Mentions", "Rewarded"], p.tab));
    o.blank();
    posts(o, p);
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
        o.blank();
    }
    o.e.push(post_entry(app, p, main, o.w, true, 0));
    post_footer(o, p, main);
    if !p.data.json.is_null() {
        proposal(o, &p.data.json);
    }
    if p.items.len() > 1 {
        o.blank();
        let n = p.items.len() - 1;
        o.text(Line::styled(
            format!("{n} comment{}", if n == 1 { "" } else { "s" }),
            head(),
        ));
    }
    for it in p.items.iter().skip(1) {
        let d = p.data.depth.get(&it.post.id).copied().unwrap_or(1);
        o.blank();
        o.e.push(post_entry(app, p, it, o.w, true, d.min(8)));
    }
}

fn post_footer(o: &mut Out, _p: &Page, it: &Item) {
    let app = o.app;
    let post = &it.post;
    let mut info = vec![datetime(post.timestamp)];
    if !post.patches.is_empty() {
        info.push(format!("{} versions (i)", post.patches.len() + 1));
    }
    if !post.reposts.is_empty() {
        info.push(format!("{} reposts", post.reposts.len()));
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
    let mut keys = vec![
        "r: reply",
        "+: react",
        "b: bookmark",
        "w: watch",
        "P: repost",
        "o: links",
        "i: info",
    ];
    if post.user == app.me() {
        keys.extend(["e: edit", "D: delete", "p: pin"]);
    } else {
        keys.push("t: tip");
    }
    if app.user.is_some() {
        o.note(&keys.join(" · "));
    }
}

fn proposal(o: &mut Out, pr: &Value) {
    let app = o.app;
    let dec = app.backend.token_decimals;
    o.blank();
    o.text(Line::styled(format!("Proposal #{}", pr["id"]), head()));
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
    let (mut yes, mut no) = (0u64, 0u64);
    let mut voted = false;
    if let Some(b) = pr["bulletins"].as_array() {
        for v in b {
            if v[0].as_u64() == Some(app.me()) {
                voted = true;
            }
            if v[1].as_bool() == Some(true) {
                yes += v[2].as_u64().unwrap_or(0);
            } else {
                no += v[2].as_u64().unwrap_or(0);
            }
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
    if let Some(b) = pr["bulletins"].as_array() {
        let names: Vec<String> = b
            .iter()
            .map(|v| {
                format!(
                    "{}@{}",
                    if v[1].as_bool() == Some(true) {
                        "✓"
                    } else {
                        "✗"
                    },
                    app.name(v[0].as_u64().unwrap_or(0))
                )
            })
            .collect();
        if !names.is_empty() {
            o.note(&names.join(" "));
        }
    }
    if status == "Open" && app.user.is_some() {
        let mut k = vec![];
        if !voted {
            k.push("A: accept · Z: reject");
        }
        if pr["proposer"].as_u64() == Some(app.me()) {
            k.push("X: cancel proposal");
        }
        if !k.is_empty() {
            o.note(&k.join(" · "));
        }
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
    o.note("Enter: open · d: dismiss · D: clear all · w: unwatch post");
    o.blank();
    if fresh.is_empty() {
        o.text(Line::styled("Inbox zero achieved! 🚀", head()));
    }
    for (title, list, archived) in [("", fresh, false), ("Archive", old, true)] {
        if !title.is_empty() && !list.is_empty() {
            if p.tab == 0 {
                o.blank();
                o.button(
                    "Show archive",
                    &format!("{} entries", list.len()),
                    |app: &mut App| {
                        if let Some(p) = app.page() {
                            p.tab = 1;
                            p.dirty();
                        }
                    },
                );
                continue;
            }
            o.heading(title);
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
            let mut lines: Vec<Ln> = blocks(app, r.blocks, o.w)
                .into_iter()
                .map(|l| match l {
                    Ln::Text(t) => {
                        let mut sp = vec![Span::styled(
                            if archived { "  " } else { "● " },
                            Style::default().fg(WARN),
                        )];
                        sp.extend(t.spans);
                        Ln::Text(Line::from(sp))
                    }
                    x => x,
                })
                .collect();
            if let Some(pid) = post
                && let Some(it) = p.items.iter().find(|i| i.post.id == pid)
            {
                let e = post_entry(app, p, it, o.w.saturating_sub(4), false, 0);
                for l in e.lines.into_iter().take(6) {
                    if let Ln::Text(t) = l {
                        let mut sp = vec![Span::styled("  │ ", dim())];
                        sp.extend(t.spans);
                        lines.push(Ln::Text(Line::from(sp)));
                    }
                }
            }
            o.e.push(Entry {
                lines,
                target: Some(Target::Notification(*id)),
                key: format!("n:{id}"),
            });
            o.blank();
        }
    }
}

fn search(o: &mut Out, p: &Page) {
    o.note("/: new search · terms: @user, #tag, /realm, text");
    if !p.query.is_empty() {
        o.text(Line::styled(format!("Results for “{}”", p.query), head()));
    }
    o.blank();
    for r in &p.data.results {
        let (label, t) = match r.result.as_str() {
            "user" => (format!("@{}", o.app.name(r.id)), Target::UserId(r.id)),
            "tag" => (format!("#{}", r.relevant), Target::Tag(r.relevant.clone())),
            "realm" => (
                format!("/{}", r.generic_id),
                Target::Realm(r.generic_id.clone()),
            ),
            _ => (
                format!("post #{} by @{}", r.id, o.app.name(r.user_id)),
                Target::Post(r.id),
            ),
        };
        let rel = if r.result == "tag" {
            String::new()
        } else {
            r.relevant.replace('\n', " ")
        };
        o.rows(
            format!("s:{}:{}:{}", r.result, r.id, r.generic_id),
            vec![
                Line::from(vec![
                    Span::styled(label, accent()),
                    Span::styled(format!("  {}", r.result), dim()),
                ]),
                Line::raw(format!("  {}", fit(&rel, o.w.saturating_sub(2)))),
            ],
            t,
        );
    }
    if p.data.results.is_empty() && p.data.loaded {
        o.text(Line::styled("no results", dim()));
    }
}

fn wallet(o: &mut Out, p: &Page) {
    let app = o.app;
    let Some(u) = &app.user else { return };
    let dec = app.backend.token_decimals;
    if u.cycles <= 200 {
        o.text(Line::styled(
            "You are low on credits! Transfer ICP to your account below and mint credits.",
            Style::default().fg(WARN),
        ));
        o.blank();
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
    o.button("Send ICP", "to an account id", |app: &mut App| {
        app.send_icp_form()
    });
    o.button(
        "Withdraw rewards",
        "converts reward points to ICP",
        |app: &mut App| app.withdraw_rewards(),
    );
    o.heading("Credits");
    o.kv("Available", isep(u.cycles));
    o.button(
        "Mint credits",
        "pays ICP from the balance above",
        |app: &mut App| app.mint_credits_form(),
    );
    o.heading(&app.backend.token_symbol.clone());
    o.kv("Wallet", tok(app, u.balance));
    if let Some(c) = &u.cold_wallet {
        o.kv("Cold wallet", format!("{} ({c})", tok(app, u.cold_balance)));
    }
    o.button("Send tokens", "", |app: &mut App| {
        let sym = app.backend.token_symbol.clone();
        let (dec, fee) = (app.backend.token_decimals, app.backend.transaction_fee);
        let c = app.cfg.canister.clone();
        app.send_tokens_form(c, sym, dec, fee);
    });
    o.row(
        "txs",
        Line::styled("▸ Transactions", accent()),
        Target::Page(
            Kind::Transactions(u.principal.clone(), String::new()),
            "My transactions".into(),
        ),
    );
    if u.cold_wallet.is_some() {
        o.button(
            "Unlink cold wallet",
            "reduces voting power",
            |app: &mut App| app.unlink_cold_wallet(),
        );
    } else {
        o.note("Linking a cold wallet needs the Plug browser extension: use the web app.");
    }
    if u.settings
        .get("icrcWallet")
        .map(|s| s == "true")
        .unwrap_or(false)
    {
        o.heading("ICRC tokens");
        for (c, sym, d, fee, bal) in &p.data.tokens {
            let (c2, sym2, d2, fee2, c3) = (c.clone(), sym.clone(), *d, *fee, c.clone());
            o.row(
                format!("tok:{c}"),
                Line::from(vec![
                    Span::styled(format!("{sym:<10}"), accent()),
                    Span::raw(format!("{:>20}", amount(*bal, *d))),
                    Span::styled(format!("  {c}"), dim()),
                ]),
                run(move |app: &mut App| {
                    app.popup = Some(super::Popup::Menu(
                        super::Menu::new(sym2.clone())
                            .add("Send", move |app| {
                                app.send_tokens_form(c2.clone(), sym2, d2, fee2)
                            })
                            .add("Remove from wallet", move |app| app.remove_wallet_token(c3)),
                    ))
                }),
            );
        }
        o.button("Add token", "by ledger canister id", |app: &mut App| {
            app.add_wallet_token_form()
        });
    }
    let _ = dec;
}

fn invites(o: &mut Out, p: &Page) {
    let app = o.app;
    o.note(&format!(
        "Invite people by giving them a code; the credits you put in are transferred to each new user (at least {} credits needed).",
        app.backend.min_credits_for_inviting
    ));
    o.button("Create invite", "", |app: &mut App| app.invite_form(None));
    o.heading("Your invites");
    if p.data.invites.is_empty() && p.data.loaded {
        o.text(Line::styled("none", dim()));
    }
    for (code, inv) in &p.data.invites {
        let mut l = vec![
            Span::styled(code.clone(), accent()),
            Span::raw(format!(
                "  {} credits left, {} per user",
                isep(inv.credits),
                isep(inv.credits_per_user)
            )),
        ];
        if let Some(r) = &inv.realm_id {
            l.push(Span::styled(format!("  /{r}"), Style::default().fg(WARN)));
        }
        let joined: Vec<String> = inv
            .joined_user_ids
            .iter()
            .map(|i| format!("@{}", app.name(*i)))
            .collect();
        let mut lines = vec![Line::from(l)];
        lines.push(Line::styled(
            format!("  {}", app.web_url(&format!("welcome/{code}"))),
            dim(),
        ));
        if !joined.is_empty() {
            lines.push(Line::styled(
                format!("  joined: {}", joined.join(" ")),
                dim(),
            ));
        }
        o.rows(format!("inv:{code}"), lines, Target::Invite(code.clone()));
    }
    if !p.data.invites.is_empty() {
        o.note("Enter: edit the selected invite");
    }
}

fn settings(o: &mut Out, p: &Page) {
    let app = o.app;
    let Some(u) = &app.user else { return };
    o.heading("Profile");
    o.kv("Name", u.name.clone());
    o.kv("About", fit(&u.about, o.w.saturating_sub(24)));
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
    o.button("Edit profile", "", |app: &mut App| app.profile_form());
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
        "Followed in other realms",
        if u.show_posts_in_realms {
            "shown"
        } else {
            "hidden"
        },
    );
    o.button("Edit filter", "", |app: &mut App| app.filter_form());
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
        o.heading(title);
        for v in items {
            let label = match kind {
                "user" | "blacklist" => format!("@{}", app.name(v.parse().unwrap_or(0))),
                "tag" => format!("#{v}"),
                _ => format!("/{v}"),
            };
            o.row(
                format!("{kind}:{v}"),
                Line::from(vec![Span::raw(label), Span::styled("  x: remove", dim())]),
                Target::Listed(kind, v),
            );
        }
    }
    o.heading("Storage");
    match &u.bucket {
        Some(b) => {
            o.kv("Storage canister", b.clone());
            let s = &p.data.json;
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
                if s["stale"].as_bool() == Some(true) {
                    o.text(Line::styled(
                        "The storage code is outdated.",
                        Style::default().fg(WARN),
                    ));
                    o.button("Upgrade storage", "", |app: &mut App| app.upgrade_bucket());
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
            }
            o.button("Top up", "ICP → cycles", |app: &mut App| {
                app.top_up_form()
            });
            o.button(
                "Add blackhole controller",
                "public status monitoring",
                |app: &mut App| app.add_blackhole(),
            );
        }
        None => {
            o.note("Pictures are stored in your own storage canister (a “bucket”), paid with ICP from your wallet.");
            o.button("Create storage", "", |app: &mut App| {
                app.create_bucket_form()
            });
        }
    }
    o.heading("Account suspension");
    o.note("Suspending encrypts all your posts with a password; reactivating decrypts them.");
    o.button(
        if u.deactivated {
            "Activate"
        } else {
            "Deactivate"
        },
        "",
        |app: &mut App| app.crypt_form(),
    );
    o.heading("Principal change");
    if p.data.text.is_empty() {
        o.note("Move this account to another principal: request the change here, then sign in with the new principal to confirm it.");
        o.button("Change principal", "", |app: &mut App| {
            app.principal_change_form()
        });
    } else {
        o.note(&format!(
            "Request registered: sign in with {} to confirm the change.",
            p.data.text
        ));
    }
}

fn welcome(o: &mut Out, _p: &Page) {
    let app = o.app;
    o.text(Line::styled("New user detected", head()));
    o.blank();
    o.kv("Your principal", app.principal.clone());
    o.blank();
    o.note("This principal has no account yet. Join with an invite code, or pay for credits with ICP, then pick a name.");
    o.blank();
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
    o.text(Line::styled(
        format!("{} token", app.backend.token_symbol),
        head(),
    ));
    o.kv("Maximum supply", amount(app.backend.maximum_supply, dec));
    o.kv("Circulating supply", amount(n("circulating_supply"), dec));
    o.kv("Holders", sep(n("holders")));
    o.kv("Held by users", amount(n("held_by_users"), dec));
    o.kv("Active voting power", amount(n("active_users_vp"), dec));
    o.kv("Nakamoto coefficient", sep(n("nakamoto_coefficient")));
    o.kv("Fees burned", amount(n("fees_burned"), dec));
    o.kv("Bridged out", amount(n("bridged_out"), dec));
    o.kv(
        "Volume 24h / 7d",
        format!(
            "{} / {}",
            amount(n("volume_day"), dec),
            amount(n("volume_week"), dec)
        ),
    );
    o.kv("ICP per 1 XDR", icp(n("e8s_for_one_xdr")));
    o.kv(
        "Revenue per 1k credits",
        format!("{} ICP", icp(n("e8s_revenue_per_1k"))),
    );
    o.kv("Last weekly chores", ago(n("last_weekly_chores")));
    if let Some((a, account)) = &p.data.auction {
        o.heading("Upcoming auction");
        o.kv("Tokens", amount(a.amount, dec));
        o.kv("Your deposit account", account.clone());
        o.note("Bids are paid in ICP from your wallet; tokens are distributed at the next weekly chores by highest price.");
        for b in &a.bids {
            o.user_link(
                b.user,
                &format!(
                    "{} tokens at {} ICP/token · {}",
                    amount(b.amount, dec),
                    icp(b.e8s_per_token * 10u64.pow(dec)),
                    ago(b.timestamp)
                ),
            );
        }
        if app.user.is_some() {
            o.button("Bid", "", |app: &mut App| app.bid_form());
            if a.bids.iter().any(|b| b.user == app.me()) {
                o.button("Cancel my bid", "", |app: &mut App| app.cancel_bid());
            }
        }
    }
    o.heading("Top token holders");
    if let Some(b) = s["balances"].as_array() {
        let circ = n("circulating_supply").max(1);
        for v in b.iter().take(100) {
            let bal = v[1].as_u64().unwrap_or(0);
            let uid = v[2].as_u64().unwrap_or(0);
            let owner = v[0]["owner"].as_str().unwrap_or("").to_string();
            let pct = bal as f64 * 100.0 / circ as f64;
            o.row(
                format!("h:{owner}"),
                Line::from(vec![
                    Span::styled(
                        format!("{:<18}", fit(&format!("@{}", app.name(uid)), 17)),
                        accent(),
                    ),
                    Span::raw(format!("{:>16}", amount(bal, dec))),
                    Span::styled(format!("  {pct:.2}%"), dim()),
                ]),
                Target::Page(
                    Kind::Transactions(owner.clone(), String::new()),
                    format!("transactions of @{}", app.name(uid)),
                ),
            );
        }
    }
    o.heading("Latest transactions");
    txs(o, p, &p.data.txs);
    o.row(
        "alltx",
        Line::styled("▸ all transactions", accent()),
        Target::Page(
            Kind::Transactions(String::new(), String::new()),
            "Transactions".into(),
        ),
    );
}

fn txs(o: &mut Out, _p: &Page, list: &[(u32, api::Transaction)]) {
    let app = o.app;
    let dec = app.backend.token_decimals;
    let short = |a: &api::Account| -> String {
        if a.owner == app.cfg.canister || a.owner.is_empty() {
            "minting".into()
        } else {
            fit(&a.owner, 14)
        }
    };
    for (id, t) in list {
        o.row(
            format!("tx:{id}"),
            Line::from(vec![
                Span::styled(format!("{id:>7}  "), dim()),
                Span::styled(format!("{:<8}", ago(t.timestamp)), dim()),
                Span::raw(format!("{:>15} → {:<15} ", short(&t.from), short(&t.to))),
                Span::styled(format!("{:>14}", amount(t.amount, dec)), accent()),
            ]),
            Target::Tx(*id),
        );
    }
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
    o.text(Line::styled("Dashboard", head()));
    o.kv(
        "Users",
        format!(
            "{} ({} active, {} online, {} invited, {} in bootcamp)",
            sep(n("users")),
            sep(n("active_users")),
            sep(n("users_online")),
            sep(n("invited_users")),
            sep(n("bootcamp_users"))
        ),
    );
    o.kv(
        "Posts / comments",
        format!("{} / {}", sep(n("posts")), sep(n("comments"))),
    );
    o.kv("Realms", sep(n("realms")));
    o.kv(
        "Credits",
        format!(
            "{} (burned {})",
            sep(n("credits")),
            sep(n("burned_credits"))
        ),
    );
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
    o.kv(
        "Cycles",
        format!(
            "{} T (burn {} B/day)",
            n("canister_cycle_balance") / 1_000_000_000_000,
            n("canister_cycle_burn") / 1_000_000_000
        ),
    );
    o.kv("State size", format!("{} MB", n("state_size") / 1_000_000));
    o.kv("Last weekly chores", ago(n("last_weekly_chores")));
    o.kv(
        "Treasury account",
        s["account"].as_str().unwrap_or("").to_string(),
    );
    o.kv(
        "Canister",
        s["canister_id"].as_str().unwrap_or("").to_string(),
    );
    let lr = &s["last_release"];
    if !lr.is_null() {
        o.kv(
            "Last release",
            format!(
                "{} ({})",
                lr["commit"].as_str().unwrap_or(""),
                ago(lr["timestamp"].as_u64().unwrap_or(0))
            ),
        );
        if let Some(pid) = lr["post_id"].as_u64() {
            o.row(
                "rel",
                Line::styled("▸ release proposal", accent()),
                Target::Post(pid),
            );
        }
    }
    if let Some(v) = s["vesting_tokens_of_x"].as_array() {
        let (vested, total) = (v[0].as_u64().unwrap_or(0), v[1].as_u64().unwrap_or(0));
        o.kv(
            "Founder vesting",
            format!("{} of {}", amount(vested, dec), amount(total, dec)),
        );
    }
    if let Some(l) = s["weekly_karma_leaders"].as_array() {
        o.heading("Weekly reward leaders");
        for v in l {
            o.user_link(v[0].as_u64().unwrap_or(0), &sep(v[1].as_u64().unwrap_or(0)));
        }
    }
    if let Some(l) = s["stalwarts"].as_array() {
        o.heading("Stalwarts");
        for v in l {
            o.user_link(v.as_u64().unwrap_or(0), "");
        }
    }
    o.heading("Logs");
    if let Some(l) = p.data.json2.as_array() {
        let mut l: Vec<&Value> = l.iter().collect();
        l.sort_by_key(|e| std::cmp::Reverse(e["timestamp"].as_u64().unwrap_or(0)));
        for e in l.iter().take(300) {
            let lvl = e["level"].as_str().unwrap_or("");
            let st = match lvl {
                "ERROR" => Style::default().fg(BAD),
                "WARN" => Style::default().fg(WARN),
                _ => dim(),
            };
            let r = crate::markdown::render(
                e["message"].as_str().unwrap_or(""),
                o.w.saturating_sub(22),
            );
            let mut first = true;
            for b in r.blocks {
                if let crate::markdown::Block::Line(t) = b {
                    let mut sp = vec![Span::styled(
                        if first {
                            format!(
                                "{:<14}{:<6}",
                                datetime(e["timestamp"].as_u64().unwrap_or(0))
                                    .chars()
                                    .take(13)
                                    .collect::<String>(),
                                lvl
                            )
                        } else {
                            " ".repeat(20)
                        },
                        st,
                    )];
                    sp.extend(t.spans);
                    o.text(Line::from(sp));
                    first = false;
                }
            }
        }
    }
}

fn distribution(o: &mut Out, p: &Page) {
    o.text(Line::styled("Distribution", head()));
    o.note("Reports of the last weekly distributions of rewards and minted tokens.");
    if o.app.user.is_some() {
        o.button(
            "Vote for delay",
            "delay the next weekly chores (stalwarts)",
            |app: &mut App| app.delay_chores(),
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
                .filter_map(|v| v.as_str().map(|s| format!("- {s}")))
                .collect();
            o.md(&items.join("\n"));
        }
    }
}

fn domains(o: &mut Out, p: &Page) {
    let app = o.app;
    o.text(Line::styled("Domains", head()));
    o.note("Each domain serves this frontend with its own realm configuration. Owners decide which realms show.");
    if app.user.is_some() {
        o.button("Add your domain", "", |app: &mut App| app.domain_form(None));
    }
    o.blank();
    for (name, c) in &p.data.domains {
        let cfg = match &c.sub_config {
            api::DomainSub::Journal(u) => format!("journal of @{}", app.name(*u)),
            api::DomainSub::WhiteListedRealms(r) => format!(
                "only: {}",
                r.iter()
                    .map(|r| format!("/{r}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
            api::DomainSub::BlackListedRealms(r) if r.is_empty() => "all realms".into(),
            api::DomainSub::BlackListedRealms(r) => format!(
                "hidden: {}",
                r.iter()
                    .map(|r| format!("/{r}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
            api::DomainSub::Unknown => "?".into(),
        };
        let mine = c.owner == Some(app.me());
        let mut l = vec![Span::styled(name.clone(), accent())];
        if let Some(ow) = c.owner {
            l.push(Span::styled(format!("  @{}", app.name(ow)), dim()));
        }
        if mine {
            l.push(Span::styled(
                "  yours: Enter edits",
                Style::default().fg(GOOD),
            ));
        }
        let mut lines = vec![Line::from(l), Line::raw(format!("  {cfg}"))];
        if name == &app.cfg.domain {
            lines.push(Line::styled("  (this client)", dim()));
        }
        o.rows(format!("d:{name}"), lines, Target::Domain(name.clone()));
    }
}

fn ethereum(o: &mut Out, p: &Page) {
    let app = o.app;
    let Some((addr, cfg, recipes)) = &p.data.eth else {
        let msg = if p.loading {
            "loading…"
        } else {
            "The Ethereum bridge is not available on this canister."
        };
        o.text(Line::styled(msg, dim()));
        return;
    };
    let dec = app.backend.token_decimals;
    o.text(Line::styled("Ethereum bridge", head()));
    o.note("Bridging burns tokens here and returns a signed mint recipe; submit it to the L1 token contract (mint) from an Ethereum wallet.");
    o.kv("Token contract", cfg.token_address.clone());
    o.kv("Chain id", cfg.chain_id.to_string());
    o.kv("Minter (canister)", addr.clone());
    o.kv("Fee", format!("{} credits", cfg.fee_credits));
    o.kv("Minimum", amount(cfg.min_amount, dec));
    if app.user.is_some() {
        o.button(
            "Bridge out",
            "burn tokens, get a recipe",
            |app: &mut App| app.bridge_form(),
        );
    }
    o.heading("My recipes");
    for r in recipes {
        let sig = format!("0x{}", hex::encode(&r.signature));
        let md = format!(
            "**Recipe #{}**\n\n- to: `{}`\n- amount: `{}` ({} base units)\n- burn block: `{}`\n- signature: `{}`\n\nCall `mint(to, amount, burnBlock, signature)` on `{}` (chain {}).",
            r.index,
            r.eth_address,
            amount(r.amount, dec),
            r.amount,
            r.burn_block,
            sig,
            cfg.token_address,
            cfg.chain_id
        );
        o.row(
            format!("eth:{}", r.index),
            Line::from(vec![
                Span::styled(format!("#{}", r.index), accent()),
                Span::raw(format!("  {} → {}", amount(r.amount, dec), r.eth_address)),
            ]),
            run(move |app: &mut App| app.text_popup("Mint recipe", &md)),
        );
    }
}

fn recovery(o: &mut Out, p: &Page) {
    o.text(Line::styled("Recovery", head()));
    o.heading("Status");
    o.md(&p.data.text);
    o.heading("Supporters");
    for s in &p.data.strings {
        o.text(Line::raw(s.clone()));
    }
    if o.app.user.is_some() {
        o.blank();
        o.button(
            "Upload emergency binary",
            "controllers only",
            |app: &mut App| app.emergency_binary_form(),
        );
        o.button("Confirm binary hash", "token holders", |app: &mut App| {
            app.confirm_hash_form()
        });
    }
}

pub const LINKS: &str = "## Price listings\n\n- [CoinGecko](https://www.coingecko.com/en/coins/taggr)\n- [ICP Coins](https://icpcoins.com/#/token/TAGGR)\n- [ICPSwap](https://app.icpswap.com/info-swap/token/details/6qfxa-ryaaa-aaaai-qbhsq-cai)\n\n\
## Community-maintained resources\n\n- [#mntyetti and #finn feed](#/feed/mntyetti+finn) by @MntYetti\n- [OpenChat community](https://oc.app/community/zbg63-qqaaa-aaaar-atika-cai)\n\n\
## Development\n\n- [user #0](#/user/0)\n- [user #935](#/user/935)\n";

fn links(o: &mut Out) {
    // headings as text, every link a selectable row
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
                    run(move |app: &mut App| app.follow_link(l)),
                );
            }
        }
    }
}
