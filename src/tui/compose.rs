//! Writing posts in $EDITOR: templates, parsing, picture upload, the send prompt.

use super::page::Kind;
use super::{App, Popup};
use crate::api::{self, Api, Extension, Poll, PostId};
use anyhow::{Result, anyhow, bail};
use diff_match_patch_rs::{Compat, DiffMatchPatch, PatchInput};
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Clone)]
pub enum What {
    New { repost: Option<PostId> },
    Poll,
    Reply(PostId),
    Edit { id: PostId, old: String, root: bool },
}

#[derive(Clone)]
pub struct Draft {
    pub what: What,
    pub realm: Option<String>,
    pub file: PathBuf,
    pub template: String,
    /// Parsed after editing.
    pub body: String,
    pub poll: Option<Poll>,
}

pub fn scratch_file(name: &str) -> PathBuf {
    let dir = crate::config::cache_dir().join("drafts");
    let _ = std::fs::create_dir_all(&dir);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    dir.join(format!("{stamp}-{name}"))
}

impl Draft {
    fn make(what: What, realm: Option<String>, template: String) -> Draft {
        let file = scratch_file("post.md");
        let _ = std::fs::write(&file, &template);
        Draft {
            what,
            realm,
            file,
            template,
            body: String::new(),
            poll: None,
        }
    }

    pub fn new_post(realm: Option<String>, repost: Option<PostId>) -> Draft {
        let t = format!("Realm: {}\n\n", realm.clone().unwrap_or_default());
        Draft::make(What::New { repost }, realm, t)
    }

    pub fn new_poll(realm: Option<String>) -> Draft {
        let t = format!(
            "Realm: {}\nDeadline: 24\nOption: \nOption: \n\n",
            realm.clone().unwrap_or_default()
        );
        Draft::make(What::Poll, realm, t)
    }

    pub fn reply(parent: PostId, author: &str) -> Draft {
        let t = if author.is_empty() {
            String::new()
        } else {
            format!("@{author} ")
        };
        Draft::make(What::Reply(parent), None, t)
    }

    pub fn edit(p: &api::Post) -> Draft {
        let root = p.parent.is_none();
        let t = if root {
            format!(
                "Realm: {}\n\n{}",
                p.realm.clone().unwrap_or_default(),
                p.body
            )
        } else {
            p.body.clone()
        };
        Draft::make(
            What::Edit {
                id: p.id,
                old: p.body.clone(),
                root,
            },
            p.realm.clone(),
            t,
        )
    }

    fn has_headers(&self) -> bool {
        matches!(
            self.what,
            What::New { .. } | What::Poll | What::Edit { root: true, .. }
        )
    }

    /// Reads the edited file: headers (top-level posts) then the body.
    fn parse(&mut self, text: &str) -> Result<()> {
        let mut body = text;
        let mut poll = Poll {
            deadline: 24,
            ..Default::default()
        };
        if self.has_headers() {
            let mut realm = None;
            let mut rest = text;
            loop {
                let (line, after) = rest.split_once('\n').unwrap_or((rest, ""));
                let Some((k, v)) = line.split_once(':') else {
                    break;
                };
                let v = v.trim();
                match k.trim().to_lowercase().as_str() {
                    "realm" => {
                        realm =
                            Some(v.trim_start_matches('/').to_uppercase()).filter(|s| !s.is_empty())
                    }
                    "deadline" => {
                        poll.deadline = v
                            .parse()
                            .map_err(|_| anyhow!("Deadline: hours as a number"))?
                    }
                    "option" => {
                        if !v.is_empty() {
                            poll.options.push(v.to_string())
                        }
                    }
                    _ => break,
                }
                rest = after;
            }
            self.realm = realm;
            body = rest;
        }
        self.body = body.trim().to_string();
        if matches!(self.what, What::Poll) {
            if poll.options.len() < 2 {
                bail!("a poll needs at least two Option: lines");
            }
            self.poll = Some(poll);
        }
        Ok(())
    }

    pub fn discard(&self) {
        let _ = std::fs::remove_file(&self.file);
    }

    pub fn title(&self) -> String {
        match &self.what {
            What::New { repost: Some(id) } => format!("Repost #{id}"),
            What::New { .. } => "New post".into(),
            What::Poll => "New poll".into(),
            What::Reply(p) => format!("Reply to #{p}"),
            What::Edit { id, .. } => format!("Edit #{id}"),
        }
    }
}

/// After the editor closed: unchanged → discarded, else the send prompt.
pub fn edited(app: &mut App, mut d: Draft) {
    let text = std::fs::read_to_string(&d.file).unwrap_or_default();
    if text == d.template {
        d.discard();
        return app.info("unchanged draft discarded");
    }
    if let Err(e) = d.parse(&text) {
        app.error(format!("{e}; e: edit again"));
    }
    if d.body.is_empty() && d.poll.is_none() {
        d.discard();
        return app.info("empty post discarded");
    }
    app.popup = Some(Popup::Send(d, 0));
}

pub fn send_key(app: &mut App, d: Draft, scroll: u16, k: KeyEvent) {
    match k.code {
        KeyCode::Char('y') => post(app, d),
        KeyCode::Char('e') => app.edit = Some(d),
        KeyCode::Char('d') | KeyCode::Esc => {
            app.info(format!("draft kept in {}", d.file.display()));
        }
        KeyCode::Char('D') => {
            d.discard();
            app.info("draft discarded");
        }
        KeyCode::Char('j') | KeyCode::Down => app.popup = Some(Popup::Send(d, scroll + 1)),
        KeyCode::Char('k') | KeyCode::Up => {
            app.popup = Some(Popup::Send(d, scroll.saturating_sub(1)))
        }
        KeyCode::PageDown | KeyCode::Char(' ') => app.popup = Some(Popup::Send(d, scroll + 10)),
        KeyCode::PageUp => app.popup = Some(Popup::Send(d, scroll.saturating_sub(10))),
        _ => app.popup = Some(Popup::Send(d, scroll)),
    }
}

/// Estimated credits: post cost per started KiB (minimum one), plus polls.
pub fn cost(app: &App, d: &Draft) -> i64 {
    let c = &app.backend;
    let kib = (d.body.len() as i64 + 1023) / 1024;
    let mut n = c.post_cost * kib.max(1);
    if d.poll.is_some() {
        n += c.poll_cost;
    }
    n
}

/// Local picture paths referenced as `![alt](path)`.
pub fn local_images(body: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = body;
    while let Some(i) = rest.find("![") {
        let after = &rest[i + 2..];
        let Some(j) = after.find("](") else { break };
        let alt = &after[..j];
        let tail = &after[j + 2..];
        let Some(k) = tail.find(')') else { break };
        let path = &tail[..k];
        if !path.starts_with("http") && !path.starts_with("/blob/") && !path.is_empty() {
            out.push((alt.to_string(), path.to_string()));
        }
        rest = &tail[k..];
    }
    out
}

/// Reads and shrinks a picture until it fits `max` bytes.
fn load_image(path: &str, max: usize) -> Result<(Vec<u8>, u32, u32)> {
    let p = super::actions::shellexpand(path);
    let bytes = std::fs::read(&p).map_err(|e| anyhow!("{}: {e}", p.display()))?;
    let img = image::load_from_memory(&bytes).map_err(|e| anyhow!("{}: {e}", p.display()))?;
    if bytes.len() <= max || max == 0 {
        return Ok((bytes, img.width(), img.height()));
    }
    let mut scale = 1.0f32;
    loop {
        let (w, h) = (
            (img.width() as f32 * scale) as u32,
            (img.height() as f32 * scale) as u32,
        );
        let small = img.resize(w.max(1), h.max(1), image::imageops::FilterType::Lanczos3);
        let mut out = std::io::Cursor::new(Vec::new());
        let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 70);
        small.to_rgb8().write_with_encoder(enc)?;
        let out = out.into_inner();
        if out.len() <= max || scale < 0.05 {
            return Ok((out, small.width(), small.height()));
        }
        scale *= 0.8;
    }
}

/// Uploads local pictures to the bucket; returns the new body and refs.
async fn upload(
    api: &Arc<Api>,
    bucket: Option<String>,
    body: String,
    max: usize,
) -> Result<(String, Vec<(String, u64, u64)>)> {
    let imgs = local_images(&body);
    if imgs.is_empty() {
        return Ok((body, vec![]));
    }
    let Some(b) = bucket else {
        bail!("no personal media bucket configured: set one up under Settings → Storage");
    };
    let bucket = candid::Principal::from_text(&b)?;
    let mut body = body;
    let mut refs = Vec::new();
    for (alt, path) in imgs {
        let (bytes, w, h) = load_image(&path, max)?;
        let key = hex::encode(&Sha256::digest(&bytes)[..4]);
        let len = bytes.len() as u64;
        let off = api.bucket_write(&bucket, bytes).await?;
        refs.push((key.clone(), off, len));
        let alt2 = if alt.is_empty() {
            format!("{w}x{h}, {}kb", len.div_ceil(1024))
        } else {
            alt.clone()
        };
        body = body.replacen(
            &format!("![{alt}]({path})"),
            &format!("![{alt2}](/blob/{key})"),
            1,
        );
    }
    Ok((body, refs))
}

fn post(app: &mut App, d: Draft) {
    if matches!(d.what, What::Poll) && d.poll.is_none() {
        app.popup = Some(Popup::Send(d, 0));
        return app.error("a poll needs at least two Option: lines; e: edit");
    }
    let bucket = app.user.as_ref().and_then(|u| u.bucket.clone());
    let max = app.backend.max_blob_size_bytes;
    let d2 = d.clone();
    app.info("posting…");
    app.bg(
        move |api| async move {
            let (body, refs) = upload(&api, bucket, d.body.clone(), max).await?;
            match &d.what {
                What::New { repost } => {
                    let ext = repost.map(Extension::Repost);
                    api.add_post(&body, refs, None, d.realm.clone(), ext.as_ref())
                        .await
                        .map(Some)
                }
                What::Poll => {
                    let ext = d.poll.clone().map(Extension::Poll);
                    api.add_post(&body, refs, None, d.realm.clone(), ext.as_ref())
                        .await
                        .map(Some)
                }
                What::Reply(p) => api
                    .add_post(&body, refs, Some(*p), None, None)
                    .await
                    .map(Some),
                What::Edit { id, old, root } => {
                    let patch = patch(&body, old)?;
                    let realm = if *root { d.realm.clone() } else { None };
                    api.edit_post(*id, &body, refs, patch, realm)
                        .await
                        .map(|_| None)
                }
            }
        },
        move |app, id: Option<PostId>| {
            d2.discard();
            match (id, &d2.what) {
                (Some(id), What::New { .. } | What::Poll) => {
                    app.info("posted");
                    app.push(Kind::Post(id), &format!("Post #{id}"));
                }
                (_, What::Reply(_)) => {
                    app.info("reply posted");
                    app.reload();
                }
                _ => {
                    app.info("saved");
                    app.reload();
                }
            }
            app.load_user();
        },
    );
}

/// The patch the backend stores: from the new text back to the old one.
pub fn patch(new: &str, old: &str) -> Result<String> {
    let dmp = DiffMatchPatch::new();
    let p = dmp
        .patch_make(PatchInput::<Compat>::Texts(new, old))
        .map_err(|e| anyhow!("diff: {e:?}"))?;
    Ok(dmp.patch_to_text(&p))
}

/// All versions of a post, oldest first, the current body last.
pub fn versions(p: &api::Post) -> Result<Vec<String>, String> {
    let dmp = DiffMatchPatch::new();
    let mut cur = p.body.clone();
    let mut out = vec![cur.clone()];
    for (_, patch) in p.patches.iter().rev() {
        let ps = dmp
            .patch_from_text::<Compat>(patch)
            .map_err(|e| format!("bad patch: {e:?}"))?;
        cur = dmp
            .patch_apply(&ps, &cur)
            .map_err(|e| format!("patch: {e:?}"))?
            .0;
        out.push(cur.clone());
    }
    out.reverse();
    Ok(out)
}

/// Frees a deleted post's pictures in their buckets (best effort).
pub async fn free_files(api: &Arc<Api>, files: &BTreeMap<String, (u64, u64)>) {
    let mut by: BTreeMap<String, Vec<(u64, u64)>> = BTreeMap::new();
    for (k, seg) in files {
        if let Some((_, b)) = k.split_once('@') {
            by.entry(b.to_string()).or_default().push(*seg);
        }
    }
    for (b, segs) in by {
        if let Ok(p) = candid::Principal::from_text(&b) {
            let _ = api.bucket_free(&p, segs).await;
        }
    }
}

/// A realm logo: the picture at `path`, shrunk to a PNG of at most `max` base64 bytes.
pub fn logo(path: &str, max: usize) -> Result<String, String> {
    let p = super::actions::shellexpand(path);
    let img = image::open(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    let mut side = 128u32;
    loop {
        let small = img.thumbnail(side, side);
        let mut out = std::io::Cursor::new(Vec::new());
        small
            .write_to(&mut out, image::ImageFormat::Png)
            .map_err(|e| e.to_string())?;
        let b = base64(&out.into_inner());
        if b.len() <= max || max == 0 || side <= 16 {
            return Ok(b);
        }
        side = side * 3 / 4;
    }
}

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_headers_and_poll() {
        let mut d = Draft::new_poll(None);
        d.parse("Realm: dao\nDeadline: 48\nOption: A\nOption: B\n\nWhich?")
            .unwrap();
        assert_eq!(d.realm.as_deref(), Some("DAO"));
        assert_eq!(d.body, "Which?");
        let p = d.poll.clone().unwrap();
        assert_eq!(p.options, vec!["A", "B"]);
        assert_eq!(p.deadline, 48);
        d.discard();
    }

    #[test]
    fn replies_have_no_headers() {
        let mut d = Draft::reply(1, "x");
        d.parse("Realm: no header here").unwrap();
        assert_eq!(d.body, "Realm: no header here");
        d.discard();
    }

    #[test]
    fn versions_roundtrip() {
        let old = "hello world";
        let new = "hello brave new world";
        let p = api::Post {
            body: new.into(),
            patches: vec![(0, patch(new, old).unwrap())],
            ..Default::default()
        };
        assert_eq!(
            versions(&p).unwrap(),
            vec![old.to_string(), new.to_string()]
        );
    }

    #[test]
    fn finds_local_images() {
        let b = "a ![](~/x.png) b ![c](/blob/1) ![d](https://e/f.png) ![g](/tmp/h.jpg)";
        assert_eq!(
            local_images(b),
            vec![
                ("".into(), "~/x.png".into()),
                ("g".into(), "/tmp/h.jpg".into())
            ]
        );
    }

    #[test]
    fn base64_matches() {
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"M"), "TQ==");
    }
}
