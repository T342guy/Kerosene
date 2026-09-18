#!/usr/bin/env sh
# Install the desktop entry and the window icon for the current user.
#
# On Wayland a window cannot set its own icon: the compositor shows the icon
# of the .desktop file whose name matches the window's app id, which the
# engine and the toolset both set to "kerosene". Without this, a Kerosene
# window has a placeholder in its title bar and its taskbar entry. X11,
# Windows and macOS take the icon from the window itself and need none of
# this.
#
# Usage: scripts/install-desktop.sh            (installs under ~/.local)
#        scripts/install-desktop.sh --uninstall
set -e
here="$(cd "$(dirname "$0")/.." && pwd)"
images="$here/.github/Images"
apps="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
icons="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor"

if [ "$1" = "--uninstall" ]; then
    rm -f "$apps/kerosene.desktop"
    for size in 32 64 128 256 512; do
        rm -f "$icons/${size}x${size}/apps/kerosene.png"
    done
    rm -f "$icons/scalable/apps/kerosene.svg"
    echo "removed"
else
    mkdir -p "$apps"
    cp "$here/scripts/kerosene.desktop" "$apps/kerosene.desktop"
    for size in 32 64 128 256 512; do
        mkdir -p "$icons/${size}x${size}/apps"
        cp "$images/kerosene-icon-$size.png" "$icons/${size}x${size}/apps/kerosene.png"
    done
    mkdir -p "$icons/scalable/apps"
    cp "$images/kerosene-icon.svg" "$icons/scalable/apps/kerosene.svg"
    echo "installed to $apps and $icons"
fi

# Tell the desktop about it, where the tools to do so exist.
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$apps" || true
command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q -t "$icons" 2>/dev/null || true
