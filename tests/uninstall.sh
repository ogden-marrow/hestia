#!/usr/bin/env bash
# uninstall.sh removes what install put on the computer, leaves the person's own things alone, deletes the clipboard
# history, gives Alt+Space back, and --purge is the only thing that deletes settings. install.sh refuses path
# characters that could change a systemd unit. Runs in a scratch HOME with stand-ins for every system command, so it can
# never touch the real session (no real systemctl, pkill or gsettings is ever called).
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
T="$(mktemp -d)"; trap 'rm -rf "$T"' EXIT
export HOME="$T/home" XDG_DATA_HOME="$T/home/.local/share" PREFIX="$T/home/.local"
mkdir -p "$T/bin" "$HOME"
fail() { echo "FAIL: $*" >&2; exit 1; }
for c in systemctl pkill gtk-update-icon-cache update-desktop-database; do printf '#!/bin/sh\necho "%s $*" >> "%s/calls.log"\n' "$c" "$T" > "$T/bin/$c"; done
cat > "$T/bin/gsettings" <<EOF
#!/bin/sh
echo "gsettings \$*" >> "$T/calls.log"
case "\$1 \$2 \$3" in
  "get org.gnome.shell enabled-extensions") echo "['hestia-clipboard@hestia.app', 'other@example']" ;;
  "get org.gnome.desktop.wm.keybindings activate-window-menu") echo "@as []" ;;
esac
EOF
chmod +x "$T"/bin/*; export PATH="$T/bin:$PATH"

lay() { # put down what an install leaves, plus things that are the person's own
  mkdir -p "$HOME/.local/bin" "$HOME/.local/libexec/hestia" "$HOME/.local/share/hestia" "$HOME/.local/share/lifestream" "$HOME/.config/hestia" "$HOME/.config/systemd/user/hestia-watch.timer.d" "$HOME/Home"
  touch "$HOME/.local/bin/hestia" "$HOME/.local/bin/hestia-tray" "$HOME/.local/bin/lifestream" "$HOME/.local/libexec/hestia/watch.sh"
  echo '{"text":"copied"}' > "$HOME/.local/share/hestia/clipboard.jsonl"; echo x > "$HOME/.local/share/hestia/launches.tsv"
  echo "scheme=dark" > "$HOME/.config/hestia/settings"; echo "mine" > "$HOME/Home/notes.txt"
  touch "$HOME/.config/systemd/user/hestia-watch.timer" "$HOME/.config/systemd/user/hestia-watch.timer.d/hestia.conf"
}

lay; : > "$T/calls.log"
bash "$here/uninstall.sh" >/dev/null
[ ! -e "$HOME/.local/bin/hestia" ] && [ ! -e "$HOME/.local/bin/lifestream" ] && [ ! -d "$HOME/.local/libexec/hestia" ] || fail "programs were not removed"
[ ! -e "$HOME/.config/systemd/user/hestia-watch.timer" ] && [ ! -e "$HOME/.config/systemd/user/hestia-watch.timer.d" ] || fail "units and overrides were not removed"
[ ! -e "$HOME/.local/share/hestia/clipboard.jsonl" ] || fail "the clipboard history survived an uninstall"
[ -f "$HOME/.config/hestia/settings" ] && [ -f "$HOME/.local/share/hestia/launches.tsv" ] || fail "settings and data must survive without --purge"
[ "$(cat "$HOME/Home/notes.txt")" = "mine" ] || fail "the person's own files were touched"
grep -q "gsettings reset org.gnome.desktop.wm.keybindings activate-window-menu" "$T/calls.log" || fail "Alt+Space was not given back"
grep -q "gsettings set org.gnome.shell enabled-extensions \\['other@example'\\]" "$T/calls.log" || fail "the extension was not dropped from the enabled list (or the others were lost)"

lay
bash "$here/uninstall.sh" --purge >/dev/null
[ ! -e "$HOME/.config/hestia" ] && [ ! -e "$HOME/.local/share/hestia" ] || fail "--purge did not delete settings and data"
[ "$(cat "$HOME/Home/notes.txt")" = "mine" ] || fail "--purge touched the home repository"

# install.sh refuses path characters that could change a systemd unit, before it builds or writes anything
for bad in "$T/a b" "$T/a'b" "$T/a\$b" "$T/a%b"; do
  out=$(HESTIA_HOME="$bad" bash "$here/install.sh" 2>&1) && fail "install.sh accepted an unsafe path: $bad"
  echo "$out" | grep -q "not safe to put in a systemd unit" || fail "no clear message for: $bad"
done
[ ! -e "$HOME/.local/bin/hestia" ] || fail "install.sh wrote something before refusing"
echo "uninstall ok"
