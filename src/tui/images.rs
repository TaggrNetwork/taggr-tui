//! Pictures in posts: fetched in the background, cached on disk, drawn with the
//! terminal's graphics protocol (kitty, sixel, iTerm2) or half blocks.

use crate::config::Config;
use image::DynamicImage;
use ratatui_image::picker::{Picker, ProtocolType};
use ratatui_image::protocol::StatefulProtocol;
use std::collections::{HashMap, HashSet};

/// Pictures are never taller than this many rows.
pub const MAX_ROWS: u16 = 20;

pub struct Images {
    picker: Option<Picker>,
    protos: HashMap<String, StatefulProtocol>,
    dims: HashMap<String, (u32, u32)>,
    pending: HashSet<String>,
    failed: HashSet<String>,
    /// Wanted by the last frame, not yet requested.
    pub queue: Vec<String>,
}

impl Images {
    pub fn new(cfg: &Config) -> Images {
        // No stdin query (Picker::from_query_stdio): when a terminal does not answer,
        // its reader thread stays blocked on stdin and swallows the first key.
        let picker = cfg.images.then(|| {
            let font = ratatui::crossterm::terminal::window_size()
                .ok()
                .filter(|w| w.width > 0 && w.columns > 0 && w.height > 0 && w.rows > 0)
                .map(|w| ratatui_image::FontSize {
                    width: w.width / w.columns,
                    height: w.height / w.rows,
                })
                .unwrap_or(ratatui_image::FontSize {
                    width: 8,
                    height: 16,
                });
            #[allow(deprecated)] // the replacement is the stdin query avoided above
            let mut p = Picker::from_fontsize(font);
            p.set_protocol_type(match cfg.image_protocol.as_str() {
                "kitty" => ProtocolType::Kitty,
                "sixel" => ProtocolType::Sixel,
                "iterm2" => ProtocolType::Iterm2,
                "halfblocks" => ProtocolType::Halfblocks,
                _ => guess_protocol(),
            });
            p
        });
        Images {
            picker,
            protos: HashMap::new(),
            dims: HashMap::new(),
            pending: HashSet::new(),
            failed: HashSet::new(),
            queue: vec![],
        }
    }

    pub fn enabled(&self) -> bool {
        self.picker.is_some()
    }

    /// Rows the picture takes at `cols` columns (not upscaled).
    pub fn rows(&self, url: &str, cols: u16) -> u16 {
        let Some(p) = &self.picker else { return 1 };
        if self.failed.contains(url) {
            return 1;
        }
        let Some(&(w, h)) = self.dims.get(url) else {
            return 1;
        };
        let fs = p.font_size();
        let (fw, fh) = (fs.width.max(1) as f32, fs.height.max(1) as f32);
        let px_w = (cols as f32 * fw).min(w as f32);
        let px_h = h as f32 * px_w / w.max(1) as f32;
        ((px_h / fh).ceil() as u16).clamp(1, MAX_ROWS)
    }

    pub fn get(&mut self, url: &str) -> Option<&mut StatefulProtocol> {
        if !self.protos.contains_key(url)
            && !self.pending.contains(url)
            && !self.failed.contains(url)
        {
            self.pending.insert(url.to_string());
            self.queue.push(url.to_string());
        }
        self.protos.get_mut(url)
    }

    pub fn failed(&self, url: &str) -> bool {
        self.failed.contains(url)
    }

    pub fn insert(&mut self, url: String, img: Option<DynamicImage>) {
        self.pending.remove(&url);
        match (img, &self.picker) {
            (Some(img), Some(p)) => {
                self.dims.insert(url.clone(), (img.width(), img.height()));
                self.protos.insert(url, p.new_resize_protocol(img));
            }
            _ => {
                self.failed.insert(url);
            }
        }
        // bound memory
        if self.protos.len() > 200 {
            let k: Vec<String> = self.protos.keys().take(50).cloned().collect();
            for k in k {
                self.protos.remove(&k);
            }
        }
    }
}

/// The graphics protocol the terminal most likely speaks, from its environment.
/// Inside tmux only half blocks are safe without passthrough.
fn guess_protocol() -> ProtocolType {
    let var = |k: &str| std::env::var(k).unwrap_or_default();
    if !var("TMUX").is_empty() {
        return ProtocolType::Halfblocks;
    }
    let term = var("TERM");
    let prog = var("TERM_PROGRAM");
    if !var("KITTY_WINDOW_ID").is_empty()
        || term.contains("kitty")
        || prog == "ghostty"
        || term.contains("ghostty")
    {
        ProtocolType::Kitty
    } else if prog == "iTerm.app" || prog == "WezTerm" || !var("WEZTERM_EXECUTABLE").is_empty() {
        ProtocolType::Iterm2
    } else if term.contains("foot") || term.contains("mlterm") || term.contains("contour") {
        ProtocolType::Sixel
    } else {
        ProtocolType::Halfblocks
    }
}

/// Downloads (or reads from the disk cache) and decodes a picture.
pub async fn fetch(url: String) -> Option<DynamicImage> {
    use sha2::Digest;
    let dir = crate::config::cache_dir().join("images");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(hex::encode(&sha2::Sha256::digest(url.as_bytes())[..12]));
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(_) => {
            let b = reqwest::get(&url)
                .await
                .ok()?
                .error_for_status()
                .ok()?
                .bytes()
                .await
                .ok()?
                .to_vec();
            let _ = std::fs::write(&path, &b);
            b
        }
    };
    tokio::task::spawn_blocking(move || image::load_from_memory(&bytes).ok())
        .await
        .ok()
        .flatten()
}
