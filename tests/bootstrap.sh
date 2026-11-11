#!/usr/bin/env bash
# bootstrap.sh restores a machine safely: dry run by default, copies (never links), moves existing files aside,
# and holds back anything that runs by itself unless asked. Runs in a scratch HOME; never touches the real one.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
b="$here/packaging/libexec/bootstrap.sh"
T="$(mktemp -d)"; trap 'rm -rf "$T"' EXIT
export HOME="$T/home"; mkdir -p "$HOME" "$T/repo"
fail() { echo "FAIL: $*" >&2; exit 1; }
. /etc/os-release
cap="$T/repo/applications/linux/${ID:-linux}/${VERSION_ID:-unknown}"
mkdir -p "$cap/dotfiles" "$cap/config/htop" "$cap/config/autostart" "$cap/config/systemd/user"
touch "$T/repo/tree.toml"
echo 'alias ll="ls -l"' > "$cap/dotfiles/.bashrc"
echo 'hide_kernel_threads=1' > "$cap/config/htop/htoprc"
printf '[Desktop Entry]\nExec=/tmp/evil\n' > "$cap/config/autostart/evil.desktop"
printf '[Service]\nExecStart=/tmp/evil\n' > "$cap/config/systemd/user/evil.service"
printf '[org/gnome/desktop/interface]\ncolor-scheme=%s\n\n[org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/custom0]\ncommand=/tmp/evil\nbinding=<Ctrl>e\n' "'prefer-dark'" > "$cap/dconf.ini"
: > "$cap/apt-manual.txt"; : > "$cap/snap.txt"; : > "$cap/flatpak.txt"
echo 'my old bashrc' > "$HOME/.bashrc"

# 1. dry run changes nothing
out=$(bash "$b" "$T/repo")
[ "$(cat "$HOME/.bashrc")" = "my old bashrc" ] || fail "a dry run changed a file"
[ ! -e "$HOME/.config" ] || fail "a dry run created files"
echo "$out" | grep -q "dry run" || fail "a dry run does not say so"
echo "$out" | grep -q "runs a command" || fail "the custom keyboard shortcut command was not flagged"

# 2. apply: copy not link, old file moved aside, executable config held back and reported
out=$(bash "$b" "$T/repo" --apply --only dotfiles,config)
[ -f "$HOME/.bashrc" ] && [ ! -L "$HOME/.bashrc" ] || fail "a dotfile must be a plain copy, never a link into the repository"
grep -q 'alias ll' "$HOME/.bashrc" || fail "the captured dotfile was not restored"
ls "$HOME"/.bashrc.bak.* >/dev/null 2>&1 || fail "the existing file was not moved aside"
grep -q 'my old bashrc' "$HOME"/.bashrc.bak.* || fail "the moved-aside file lost its content"
[ -f "$HOME/.config/htop/htoprc" ] || fail "ordinary config was not restored"
[ ! -e "$HOME/.config/autostart" ] || fail "an autostart entry was restored without --allow-executable"
[ ! -e "$HOME/.config/systemd" ] || fail "a systemd user unit was restored without --allow-executable"
echo "$out" | grep -q "autostart" && echo "$out" | grep -q "systemd" || fail "held-back items are not listed for review"

# 3. nothing is overwritten when run again; unchanged files are left alone
bash "$b" "$T/repo" --apply --only dotfiles,config >/dev/null
[ "$(ls "$HOME"/.bashrc.bak.* | wc -l)" = 1 ] || fail "re-running made another backup of an identical file"

# 4. only with the explicit flag do executable items come back
bash "$b" "$T/repo" --apply --only config --allow-executable >/dev/null
[ -f "$HOME/.config/autostart/evil.desktop" ] || fail "--allow-executable did not restore the autostart entry"

# 5. third-party sources are never part of the default steps
bash "$b" "$T/repo" | grep -q "third-party apt sources" && fail "apt sources must only run when named"
echo "bootstrap ok"
