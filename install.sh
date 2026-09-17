#!/bin/sh
#
# Installer for Claude Code Usage on Linux.
#
#   curl -fsSL https://raw.githubusercontent.com/Amithkrishna29z/claude-code-usage/main/install.sh | sh
#
# Downloads the released binary, puts it on PATH, and writes the menu and autostart
# entries. Runtime libraries are never installed behind your back: the command is
# printed and only run if you say yes.

set -eu

REPO="Amithkrishna29z/claude-code-usage"
ASSET="claude-usage-linux-x86_64"

BIN_DIR="${XDG_BIN_HOME:-$HOME/.local/bin}"
BIN="$BIN_DIR/claude-usage"
MENU_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
AUTOSTART_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/autostart"
ENTRY="claude-usage.desktop"

channel="release"
autostart="yes"
action="install"

say() { printf '%s\n' "$*"; }
die() {
	printf 'install.sh: %s\n' "$*" >&2
	exit 1
}

usage() {
	cat <<'EOF'
Usage: install.sh [options]

  --nightly        Install the rolling build of main instead of the last release
  --no-autostart   Skip the ~/.config/autostart entry
  --uninstall      Remove the binary and both desktop entries
  -h, --help       Show this message
EOF
}

while [ $# -gt 0 ]; do
	case "$1" in
	--nightly) channel="nightly" ;;
	--no-autostart) autostart="no" ;;
	--uninstall) action="uninstall" ;;
	-h | --help)
		usage
		exit 0
		;;
	*) die "unknown option: $1 (try --help)" ;;
	esac
	shift
done

# Asks on the controlling terminal rather than stdin, which is the script itself
# when this is piped from curl. Nobody watching means no prompt and no action, so a
# non-interactive run finishes instead of blocking on an answer that never comes.
ask() {
	[ -t 1 ] || return 1
	[ -r /dev/tty ] || return 1
	printf '%s [y/N] ' "$1" >/dev/tty
	read -r reply </dev/tty || return 1
	case "$reply" in
	[Yy] | [Yy][Ee][Ss]) return 0 ;;
	*) return 1 ;;
	esac
}

if [ "$action" = "uninstall" ]; then
	rm -f "$BIN" "$MENU_DIR/$ENTRY" "$AUTOSTART_DIR/$ENTRY"
	say "Removed $BIN and its desktop entries."
	say "Settings at ${XDG_CONFIG_HOME:-$HOME/.config}/ClaudeCodeUsage are left alone."
	exit 0
fi

[ "$(uname -s)" = "Linux" ] || die "this installer is for Linux; see the README for Windows and macOS"
[ "$(uname -m)" = "x86_64" ] || die "no published binary for $(uname -m); build from source instead"

# --- runtime libraries -------------------------------------------------------
# The binary is dynamically linked: without these it either fails to start or comes
# up with no tray icon.

ldconfig_bin=""
for candidate in ldconfig /sbin/ldconfig /usr/sbin/ldconfig; do
	if command -v "$candidate" >/dev/null 2>&1; then
		ldconfig_bin="$candidate"
		break
	fi
done

missing=""
if [ -z "$ldconfig_bin" ]; then
	say "Note: no ldconfig here, so the runtime libraries were not checked."
else
	have_lib() { "$ldconfig_bin" -p 2>/dev/null | grep -q "$1"; }

	have_lib "libgtk-3.so.0" || missing="$missing gtk3"
	have_lib "libayatana-appindicator3.so.1" || have_lib "libappindicator3.so.1" ||
		missing="$missing appindicator"
	have_lib "libxdo.so.3" || missing="$missing xdo"
fi

if [ -n "$missing" ]; then
	if command -v apt-get >/dev/null 2>&1; then
		dep_cmd="sudo apt-get update && sudo apt-get install -y libgtk-3-0 libayatana-appindicator3-1 libxdo3"
	elif command -v dnf >/dev/null 2>&1; then
		dep_cmd="sudo dnf install -y gtk3 libappindicator-gtk3 xdotool"
	elif command -v pacman >/dev/null 2>&1; then
		dep_cmd="sudo pacman -S --needed gtk3 libayatana-appindicator xdotool"
	elif command -v zypper >/dev/null 2>&1; then
		dep_cmd="sudo zypper install -y gtk3 libayatana-appindicator3-1 xdotool"
	else
		dep_cmd=""
	fi

	say "Missing runtime libraries:$missing"
	if [ -z "$dep_cmd" ]; then
		say "No known package manager here. Install GTK 3, libayatana-appindicator and libxdo,"
		say "then re-run this script."
	else
		say ""
		say "  $dep_cmd"
		say ""
		if ask "Run that now?"; then
			sh -c "$dep_cmd" || die "installing the runtime libraries failed"
		else
			say "Skipped. Run it yourself before starting the widget."
		fi
	fi
fi

# --- binary ------------------------------------------------------------------

case "$channel" in
# A prerelease is not what /releases/latest/ resolves to, so the rolling build has
# to be addressed by its own tag.
nightly) url="https://github.com/$REPO/releases/download/latest/$ASSET" ;;
*) url="https://github.com/$REPO/releases/latest/download/$ASSET" ;;
esac

tmp="$(mktemp)"
trap 'rm -f "$tmp"' EXIT INT TERM

say "Downloading the $channel build..."
if command -v curl >/dev/null 2>&1; then
	curl -fsSL "$url" -o "$tmp" || die "download failed: $url"
elif command -v wget >/dev/null 2>&1; then
	wget -qO "$tmp" "$url" || die "download failed: $url"
else
	die "neither curl nor wget is installed"
fi

# GitHub answers a missing asset with an HTML page, which would otherwise be
# installed as a perfectly executable file that does nothing.
magic="$(dd if="$tmp" bs=4 count=1 2>/dev/null | od -An -tx1 | tr -d ' \n')"
[ "$magic" = "7f454c46" ] || die "what came back is not a Linux binary; check $url"

mkdir -p "$BIN_DIR"
install -m 755 "$tmp" "$BIN"
say "Installed $BIN"

# --- desktop entries ---------------------------------------------------------

write_entry() {
	mkdir -p "$(dirname "$1")"
	cat >"$1" <<EOF
[Desktop Entry]
Type=Application
Name=Claude Code Usage
Comment=How much of your Claude Code allowance is gone
Exec=$BIN
Icon=utilities-system-monitor
Terminal=false
Categories=Utility;Monitor;
EOF
	if [ "$2" = "autostart" ]; then
		printf 'X-GNOME-Autostart-enabled=true\n' >>"$1"
	fi
}

write_entry "$MENU_DIR/$ENTRY" menu
say "Added it to the application menu."

if [ "$autostart" = "yes" ]; then
	write_entry "$AUTOSTART_DIR/$ENTRY" autostart
	say "It will start at login (--no-autostart, or delete $AUTOSTART_DIR/$ENTRY, to stop that)."
fi

case ":$PATH:" in
*":$BIN_DIR:"*) ;;
*) say "Note: $BIN_DIR is not on your PATH, so 'claude-usage' will not resolve until it is." ;;
esac

say ""
if [ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ] && ask "Start it now?"; then
	nohup "$BIN" >/dev/null 2>&1 &
	say "Running. The icon is in your system tray."
else
	say "Start it with: $BIN"
fi
