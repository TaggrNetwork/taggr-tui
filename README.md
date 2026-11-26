# taggr-tui

A terminal client for [Taggr](https://taggr.link), the decentralised social network on the
Internet Computer. Browse feeds, realms and profiles, post, react, tip and use the wallet,
all from the terminal.

## Install

Linux and macOS, x86_64 and aarch64:

```sh
curl -fsSL https://raw.githubusercontent.com/TaggrNetwork/taggr-tui/main/install.sh | sh
```

This installs the latest release to `~/.local/bin/taggr-tui`. Set `TAGGR_TUI_INSTALL_DIR` to
install elsewhere:

```sh
curl -fsSL https://raw.githubusercontent.com/TaggrNetwork/taggr-tui/main/install.sh | TAGGR_TUI_INSTALL_DIR=/usr/local/bin sh
```

### Nix

On NixOS, `install.sh` installs the flake instead. To do it by hand:

```sh
nix profile add --no-write-lock-file github:TaggrNetwork/taggr-tui#default
```

The flake also has a Home Manager module, `homeManagerModules.default`, which writes
`programs.taggr-tui.settings` to `config.toml`.

### From source

```sh
cargo install --git https://github.com/TaggrNetwork/taggr-tui --locked
```

## Usage

```sh
taggr-tui           # start
taggr-tui --help    # sign-in, configuration, keys and post format
```

Browsing works without signing in. To post, react and use the wallet, sign in with the seed
phrase you use with the web app's "Seed Phrase" sign-in.
