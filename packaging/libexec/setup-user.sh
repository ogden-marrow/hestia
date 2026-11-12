#!/usr/bin/env bash
# setup-user.sh (also installed as `hestia-setup`) — the per-person part of installing Hestia: seed the home repository
# setting, write the systemd user units that run the nightly scan, the morning note, the tray and the machine watcher, and
# enable them. install.sh calls this; a .deb install (which can only put files under /usr) tells you to run `hestia-setup` once.
# It is safe to run again. It lives next to the units' templates (./systemd) and the programs (../../bin).
#   HESTIA_HOME=~/Home hestia-setup     # the first value of the home repository setting; never overwrites one that is set
set -euo pipefail
self="$(readlink -f "${BASH_SOURCE[0]}")"; libexec="$(dirname "$self")"; prefix="$(cd "$libexec/../.." && pwd)"
home_repo="${HESTIA_HOME:-$HOME/Home}"
# These values end up in a settings file and systemd units: refuse characters that could change what a line or unit does.
for v in "$libexec" "$prefix" "$home_repo"; do
  case "$v" in
    *[!A-Za-z0-9_./~+@:,=-]*) echo "hestia-setup: '$v' has a character that is not safe to put in a systemd unit (letters, digits and _ . / ~ + @ : , = - only)." >&2; exit 1 ;;
  esac
done
[ -d "$libexec/systemd" ] || { echo "hestia-setup: the unit templates are missing ($libexec/systemd)" >&2; exit 1; }

# where the home repository is: ONE setting (`home=` in the app's settings file) that the app, the watcher and the restore script
# all read. Seeded only if the person has not set one; never overwritten (the app and signing in own it after that).
settings="$HOME/.config/hestia/settings"; mkdir -p "$(dirname "$settings")"
if ! grep -q '^home=.\+' "$settings" 2>/dev/null; then
  { grep -v '^home=' "$settings" 2>/dev/null || true; printf 'home=%s\n' "$home_repo"; } > "$settings.new" && mv "$settings.new" "$settings"
fi

# systemd user units: overnight scan (~02:30), morning notification (07:30), tray, machine watch
unit="$HOME/.config/systemd/user"; mkdir -p "$unit"
for f in "$libexec"/systemd/*; do
  sed -e "s|@LIBEXEC@|$libexec|g" -e "s|%h/\.local/bin|${prefix/#"$HOME"/%h}/bin|g" "$f" > "$unit/$(basename "$f")"
done
systemctl --user daemon-reload
systemctl --user enable --now hestia-tray.service lifestream-nightly.timer lifestream-morning.timer
systemctl --user enable hestia-watch.timer
# restart, not just start: a timer that is already running keeps its old schedule until restarted
systemctl --user restart hestia-watch.timer
echo "Hestia is set up for $(id -un): the home repository setting, the schedules and the tray."
systemctl --user list-timers 'lifestream-*' 'hestia-*' || true
