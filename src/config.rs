//! `~/.config/taggr-tui/config.toml` (`TAGGR_TUI_CONFIG` overrides). Every key is optional.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::PathBuf;

pub const HASH_ITERATIONS: usize = 15_000;

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Taggr canister id.
    pub canister: String,
    /// IC API boundary node.
    pub url: String,
    /// Domain configuration the backend filters realms by (see the Domains page).
    pub domain: String,
    /// TUI symbols: plain Unicode, or Nerd Font glyphs.
    pub icons: IconSet,
    /// Show pictures inside posts.
    pub images: bool,
    /// Show pictures expanded instead of as a one-line placeholder (I toggles per post).
    pub expand_images: bool,
    /// auto | kitty | sixel | iterm2 | halfblocks
    pub image_protocol: String,
    /// Where `s` saves pictures and other downloads.
    pub download_dir: PathBuf,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            canister: "6qfxa-ryaaa-aaaai-qbhsq-cai".into(),
            url: "https://icp-api.io".into(),
            domain: "6qfxa-ryaaa-aaaai-qbhsq-cai.icp.net".into(),
            icons: IconSet::Nerd,
            images: true,
            expand_images: false,
            image_protocol: "auto".into(),
            download_dir: dirs::download_dir()
                .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join("Downloads")),
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum IconSet {
    Plain,
    #[default]
    Nerd,
}

impl Config {
    pub fn path() -> PathBuf {
        std::env::var_os("TAGGR_TUI_CONFIG")
            .map(PathBuf::from)
            .unwrap_or_else(|| config_dir().join("config.toml"))
    }

    pub fn load() -> Result<Config> {
        let path = Self::path();
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Ok(Config::default());
        };
        let mut cfg: Config =
            toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        if let Ok(rest) = cfg.download_dir.strip_prefix("~") {
            cfg.download_dir = dirs::home_dir().unwrap_or_default().join(rest);
        }
        Ok(cfg)
    }

    /// `host:port` of a local replica (None on mainnet), for canister subdomains.
    pub fn local_host(&self) -> Option<String> {
        (!self.mainnet()).then(|| {
            let u = self.url.trim_end_matches('/');
            u.split_once("://").map(|x| x.1).unwrap_or(u).to_string()
        })
    }

    /// Whether `url` points to a mainnet boundary node (else: a local replica).
    pub fn mainnet(&self) -> bool {
        ["ic0.app", "icp0.io", "icp-api.io"]
            .iter()
            .any(|h| self.url.contains(h))
    }
}

fn config_dir() -> PathBuf {
    dirs::config_dir().unwrap_or_default().join("taggr-tui")
}

/// Where the signing key (hex Ed25519 seed) is kept, mode 0600.
pub fn identity_file() -> PathBuf {
    std::env::var_os("TAGGR_TUI_IDENTITY")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::data_dir()
                .unwrap_or_default()
                .join("taggr-tui/identity")
        })
}

pub fn cache_dir() -> PathBuf {
    dirs::cache_dir().unwrap_or_default().join("taggr-tui")
}
