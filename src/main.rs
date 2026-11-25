mod api;
mod config;
mod ic;
mod identity;
mod markdown;
mod storage;
mod tui;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("-h" | "--help" | "help") => {
            print!("{}", include_str!("guide.txt"));
            Ok(())
        }
        Some(a) => anyhow::bail!("unknown argument {a}; see taggr-tui --help"),
        None => tui::run(config::Config::load()?),
    }
}
