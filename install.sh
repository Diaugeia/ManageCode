#!/usr/bin/env bash
# Download and install a prebuilt minionscode binary from GitHub Releases.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/ChengAoShen/MinionsCode/main/install.sh | bash
#
# Env overrides:
#   VERSION       specific release tag (default: latest)
#   INSTALL_DIR   destination directory (default: ~/.local/bin)
#
# Requires: curl, tar. No Rust toolchain needed.
set -euo pipefail

REPO="ChengAoShen/MinionsCode"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
VERSION="${VERSION:-}"

err() { printf 'error: %s\n' "$*" >&2; exit 1; }

case "$(uname -s)" in
  Linux)  os="linux"  ;;
  Darwin) os="macos"  ;;
  *) err "unsupported OS: $(uname -s). Build from source: https://github.com/${REPO}#build-from-source" ;;
esac

case "$(uname -m)" in
  x86_64|amd64)  arch="x86_64"   ;;
  arm64|aarch64) arch="aarch64"  ;;
  *) err "unsupported arch: $(uname -m)" ;;
esac

# Linux prebuilts are currently only x86_64.
if [ "$os" = "linux" ] && [ "$arch" != "x86_64" ]; then
  err "no prebuilt for linux-${arch}. Build from source: https://github.com/${REPO}#build-from-source"
fi

# Resolve version (latest tag) when not pinned.
if [ -z "$VERSION" ]; then
  VERSION=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
    | grep -m1 '"tag_name"' | cut -d'"' -f4)
  [ -n "$VERSION" ] || err "could not resolve the latest release tag"
fi

ASSET="minionscode-${VERSION}-${os}-${arch}.tar.gz"
URL="https://github.com/${REPO}/releases/download/${VERSION}/${ASSET}"

printf '→ %s\n' "${ASSET}"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

curl -fsSL "$URL" -o "$TMP/$ASSET" \
  || err "download failed: $URL"

tar -xzf "$TMP/$ASSET" -C "$TMP"

[ -f "$TMP/minionscode" ] || err "archive did not contain a 'minionscode' binary"

mkdir -p "$INSTALL_DIR"
mv "$TMP/minionscode" "$INSTALL_DIR/minionscode"
chmod +x "$INSTALL_DIR/minionscode"

printf '✓ installed: %s/minionscode (%s)\n' "$INSTALL_DIR" "$VERSION"

# PATH hint.
if ! printf ':%s:' "$PATH" | grep -q ":${INSTALL_DIR}:"; then
  printf '\n  note: %s is not on your PATH.\n' "$INSTALL_DIR"
  printf '  add this to your shell rc:\n'
  printf '    export PATH="%s:$PATH"\n' "$INSTALL_DIR"
fi

# macOS gatekeeper note: prebuilt binary is unsigned.
if [ "$os" = "macos" ]; then
  printf '\n  note: macOS may quarantine unsigned binaries on first run.\n'
  printf '  if blocked: xattr -d com.apple.quarantine %s/minionscode\n' "$INSTALL_DIR"
fi
