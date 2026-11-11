#!/usr/bin/env bash
# Installs Hestia for the current user: programs, desktop entry, icons, the GNOME Shell extension and
# the systemd user timers. Works from a release tarball (bin/ next to this script) or a source checkout.
#
#   HESTIA_HOME=~/Home ./install.sh      # where your home repository lives (default ~/Home)
#   PREFIX=~/.local ./install.sh         # programs go to $PREFIX/bin (default ~/.local)
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
prefix="${PREFIX:-$HOME/.local}"
home_repo="${HESTIA_HOME:-$HOME/Home}"
share="${XDG_DATA_HOME:-$HOME/.local/share}"
pk="$here/packaging"

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

# where the home repository is, for the app and the services (a login is needed once)
mkdir -p "$HOME/.config/environment.d"
printf 'HESTIA_HOME=%s\n' "$home_repo" > "$HOME/.config/environment.d/hestia.conf"

# systemd user units: overnight scan (~02:30), morning notification (07:30), machine watch
unit="$HOME/.config/systemd/user"
mkdir -p "$unit"
for f in "$pk"/systemd/*; do
  sed -e "s|@HOME_REPO@|$home_repo|g" -e "s|%h/\.local/bin|${prefix/#"$HOME"/%h}/bin|g" "$f" > "$unit/$(basename "$f")"
done
systemctl --user daemon-reload
systemctl --user enable --now hestia-tray.service lifestream-nightly.timer lifestream-morning.timer
# the machine watch runs a script from your home repository: only turn it on when the repository has it
if [ -x "$home_repo/applications/linux/ubuntu/26.10/watch.sh" ]; then systemctl --user enable --now hestia-watch.timer; fi

echo "Hestia is installed. Log out and in once so the launcher and the home repository setting take effect."
systemctl --user list-timers 'lifestream-*' 'hestia-*' || true
