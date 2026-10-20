#!/usr/bin/env bash
# Installs SlimSpot for the current user on Linux (no root needed).
#   ./install.sh                 install; builds first unless a prebuilt `slimspot` sits next to this script
#   ./install.sh --no-autostart  same, but never turn autostart on
#   ./install.sh --release       build with the full LTO profile (~4 min per change; same RAM)
#   ./install.sh --uninstall     remove the binary, menu entry, icon and autostart entry
#                                (keeps ~/.config/SlimSpot: login, settings, logs)
# Re-run after pulling changes (or unpacking a newer release) to update the installed copy.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
data="${XDG_DATA_HOME:-$HOME/.local/share}"
config="${XDG_CONFIG_HOME:-$HOME/.config}"
bin_dir="$HOME/.local/bin"
exe="$bin_dir/slimspot"
desktop="$data/applications/slimspot.desktop"
icon="$data/icons/hicolor/256x256/apps/slimspot.png"
# Must match autostart_linux.rs: "<exe>" --tray
autostart="$config/autostart/slimspot.desktop"

profile=fast autostart_mode=auto uninstall=0
for arg in "$@"; do
    case "$arg" in
        --no-autostart) autostart_mode=off ;;
        --release) profile=release ;;
        --uninstall) uninstall=1 ;;
        *) echo "Unknown option: $arg" >&2; exit 2 ;;
    esac
done

stop_running() {
    if pgrep -x slimspot >/dev/null; then
        echo "Stopping the running SlimSpot (playback will stop)..."
        pkill -x slimspot || true
        for _ in $(seq 20); do pgrep -x slimspot >/dev/null || break; sleep 0.5; done
    fi
}

if [ "$uninstall" = 1 ]; then
    stop_running
    rm -f "$exe" "$desktop" "$icon" "$autostart"
    echo "Uninstalled. Login, settings and logs remain in $config/SlimSpot."
    exit 0
fi

# A release archive ships the binary and its icon; a source checkout builds them.
if [ -x "$here/slimspot" ]; then
    built="$here/slimspot"
    icon_src="$here/slimspot.png"
else
    echo "Building ($profile)..."
    (cd "$here" && cargo build --profile "$profile")
    built="$here/target/$profile/slimspot"
    icon_src="$here/assets/icon-256.png"
fi

first_install=1
[ -e "$exe" ] && first_install=0
stop_running
mkdir -p "$bin_dir" "$(dirname "$desktop")" "$(dirname "$icon")"
# Copy then rename, so a running copy (if any survived) keeps its old file.
cp "$built" "$exe.new" && chmod 755 "$exe.new" && mv -f "$exe.new" "$exe"
cp "$icon_src" "$icon"
cat > "$desktop" <<EOF
[Desktop Entry]
Type=Application
Name=SlimSpot
Comment=Minimal Spotify client
Exec="$exe"
Icon=slimspot
Categories=AudioVideo;Audio;Player;
StartupWMClass=slimspot
EOF
echo "Installed to $exe"

# Autostart: on for a first install unless --no-autostart; afterwards only keep an existing entry,
# so turning it off in the app sticks across updates.
if [ "$autostart_mode" = off ]; then
    rm -f "$autostart"
    echo "Autostart: off"
elif [ -e "$autostart" ] || [ "$first_install" = 1 ]; then
    mkdir -p "$(dirname "$autostart")"
    printf '[Desktop Entry]\nType=Application\nName=SlimSpot\nExec="%s" --tray\nIcon=slimspot\nX-GNOME-Autostart-enabled=true\n' "$exe" > "$autostart"
    echo "Autostart: on (starts in the tray; toggle it in the app)"
else
    echo "Autostart: left off (turned off in the app earlier)"
fi

setsid "$exe" >/dev/null 2>&1 < /dev/null &
echo "SlimSpot started."
