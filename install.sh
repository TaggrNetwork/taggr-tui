#!/bin/sh
# Install taggr-tui from the latest release: one per version, made by CI when
# a push to main carries a new Cargo.toml version.
#   curl -fsSL https://raw.githubusercontent.com/TaggrNetwork/taggr-tui/main/install.sh | sh
# TAGGR_TUI_INSTALL_DIR overrides the destination; the default is ~/.local/bin.
# NixOS installs the flake instead: nix profile add --no-write-lock-file github:TaggrNetwork/taggr-tui#default.
set -eu

repo=TaggrNetwork/taggr-tui
dir=${TAGGR_TUI_INSTALL_DIR:-"${HOME:-}/.local/bin"}

die() {
	printf 'install.sh: %s\n' "$*" >&2
	exit 1
}

case ${HOME:-} in
'') [ -n "${TAGGR_TUI_INSTALL_DIR:-}" ] || die "HOME is not set; set TAGGR_TUI_INSTALL_DIR" ;;
esac

# NixOS: the package manager is the install that belongs there; the static
# binary would run, but the flake is what the Home Manager module builds on.
if [ -e /etc/NIXOS ]; then
	command -v nix >/dev/null 2>&1 ||
		die "NixOS without nix; install with: nix profile add --no-write-lock-file github:$repo#default"
	nix profile add --no-write-lock-file "github:$repo#default" ||
		die "nix profile add failed"
	printf 'installed taggr-tui via nix from github:%s#default\n' "$repo"
	exit 0
fi

os=$(uname -s)
arch=$(uname -m)

case $os in
Darwin) os=apple-darwin ;;
Linux) os=unknown-linux-musl ;;
*) die "unsupported system: $os (Linux and macOS only)" ;;
esac

case $arch in
x86_64 | amd64) arch=x86_64 ;;
arm64 | aarch64) arch=aarch64 ;;
*) die "unsupported machine: $arch (x86_64 and aarch64 only)" ;;
esac

triple=$arch-$os
tar=taggr-tui-$triple.tar.gz

# Each tool named up front: a missing one would otherwise read as a failed
# download or, worse, a checksum mismatch.
command -v curl >/dev/null 2>&1 || die "curl is required"
if command -v sha256sum >/dev/null 2>&1; then
	sum="sha256sum -c"
elif command -v shasum >/dev/null 2>&1; then
	sum="shasum -a 256 -c"
else
	die "sha256sum or shasum is required to check the download"
fi

# The latest release is resolved to its tag once, so the version reported is
# the one downloaded even if a release lands meanwhile.
latest=$(curl -fsSLI -o /dev/null -w '%{url_effective}' "https://github.com/$repo/releases/latest") ||
	die "cannot find the latest release of $repo"
tag=${latest##*/}
case $tag in
v[0-9]*) ;;
*) die "unexpected latest release: $latest" ;;
esac
base="https://github.com/$repo/releases/download/$tag"

tmp=$(mktemp -d) || die "mktemp failed"
trap 'rm -rf "$tmp"' EXIT
trap 'exit 1' INT TERM HUP

curl -fsSL -o "$tmp/$tar" "$base/$tar" || die "download failed: $base/$tar"
curl -fsSL -o "$tmp/$tar.sha256" "$base/$tar.sha256" ||
	die "download failed: $base/$tar.sha256"

(cd "$tmp" && $sum "$tar.sha256") >/dev/null 2>&1 || die "checksum mismatch for $tar"

tar -xzf "$tmp/$tar" -C "$tmp" taggr-tui || die "extract failed: $tar"

# The binary is the authority on whether it runs here, and it is asked before
# it replaces anything: a failure leaves an existing install as it was.
chmod 755 "$tmp/taggr-tui"
"$tmp/taggr-tui" --help >/dev/null ||
	die "the binary does not run on this system ($(uname -s)/$(uname -m)); build from source with
  cargo install --git https://github.com/$repo --locked"

mkdir -p "$dir" || die "cannot create $dir"
install -m 755 "$tmp/taggr-tui" "$dir/taggr-tui" || die "cannot install to $dir"
printf 'installed taggr-tui %s to %s\n' "$tag" "$dir/taggr-tui"

case ":$PATH:" in
*":$dir:"*) ;;
*) printf 'add %s to PATH to run it as `taggr-tui`\n' "$dir" >&2 ;;
esac
