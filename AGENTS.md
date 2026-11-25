# AGENTS.md

Guidance for coding agents working on this repository. User-facing documentation is the
built-in help, `src/guide.txt` (`taggr-tui --help`); keep it consistent with behaviour.

## What this is

`taggr-tui`: a terminal client for [Taggr](../taggr) in Rust (edition 2024), a sibling of
`../mailbox` and `../organiser` with the same look and conventions. One binary, a ratatui TUI.
It talks to the Taggr canister on the Internet Computer through `ic-agent` and aims to cover
everything the web app (`../taggr/src/frontend/src`) does, except Internet Identity sign-in,
cold wallet linking (Plug) and the web app's appearance settings.

Sign-in is by seed phrase only, derived exactly like the web app's "Seed Phrase" method
(`identity.rs`). Only the derived key is stored.

## Commands

```sh
cargo build                 # debug binary: ./target/debug/taggr-tui
cargo test                  # unit tests (markdown, drafts, patches, identity, account ids)
cargo clippy                # keep it clean
cargo fmt
nix build                   # package; only when touching nix/ or dependencies
./target/debug/taggr-tui --help
```

Do **not** run `nix profile install`, push, or touch the user's real `~/.config/taggr-tui`,
`~/.local/share/taggr-tui/identity` or `~/.cache/taggr-tui`. Do not commit unless asked.
Never sign in with a real seed phrase or call update methods on mainnet: they cost credits,
move tokens and publish content. Write paths are tested against a local replica only.

## Running safely

Read-only browsing of mainnet is anonymous and harmless. Sandbox every run so real files
stay untouched:

```sh
S=$(mktemp -d)
tmux -L uitest new -d -s t -x 140 -y 45 \
  "env XDG_CACHE_HOME=$S/cache XDG_CONFIG_HOME=$S/cfg TAGGR_TUI_IDENTITY=$S/id ./target/debug/taggr-tui"
sleep 4; tmux -L uitest capture-pane -p -t t      # add -e to see colours
tmux -L uitest send -t t j Enter                   # keys; sleep between Esc and letters
tmux -L uitest kill-server
```

On this machine `tmux` may be a shell alias; call `/usr/bin/tmux`. For a local replica set
`url` (anything that is not a mainnet boundary node) and `canister` in a sandbox
`$S/cfg/taggr-tui/config.toml`.

## Layout

| File | Responsibility |
|---|---|
| `src/main.rs` | entry point: `--help` prints `guide.txt`, otherwise loads config and runs the TUI |
| `src/guide.txt` | the help text: sign-in, config reference, keys, post format |
| `src/config.rs` | `config.toml` (`TAGGR_TUI_CONFIG` overrides), identity and cache paths |
| `src/identity.rs` | seed phrase → Ed25519 key (sha256 × 15000), key file (0600) |
| `src/ic.rs` | `ic-agent` wrapper: Taggr's raw JSON methods, Candid calls, management canister |
| `src/api.rs` | typed models and one method per canister/ledger call |
| `src/storage.rs` | the user's media bucket: create via CMC, top up, upgrade, status, controllers |
| `src/markdown.rs` | Markdown → styled lines; `@user #tag $TOKEN /REALM`, `/blob/` pictures, cut |
| `src/tui/mod.rs` | `App`: background calls, sidebar, page stack, popups, sign-in, event loop |
| `src/tui/page.rs` | `Page`, `Kind`, `Target`, entries and selection; what each page loads |
| `src/tui/view.rs` | how each page kind turns into entries |
| `src/tui/actions.rs` | keys on pages and every action/form (react, tip, wallet, settings, governance…) |
| `src/tui/compose.rs` | posts in $EDITOR: templates, headers, picture upload, edit patches, versions |
| `src/tui/form.rs` | form popups (text, secret, yes/no, choice, long text via $EDITOR) |
| `src/tui/images.rs` | picture fetching, disk cache, graphics protocol choice |
| `src/tui/draw.rs` | all rendering; theme in `page.rs` constants |
| `nix/`, `flake.nix` | package, Home Manager module (`programs.taggr-tui.settings` → config.toml) |

## Model

- **Calls**: `App::bg(fut, then)` runs a future on the tokio runtime and hands its value to
  `then` on the UI thread (via `App::tx`); errors go to the status line and stop page
  spinners. `App::act` is the same for updates: status message, then reload page and user.
- **Pages**: `App::pages` is a stack; the sidebar replaces it, Enter pushes, Esc pops. Each
  `Page` has a unique `id`; loads find their page by it, so late replies never land on
  another page. `view::build` turns a page into `Entry`s (lines + optional `Target`) for the
  current width; `Page::dirty` forces a rebuild, `sel_key` keeps the selection across rebuilds.
- **Selection**: only entries with a target are selectable; ↑/↓ (j/k) always move to the
  previous/next one, Space/PgUp/PgDn scroll. Only Enter opens; ←/h/Esc go back. Loads that
  reset a page (tab switch) must not leave `sel_key` on the "load more" row; "load more"
  selects the first new post.
- **Layout helpers** (`page::Out`): consecutive `kv` pairs become a multi-column grid,
  `users` flows names inline, `table` aligns columns, `actions` is a row of ▸ actions (Enter:
  menu), `tab_bar` records click ranges, `boxed` frames reposts, polls and reports; comments
  hang off `│` guides. Feed posts and the open post are framed cards (`page::post_card`,
  `frame_style`); `draw` lights the selected frame up. Keep screens dense: no one-item-per-line lists. The bottom line
  (`draw::hints`) lists the keys for the selected target.
- **Backend shapes**: JSON methods take `null`, a value, or an array of arguments
  (`ic::encode_args`); updates reply `{"Ok": …}`/`{"Err": …}` (`Api::call`), a few toggles
  reply bare values (`Api::call_any`). `add_post`, `edit_post`, ledgers and the management
  canister are Candid. Source of truth: `../taggr/src/backend/{queries,updates}.rs`.
- **Posts**: `compose::Draft` writes a template to `~/.cache/taggr-tui/drafts`; top-level
  posts have header lines (`Realm:`, polls `Deadline:`/`Option:`). Local `![](path)`
  pictures are uploaded to the user's bucket on posting. Edits send a diff-match-patch patch
  from the new body back to the old one (as the web app does).
- **Pictures**: collapsed to one line by default (`I` per post, `expand_images` config). No
  terminal capability query (it swallows the first key when a terminal does not answer); the
  protocol comes from the environment or `image_protocol`, half blocks in tmux.
- **Icons**: Nerd Font glyphs by default via `page::ic(app, nerd, plain)`; always give a plain
  fallback for `icons = "plain"`. Emoji variation selectors and joiners are stripped
  (`markdown::render`, `draw::plain_width`): terminals disagree on their width.
- British English in user-facing text, like the web app.
