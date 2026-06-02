#!/usr/bin/env bash
# Remove a managecode binary installed via install.sh.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/Diaugeia/ManageCode/main/uninstall.sh | bash
#
# Env overrides:
#   INSTALL_DIR   directory the binary lives in (default: ~/.local/bin)
#
# Flags:
#   --purge       also delete the config directory (~/.managecode)
#   -y, --yes     do not prompt before deleting the config directory
#
# Note: ~/.claude and ~/.codex are scanned (not created) by managecode and are
# never touched by this script.
set -euo pipefail

INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
CONFIG_DIR="$HOME/.managecode"
PURGE=0
ASSUME_YES=0

for arg in "$@"; do
  case "$arg" in
    --purge)   PURGE=1 ;;
    -y|--yes)  ASSUME_YES=1 ;;
    *) printf 'error: unknown argument: %s\n' "$arg" >&2; exit 1 ;;
  esac
done

BIN="$INSTALL_DIR/managecode"
removed=0

if [ -e "$BIN" ]; then
  rm -f "$BIN"
  printf '✓ removed: %s\n' "$BIN"
  removed=1
else
  printf '· no binary at %s\n' "$BIN"
fi

if [ "$PURGE" -eq 1 ]; then
  if [ -d "$CONFIG_DIR" ]; then
    if [ "$ASSUME_YES" -ne 1 ]; then
      printf 'delete config directory %s? [y/N] ' "$CONFIG_DIR"
      read -r reply </dev/tty || reply=""
      case "$reply" in
        y|Y|yes|YES) ;;
        *) printf '· kept: %s\n' "$CONFIG_DIR"; PURGE=0 ;;
      esac
    fi
  fi
  if [ "$PURGE" -eq 1 ] && [ -d "$CONFIG_DIR" ]; then
    rm -rf "$CONFIG_DIR"
    printf '✓ removed: %s\n' "$CONFIG_DIR"
    removed=1
  fi
elif [ -d "$CONFIG_DIR" ]; then
  printf '· kept config: %s (use --purge to remove)\n' "$CONFIG_DIR"
fi

if [ "$removed" -eq 1 ]; then
  printf '✓ managecode uninstalled\n'
else
  printf '· nothing to remove\n'
fi
