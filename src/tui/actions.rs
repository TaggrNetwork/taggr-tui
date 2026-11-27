//! Keys on pages and the actions behind them: one method per thing the web app can do.

use super::page::{Kind, Target, amount, icp};
use super::{App, Field, Focus, Form, Menu, Popup, compose};
use crate::api::{self, Api, PostId, UserFilter};
use anyhow::anyhow;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::{Value, json};
use std::sync::Arc;

/// Parses a decimal amount into base units with `dec` decimals.
pub fn parse_amount(s: &str, dec: u32) -> Result<u64, String> {
    let s = s.trim().replace(['_', ','], "");
    if s.is_empty() {
        return Err("enter an amount".into());
    }
    let (int, frac) = s.split_once('.').unwrap_or((&s, ""));
    if frac.len() > dec as usize {
        return Err(format!("at most {dec} decimals"));
    }
    let int: u64 = if int.is_empty() {
        0
    } else {
        int.parse().map_err(|_| format!("not a number: {s}"))?
    };
    let frac_v: u64 = if frac.is_empty() {
        0
    } else {
        format!("{frac:0<w$}", w = dec as usize)
            .parse()
            .map_err(|_| format!("not a number: {s}"))?
    };
    int.checked_mul(10u64.pow(dec))
        .and_then(|v| v.checked_add(frac_v))
        .ok_or_else(|| "amount too large".into())
}

fn num(s: &str) -> Result<u64, String> {
    s.trim()
        .replace(['_', ','], "")
        .parse()
        .map_err(|_| format!("not a number: {s}"))
}

impl App {
    pub fn view_h(&self) -> usize {
        self.hits.page.height as usize
    }

    pub fn global_key(&mut self, k: KeyEvent) {
        match k.code {
            KeyCode::Char('/') => {
                if matches!(self.pages.last().map(|p| &p.kind), Some(Kind::Realms)) {
                    self.realm_search_prompt();
                } else {
                    self.search_prompt();
                }
            }
            KeyCode::Char('c') => self.new_post(self.current_realm()),
            KeyCode::Char('C') => self.new_poll(self.current_realm()),
            KeyCode::Char('R') if k.modifiers.contains(KeyModifiers::CONTROL) => self.reload(),
            KeyCode::Char('s') => {
                self.load_user();
                self.reload();
            }
            _ => {}
        }
    }

    fn current_realm(&self) -> Option<String> {
        match self.pages.last().map(|p| &p.kind) {
            Some(Kind::Realm(r)) => Some(r.clone()),
            _ => None,
        }
    }

    pub fn search_prompt(&mut self) {
        let f = Form::new("Search", "search")
            .note("@user, #tag, /realm, or words")
            .field(Field::text("Query", ""))
            .on_submit(|app, v| {
                let q = v[0].trim().to_string();
                if q.is_empty() {
                    return Err("enter something to search".into());
                }
                match app.pages.last_mut() {
                    Some(p) if matches!(p.kind, Kind::Search(_)) => {
                        p.query = q.clone();
                        p.title = format!("Search: {q}");
                    }
                    _ => app.pages.push(super::Page::new(
                        Kind::Search(q.clone()),
                        &format!("Search: {q}"),
                    )),
                }
                app.focus = Focus::Page;
                app.reload();
                Ok(())
            });
        self.popup = Some(Popup::Form(f));
    }

    pub fn realm_search_prompt(&mut self) {
        let q = self
            .pages
            .last()
            .map(|p| p.query.clone())
            .unwrap_or_default();
        let f = Form::new("Search realms", "search")
            .field(Field::text("Query", &q))
            .on_submit(|app, v| {
                if let Some(p) = app.page() {
                    p.query = v[0].trim().to_string();
                }
                app.reload();
                Ok(())
            });
        self.popup = Some(Popup::Form(f));
    }

    pub fn enter(&mut self) {
        let Some(p) = self.pages.last() else { return };
        let Some(t) = p.selected_target().cloned() else {
            return;
        };
        match t {
            Target::More => self.load(true),
            Target::Notification(id) => {
                let n = self
                    .user
                    .as_ref()
                    .and_then(|u| u.notifications.get(&id))
                    .map(|n| n.0.clone());
                self.dismiss(vec![id], false);
                match n {
                    Some(api::Notification::NewPost(_, p)) => self.open_target(Target::Post(p)),
                    Some(api::Notification::WatchedPostEntries(_, ids)) if !ids.is_empty() => {
                        self.open_target(Target::Post(*ids.last().unwrap_or(&0)))
                    }
                    Some(api::Notification::WatchedPostEntries(p, _)) => {
                        self.open_target(Target::Post(p))
                    }
                    Some(api::Notification::Conditional(
                        _,
                        api::Predicate::Proposal(p) | api::Predicate::ReportOpen(p),
                    )) => self.open_target(Target::Post(p)),
                    Some(api::Notification::Conditional(_, api::Predicate::UserReportOpen(u))) => {
                        self.open_target(Target::UserId(u))
                    }
                    _ => {}
                }
            }
            Target::Invite(code) => self.invite_form(Some(code)),
            Target::Domain(d) => self.domain_menu(d),
            Target::Tx(id) => self.show_tx(id),
            Target::Listed(kind, v) => self.unlist(kind, v),
            Target::Post(id) if matches!(p.kind, Kind::Post(cur) if cur == id) => {}
            t => self.open_target(t),
        }
    }

    pub fn page_key(&mut self, k: KeyEvent) {
        let h = self.view_h().max(3);
        let Some(p) = self.pages.last_mut() else {
            return;
        };
        match k.code {
            KeyCode::Char('j') | KeyCode::Down => p.step(1, h),
            KeyCode::Char('k') | KeyCode::Up => p.step(-1, h),
            KeyCode::Char('g') | KeyCode::Home => p.first(),
            KeyCode::Char('G') | KeyCode::End => p.last(h),
            KeyCode::PageDown | KeyCode::Char(' ') => p.scroll(h as isize - 2),
            KeyCode::PageUp => p.scroll(-(h as isize - 2)),
            KeyCode::Char('d') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                p.scroll(h as isize / 2)
            }
            KeyCode::Char('u') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                p.scroll(-(h as isize / 2))
            }
            KeyCode::Enter => self.enter(),
            KeyCode::Esc if !p.query.is_empty() && matches!(p.kind, Kind::Realms) => {
                p.query.clear();
                self.reload();
            }
            KeyCode::Esc | KeyCode::Char('h') | KeyCode::Left | KeyCode::Backspace => self.back(),
            KeyCode::Char(']') => {
                p.tab += 1;
                self.tab_changed();
            }
            KeyCode::Char('[') => {
                let n = p.tab_row.as_ref().map(|t| t.1.len()).unwrap_or(1);
                p.tab = p.tab.checked_sub(1).unwrap_or(n.saturating_sub(1));
                self.tab_changed();
            }
            KeyCode::Char(c @ '1'..='9')
                if p.tab_row
                    .as_ref()
                    .is_some_and(|t| (c as usize - '1' as usize) < t.1.len()) =>
            {
                p.tab = c as usize - '1' as usize;
                self.tab_changed();
            }
            _ => self.action_key(k),
        }
    }

    pub fn tab_changed(&mut self) {
        let Some(p) = self.pages.last_mut() else {
            return;
        };
        let n = p.tab_row.as_ref().map(|t| t.1.len()).unwrap_or(1).max(1);
        p.tab %= n;
        p.sel_key.clear();
        p.sel = 0;
        p.off = 0;
        p.more = false;
        p.items.clear();
        p.dirty();
        if !matches!(p.kind, Kind::Inbox) {
            self.reload();
        }
    }

    /// Keys acting on the selected thing or on the page.
    pub fn action_key(&mut self, k: KeyEvent) {
        let kind = self.pages.last().map(|p| p.kind.clone());
        let target = self.pages.last().and_then(|p| p.selected_target().cloned());
        let post = self.sel_post_id().or(match &kind {
            Some(Kind::Post(id)) => Some(*id),
            _ => None,
        });
        let c = match k.code {
            KeyCode::Char(c) => c,
            _ => return,
        };
        // post actions
        if let Some(id) = post {
            match c {
                'r' => return self.reply(id),
                '+' => return self.react_menu(id),
                'b' => return self.toggle_bookmark(id),
                'w' if !matches!(target, Some(Target::Notification(_))) => {
                    return self.toggle_watch(id);
                }
                'H' => return self.toggle_hide(id),
                'p' => return self.toggle_pin(id),
                't' => return self.tip_form(id),
                'e' => return self.edit_post(id),
                'D' if !matches!(kind, Some(Kind::Inbox)) => return self.delete_post(id),
                'P' => return self.repost(id),
                'v' => return self.poll_vote(id),
                'm' => return self.realm_cleanup(id),
                'i' => return self.post_info(id),
                'I' => return self.toggle_pictures(id),
                'x' => return self.decrypt_post(id),
                'o' => return self.links_menu(id),
                'O' => {
                    let u = self.web_url(&format!("post/{id}"));
                    return self.open_url(&u);
                }
                _ => {}
            }
        }
        match (&kind, c) {
            (Some(Kind::User(_)), _) | (_, 'f' | 'M' | 'B' | '$' | '!')
                if self.page_user().is_some() || matches!(target, Some(Target::UserId(_))) =>
            {
                let uid = match &target {
                    Some(Target::UserId(u)) => Some(*u),
                    _ => self.page_user(),
                };
                if let Some(uid) = uid {
                    match c {
                        'f' => return self.toggle_follow(uid),
                        'M' => return self.toggle_filter("user", uid.to_string()),
                        'B' => return self.toggle_block(uid),
                        '$' => return self.send_credits_form(uid),
                        '!' => return self.report_form(uid),
                        'A' => return self.vote_report(uid, true),
                        'Z' => return self.vote_report(uid, false),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        match (&kind, c) {
            (Some(Kind::Post(_)), 'A') => self.vote_proposal(true),
            (Some(Kind::Post(_)), 'Z') => self.vote_proposal(false),
            (Some(Kind::Post(_)), 'X') => self.cancel_proposal(),
            (Some(Kind::Realm(r)), 'J') => self.toggle_membership(r.clone()),
            (Some(Kind::Realm(r)), 'E') => self.realm_form(Some(r.clone())),
            (Some(Kind::Realm(r)), 'M') => self.toggle_filter("realm", r.clone()),
            (Some(Kind::Realms), 'n') => self.realm_form(None),
            (Some(Kind::Realms), 'J') => {
                if let Some(Target::Realm(r)) = target {
                    self.toggle_membership(r)
                }
            }
            (Some(Kind::Tags(t)), 'f') => self.toggle_feed(t.clone()),
            (Some(Kind::Tags(t)), 'M') => {
                if let Some(t) = t.first() {
                    self.toggle_filter("tag", t.clone())
                }
            }
            (Some(Kind::Inbox), 'd') => {
                if let Some(Target::Notification(id)) = target {
                    self.dismiss(vec![id], true)
                }
            }
            (Some(Kind::Inbox), 'D') => {
                let ids: Vec<u64> = self
                    .user
                    .as_ref()
                    .map(|u| u.notifications.keys().copied().collect())
                    .unwrap_or_default();
                self.confirm("Clear all notifications?", move |app| {
                    app.dismiss(ids, true)
                });
            }
            (Some(Kind::Inbox), 'w') => {
                if let Some(Target::Notification(id)) = target
                    && let Some((api::Notification::WatchedPostEntries(p, _), _)) = self
                        .user
                        .as_ref()
                        .and_then(|u| u.notifications.get(&id))
                        .cloned()
                {
                    self.dismiss(vec![id], false);
                    self.toggle_watch(p);
                }
            }
            (Some(Kind::Proposals), 'n') => self.new_proposal_menu(),
            (Some(Kind::Settings), 'x') => {
                if let Some(Target::Listed(kind, v)) = target {
                    self.unlist(kind, v)
                }
            }
            (Some(Kind::Links | Kind::Whitepaper), 'o') => self.page_links_menu(),
            _ => self.global_key(k),
        }
    }

    /// The user a user page shows.
    fn page_user(&self) -> Option<u64> {
        match self.pages.last() {
            Some(p) if matches!(p.kind, Kind::User(_)) => p.data.user.as_ref().map(|u| u.id),
            _ => None,
        }
    }

    // ---- posts --------------------------------------------------------------------

    fn item(&self, id: PostId) -> Option<api::Item> {
        self.pages.iter().rev().find_map(|p| p.post(id)).cloned()
    }

    /// Expands or collapses the pictures of a post.
    pub fn toggle_pictures(&mut self, id: PostId) {
        if !self.expanded.remove(&id) {
            self.expanded.insert(id);
        }
        for p in &mut self.pages {
            p.dirty();
        }
    }

    /// Decrypt the PGP message in a post, or hide its plain text again.
    pub fn decrypt_post(&mut self, id: PostId) {
        if self.decrypted.remove(&id).is_some() {
            return self.redraw_pages();
        }
        let body = self
            .pages
            .last()
            .and_then(|p| p.post(id))
            .map(|it| it.post.body.clone())
            .unwrap_or_default();
        let Some(range) = crate::pgp::find_block(&body) else {
            return self.error("no PGP message in this post");
        };
        let armor = crate::pgp::armor(&body, range);
        let gpg = self.cfg.gpg.clone();
        self.info("decrypting…");
        self.bg(
            move |_| async move {
                let a = armor.clone();
                let r = tokio::task::spawn_blocking(move || crate::pgp::decrypt(&gpg, &a, false))
                    .await?;
                Ok((r, armor))
            },
            move |app, (r, armor)| match r {
                // the agent wants a passphrase: hand the terminal to pinentry
                Err(crate::pgp::Error::NeedPassphrase) => app.gpg_job = Some((id, armor)),
                r => app.decrypted_done(id, r),
            },
        );
    }

    pub fn decrypted_done(
        &mut self,
        id: PostId,
        r: Result<crate::pgp::Decrypted, crate::pgp::Error>,
    ) {
        match r {
            Ok(d) => {
                self.info(d.signature.clone().unwrap_or_else(|| "decrypted".into()));
                self.decrypted.insert(id, d);
                self.redraw_pages();
            }
            Err(e) => self.error(format!("decrypt: {e}")),
        }
    }

    fn redraw_pages(&mut self) {
        for p in &mut self.pages {
            p.dirty();
        }
    }

    pub fn reply(&mut self, id: PostId) {
        if !self.need_user() {
            return;
        }
        let quote = self
            .item(id)
            .map(|i| i.meta.author_name)
            .unwrap_or_default();
        self.edit = Some(compose::Draft::reply(id, &quote));
    }

    pub fn new_post(&mut self, realm: Option<String>) {
        if !self.need_user() {
            return;
        }
        self.edit = Some(compose::Draft::new_post(realm, None));
    }

    pub fn new_poll(&mut self, realm: Option<String>) {
        if !self.need_user() {
            return;
        }
        self.edit = Some(compose::Draft::new_poll(realm));
    }

    pub fn repost(&mut self, id: PostId) {
        if !self.need_user() {
            return;
        }
        self.bg(
            |api| async move { api.call_any("toggle_following_post", vec![json!(id)]).await },
            |_, _| {},
        );
        self.edit = Some(compose::Draft::new_post(None, Some(id)));
    }

    pub fn edit_post(&mut self, id: PostId) {
        let Some(it) = self.item(id) else { return };
        if it.post.user != self.me() {
            return self.error("you can only edit your own posts");
        }
        self.edit = Some(compose::Draft::edit(&it.post));
    }

    pub fn react_menu(&mut self, id: PostId) {
        if !self.need_user() {
            return;
        }
        let mut m = Menu::new("React");
        let mut reactions = self.backend.reactions.clone();
        reactions.sort_by_key(|(id, _)| if *id == 1 { u16::MAX } else { *id });
        for (r, cost) in reactions {
            let label = format!(
                "{}  {}",
                super::page::reaction_icon(r),
                if r == 1 {
                    format!("downvote ({} credits)", cost.abs())
                } else {
                    format!("+{} reward points ({} credits)", cost, cost + 1)
                }
            );
            m = m.add(label, move |app| {
                app.act(
                    move |api| async move { api.call("react", vec![json!(id), json!(r)]).await },
                    "reacted",
                )
            });
        }
        self.popup = Some(Popup::Menu(m));
    }

    pub fn toggle(&mut self, method: &'static str, id: PostId, done: &'static str) {
        if !self.need_user() {
            return;
        }
        self.act(
            move |api| async move { api.call_any(method, vec![json!(id)]).await },
            done,
        );
    }

    pub fn toggle_bookmark(&mut self, id: PostId) {
        let on = self
            .user
            .as_ref()
            .is_some_and(|u| u.bookmarks.contains(&id));
        self.toggle(
            "toggle_bookmark",
            id,
            if on { "bookmark removed" } else { "bookmarked" },
        );
    }

    pub fn toggle_watch(&mut self, id: PostId) {
        let on = self
            .item(id)
            .is_some_and(|i| i.post.watchers.contains(&self.me()));
        self.toggle(
            "toggle_following_post",
            id,
            if on { "unwatched" } else { "watching" },
        );
    }

    pub fn toggle_hide(&mut self, id: PostId) {
        let on = self
            .item(id)
            .is_some_and(|i| i.post.hidden_for.contains(&self.me()));
        self.toggle(
            "toggle_hide_post",
            id,
            if on { "post shown" } else { "post hidden" },
        );
    }

    pub fn toggle_pin(&mut self, id: PostId) {
        if self.item(id).is_some_and(|i| i.post.user != self.me()) {
            return self.error("you can only pin your own posts");
        }
        let on = self
            .user
            .as_ref()
            .is_some_and(|u| u.pinned_posts.contains(&id));
        self.toggle(
            "toggle_pinned_post",
            id,
            if on {
                "unpinned"
            } else {
                "pinned to your profile"
            },
        );
    }

    pub fn tip_form(&mut self, id: PostId) {
        if !self.need_user() {
            return;
        }
        let Some(it) = self.item(id) else { return };
        if it.post.user == self.me() {
            return self.error("you cannot tip yourself");
        }
        let sym = self.backend.token_symbol.clone();
        let dec = self.backend.token_decimals;
        let f = Form::new(format!("Tip @{}", it.meta.author_name), "tip")
            .note(&format!(
                "Sends {sym} from your wallet to the author; costs 5 credits."
            ))
            .field(Field::text(&format!("Amount ({sym})"), "1"))
            .on_submit(move |app, v| {
                let a = parse_amount(&v[0], dec)?;
                app.act(
                    move |api| async move { api.call("tip", vec![json!(id), json!(a)]).await },
                    "tipped",
                );
                Ok(())
            });
        self.popup = Some(Popup::Form(f));
    }

    pub fn delete_post(&mut self, id: PostId) {
        let Some(it) = self.item(id) else { return };
        if it.post.user != self.me() {
            return self.error("you can only delete your own posts");
        }
        let c = &self.backend;
        let costs: std::collections::BTreeMap<u16, i64> = c.reactions.iter().copied().collect();
        let reactions: i64 = it
            .post
            .reactions
            .iter()
            .map(|(r, u)| costs.get(r).copied().unwrap_or(0).max(0) * u.len() as i64)
            .sum();
        let cost =
            reactions + c.post_cost + it.post.tree_size as i64 * c.post_deletion_penalty_factor;
        let post = it.post.clone();
        self.confirm(
            format!("Delete post #{id}? It costs {cost} credits."),
            move |app| {
                let versions = match compose::versions(&post) {
                    Ok(v) => v,
                    Err(e) => return app.error(e),
                };
                let files = post.files.clone();
                app.act(
                    move |api| async move {
                        let r = api
                            .call("delete_post", vec![json!(id), json!(versions)])
                            .await?;
                        compose::free_files(&api, &files).await;
                        Ok(r)
                    },
                    "post deleted",
                );
            },
        );
    }

    pub fn poll_vote(&mut self, id: PostId) {
        if !self.need_user() {
            return;
        }
        let Some(it) = self.item(id) else { return };
        let Some(poll) = it.post.poll() else {
            return self.error("not a poll");
        };
        let mut m = Menu::new("Vote");
        for (i, o) in poll.options.iter().enumerate() {
            let i = i as u16;
            m = m.add(format!("{}. {o}", i + 1), move |app| {
                app.popup = Some(Popup::Menu(
                    Menu::new("Submit")
                        .add("Submit", move |app| {
                            app.act(
                                move |api| async move {
                                    api.call(
                                        "vote_on_poll",
                                        vec![json!(id), json!(i), json!(false)],
                                    )
                                    .await
                                },
                                "voted",
                            )
                        })
                        .add("Submit anonymously", move |app| {
                            app.act(
                                move |api| async move {
                                    api.call("vote_on_poll", vec![json!(id), json!(i), json!(true)])
                                        .await
                                },
                                "voted anonymously",
                            )
                        }),
                ))
            });
        }
        self.popup = Some(Popup::Menu(m));
    }

    pub fn realm_cleanup(&mut self, id: PostId) {
        let Some(it) = self.item(id) else { return };
        let Some(realm) = it.post.realm.clone() else {
            return self.error("the post is in no realm");
        };
        let f = Form::new(format!("Remove #{id} from /{realm}"), "remove")
            .note("Realm controllers can move posts out of their realm; the author pays the realm's clean-up penalty.")
            .field(Field::text("Reason", "rules violation"))
            .on_submit(move |app, v| {
                let reason = v[0].clone();
                app.act(move |api| async move { api.call("realm_clean_up", vec![json!(id), json!(reason)]).await }, "post moved out of the realm");
                Ok(())
            });
        self.popup = Some(Popup::Form(f));
    }

    pub fn post_info(&mut self, id: PostId) {
        let Some(it) = self.item(id) else { return };
        let p = &it.post;
        let mut md = format!(
            "**Post #{}** by @{}\n\n- created: {}\n",
            p.id,
            it.meta.author_name,
            super::page::datetime(p.timestamp)
        );
        if let Some(r) = &p.realm {
            md += &format!("- realm: /{r}\n");
        }
        if let Some(par) = p.parent {
            md += &format!("- reply to: #{par}\n");
        }
        md += &format!(
            "- comments in tree: {}\n- watchers: {}\n",
            p.tree_size,
            p.watchers.len()
        );
        if !p.reactions.is_empty() {
            md += "\n**Reactions**\n\n";
            for (r, users) in &p.reactions {
                let names: Vec<String> = users
                    .iter()
                    .map(|u| format!("@{}", self.name(*u)))
                    .collect();
                md += &format!("- {} {}\n", super::page::reaction_icon(*r), names.join(" "));
            }
        }
        if !p.tips.is_empty() {
            md += "\n**Tips**\n\n";
            for (u, a) in &p.tips {
                md += &format!(
                    "- @{}: {} {}\n",
                    self.name(*u),
                    amount(*a, self.backend.token_decimals),
                    self.backend.token_symbol
                );
            }
        }
        let mut m = Menu::new(format!("Post #{id}"))
            .add("Details", move |app| app.text_popup("Post info", &md));
        if !p.patches.is_empty() {
            match compose::versions(p) {
                Ok(vs) => {
                    let n = vs.len();
                    let times: Vec<u64> =
                        p.patches.iter().map(|x| x.0).chain([p.timestamp]).collect();
                    for (i, v) in vs.into_iter().enumerate() {
                        let label = if i + 1 == n {
                            format!("Version {i} (current)")
                        } else {
                            format!("Version {i} ({})", super::page::ago(times[i]))
                        };
                        m = m.add(label, move |app| app.text_popup(format!("Version {i}"), &v));
                    }
                }
                Err(e) => self.error(e),
            }
        }
        self.popup = Some(Popup::Menu(m));
    }

    pub fn links_menu(&mut self, id: PostId) {
        let Some(it) = self.item(id) else { return };
        let r = crate::markdown::render(&it.post.body, 200);
        let mut m = Menu::new("Links");
        let web = self.web_url(&format!("post/{id}"));
        m = m.add(format!("this post on the web: {web}"), move |app| {
            app.open_url(&web)
        });
        m = m.add(format!("@{}", it.meta.author_name), {
            let a = it.meta.author_name.clone();
            move |app| app.open_target(Target::User(a))
        });
        if let Some(rm) = it.post.realm.clone() {
            m = m.add(format!("/{rm}"), move |app| {
                app.open_target(Target::Realm(rm))
            });
        }
        for (i, url) in it
            .post
            .images(self.api.local.as_deref())
            .into_values()
            .enumerate()
        {
            m = m.add(format!("picture {}", i + 1), move |app| app.open_url(&url));
        }
        for l in r.links {
            m = m.add(l.label(), move |app| app.follow_link(l));
        }
        self.popup = Some(Popup::Menu(m));
    }

    pub fn page_links_menu(&mut self) {
        let text = match self.pages.last() {
            Some(p) if matches!(p.kind, Kind::Whitepaper) => p.data.text.clone(),
            _ => super::view::LINKS.to_string(),
        };
        let r = crate::markdown::render(&text, 200);
        let mut m = Menu::new("Links");
        for l in r.links {
            m = m.add(l.label(), move |app| app.follow_link(l));
        }
        self.popup = Some(Popup::Menu(m));
    }

    pub fn follow_link(&mut self, l: crate::markdown::Link) {
        use crate::markdown::Link;
        match l {
            Link::Url(u) => self.open_url(&u),
            Link::User(u) => self.open_target(Target::User(u)),
            Link::Tag(t) => {
                // `#/feed/a+b` style multi-tag feeds
                let tags: Vec<String> = t
                    .split('+')
                    .map(|s| s.trim_start_matches('@').to_string())
                    .collect();
                let title = t
                    .split('+')
                    .map(|s| format!("#{s}"))
                    .collect::<Vec<_>>()
                    .join(" ");
                self.push(Kind::Tags(tags), &title)
            }
            Link::Realm(r) => self.open_target(Target::Realm(r)),
            Link::Post(p) => self.open_target(Target::Post(p)),
        }
    }

    // ---- users ------------------------------------------------------------------------

    pub fn toggle_follow(&mut self, uid: u64) {
        if !self.need_user() {
            return;
        }
        let on = self
            .user
            .as_ref()
            .is_some_and(|u| u.followees.contains(&uid));
        self.act(
            move |api| async move {
                api.call_any("toggle_following_user", vec![json!(uid)])
                    .await
            },
            if on { "unfollowed" } else { "following" },
        );
    }

    pub fn toggle_block(&mut self, uid: u64) {
        if !self.need_user() {
            return;
        }
        let on = self
            .user
            .as_ref()
            .is_some_and(|u| u.blacklist.contains(&uid));
        let go = move |app: &mut App| {
            app.act(
                move |api| async move { api.call_any("toggle_blacklist", vec![json!(uid)]).await },
                if on { "unblocked" } else { "blocked" },
            )
        };
        if on {
            go(self)
        } else {
            self.confirm(
                format!(
                    "Block @{}? They cannot reply to your posts anymore.",
                    self.name(uid)
                ),
                go,
            );
        }
    }

    pub fn toggle_filter(&mut self, filter: &'static str, value: String) {
        if !self.need_user() {
            return;
        }
        self.act(
            move |api| async move {
                api.call_any("toggle_filter", vec![json!(filter), json!(value)])
                    .await
            },
            "filter updated",
        );
    }

    pub fn unlist(&mut self, kind: &'static str, v: String) {
        match kind {
            "blacklist" => self.toggle_block(v.parse().unwrap_or(0)),
            k => self.toggle_filter(k, v),
        }
    }

    pub fn send_credits_form(&mut self, uid: u64) {
        if !self.need_user() {
            return;
        }
        let f = Form::new(format!("Send credits to @{}", self.name(uid)), "send")
            .note("The transfer fee is 1 credit.")
            .field(Field::text("Credits", ""))
            .on_submit(move |app, v| {
                let n = num(&v[0])?;
                app.act(
                    move |api| async move {
                        api.call("transfer_credits", vec![json!(uid), json!(n)])
                            .await
                    },
                    "credits sent",
                );
                Ok(())
            });
        self.popup = Some(Popup::Form(f));
    }

    pub fn report_form(&mut self, uid: u64) {
        if !self.need_user() {
            return;
        }
        let penalty = self.backend.reporting_penalty_misbehaviour;
        let max = self.backend.max_report_length;
        let f = Form::new(format!("Report @{}", self.name(uid)), "report")
            .note(&format!(
                "You are about to report this user to stalwarts. If there is a chance to convince the user to stop misbehaving, do that without a report! If the report gets rejected, you lose {penalty} credits and rewards. Justify the report well."
            ))
            .field(Field::long("Reason", ""))
            .on_submit(move |app, v| {
                let reason = v[0].trim().to_string();
                if reason.is_empty() || reason.len() > max {
                    return Err(format!("the reason must have 1 to {max} characters"));
                }
                app.act(move |api| async move { api.call("report", vec![json!(uid), json!(reason)]).await }, "Report accepted! Thank you!");
                Ok(())
            });
        self.popup = Some(Popup::Form(f));
    }

    pub fn vote_report(&mut self, uid: u64, agree: bool) {
        if !self.user.as_ref().is_some_and(|u| u.stalwart) {
            return self.error("only stalwarts vote on reports");
        }
        self.act(
            move |api| async move {
                api.call("vote_on_report", vec![json!(uid), json!(agree)])
                    .await
            },
            "vote recorded",
        );
    }

    // ---- realms, feeds ------------------------------------------------------------

    pub fn toggle_membership(&mut self, r: String) {
        if !self.need_user() {
            return;
        }
        let member = self.user.as_ref().is_some_and(|u| u.realms.contains(&r));
        let go = move |app: &mut App| {
            app.act(
                move |api| async move {
                    api.call_any("toggle_realm_membership", vec![json!(r)])
                        .await
                },
                if member {
                    "left the realm"
                } else {
                    "joined the realm"
                },
            )
        };
        if member {
            self.confirm("Leave the realm?", go)
        } else {
            go(self)
        }
    }

    pub fn toggle_feed(&mut self, tags: Vec<String>) {
        if !self.need_user() {
            return;
        }
        self.act(
            move |api| async move {
                api.call_any("toggle_following_feed", vec![json!(tags)])
                    .await
            },
            "feeds updated",
        );
    }

    pub fn realm_form(&mut self, existing: Option<String>) {
        if !self.need_user() {
            return;
        }
        let realm = existing
            .as_ref()
            .and_then(|r| {
                self.pages
                    .iter()
                    .rev()
                    .find(|p| p.kind == Kind::Realm(r.clone()))
            })
            .and_then(|p| p.data.realm.clone());
        if existing.is_some() && realm.is_none() {
            return self.error("realm not loaded yet");
        }
        let max_downvotes = self
            .backend
            .rest
            .get("default_max_downvotes")
            .and_then(|v| v.as_u64())
            .unwrap_or(5) as u32;
        let r = realm.unwrap_or_else(|| api::Realm {
            comments_filtering: true,
            label_color: "#ffffff".into(),
            max_downvotes,
            ..Default::default()
        });
        let names = |ids: &[u64]| {
            ids.iter()
                .map(|i| self.name(*i))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let dec = self.backend.token_decimals;
        let me = self
            .user
            .as_ref()
            .map(|u| u.name.clone())
            .unwrap_or_default();
        let controllers = if r.controllers.is_empty() {
            me
        } else {
            names(&r.controllers)
        };
        let title = match &existing {
            Some(n) => format!("Edit realm {n}"),
            None => format!("Create a realm (costs {} credits)", self.backend.realm_cost),
        };
        let mut f = Form::new(title, "save");
        if existing.is_none() {
            f = f.field(Field::text("Name", ""));
        }
        let editing = existing.clone();
        let base = r.clone();
        f = f
            .field(Field::long("Description", &r.description))
            .field(Field::text(
                "Controllers (names, comma-separated)",
                &controllers,
            ))
            .field(Field::text("Label colour", &r.label_color))
            .field(Field::text("Theme (JSON)", &r.theme))
            .field(Field::text("Logo (image file path, empty: keep)", ""))
            .field(Field::bool("Adult content", r.adult_content))
            .field(Field::bool("Comments filtering", r.comments_filtering))
            .field(Field::text(
                "Clean-up penalty (credits)",
                &r.cleanup_penalty.to_string(),
            ))
            .field(Field::text("Max downvotes", &r.max_downvotes.to_string()))
            .field(Field::text(
                "Whitelist (names, comma-separated)",
                &names(&r.whitelist),
            ))
            .field(Field::text(
                "Min. account age (days)",
                &r.filter.age_days.to_string(),
            ))
            .field(Field::text(
                "Min. token balance",
                &amount(r.filter.balance, dec),
            ))
            .field(Field::text(
                "Min. followers",
                &r.filter.num_followers.to_string(),
            ))
            .field(Field::bool("Non-controversial users only", r.filter.safe))
            .on_submit(move |app, v| {
                let mut v = v.to_vec();
                let name = match &editing {
                    Some(n) => n.clone(),
                    None => v.remove(0).trim().to_uppercase(),
                };
                if name.is_empty() {
                    return Err("enter a name".into());
                }
                if v[0].trim().is_empty() {
                    return Err("enter a description".into());
                }
                let mut realm = base.clone();
                realm.description = v[0].clone();
                realm.label_color = v[2].trim().to_string();
                realm.theme = v[3].trim().to_string();
                let logo = v[4].trim().to_string();
                realm.adult_content = v[5] == "true";
                realm.comments_filtering = v[6] == "true";
                realm.cleanup_penalty = num(&v[7])?;
                realm.max_downvotes = num(&v[8])? as u32;
                realm.filter = UserFilter {
                    age_days: num(&v[10])?,
                    balance: parse_amount(&v[11], dec)?,
                    num_followers: num(&v[12])?,
                    safe: v[13] == "true",
                };
                let logo = if logo.is_empty() {
                    None
                } else {
                    Some(compose::logo(&logo, app.backend.max_realm_logo_len)?)
                };
                let split = |s: &str| {
                    s.split(',')
                        .map(|x| x.trim().trim_start_matches('@').to_string())
                        .filter(|x| !x.is_empty())
                        .collect::<Vec<_>>()
                };
                let (ctl, wl) = (split(&v[1]), split(&v[9]));
                let create = editing.is_none();
                app.act(
                    move |api| async move {
                        realm.controllers = resolve(&api, &ctl).await?;
                        realm.whitelist = resolve(&api, &wl).await?;
                        if let Some(l) = logo {
                            realm.logo = l;
                        }
                        if !create {
                            return api
                                .call(
                                    "edit_realm",
                                    vec![json!(name), serde_json::to_value(&realm)?],
                                )
                                .await;
                        }
                        api.call(
                            "create_realm",
                            vec![json!(name), serde_json::to_value(&realm)?],
                        )
                        .await?;
                        // the creator joins, as on the web
                        api.call_any("toggle_realm_membership", vec![json!(name)])
                            .await
                    },
                    if existing.is_some() {
                        "realm updated"
                    } else {
                        "realm created"
                    },
                );
                Ok(())
            });
        self.popup = Some(Popup::Form(f));
    }

    // ---- inbox ----------------------------------------------------------------------

    pub fn dismiss(&mut self, ids: Vec<u64>, say: bool) {
        if let Some(u) = self.user.as_mut() {
            for i in &ids {
                if let Some(n) = u.notifications.get_mut(i) {
                    n.1 = true;
                }
            }
        }
        self.build_nav();
        if let Some(p) = self.page() {
            p.dirty();
        }
        self.bg(
            move |api| async move { api.call("clear_notifications", vec![json!(ids)]).await },
            move |app, _| {
                if say {
                    app.info("dismissed");
                }
                app.load_user();
            },
        );
    }

    // ---- wallet --------------------------------------------------------------------------

    pub fn send_icp_form(&mut self) {
        let f = Form::new("Send ICP", "send")
            .note("The ledger fee of 0.0001 ICP is added.")
            .field(Field::text("Account id (hex) or principal", ""))
            .field(Field::text("Amount (ICP)", ""))
            .on_submit(|app, v| {
                let to = v[0].trim().to_string();
                let e8s = parse_amount(&v[1], 8)?;
                app.confirm(format!("Send {} ICP to {to}?", icp(e8s)), move |app| {
                    app.act(
                        move |api| async move {
                            let idx = if to.contains('-') {
                                let p = candid::Principal::from_text(&to)?;
                                let ledger = candid::Principal::from_text(api::ICP_LEDGER)?;
                                api.icrc_transfer(
                                    &ledger,
                                    api::IcrcAccount {
                                        owner: p,
                                        subaccount: None,
                                    },
                                    e8s,
                                    api::ICP_FEE,
                                )
                                .await?
                            } else {
                                api.icp_transfer(&to, e8s, 0).await?
                            };
                            Ok(json!(idx))
                        },
                        "ICP sent",
                    )
                });
                Ok(())
            });
        self.popup = Some(Popup::Form(f));
    }

    pub fn withdraw_rewards(&mut self) {
        self.confirm("Withdraw your rewards as ICP?", |app| {
            app.act(
                |api| async move { api.call("withdraw_rewards", vec![]).await },
                "rewards withdrawn",
            )
        });
    }

    pub fn mint_credits_form(&mut self) {
        let max = self.backend.max_credits_mint_kilos;
        let f = Form::new("Mint credits", "mint")
            .note(&format!("Enter the number of 1000s of credits to mint (max: {max}). The ICP is paid from your wallet."))
            .field(Field::text("Thousands of credits", "1"))
            .on_submit(move |app, v| {
                let kilos = num(&v[0])?;
                if kilos == 0 || kilos > max {
                    return Err(format!("between 1 and {max}"));
                }
                app.info("creating invoice…");
                app.bg(
                    |api| async move { api.call("mint_credits_with_icp", vec![json!(0)]).await },
                    move |app, inv: Value| {
                        let inv: api::Invoice = serde_json::from_value(inv).unwrap_or_default();
                        let amount = inv.e8s * kilos;
                        let account = inv.account_hex();
                        app.confirm(format!("Pay {} ICP (+ fee) for {} credits?", icp(amount), kilos * 1000), move |app| {
                            app.act(
                                move |api| async move {
                                    api.icp_transfer(&account, amount, 0).await.map_err(|e| anyhow!("couldn't transfer ICP for minting ({e}); make sure you have {} ICP", icp(amount + api::ICP_FEE)))?;
                                    api.call("mint_credits_with_icp", vec![json!(kilos)]).await
                                },
                                "credits minted",
                            )
                        });
                    },
                );
                Ok(())
            });
        self.popup = Some(Popup::Form(f));
    }

    pub fn send_tokens_form(&mut self, canister: String, sym: String, dec: u32, fee: u64) {
        let f = Form::new(format!("Send {sym}"), "send")
            .note(&format!("Fee: {} {sym}", amount(fee, dec)))
            .field(Field::text(
                "Recipient principal (or @name for Taggr users)",
                "",
            ))
            .field(Field::text(&format!("Amount ({sym})"), ""))
            .on_submit(move |app, v| {
                let to = v[0].trim().to_string();
                let a = parse_amount(&v[1], dec)?;
                let (canister, sym) = (canister.clone(), sym.clone());
                app.confirm(
                    format!("Send {} {sym} to {to}?", amount(a, dec)),
                    move |app| {
                        app.act(
                            move |api| async move {
                                let owner = if let Some(name) = to.strip_prefix('@') {
                                    let u = api
                                        .user(Some(name))
                                        .await?
                                        .ok_or_else(|| anyhow!("no user {name}"))?;
                                    candid::Principal::from_text(&u.principal)?
                                } else {
                                    candid::Principal::from_text(&to)?
                                };
                                let c = candid::Principal::from_text(&canister)?;
                                let idx = api
                                    .icrc_transfer(
                                        &c,
                                        api::IcrcAccount {
                                            owner,
                                            subaccount: None,
                                        },
                                        a,
                                        fee,
                                    )
                                    .await?;
                                Ok(json!(idx))
                            },
                            "sent",
                        )
                    },
                );
                Ok(())
            });
        self.popup = Some(Popup::Form(f));
    }

    pub fn unlink_cold_wallet(&mut self) {
        self.confirm(
            "Unlinking the cold wallet reduces your voting power. Unlink?",
            |app| {
                app.act(
                    |api| async move { api.unlink_cold_wallet().await.map(|_| Value::Null) },
                    "cold wallet unlinked",
                )
            },
        );
    }

    pub fn add_wallet_token_form(&mut self) {
        let f = Form::new("Add ICRC token", "add")
            .field(Field::text("Ledger canister id", ""))
            .on_submit(|app, v| {
                let c = v[0].trim().to_string();
                candid::Principal::from_text(&c).map_err(|e| e.to_string())?;
                let mut list = app
                    .user
                    .as_ref()
                    .map(|u| u.wallet_tokens.clone())
                    .unwrap_or_default();
                if !list.contains(&c) {
                    list.push(c.clone());
                }
                app.act(
                    move |api| async move {
                        api.icrc_meta(&candid::Principal::from_text(&c)?).await?;
                        api.call("update_wallet_tokens", vec![json!(list)]).await
                    },
                    "token added",
                );
                Ok(())
            });
        self.popup = Some(Popup::Form(f));
    }

    pub fn remove_wallet_token(&mut self, c: String) {
        let list: Vec<String> = self
            .user
            .as_ref()
            .map(|u| {
                u.wallet_tokens
                    .iter()
                    .filter(|t| **t != c)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        self.act(
            move |api| async move { api.call("update_wallet_tokens", vec![json!(list)]).await },
            "token removed",
        );
    }

    pub fn show_tx(&mut self, id: u32) {
        let dec = self.backend.token_decimals;
        let sym = self.backend.token_symbol.clone();
        self.bg(
            move |api| async move { api.transaction(id).await },
            move |app, t: api::Transaction| {
                let acc = |a: &api::Account| match &a.subaccount {
                    Some(s) if s.iter().any(|b| *b != 0) => format!("{} (sub {})", a.owner, hex::encode(s)),
                    _ => a.owner.clone(),
                };
                let mut md = format!(
                    "- id: {id}\n- time: {}\n- from: `{}`\n- to: `{}`\n- amount: {} {sym}\n- fee: {} {sym}\n",
                    super::page::datetime(t.timestamp),
                    acc(&t.from),
                    acc(&t.to),
                    amount(t.amount, dec),
                    amount(t.fee, dec)
                );
                if let Some(m) = &t.memo {
                    md += &format!("- memo: `{}`\n", String::from_utf8(m.clone()).unwrap_or_else(|_| hex::encode(m)));
                }
                let (from, to) = (t.from.owner.clone(), t.to.owner.clone());
                let mut m = Menu::new(format!("Transaction {id}")).add("Details", move |app| app.text_popup(format!("Transaction {id}"), &md));
                m = m.add(format!("Transactions of {}", super::page::fit(&from, 30)), {
                    let f = from.clone();
                    move |app| app.push(Kind::Transactions(f.clone(), String::new()), &format!("transactions of {f}"))
                });
                m = m.add(format!("Transactions of {}", super::page::fit(&to, 30)), move |app| {
                    app.push(Kind::Transactions(to.clone(), String::new()), &format!("transactions of {to}"))
                });
                app.popup = Some(Popup::Menu(m));
            },
        );
    }

    // ---- invites -----------------------------------------------------------------------------

    pub fn invite_form(&mut self, code: Option<String>) {
        if !self.need_user() {
            return;
        }
        let existing = code.as_ref().and_then(|c| {
            self.pages
                .last()
                .and_then(|p| p.data.invites.iter().find(|(k, _)| k == c))
                .map(|x| x.1.clone())
        });
        let inv = existing.unwrap_or_default();
        let min = self.backend.min_credits_for_inviting;
        let mut f = Form::new(
            if code.is_some() {
                "Edit invite"
            } else {
                "Create invite"
            },
            "save",
        );
        f = f.field(Field::text(
            "Credits",
            &if code.is_some() {
                inv.credits.to_string()
            } else {
                min.to_string()
            },
        ));
        if code.is_none() {
            f = f.field(Field::text("Credits per user (empty: all)", ""));
        }
        f = f.field(Field::text(
            "Realm to join (optional)",
            inv.realm_id.as_deref().unwrap_or(""),
        ));
        if let Some(c) = &code {
            f = f.note(&format!(
                "Invite link: {}",
                self.web_url(&format!("welcome/{c}"))
            ));
        }
        f = f.on_submit(move |app, v| {
            let credits = num(&v[0])?;
            match &code {
                None => {
                    let per = if v[1].trim().is_empty() {
                        None
                    } else {
                        Some(num(&v[1])?)
                    };
                    let realm = Some(v[2].trim().to_uppercase()).filter(|s| !s.is_empty());
                    app.act(
                        move |api| async move {
                            api.call(
                                "create_invite",
                                vec![json!(credits), json!(per), json!(realm)],
                            )
                            .await
                        },
                        "invite created",
                    );
                }
                Some(c) => {
                    let c = c.clone();
                    let realm = Some(v[1].trim().to_uppercase()).filter(|s| !s.is_empty());
                    app.act(
                        move |api| async move {
                            api.call(
                                "update_invite",
                                vec![json!(c), json!(Some(credits)), json!(realm)],
                            )
                            .await
                        },
                        "invite updated",
                    );
                }
            }
            Ok(())
        });
        self.popup = Some(Popup::Form(f));
    }

    // ---- settings -------------------------------------------------------------------------

    pub fn profile_form(&mut self) {
        let Some(u) = self.user.clone() else { return };
        let cost = self.backend.identity_change_cost;
        let sym = self.backend.token_symbol.clone();
        let f = Form::new("Profile", "save")
            .note(&format!(
                "A name change costs {cost} credits; the old name keeps routing to you."
            ))
            .field(Field::text("Name", &u.name))
            .field(Field::long("About", &u.about))
            .field(
                Field::choice("Usage mode", &["Credits", "Rewards", "Mining"], &u.mode).describe(
                    &[
                        "Convert rewards to credits automatically".into(),
                        "Receive ICP rewards".into(),
                        format!("Mine {sym} tokens"),
                    ],
                ),
            )
            .field(Field::bool("Participate in governance", u.governance))
            .field(Field::long(
                "PGP public key",
                u.settings.get("pgp").map(String::as_str).unwrap_or(""),
            ))
            .field(Field::bool(
                "ICRC tokens in the wallet",
                u.settings.get("icrcWallet").is_some_and(|v| v == "true"),
            ))
            .on_submit(move |app, v| {
                let name = v[0].trim().to_string();
                let new_name = if name == u.name { String::new() } else { name };
                let about = v[1].clone();
                let mode = v[2].clone();
                let gov = v[3] == "true";
                let mut settings = u.settings.clone();
                settings.insert("pgp".into(), v[4].clone());
                settings.insert("icrcWallet".into(), v[5].clone());
                let noise = u.filters.noise.clone();
                let realms = u.show_posts_in_realms;
                let go = move |app: &mut App| {
                    app.act(
                        move |api| async move {
                            api.call(
                                "update_user",
                                vec![
                                    json!(new_name),
                                    json!(about),
                                    serde_json::to_value(&noise)?,
                                    json!(gov),
                                    json!(mode),
                                    json!(realms),
                                ],
                            )
                            .await?;
                            api.call("update_user_settings", vec![json!(settings)])
                                .await
                        },
                        "profile saved",
                    )
                };
                if v[0].trim() != u.name {
                    app.confirm(
                        format!("Changing the name costs {cost} credits. Continue?"),
                        go,
                    );
                } else {
                    go(app);
                }
                Ok(())
            });
        self.popup = Some(Popup::Form(f));
    }

    pub fn filter_form(&mut self) {
        let Some(u) = self.user.clone() else { return };
        let dec = self.backend.token_decimals;
        let n = &u.filters.noise;
        let f = Form::new("Noise filter", "save")
            .field(Field::text(
                "Min. account age (days)",
                &n.age_days.to_string(),
            ))
            .field(Field::text("Min. token balance", &amount(n.balance, dec)))
            .field(Field::text("Min. followers", &n.num_followers.to_string()))
            .field(Field::bool("Non-controversial users only", n.safe))
            .field(Field::bool(
                "Show posts of followed users in realms I'm not in",
                u.show_posts_in_realms,
            ))
            .on_submit(move |app, v| {
                let filter = UserFilter {
                    age_days: num(&v[0])?,
                    balance: parse_amount(&v[1], dec)?,
                    num_followers: num(&v[2])?,
                    safe: v[3] == "true",
                };
                let realms = v[4] == "true";
                let (about, gov, mode) = (u.about.clone(), u.governance, u.mode.clone());
                app.act(
                    move |api| async move {
                        api.call(
                            "update_user",
                            vec![
                                json!(""),
                                json!(about),
                                serde_json::to_value(&filter)?,
                                json!(gov),
                                json!(mode),
                                json!(realms),
                            ],
                        )
                        .await
                    },
                    "filter saved",
                );
                Ok(())
            });
        self.popup = Some(Popup::Form(f));
    }

    pub fn crypt_form(&mut self) {
        let Some(u) = &self.user else { return };
        let active = !u.deactivated;
        let cost = self.backend.account_activation_cost;
        let f = Form::new(if active { "Deactivate account" } else { "Activate account" }, if active { "deactivate" } else { "activate" })
            .note(if active {
                "All your posts get encrypted with this password and your account is suspended. Keep the password: it is the only way back."
            } else {
                "Decrypts your posts with the password used for deactivation."
            })
            .note(&format!("Costs {cost} credits."))
            .field(Field::secret("Encryption password", ""))
            .on_submit(move |app, v| {
                let pw = v[0].clone();
                if pw.is_empty() {
                    return Err("enter the password".into());
                }
                app.bg(
                    move |api| async move { api.call("crypt", vec![json!(pw)]).await },
                    move |app, n: Value| {
                        app.info(format!("{n} posts successfully {}crypted!", if active { "en" } else { "de" }));
                        app.load_user();
                        app.reload();
                    },
                );
                Ok(())
            });
        self.popup = Some(Popup::Form(f));
    }

    pub fn principal_change_form(&mut self) {
        let f = Form::new("Change principal", "request")
            .note("1. Sign in elsewhere with the new seed phrase or method and copy the principal shown on its Welcome page. 2. Enter it here. 3. Sign in with the new principal: Welcome → Confirm principal change. Withdraw all ICP first.")
            .field(Field::text("New principal", ""))
            .on_submit(|app, v| {
                let p = v[0].trim().to_string();
                candid::Principal::from_text(&p).map_err(|e| format!("invalid principal: {e}"))?;
                let me = app.principal.clone();
                let bucket = app.user.as_ref().and_then(|u| u.bucket.clone());
                app.act(
                    move |api| async move {
                        let bal = api.icp_balance(candid::Principal::from_text(&me)?).await.unwrap_or(0);
                        if bal > 0 {
                            return Err(anyhow!("your ICP balance is not empty; withdraw all funds before changing the principal"));
                        }
                        if let Some(b) = bucket {
                            // keep control of the storage canister
                            crate::storage::add_controller(&api, &b, &p).await?;
                        }
                        api.call("request_principal_change", vec![json!(p)]).await
                    },
                    "request registered: sign in with the new principal to confirm",
                );
                Ok(())
            });
        self.popup = Some(Popup::Form(f));
    }

    pub fn confirm_principal_change(&mut self) {
        self.bg(
            |api| async move {
                if !api.migration_pending().await? {
                    return Err(anyhow!("no principal change pending for this principal"));
                }
                api.call("confirm_principal_change", vec![]).await
            },
            |app, _| {
                app.info("principal changed");
                app.load_user();
                app.open_root(Kind::Home, "Home");
            },
        );
    }

    // ---- welcome ---------------------------------------------------------------------------

    pub fn join_form(&mut self, invite: String) {
        let f = Form::new("Create your account", "join")
            .note("Names: 2–16 letters or digits, starting with a letter. Leave the invite empty if you paid with ICP.")
            .field(Field::text("Name", ""))
            .field(Field::text("Invite code", &invite))
            .on_submit(|app, v| {
                let name = v[0].trim().to_string();
                let code = v[1].trim().to_string();
                app.bg(
                    move |api| async move {
                        api.validate_username(&name).await?;
                        if !code.is_empty() && !api.check_invite(&code).await? {
                            return Err(anyhow!("invalid invite code"));
                        }
                        let realm = api.call("create_user", vec![json!(name), json!(code)]).await?;
                        if !code.is_empty() {
                            // invited users start converting rewards to credits
                            let _ = api.call("update_user", vec![json!(""), json!(""), serde_json::to_value(UserFilter::default())?, json!(false), json!("Credits"), json!(false)]).await;
                        }
                        Ok(realm)
                    },
                    |app, realm: Value| {
                        app.info("welcome aboard!");
                        app.load_user();
                        match realm.as_str() {
                            Some(r) if !r.is_empty() => app.open_root(Kind::Realm(r.to_string()), &format!("/{r}")),
                            _ => app.open_root(Kind::New, "New"),
                        }
                    },
                );
                Ok(())
            });
        self.popup = Some(Popup::Form(f));
    }

    pub fn welcome_mint(&mut self) {
        self.bg(
            |api| async move { api.call("mint_credits_with_icp", vec![json!(0)]).await },
            |app, inv: Value| {
                let inv: api::Invoice = serde_json::from_value(inv).unwrap_or_default();
                if inv.paid {
                    app.info("Credits minted! ✅ Now register.");
                    return app.join_form(String::new());
                }
                let md = format!(
                    "To mint **1000 credits**, send at least **{} ICP** to this account:\n\n`{}`\n\nThen choose *Mint* below; once the payment is seen, *Register (after paying)*.",
                    icp(inv.e8s),
                    inv.account_hex()
                );
                let m = Menu::new("Mint credits with ICP")
                    .add("Show payment details", move |app| app.text_popup("Payment", &md))
                    .add("Mint (after paying)", |app| {
                        app.bg(
                            |api| async move { api.call("mint_credits_with_icp", vec![json!(1)]).await },
                            |app, inv: Value| {
                                let inv: api::Invoice = serde_json::from_value(inv).unwrap_or_default();
                                if inv.paid {
                                    app.info("Credits minted! ✅");
                                    app.join_form(String::new());
                                } else {
                                    app.error("payment not seen yet; try again in a moment");
                                }
                            },
                        )
                    });
                app.popup = Some(Popup::Menu(m));
            },
        );
    }

    // ---- governance -------------------------------------------------------------------------

    fn proposal_value(&self) -> Option<Value> {
        let p = self.pages.last()?;
        (!p.data.json.is_null()).then(|| p.data.json.clone())
    }

    pub fn vote_proposal(&mut self, adopt: bool) {
        if !self.need_user() {
            return;
        }
        let Some(pr) = self.proposal_value() else {
            return;
        };
        let id = pr["id"].as_u64().unwrap_or(0);
        let send = move |app: &mut App, data: String| {
            app.act(
                move |api| async move {
                    api.call(
                        "vote_on_proposal",
                        vec![json!(id), json!(adopt), json!(data)],
                    )
                    .await
                },
                "vote recorded",
            )
        };
        if !adopt {
            return self.confirm(
                "You're rejecting the proposal. Please confirm.",
                move |app| send(app, String::new()),
            );
        }
        if pr["payload"].get("Release").is_some() {
            let f = Form::new("Accept release", "accept")
                .note("Enter the build hash from the source code commit mentioned in the proposal (proves the binary can be reproduced from the source).")
                .field(Field::text("Build hash", ""))
                .on_submit(move |app, v| {
                    send(app, v[0].trim().to_string());
                    Ok(())
                });
            self.popup = Some(Popup::Form(f));
        } else if pr["payload"].get("Rewards").is_some() {
            let max = self
                .backend
                .rest
                .get("max_funding_amount")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let (sym, dec) = (
                self.backend.token_symbol.clone(),
                self.backend.token_decimals,
            );
            let f = Form::new("Accept rewards", "accept")
                .note(&format!("Enter the amount of {sym} tokens that would be an appropriate reward for the efforts described (max. {} {sym}).", amount(max, dec)))
                .field(Field::text("Tokens", ""))
                .on_submit(move |app, v| {
                    send(app, v[0].trim().to_string());
                    Ok(())
                });
            self.popup = Some(Popup::Form(f));
        } else {
            self.confirm(
                "You're accepting the proposal. Please confirm.",
                move |app| send(app, String::new()),
            );
        }
    }

    pub fn cancel_proposal(&mut self) {
        let Some(pr) = self.proposal_value() else {
            return;
        };
        let id = pr["id"].as_u64().unwrap_or(0);
        self.confirm(
            "You're cancelling the proposal. Please confirm.",
            move |app| {
                app.act(
                    move |api| async move { api.call("cancel_proposal", vec![json!(id)]).await },
                    "proposal cancelled",
                )
            },
        );
    }

    pub fn new_proposal_menu(&mut self) {
        if !self.need_user() {
            return;
        }
        let m = Menu::new("New proposal")
            .add("Add realm controller", |app| {
                app.proposal_form("AddRealmController")
            })
            .add("ICP transfer", |app| app.proposal_form("ICPTransfer"))
            .add("Funding", |app| app.proposal_form("Funding"))
            .add("Rewards", |app| app.proposal_form("Rewards"))
            .add("Release", |app| app.proposal_form("Release"));
        self.popup = Some(Popup::Menu(m));
    }

    pub fn proposal_form(&mut self, kind: &'static str) {
        self.bg(
            |api| async move {
                api.ic
                    .query::<(u64, u64)>("proposal_escrow_balance_required", vec![])
                    .await
            },
            move |app, (escrow, locked)| app.proposal_form_with(kind, escrow + locked),
        );
    }

    pub fn proposal_form_with(&mut self, kind: &'static str, need: u64) {
        let have = self.user.as_ref().map(|u| u.balance).unwrap_or(0);
        if have < need {
            return self.error(format!(
                "to open a new proposal, you need at least {} {} on your balance",
                amount(need, self.backend.token_decimals),
                self.backend.token_symbol
            ));
        }
        let dec = self.backend.token_decimals;
        let sym = self.backend.token_symbol.clone();
        let mut f = Form::new(format!("Proposal: {kind}"), "propose")
            .note("The description becomes the proposal post. An escrow of tokens is locked and burned if the proposal is rejected as controversial.")
            .field(Field::long("Description", ""));
        f = match kind {
            "AddRealmController" => f
                .field(Field::text("New controller (name)", ""))
                .field(Field::text("Realm", "")),
            "ICPTransfer" => f
                .field(Field::text("ICP account (hex)", ""))
                .field(Field::text("ICP amount", "")),
            "Funding" => f
                .field(Field::text("Principal", ""))
                .field(Field::text(&format!("Token amount ({sym})"), "")),
            "Rewards" => f.field(Field::text("Principal", "")),
            _ => f
                .field(Field::text("Git commit", ""))
                .field(Field::text("Binary (path to .wasm.gz)", "")),
        };
        f = f.on_submit(move |app, v| {
            let desc = v[0].trim().to_string();
            if desc.is_empty() {
                return Err("enter a description".into());
            }
            let (a, b) = (
                v[1].trim().to_string(),
                v.get(2).map(|s| s.trim().to_string()).unwrap_or_default(),
            );
            let binary = if kind == "Release" {
                Some(std::fs::read(shellexpand(&b)).map_err(|e| format!("{b}: {e}"))?)
            } else {
                None
            };
            let parsed = match kind {
                "ICPTransfer" => Some(parse_amount(&b, 8)?),
                "Funding" => Some(parse_amount(&b, dec)?),
                _ => None,
            };
            app.act(
                move |api| async move {
                    let payload = match kind {
                        "AddRealmController" => {
                            let u = api
                                .user(Some(a.trim_start_matches('@')))
                                .await?
                                .ok_or_else(|| anyhow!("no user {a}"))?;
                            json!({"AddRealmController": [b.to_uppercase(), u.id]})
                        }
                        "ICPTransfer" => {
                            let bytes =
                                hex::decode(&a).map_err(|_| anyhow!("invalid account id"))?;
                            json!({"ICPTransfer": [bytes, {"e8s": parsed}]})
                        }
                        "Funding" => json!({"Funding": [a, parsed]}),
                        "Rewards" => json!({"Rewards": {"receiver": a, "minted": 0}}),
                        _ => Value::Null,
                    };
                    if !payload.is_null() {
                        api.validate_proposal(&payload).await?;
                    }
                    let post = api.add_post(&desc, vec![], None, None, None).await?;
                    let r = match binary {
                        Some(bin) => api.propose_release(post, a, bin).await.map(|v| json!(v)),
                        None => api.call("create_proposal", vec![json!(post), payload]).await,
                    };
                    let r = match r {
                        Ok(r) => r,
                        Err(e) => {
                            return Err(anyhow!(
                                "post #{post} could be created, but the proposal creation failed: {e}"
                            ));
                        }
                    };
                    Ok(r)
                },
                "proposal created",
            );
            Ok(())
        });
        self.popup = Some(Popup::Form(f));
    }

    pub fn bid_form(&mut self) {
        let dec = self.backend.token_decimals;
        let sym = self.backend.token_symbol.clone();
        let account = self
            .pages
            .last()
            .and_then(|p| p.data.auction.as_ref().map(|a| a.1.clone()))
            .unwrap_or_default();
        let f = Form::new("Bid in the auction", "bid")
            .note("The ICP for the bid is transferred from your wallet to your auction deposit account first.")
            .field(Field::text(&format!("ICP per 1 {sym}"), ""))
            .field(Field::text(&format!("Number of {sym} tokens"), ""))
            .on_submit(move |app, v| {
                let price = parse_amount(&v[0], 8)?;
                let tokens = parse_amount(&v[1], dec)?;
                let e8s_per_token = price / 10u64.pow(dec);
                let payment = e8s_per_token * tokens;
                if payment == 0 {
                    return Err("bid too small".into());
                }
                let account = account.clone();
                app.confirm(format!("Transfer {} ICP to {account} and bid?", icp(payment)), move |app| {
                    app.act(
                        move |api| async move {
                            api.icp_transfer(&account, payment, 0).await?;
                            api.call("create_bid", vec![json!(tokens), json!(e8s_per_token)]).await
                        },
                        "bid created",
                    )
                });
                Ok(())
            });
        self.popup = Some(Popup::Form(f));
    }

    pub fn cancel_bid(&mut self) {
        self.confirm(
            "Your bid will be cancelled and the ICP funds moved to your wallet. Continue?",
            |app| {
                app.act(
                    |api| async move { api.call("cancel_bid", vec![]).await },
                    "bid cancelled",
                )
            },
        );
    }

    pub fn delay_chores(&mut self) {
        self.act(
            |api| async move { api.call("delay_weekly_chores", vec![]).await },
            "vote for delay recorded",
        );
    }

    // ---- domains ----------------------------------------------------------------------

    pub fn domain_menu(&mut self, d: String) {
        let mine = self
            .pages
            .last()
            .and_then(|p| p.data.domains.get(&d))
            .is_some_and(|c| c.owner == Some(self.me()));
        if !mine {
            return self.open_url(&format!("https://{d}"));
        }
        let d2 = d.clone();
        let m = Menu::new(d.clone())
            .add("Edit", move |app| app.domain_form(Some(d2)))
            .add("Open in browser", {
                let d = d.clone();
                move |app| app.open_url(&format!("https://{d}"))
            })
            .add("Remove", move |app| {
                let d = d.clone();
                app.confirm(format!("Remove the domain {d}?"), move |app| {
                    app.act(
                        move |api| async move {
                            api.call(
                                "set_domain_config",
                                vec![json!(d), json!({}), json!("remove")],
                            )
                            .await
                        },
                        "domain removed",
                    )
                })
            });
        self.popup = Some(Popup::Menu(m));
    }

    pub fn domain_form(&mut self, d: Option<String>) {
        let cfg = d
            .as_ref()
            .and_then(|d| {
                self.pages
                    .last()
                    .and_then(|p| p.data.domains.get(d))
                    .cloned()
            })
            .unwrap_or_default();
        let (mode, list) = match &cfg.sub_config {
            api::DomainSub::Journal(u) => ("Journal", self.name(*u)),
            api::DomainSub::WhiteListedRealms(r) => ("Whitelisted realms", r.join(", ")),
            api::DomainSub::BlackListedRealms(r) => ("Blacklisted realms", r.join(", ")),
        };
        let mut f = Form::new(
            if d.is_some() {
                "Edit domain"
            } else {
                "Add your domain"
            },
            "save",
        );
        if d.is_none() {
            f = f
                .note("Point the domain to the canister first (see the Taggr docs on custom domains), then register it here.")
                .field(Field::text("Domain", ""));
        }
        let existing = d.clone();
        f = f
            .field(Field::choice(
                "Domain content",
                &["Blacklisted realms", "Whitelisted realms", "Journal"],
                mode,
            ))
            .field(Field::text(
                "Realms (comma-separated) or journal user",
                &list,
            ))
            .field(Field::text(
                "Max downvotes",
                &cfg.max_downvotes.max(5).to_string(),
            ))
            .on_submit(move |app, v| {
                let mut v = v.to_vec();
                let (name, cmd) = match &existing {
                    Some(n) => (n.clone(), "update"),
                    None => (v.remove(0).trim().to_lowercase(), "insert"),
                };
                if name.is_empty() {
                    return Err("enter the domain".into());
                }
                let mode = v[0].clone();
                let list: Vec<String> = v[1]
                    .split(',')
                    .map(|s| s.trim().trim_start_matches('/').to_uppercase())
                    .filter(|s| !s.is_empty())
                    .collect();
                let journal = v[1].trim().trim_start_matches('@').to_string();
                let max = num(&v[2])? as u32;
                let me = app.me();
                app.act(
                    move |api| async move {
                        let sub = match mode.as_str() {
                            "Journal" => {
                                let u = api
                                    .user(Some(&journal))
                                    .await?
                                    .ok_or_else(|| anyhow!("no user {journal}"))?;
                                api::DomainSub::Journal(u.id)
                            }
                            "Whitelisted realms" => api::DomainSub::WhiteListedRealms(list),
                            _ => api::DomainSub::BlackListedRealms(list),
                        };
                        let cfg = api::DomainConfig {
                            owner: Some(me),
                            sub_config: sub,
                            max_downvotes: max,
                        };
                        api.call(
                            "set_domain_config",
                            vec![json!(name), serde_json::to_value(&cfg)?, json!(cmd)],
                        )
                        .await
                    },
                    "domain saved",
                );
                Ok(())
            });
        self.popup = Some(Popup::Form(f));
    }

    // ---- storage ---------------------------------------------------------------------------

    pub fn create_bucket_form(&mut self) {
        let f = Form::new("Create storage", "create")
            .note("Creates your storage canister with ICP from your wallet (converted to cycles), installs the storage code and registers it. Resumes where it stopped if interrupted.")
            .field(Field::text("ICP to spend", "0.5"))
            .on_submit(|app, v| {
                let e8s = parse_amount(&v[0], 8)?;
                let settings = app.user.as_ref().map(|u| u.settings.clone()).unwrap_or_default();
                let me = app.principal.clone();
                app.info("creating storage… (this takes a minute)");
                app.act(move |api| async move { crate::storage::create(&api, &me, e8s, settings).await.map(|b| json!(b)) }, "storage created");
                Ok(())
            });
        self.popup = Some(Popup::Form(f));
    }

    pub fn top_up_form(&mut self) {
        let Some(b) = self.user.as_ref().and_then(|u| u.bucket.clone()) else {
            return;
        };
        let f = Form::new("Top up storage", "top up")
            .field(Field::text("ICP to convert to cycles", "0.1"))
            .on_submit(move |app, v| {
                let e8s = parse_amount(&v[0], 8)?;
                let b = b.clone();
                app.act(
                    move |api| async move {
                        crate::storage::top_up(&api, &b, e8s)
                            .await
                            .map(|c| json!(c))
                    },
                    "storage topped up",
                );
                Ok(())
            });
        self.popup = Some(Popup::Form(f));
    }

    pub fn upgrade_bucket(&mut self) {
        let Some(b) = self.user.as_ref().and_then(|u| u.bucket.clone()) else {
            return;
        };
        self.act(
            move |api| async move { crate::storage::upgrade(&api, &b).await.map(|_| Value::Null) },
            "storage upgraded",
        );
    }

    pub fn add_blackhole(&mut self) {
        let Some(b) = self.user.as_ref().and_then(|u| u.bucket.clone()) else {
            return;
        };
        self.confirm("Add the blackhole canister as controller? It lets anyone see the status (cycles) of your storage.", move |app| {
            app.act(move |api| async move { crate::storage::add_controller(&api, &b, crate::storage::BLACKHOLE).await.map(|_| Value::Null) }, "controller added")
        });
    }
}

/// User names to ids.
pub async fn resolve(api: &Arc<Api>, names: &[String]) -> anyhow::Result<Vec<u64>> {
    let mut out = Vec::new();
    for n in names {
        let u = api
            .user(Some(n))
            .await?
            .ok_or_else(|| anyhow!("no user {n}"))?;
        out.push(u.id);
    }
    Ok(out)
}

pub fn shellexpand(p: &str) -> std::path::PathBuf {
    match p.strip_prefix("~/") {
        Some(rest) => dirs::home_dir().unwrap_or_default().join(rest),
        None => std::path::PathBuf::from(p),
    }
}
