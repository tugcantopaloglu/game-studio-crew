#!/usr/bin/env sh
# Game Studio Crew installer for Linux and macOS.
#
#   curl -fsSL https://raw.githubusercontent.com/tugcantopaloglu/game-studio-crew/main/scripts/install.sh | sh
#
# Environment:
#   PREFIX=/usr/local    where to install on Linux (default: ~/.local)
#   VERSION=v1.0.1       which release to fetch (default: latest)

set -eu

REPO="tugcantopaloglu/game-studio-crew"
PREFIX="${PREFIX:-$HOME/.local}"
VERSION="${VERSION:-latest}"

say() { printf '%s\n' "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

need() {
  command -v "$1" >/dev/null 2>&1 || die "$1 is required and was not found on PATH"
}

need curl
need tar

case "$(uname -s)" in
  Linux)  OS=linux ;;
  Darwin) OS=macos ;;
  *)      die "unsupported operating system: $(uname -s). Windows users want scripts/install.ps1" ;;
esac

case "$(uname -m)" in
  x86_64|amd64)  ARCH=x86_64 ;;
  arm64|aarch64) ARCH=aarch64 ;;
  *)             die "unsupported architecture: $(uname -m)" ;;
esac

if [ "$OS" = linux ] && [ "$ARCH" = aarch64 ]; then
  die "there is no prebuilt Linux arm64 build yet; build from source with 'cargo build --release'"
fi

LABEL="$OS-$ARCH"
say "Game Studio Crew installer"
say "  platform: $LABEL"

if [ "$VERSION" = latest ]; then
  API="https://api.github.com/repos/$REPO/releases/latest"
else
  API="https://api.github.com/repos/$REPO/releases/tags/$VERSION"
fi

URL="$(curl -fsSL "$API" \
  | tr ',' '\n' \
  | grep '"browser_download_url"' \
  | grep -- "-$LABEL\.tar\.gz" \
  | head -n 1 \
  | sed 's/.*"browser_download_url"[[:space:]]*:[[:space:]]*"//; s/".*//')"

[ -n "$URL" ] || die "no $LABEL build in the $VERSION release of $REPO"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT INT TERM

say "  fetching: $(basename "$URL")"
curl -fsSL "$URL" -o "$TMP/pkg.tar.gz"
tar xzf "$TMP/pkg.tar.gz" -C "$TMP"

UNPACKED="$(find "$TMP" -mindepth 1 -maxdepth 1 -type d ! -name pkg -print | head -n 1)"
[ -n "$UNPACKED" ] || die "the downloaded archive did not contain a directory"

if [ "$OS" = macos ]; then
  APP="$(find "$UNPACKED" -maxdepth 1 -name '*.app' -print | head -n 1)"
  [ -n "$APP" ] || die "the macOS archive did not contain an .app bundle"

  DEST="/Applications"
  [ -w "$DEST" ] || DEST="$HOME/Applications"
  mkdir -p "$DEST"
  rm -rf "$DEST/$(basename "$APP")"
  cp -R "$APP" "$DEST/"

  BIN="$HOME/.local/bin"
  mkdir -p "$BIN"
  ln -sf "$DEST/$(basename "$APP")/Contents/MacOS/studiod" "$BIN/studiod"

  say ""
  say "Installed $DEST/$(basename "$APP")"
  say "The build is unsigned, so the first launch needs right-click then Open."
  say "The 'studiod' command is linked into $BIN."
  case ":$PATH:" in
    *":$BIN:"*) ;;
    *) say "Add $BIN to your PATH to use it." ;;
  esac
else
  ( cd "$UNPACKED" && PREFIX="$PREFIX" ./install.sh )
fi

say ""
say "Run 'studiod doctor' to see what the studio still needs."
