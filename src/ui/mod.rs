pub mod app;
pub mod images;
pub mod render;

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui_image::picker::{Picker, ProtocolType};
use tokio::sync::mpsc;

use crate::api::Client;
use crate::config::{Config, ImageProtocol, Paths};

use app::{App, Msg};
use images::ImageStore;

pub async fn run(
    config: Config,
    paths: Paths,
    client: Arc<Client>,
    logged_in: bool,
    principal: String,
) -> Result<()> {
    install_panic_hook();
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let in_tmux = std::env::var_os("TMUX").is_some_and(|v| !v.is_empty());
    let mut picker = match config.image_protocol {
        ImageProtocol::Halfblocks => Picker::halfblocks(),
        _ => Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks()),
    };
    // Inside tmux, graphics protocols only work with `allow-passthrough on`;
    // halfblocks is the only guaranteed path, so default to it unless the user
    // explicitly forced a graphics protocol.
    let mut protocol = config.image_protocol;
    if in_tmux && protocol == ImageProtocol::Auto {
        protocol = ImageProtocol::Halfblocks;
    }
    match protocol {
        ImageProtocol::Kitty => picker.set_protocol_type(ProtocolType::Kitty),
        ImageProtocol::Sixel => picker.set_protocol_type(ProtocolType::Sixel),
        ImageProtocol::Iterm2 => picker.set_protocol_type(ProtocolType::Iterm2),
        ImageProtocol::Halfblocks => picker.set_protocol_type(ProtocolType::Halfblocks),
        ImageProtocol::Auto => {}
    }
    let startup_note = if !config.images_enabled {
        None
    } else if in_tmux {
        Some(
            "tmux: halfblocks previews (set 'allow-passthrough on' + --protocol kitty for graphics)"
                .to_string(),
        )
    } else {
        Some(format!("image protocol: {:?}", picker.protocol_type()))
    };
    let image_store = ImageStore::new(picker, config.images_enabled);

    let (tx, mut rx) = mpsc::unbounded_channel::<Msg>();

    let input_tx = tx.clone();
    std::thread::spawn(move || {
        loop {
            let ready = event::poll(Duration::from_millis(250)).unwrap_or(false);
            if !ready {
                if input_tx.send(Msg::Tick).is_err() {
                    break;
                }
                continue;
            }
            let failed = match event::read() {
                Ok(Event::Key(key)) => input_tx.send(Msg::Key(key)).is_err(),
                Ok(Event::Resize(_, _)) => input_tx.send(Msg::Resize).is_err(),
                _ => false,
            };
            if failed {
                break;
            }
        }
    });

    let mut app = App::new(client, paths, config, tx, image_store, logged_in, principal);
    if let Some(note) = startup_note {
        app.toast = Some(note);
        app.toast_at = Some(std::time::Instant::now());
    }
    app.start();

    let mut result: Result<()> = Ok(());
    loop {
        if let Err(e) = terminal.draw(|f| render::draw(f, &mut app)) {
            result = Err(e.into());
            break;
        }
        match rx.recv().await {
            Some(msg) => app.handle(msg),
            None => break,
        }
        if app.quit {
            break;
        }
    }

    disable_raw_mode().ok();
    execute!(terminal.backend_mut(), LeaveAlternateScreen).ok();
    terminal.show_cursor().ok();
    result
}

fn install_panic_hook() {
    let original = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        disable_raw_mode().ok();
        let mut stdout = std::io::stdout();
        execute!(stdout, LeaveAlternateScreen).ok();
        original(info);
    }));
}
