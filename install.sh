#!/usr/bin/env bash
# Installs Hestia for the current user: programs, desktop entry, icons, the GNOME Shell extension and
# the systemd user timers. Works from a release tarball (bin/ next to this script) or a source checkout.
#
#   HESTIA_HOME=~/Home ./install.sh      # the first home repository setting (default ~/Home); after that the app owns it
#   PREFIX=~/.local ./install.sh         # programs go to $PREFIX/bin (default ~/.local)
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
prefix="${PREFIX:-$HOME/.local}"
home_repo="${HESTIA_HOME:-$HOME/Home}"
share="${XDG_DATA_HOME:-$HOME/.local/share}"
pk="$here/packaging"

# These paths are written into systemd units and a shell command. Refuse characters that could change what a unit
# does (quotes, backslash, $, %, newlines, spaces) rather than try to escape them.
for v in "$prefix" "$home_repo" "$here"; do
  case "$v" in
    *[!A-Za-z0-9_./~+@:,=-]*) echo "install.sh: '$v' has a character that is not safe to put in a systemd unit (letters, digits and _ . / ~ + @ : , = - only). Choose another place." >&2; exit 1 ;;
  esac
done

# programs
mkdir -p "$prefix/bin"
if [ -d "$here/bin" ]; then
  install -m755 "$here"/bin/{hestia,hestia-tray,lifestream} "$prefix/bin/"
else
  (cd "$here" && cargo build --release --locked)
  install -m755 "$here"/target/release/{hestia,hestia-tray,lifestream} "$prefix/bin/"
fi

# desktop entry, metainfo, icons
mkdir -p "$share/applications" "$share/metainfo" "$share/icons/hicolor/scalable/apps" "$share/icons/hicolor/symbolic/apps"
cp "$pk/desktop/app.hestia.Hestia.metainfo.xml" "$share/metainfo/"
sed -e "s|@HOME@|$HOME|g" -e "s|\.local/bin|${prefix#"$HOME"/}/bin|g" "$pk/desktop/app.hestia.Hestia.desktop" > "$share/applications/app.hestia.Hestia.desktop"
cp "$pk/desktop/app.hestia.Hestia.svg" "$share/icons/hicolor/scalable/apps/"
cp "$pk/desktop/app.hestia.Hestia-symbolic.svg" "$share/icons/hicolor/symbolic/apps/"
# sized PNG copies beside the SVG, for shells that prefer a raster at a fixed size (optional: needs python3-gi)
python3 - "$pk/desktop/app.hestia.Hestia.svg" "$share" <<'PY' 2>/dev/null || true
import sys, os, gi
gi.require_version('GdkPixbuf', '2.0')
from gi.repository import GdkPixbuf
for n in (48, 64, 128, 256, 512):
    dest = os.path.join(sys.argv[2], f"icons/hicolor/{n}x{n}/apps")
    os.makedirs(dest, exist_ok=True)
    GdkPixbuf.Pixbuf.new_from_file_at_size(sys.argv[1], n, n).savev(os.path.join(dest, "app.hestia.Hestia.png"), "png", [], [])
PY
gtk-update-icon-cache -q -f -t "$share/icons/hicolor" 2>/dev/null || true
update-desktop-database "$share/applications" 2>/dev/null || true

# the launcher and clipboard recorder: one GNOME Shell extension (Hestia's welcome tour turns it on)
rm -rf "$share/gnome-shell/extensions/hestia-clipboard@hestia.app"
mkdir -p "$share/gnome-shell/extensions"
cp -r "$pk/gnome-extension/hestia-clipboard@hestia.app" "$share/gnome-shell/extensions/"
glib-compile-schemas "$share/gnome-shell/extensions/hestia-clipboard@hestia.app/schemas" 2>/dev/null || echo "note: glib-compile-schemas not found; install libglib2.0-bin" >&2

# where the home repository is: ONE setting (`home=` in the app's settings file) that the app, the watcher and the restore
# script all read. Seeded here only if the person has not set one; never overwritten (the app and signing in own it after that).
settings="$HOME/.config/hestia/settings"; mkdir -p "$(dirname "$settings")"
if ! grep -q '^home=.\+' "$settings" 2>/dev/null; then
  { grep -v '^home=' "$settings" 2>/dev/null || true; printf 'home=%s\n' "$home_repo"; } > "$settings.new" && mv "$settings.new" "$settings"
fi

# the machine capture scripts ship with Hestia, never from the home repository
libexec="$prefix/libexec/hestia"
rm -rf "$libexec"; mkdir -p "$libexec"
cp "$pk"/libexec/* "$libexec/" && chmod +x "$libexec"/*.sh

# systemd user units: overnight scan (~02:30), morning notification (07:30), machine watch
unit="$HOME/.config/systemd/user"
mkdir -p "$unit"
for f in "$pk"/systemd/*; do
  sed -e "s|@LIBEXEC@|$libexec|g" -e "s|%h/\.local/bin|${prefix/#"$HOME"/%h}/bin|g" "$f" > "$unit/$(basename "$f")"
done
systemctl --user daemon-reload
systemctl --user enable --now hestia-tray.service lifestream-nightly.timer lifestream-morning.timer
systemctl --user enable hestia-watch.timer
# restart, not just start: a timer that is already running keeps its old schedule until restarted
systemctl --user restart hestia-watch.timer

echo "Hestia is installed. Log out and in once so the launcher takes effect."
systemctl --user list-timers 'lifestream-*' 'hestia-*' || true
