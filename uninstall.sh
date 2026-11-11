#!/usr/bin/env bash
# Removes what install.sh put on this computer: programs, desktop entry, icons, the GNOME Shell extension,
# the systemd user units and their overrides, and the home-repository setting.
# It does NOT touch your home repository, your settings or your data, unless you pass --purge, which also
# deletes ~/.config/hestia and ~/.local/share/{hestia,lifestream} (including what you taught Hestia).
set -uo pipefail
purge=0; [ "${1:-}" = "--purge" ] && purge=1
prefix="${PREFIX:-$HOME/.local}"
share="${XDG_DATA_HOME:-$HOME/.local/share}"
unit="$HOME/.config/systemd/user"

systemctl --user disable --now hestia-tray.service lifestream-nightly.timer lifestream-morning.timer hestia-watch.timer 2>/dev/null
systemctl --user stop lifestream-nightly.service lifestream-morning.service hestia-watch.service 2>/dev/null
for u in hestia-tray.service hestia-watch.service hestia-watch.timer lifestream-nightly.service lifestream-nightly.timer lifestream-morning.service lifestream-morning.timer; do
  rm -rf "$unit/$u" "$unit/$u.d"
done
systemctl --user daemon-reload

pkill -x hestia 2>/dev/null; pkill -x hestia-tray 2>/dev/null
rm -f "$prefix/bin/hestia" "$prefix/bin/hestia-tray" "$prefix/bin/lifestream"
rm -rf "$prefix/libexec/hestia"
rm -f "$share/applications/app.hestia.Hestia.desktop" "$share/metainfo/app.hestia.Hestia.metainfo.xml"
rm -f "$share"/icons/hicolor/*/apps/app.hestia.Hestia*.{svg,png}
gtk-update-icon-cache -q -f -t "$share/icons/hicolor" 2>/dev/null
update-desktop-database "$share/applications" 2>/dev/null
rm -rf "$share/gnome-shell/extensions/hestia-clipboard@hestia.app"
rm -f "$HOME/.config/environment.d/hestia.conf"
if command -v gsettings >/dev/null; then
  # drop the extension from the enabled list so GNOME does not look for it
  cur="$(gsettings get org.gnome.shell enabled-extensions 2>/dev/null)"
  new="$(printf '%s' "$cur" | sed -E "s/'hestia-clipboard@hestia.app',? ?//; s/, ?\]/]/")"
  [ "$cur" != "$new" ] && gsettings set org.gnome.shell enabled-extensions "$new"
fi
if [ "$purge" = 1 ]; then
  rm -rf "$HOME/.config/hestia" "$share/hestia" "$share/lifestream"
  echo "Hestia is uninstalled, and its settings and data are deleted."
else
  echo "Hestia is uninstalled. Your settings, data and home repository were left alone (--purge deletes settings and data)."
fi
echo "Log out and in once so GNOME forgets the extension."
