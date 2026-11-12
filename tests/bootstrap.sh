#!/usr/bin/env bash
# bootstrap.sh restores a machine safely: dry run by default, copies (never links), moves existing files aside,
# and holds back anything that runs by itself unless asked. Runs in a scratch HOME; never touches the real one.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
b="$here/packaging/libexec/bootstrap.sh"
T="$(mktemp -d)"; trap 'rm -rf "$T"' EXIT
export HOME="$T/home"; mkdir -p "$HOME" "$T/repo"
fail() { echo "FAIL: $*" >&2; exit 1; }
case "$HOME" in "$T"/*) ;; *) echo "FAIL: HOME is not the scratch dir" >&2; exit 1 ;; esac
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
echo 'my old gtk' > "$HOME/.keep"; mkdir -p "$HOME/.config/gtk-3.0"; echo 'mine' > "$HOME/.config/gtk-3.0/settings.ini"
mkdir -p "$cap/config/gtk-3.0"; echo 'new' > "$cap/config/gtk-3.0/settings.ini"; echo 'bm' > "$cap/config/gtk-3.0/extra.ini"
mkdir -p "$cap/config/private"; echo x > "$cap/config/private/f"

# 1. dry run changes nothing
out=$(bash "$b" "$T/repo")
[ "$(cat "$HOME/.bashrc")" = "my old bashrc" ] || fail "a dry run changed a file"
[ ! -e "$HOME/.config/htop" ] && [ ! -e "$HOME/.config/gtk-3.0/extra.ini" ] || fail "a dry run created files"
echo "$out" | grep -q "dry run" || fail "a dry run does not say so"
echo "$out" | grep -q "runs a command" || fail "the custom keyboard shortcut command was not flagged"

# 2. apply: copy not link, old file moved aside, executable config held back and reported
out=$(bash "$b" "$T/repo" --apply --only dotfiles,config)
[ "$(cat "$HOME/.bashrc")" = "my old bashrc" ] || fail "a shell startup file was restored without --allow-executable"
echo "$out" | grep -q "dotfiles/.bashrc" || fail "the held-back .bashrc is not listed"
[ "$(cat "$HOME/.config/gtk-3.0/settings.ini")" = "new" ] || fail "a different file in an existing folder was not restored"
grep -q mine "$HOME"/.config/gtk-3.0/settings.ini.bak.* || fail "the different file was not kept as .bak"
[ -f "$HOME/.config/gtk-3.0/extra.ini" ] || fail "a new file in an existing folder was skipped (whole folders must not be skipped)"
echo "$out" | grep -q "1 replaced" || fail "the summary does not count the replaced file"
echo "$out" | grep -q "private" && echo "$out" | grep -q "not on the restore list" || fail "an unlisted name was not reported as skipped"
[ ! -e "$HOME/.config/private" ] || fail "a name outside the allowlist was restored"
[ -f "$HOME/.config/htop/htoprc" ] || fail "ordinary config was not restored"
[ ! -e "$HOME/.config/autostart" ] || fail "an autostart entry was restored without --allow-executable"
[ ! -e "$HOME/.config/systemd" ] || fail "a systemd user unit was restored without --allow-executable"
echo "$out" | grep -q "autostart" && echo "$out" | grep -q "systemd" || fail "held-back items are not listed for review"

# 3. nothing is overwritten when run again; unchanged files are left alone
bash "$b" "$T/repo" --apply --only dotfiles,config >/dev/null
[ "$(ls "$HOME"/.config/gtk-3.0/settings.ini.bak.* | wc -l)" = 1 ] || fail "re-running made another backup of an identical file"

# 4. only with the explicit flag do executable items come back
bash "$b" "$T/repo" --apply --only config --allow-executable >/dev/null
[ -f "$HOME/.config/autostart/evil.desktop" ] || fail "--allow-executable did not restore the autostart entry"
bash "$b" "$T/repo" --apply --only dotfiles --allow-executable >/dev/null
grep -q 'alias ll' "$HOME/.bashrc" && ls "$HOME"/.bashrc.bak.* >/dev/null 2>&1 || fail "--allow-executable did not restore .bashrc with a backup"

# 5. third-party sources are never part of the default steps
bash "$b" "$T/repo" | grep -q "third-party apt sources" && fail "apt sources must only run when named"

# 6. the poisoned repository: hostile dotfiles and config, and a victim with a real ~/.ssh
P="$T/poison"; HOME="$T/victim"; export HOME; mkdir -p "$HOME/.ssh" "$P"
case "$HOME" in "$T"/*) ;; *) fail "HOME escaped the scratch dir" ;; esac
chmod 700 "$HOME/.ssh"; echo 'ssh-ed25519 VICTIM' > "$HOME/.ssh/authorized_keys"; echo 'victim key' > "$HOME/.ssh/id_ed25519"
pc="$P/applications/linux/${ID:-linux}/${VERSION_ID:-unknown}"
mkdir -p "$pc/dotfiles/.ssh" "$pc/dotfiles/.gnupg" "$pc/config/environment.d" "$pc/config/fish" "$pc/config/htop" "$pc/config/gtk-3.0" "$pc/config/keyrings"
touch "$P/tree.toml"; : > "$pc/apt-manual.txt"; : > "$pc/snap.txt"; : > "$pc/flatpak.txt"
echo 'ssh-ed25519 ATTACKER' > "$pc/dotfiles/.ssh/authorized_keys"
echo 'evil' > "$pc/dotfiles/.gnupg/gpg.conf"
printf '[core]\n\tsshCommand = /tmp/evil\n' > "$pc/dotfiles/.gitconfig"
echo 'LD_PRELOAD=/tmp/evil.so' > "$pc/config/environment.d/10.conf"
echo '/tmp/evil' > "$pc/config/fish/config.fish"
echo 'x' > "$pc/config/keyrings/login.keyring"
ln -s "$HOME/.ssh/id_ed25519" "$pc/config/htop/htoprc"
echo 'ok' > "$pc/config/htop/real"; chmod 600 "$pc/config/htop/real"
mkdir -p "$pc/config/gtk-3.0/sub"; chmod 700 "$pc/config/gtk-3.0/sub"; echo s > "$pc/config/gtk-3.0/sub/f"
out=$(bash "$b" "$P" --apply --only dotfiles,config)
[ "$(cat "$HOME/.ssh/authorized_keys")" = "ssh-ed25519 VICTIM" ] || fail "authorized_keys was overwritten by the repository"
[ "$(cat "$HOME/.ssh/id_ed25519")" = "victim key" ] || fail "the ssh folder was changed"
ls "$HOME"/.ssh.bak.* >/dev/null 2>&1 && fail "the ssh folder was moved aside"
[ -d "$HOME/.ssh" ] && [ "$(stat -c %a "$HOME/.ssh")" = 700 ] || fail "the ssh folder is gone or lost its mode"
[ ! -e "$HOME/.gnupg" ] || fail ".gnupg was restored"
[ ! -e "$HOME/.gitconfig" ] || fail ".gitconfig (core.sshCommand) was restored without --allow-executable"
[ ! -e "$HOME/.config/environment.d" ] || fail "environment.d was restored without --allow-executable"
[ ! -e "$HOME/.config/fish" ] || fail "fish config was restored without --allow-executable"
[ ! -e "$HOME/.config/keyrings" ] || fail "a keyring was restored"
[ ! -e "$HOME/.config/htop/htoprc" ] || fail "a symlink from the repository was restored"
echo "$out" | grep -q "environment.d" && echo "$out" | grep -q "gitconfig" || fail "held-back poisoned files are not listed"
[ "$(stat -c %a "$HOME/.config/htop/real")" = 600 ] || fail "the file mode was not restored"
[ "$(stat -c %a "$HOME/.config/gtk-3.0/sub")" = 700 ] || fail "the folder mode was not restored"
bash "$b" "$P" --apply --only dotfiles,config --allow-executable >/dev/null
[ "$(cat "$HOME/.ssh/authorized_keys")" = "ssh-ed25519 VICTIM" ] || fail "--allow-executable let the repository overwrite the ssh folder"
[ ! -e "$HOME/.gnupg" ] && [ ! -e "$HOME/.config/keyrings" ] || fail "--allow-executable restored keys or keyring"
[ -f "$HOME/.gitconfig" ] || fail "--allow-executable did not restore .gitconfig"
echo "bootstrap ok"
