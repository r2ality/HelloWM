#!/bin/sh
set -eu

say() { printf '%s\n' "$*"; }
die() { printf 'HelloWM uninstaller: %s\n' "$*" >&2; exit 1; }

PURGE_CONFIG=no
case "${1:-}" in
    "") ;;
    --purge-config) PURGE_CONFIG=yes ;;
    -h|--help)
        say "Usage: ./uninstall.sh [--purge-config]"
        say "By default, installed program/session files are removed but your config is kept."
        exit 0
        ;;
    *) die "Unknown option: $1 (use --help)." ;;
esac

[ "$(id -u)" -ne 0 ] || die "Run this as your normal user, not with sudo. The script will use sudo for system files."

SCRIPT_DIR=$(CDPATH= cd "$(dirname "$0")" && pwd)
CONFIG_DIR="$HOME/.config/hellowm"
STATE_DIR="$HOME/.local/state/hellowm"
MANIFEST="$STATE_DIR/install-manifest"
HELPERS_DIR="$HOME/.local/libexec/hellowm"
ASSET_PATH="$HOME/.config/assets/windows.png"
XINITRC="$HOME/.xinitrc"

OS_ID=unknown
if [ -r /etc/os-release ]; then
    # shellcheck disable=SC1091
    . /etc/os-release
    OS_ID=${ID:-unknown}
fi
if [ -e /etc/NIXOS ] || [ "$OS_ID" = nixos ]; then
    DETECTED_MODE=nixos
else
    DETECTED_MODE=generic
fi

as_root() {
    if [ "$(id -u)" -eq 0 ]; then
        "$@"
    elif command -v sudo >/dev/null 2>&1; then
        sudo "$@"
    else
        die "sudo is required to remove files outside your home directory."
    fi
}

manifest_value() {
    key=$1
    [ -f "$MANIFEST" ] || return 0
    awk -F= -v wanted="$key" '$1 == wanted { sub(/^[^=]*=/, ""); print; exit }' "$MANIFEST"
}

MODE=$(manifest_value MODE)
[ -n "$MODE" ] || MODE=$DETECTED_MODE
BIN_PATH=$(manifest_value BIN_PATH)
SESSION_FILE=$(manifest_value SESSION_FILE)
MODULE_PATH=$(manifest_value MODULE_PATH)
HELPERS_DIR=$(manifest_value HELPERS_DIR)
ASSET_PATH=$(manifest_value ASSET_PATH)
CREATED_START_ASSET=$(manifest_value CREATED_START_ASSET)
CREATED_XINITRC=$(manifest_value CREATED_XINITRC)
XINITRC_TEMPLATE=$(manifest_value XINITRC_TEMPLATE)

if [ "$MODE" = nixos ]; then
    BIN_PATH=${BIN_PATH:-$HOME/.local/bin/hellowm}
    MODULE_PATH=${MODULE_PATH:-$CONFIG_DIR/nixos-session.nix}

    # Don't leave a broken NixOS import behind. Remove the import from your
    # NixOS configuration and rebuild first, then run this script again.
    if [ -d /etc/nixos ] && grep -R -F -q -- "$MODULE_PATH" /etc/nixos 2>/dev/null; then
        die "NixOS configuration still imports $MODULE_PATH. Remove that import, run 'sudo nixos-rebuild switch', then rerun ./uninstall.sh."
    fi

    rm -f "$BIN_PATH" "$MODULE_PATH"
    rmdir "$(dirname "$BIN_PATH")" 2>/dev/null || true
    say "Removed HelloWM binary and generated NixOS session module."
else
    BIN_PATH=${BIN_PATH:-/usr/local/bin/hellowm}
    SESSION_FILE=${SESSION_FILE:-/usr/share/xsessions/hellowm.desktop}
    as_root rm -f "$BIN_PATH" "$SESSION_FILE"
    say "Removed HelloWM binary and X session entry."
fi

if [ "$CREATED_XINITRC" = yes ] && [ -n "$XINITRC_TEMPLATE" ] && [ -f "$XINITRC" ] && [ -f "$XINITRC_TEMPLATE" ]; then
    if cmp -s "$XINITRC" "$XINITRC_TEMPLATE"; then
        rm -f "$XINITRC"
        say "Removed the unchanged .xinitrc created by HelloWM."
    else
        say "Kept $XINITRC because it has been edited since HelloWM created it."
    fi
fi
[ -z "$XINITRC_TEMPLATE" ] || rm -f "$XINITRC_TEMPLATE"

if [ -n "$HELPERS_DIR" ]; then
    rm -f "$HELPERS_DIR/about-computer.sh" "$HELPERS_DIR/create-launcher.sh"
    rmdir "$HELPERS_DIR" 2>/dev/null || true
fi
if [ "$CREATED_START_ASSET" = yes ] && [ -n "$ASSET_PATH" ] && [ -f "$ASSET_PATH" ] \
    && [ -f "$SCRIPT_DIR/assets/windows.png" ] && cmp -s "$ASSET_PATH" "$SCRIPT_DIR/assets/windows.png"; then
    rm -f "$ASSET_PATH"
    rmdir "$(dirname "$ASSET_PATH")" 2>/dev/null || true
fi

rm -f "$MANIFEST"
rmdir "$STATE_DIR" 2>/dev/null || true

if [ "$PURGE_CONFIG" = yes ]; then
    rm -rf "$CONFIG_DIR"
    say "Removed $CONFIG_DIR"
else
    say "Kept $CONFIG_DIR so your settings are preserved. Use --purge-config to remove them."
fi

say "HelloWM uninstall complete."
