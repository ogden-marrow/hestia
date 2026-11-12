#!/usr/bin/env bash
# A clean computer, end to end: install from this checkout into an empty home folder, check what was installed, make a
# home repository, let the watcher capture it once, dry-run a restore, scan the result, uninstall, and check the person's
# own files survived. Runs in a scratch HOME with stand-ins for every system command (systemctl, gsettings, ...), so it
# can never touch a real session. (Build first: install.sh builds if needed.)
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}" RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"   # keep the toolchain findable
T="$(mktemp -d)"; trap 'rm -rf "$T"' EXIT
export HOME="$T/home" XDG_DATA_HOME="$T/home/.local/share" XDG_RUNTIME_DIR="$T" PREFIX="$T/home/.local" HESTIA_HOME="$T/home/Home" HESTIA_NOW=1
mkdir -p "$T/bin" "$HOME"
fail() { echo "FAIL: $*" >&2; exit 1; }
for c in systemctl pkill gsettings gtk-update-icon-cache update-desktop-database dconf; do printf '#!/bin/sh\necho "%s $*" >> "%s/calls.log"\n' "$c" "$T" > "$T/bin/$c"; done
chmod +x "$T"/bin/*; export PATH="$T/bin:$PATH"
git config --global user.email t@t.t; git config --global user.name t; git config --global init.defaultBranch main

# 1. install onto a computer that has never seen Hestia
bash "$here/install.sh" >"$T/install.log" 2>&1 || { cat "$T/install.log" >&2; fail "install.sh failed"; }
for b in hestia hestia-tray lifestream; do [ -x "$PREFIX/bin/$b" ] || fail "$b was not installed"; done
"$PREFIX/bin/lifestream" --version >/dev/null || fail "lifestream does not run"
for f in watch.sh capture.sh bootstrap.sh dconf-filter.sh capture-paths.default; do [ -e "$PREFIX/libexec/hestia/$f" ] || fail "libexec/$f missing"; done
for f in watch.sh capture.sh bootstrap.sh dconf-filter.sh; do [ -x "$PREFIX/libexec/hestia/$f" ] || fail "$f is not executable"; done
grep -qx "home=$HESTIA_HOME" "$HOME/.config/hestia/settings" || fail "the home repository setting was not seeded in the app's settings"
[ ! -e "$HOME/.config/environment.d/hestia.conf" ] || fail "a second place for the home repository was written (one setting only)"
unit="$HOME/.config/systemd/user"
for u in hestia-watch.service hestia-watch.timer lifestream-nightly.service lifestream-morning.service hestia-tray.service; do [ -f "$unit/$u" ] || fail "$u missing"; done
grep -l '@[A-Z_]*@' "$unit"/* 2>/dev/null && fail "a systemd unit still has an unfilled placeholder"
grep -qx "ExecStart=$PREFIX/libexec/hestia/watch.sh" "$unit/hestia-watch.service" || fail "the watch unit must run the installed script and take the home repository from the setting, not carry its own copy"
grep -q '^OnActiveSec=' "$unit/hestia-watch.timer" || fail "the watch timer would not fire after install"
grep -q "Exec=" "$HOME/.local/share/applications/app.hestia.Hestia.desktop" || fail "no desktop entry"
[ -d "$HOME/.local/share/gnome-shell/extensions/hestia-clipboard@hestia.app" ] || fail "the shell extension was not installed"
grep -q "systemctl --user enable hestia-watch.timer" "$T/calls.log" || fail "the watch timer was not enabled"

# 2. the person's own home repository, and a file of theirs that must survive everything below
mkdir -p "$HESTIA_HOME"; ( cd "$HESTIA_HOME" && git init -q . && touch tree.toml && git add -A && git commit -qm "Start the home repository" )
echo 'alias ll="ls -l"' > "$HOME/.bashrc"; echo "mine" > "$HESTIA_HOME/notes.txt"
git -C "$HESTIA_HOME" add notes.txt && git -C "$HESTIA_HOME" commit -qm "my notes"

# 3. the watcher captures this clean computer once; a local remote is refused (only a private GitHub repository is pushed to)
git init -q --bare "$T/remote.git"; git -C "$HESTIA_HOME" remote add origin "$T/remote.git"
bash "$PREFIX/libexec/hestia/watch.sh" || fail "the watcher failed on a clean computer"   # no argument: it finds the repository from the setting
git -C "$HESTIA_HOME" log --format=%s | grep -q "first capture of this computer" || fail "the first capture was not committed"
grep -q "not pushed: remote is not a GitHub repository" "$XDG_DATA_HOME/hestia/watch.json" || fail "the watcher must refuse a remote that is not a private GitHub repository: $(cat "$XDG_DATA_HOME/hestia/watch.json")"
[ -z "$(git -C "$T/remote.git" rev-parse --verify -q main 2>/dev/null)" ] || fail "something was pushed to a non-GitHub remote"
[ -z "$(git -C "$HESTIA_HOME" status --short)" ] || fail "the home repository is not clean after a capture"

# 3b. one setting decides: pointing it elsewhere (absolute, ~/ and relative forms) moves the watcher, and nothing else is touched
before=$(git -C "$HESTIA_HOME" rev-list --count HEAD)
mkdir -p "$HOME/Other"; ( cd "$HOME/Other" && git init -q . && touch tree.toml && git add -A && git commit -qm "other" )
for form in "$HOME/Other" "~/Other" "Other"; do
  sed -i '/^home=/d' "$HOME/.config/hestia/settings"; echo "home=$form" >> "$HOME/.config/hestia/settings"
  bash "$PREFIX/libexec/hestia/watch.sh" || fail "the watcher failed for home=$form"
  git -C "$HOME/Other" log --format=%s | grep -q "first capture of this computer" || fail "home=$form did not move the watcher to the other repository"
  git -C "$HOME/Other" reset -q --hard HEAD~1 2>/dev/null || true
done
[ "$(git -C "$HESTIA_HOME" rev-list --count HEAD)" = "$before" ] || fail "the watcher touched a repository the setting no longer names"
sed -i '/^home=/d' "$HOME/.config/hestia/settings"; echo "home=$HESTIA_HOME" >> "$HOME/.config/hestia/settings"

# 4. nothing private ended up in what was captured
"$PREFIX/bin/lifestream" secret-scan --repo "$HESTIA_HOME" --range all >/dev/null || fail "the captured history has a possible secret"
git -C "$HESTIA_HOME" ls-files | grep -qE '(^|/)\.(ssh|gnupg)/' && fail "a sensitive path was captured"

# 5. a restore is a dry run by default and changes nothing
echo 'my own bashrc' > "$HOME/.bashrc"
out=$(bash "$PREFIX/libexec/hestia/bootstrap.sh" "$HESTIA_HOME") || fail "bootstrap dry run failed"
echo "$out" | grep -q "dry run" || fail "bootstrap did not say it was a dry run"
[ "$(cat "$HOME/.bashrc")" = "my own bashrc" ] || fail "a dry run changed a file"

# 6. uninstall leaves the person's own things alone
bash "$here/uninstall.sh" >/dev/null || fail "uninstall failed"
for b in hestia hestia-tray lifestream; do [ ! -e "$PREFIX/bin/$b" ] || fail "$b survived uninstall"; done
[ ! -e "$PREFIX/libexec/hestia" ] && [ ! -e "$unit/hestia-watch.timer" ] || fail "libexec or units survived uninstall"
[ "$(cat "$HESTIA_HOME/notes.txt")" = "mine" ] && [ -d "$HESTIA_HOME/.git" ] || fail "uninstall touched the home repository"
[ "$(cat "$HOME/.bashrc")" = "my own bashrc" ] || fail "uninstall touched the person's files"
echo "install end to end ok"
