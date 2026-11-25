#![allow(dead_code)]

mod api;
mod config;
mod ic;
mod identity;
mod media;
mod ui;

use std::sync::Arc;

use anyhow::Result;
use clap::Parser;

use api::Client;
use config::{Config, ImageProtocol, Paths};
use ic::Ic;

#[derive(Parser, Debug)]
#[command(
    name = "taggr-tui",
    about = "Terminal client for Taggr (Internet Computer)"
)]
struct Args {
    /// Canister id to talk to.
    #[arg(long)]
    canister: Option<String>,
    /// IC gateway URL.
    #[arg(long)]
    gateway: Option<String>,
    /// Domain string used by the backend for realm filtering.
    #[arg(long)]
    domain: Option<String>,
    /// Disable all image rendering.
    #[arg(long)]
    no_images: bool,
    /// Force the unicode halfblocks image protocol.
    #[arg(long)]
    halfblocks: bool,
    /// Force an image protocol: auto|kitty|sixel|iterm2|halfblocks.
    #[arg(long, value_name = "PROTOCOL")]
    protocol: Option<String>,
    /// Ignore a stored identity and ask for the seed phrase.
    #[arg(long)]
    login: bool,
    /// Fetch the latest feed, print it as text, and exit (no TUI).
    #[arg(long, value_name = "N")]
    dump: Option<usize>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let paths = Paths::discover()?;
    let mut config = Config::load(&paths).unwrap_or_default();

    if let Some(c) = args.canister {
        config.canister_id = c;
    }
    if let Some(g) = args.gateway {
        config.gateway = g;
    }
    if let Some(d) = args.domain {
        config.domain = d;
    }
    if args.no_images {
        config.images_enabled = false;
    }
    if args.halfblocks {
        config.image_protocol = ImageProtocol::Halfblocks;
    }
    if let Some(p) = &args.protocol {
        config.image_protocol = match p.to_lowercase().as_str() {
            "kitty" => ImageProtocol::Kitty,
            "sixel" => ImageProtocol::Sixel,
            "iterm2" | "iterm" => ImageProtocol::Iterm2,
            "halfblocks" | "half" => ImageProtocol::Halfblocks,
            _ => ImageProtocol::Auto,
        };
    }

    init_tracing(&paths);

    let mut identity_opt = None;
    let mut principal = String::new();
    let identity_file = paths.identity_file();
    if !args.login
        && identity_file.exists()
        && let Ok(seed) = identity::read_seed(&identity_file)
    {
        let id = identity::identity_from_seed(&seed);
        principal = identity::principal_of(&id);
        identity_opt = Some(id);
    }

    let ic = Arc::new(Ic::new(&config.gateway, &config.canister_id, identity_opt).await?);
    let client = Arc::new(Client::new(
        ic,
        config.domain.clone(),
        config.raw_suffix.clone(),
    ));

    if let Some(n) = args.dump {
        return dump(client, n).await;
    }

    let logged_in = !principal.is_empty();
    ui::run(config, paths, client, logged_in, principal).await
}

async fn dump(client: Arc<Client>, n: usize) -> Result<()> {
    let items = client.latest_posts("", 0, 0).await?;
    println!("fetched {} posts", items.len());
    for item in &items {
        let imgs = media::image_refs(&item.post).len();
        println!(
            "#{} @{}  reactions={} replies={} images={}  {}",
            item.post.id,
            item.author,
            item.post.total_reactions(),
            item.post.children.len(),
            imgs,
            item.post.body.lines().next().unwrap_or("")
        );
    }
    if let Some(item) = items.iter().find(|i| !i.post.files.is_empty()) {
        let r = &media::image_refs(&item.post)[0];
        let http = reqwest::Client::new();
        let cache = std::env::temp_dir().join("taggr-tui-dump");
        let img = media::load_or_fetch(&http, &client, &cache, r).await?;
        println!(
            "image ok: {}x{} from {}",
            img.width(),
            img.height(),
            client.image_url(&r.bucket, r.offset, r.len)
        );
    }
    let _ = n;
    Ok(())
}

fn init_tracing(paths: &Paths) {
    use tracing_subscriber::EnvFilter;
    if std::env::var("RUST_LOG").is_err() {
        return;
    }
    let log_file = paths.cache_dir.join("taggr-tui.log");
    if let Ok(file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_file)
    {
        let filter = EnvFilter::from_default_env();
        let _ = tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(std::sync::Mutex::new(file))
            .try_init();
    }
}
