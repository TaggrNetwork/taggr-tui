use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const MAINNET_CANISTER: &str = "6qfxa-ryaaa-aaaai-qbhsq-cai";
pub const MAINNET_GATEWAY: &str = "https://ic0.app";
pub const MAINNET_RAW_SUFFIX: &str = "raw.icp0.io";
pub const FEED_PAGE_SIZE: usize = 30;
pub const HASH_ITERATIONS: usize = 15_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ImageProtocol {
    #[default]
    Auto,
    Kitty,
    Sixel,
    Iterm2,
    Halfblocks,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_canister")]
    pub canister_id: String,
    #[serde(default = "default_gateway")]
    pub gateway: String,
    #[serde(default = "default_raw_suffix")]
    pub raw_suffix: String,
    #[serde(default = "default_domain")]
    pub domain: String,
    #[serde(default)]
    pub image_protocol: ImageProtocol,
    #[serde(default = "default_true")]
    pub images_enabled: bool,
}

fn default_canister() -> String {
    MAINNET_CANISTER.to_string()
}
fn default_gateway() -> String {
    MAINNET_GATEWAY.to_string()
}
fn default_raw_suffix() -> String {
    MAINNET_RAW_SUFFIX.to_string()
}
fn default_domain() -> String {
    "taggr.link".to_string()
}
fn default_true() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        Self {
            canister_id: default_canister(),
            gateway: default_gateway(),
            raw_suffix: default_raw_suffix(),
            domain: default_domain(),
            image_protocol: ImageProtocol::Auto,
            images_enabled: true,
        }
    }
}

pub struct Paths {
    pub config_dir: PathBuf,
    pub cache_dir: PathBuf,
}

impl Paths {
    pub fn discover() -> Result<Self> {
        let dirs = ProjectDirs::from("network", "taggr", "taggr-tui")
            .context("could not determine home directory")?;
        let config_dir = dirs.config_dir().to_path_buf();
        let cache_dir = dirs.cache_dir().to_path_buf();
        std::fs::create_dir_all(&config_dir).ok();
        std::fs::create_dir_all(&cache_dir).ok();
        Ok(Self {
            config_dir,
            cache_dir,
        })
    }

    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    pub fn identity_file(&self) -> PathBuf {
        self.config_dir.join("identity")
    }

    pub fn image_cache(&self) -> PathBuf {
        self.cache_dir.join("images")
    }
}

impl Config {
    pub fn load(paths: &Paths) -> Result<Self> {
        let file = paths.config_file();
        if !file.exists() {
            let cfg = Config::default();
            cfg.save(paths).ok();
            return Ok(cfg);
        }
        let raw = std::fs::read_to_string(&file)
            .with_context(|| format!("reading {}", file.display()))?;
        let cfg: Config = toml::from_str(&raw).unwrap_or_default();
        Ok(cfg)
    }

    pub fn save(&self, paths: &Paths) -> Result<()> {
        let raw = toml::to_string_pretty(self)?;
        std::fs::write(paths.config_file(), raw)?;
        Ok(())
    }
}
