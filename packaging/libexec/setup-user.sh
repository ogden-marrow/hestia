#!/usr/bin/env bash
# setup-user.sh (also installed as `hestia-setup`) — the per-person part of installing Hestia: seed the home repository
# setting, write the systemd user units that run the nightly scan, the morning note, the tray, the machine watcher and the weekly restore rehearsal, and
# enable them. install.sh calls this; a .deb install (which can only put files under /usr) tells you to run `hestia-setup` once.
# It is safe to run again. It lives next to the units' templates (./systemd) and the programs (../../bin).
#   HESTIA_HOME=~/Home hestia-setup     # the first value of the home repository setting; never overwrites one that is set
set -euo pipefail
# Where it was run from, keeping the stable path: install.sh runs it as $PREFIX/libexec/hestia/setup-user.sh (a link through the
# current build, which the units must name, never a build folder); a .deb runs it through the /usr/bin/hestia-setup link.
self="${BASH_SOURCE[0]}"
if [ -L "$self" ]; then t="$(readlink "$self")"; case "$t" in /*) self="$t" ;; *) self="$(dirname "$self")/$t" ;; esac; fi
libexec="$(cd "$(dirname "$self")" && pwd)"; prefix="$(cd "$libexec/../.." && pwd)"
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
changed=" "
for f in "$libexec"/systemd/*; do
  u="$unit/$(basename "$f")"
  sed -e "s|@LIBEXEC@|$libexec|g" -e "s|%h/\.local/bin|${prefix/#"$HOME"/%h}/bin|g" "$f" > "$u.new"
  cmp -s "$u.new" "$u" || changed="$changed$(basename "$f") "
  mv -f "$u.new" "$u"
done
systemctl --user daemon-reload
systemctl --user enable --now hestia-tray.service
# Each timer is enabled and started. Restarted only when its unit file changed: a running timer keeps its old schedule until
# restarted, and restarting one whose file is the same would only reset its countdown (an update must not disturb the backup).
for t in lifestream-nightly.timer lifestream-morning.timer hestia-watch.timer hestia-rehearse.timer; do
  systemctl --user enable "$t"
  case "$changed" in *" $t "*) systemctl --user restart "$t" ;; *) systemctl --user start "$t" ;; esac
done
echo "Hestia is set up for $(id -un): the home repository setting, the schedules and the tray."
systemctl --user list-timers 'lifestream-*' 'hestia-*' || true
