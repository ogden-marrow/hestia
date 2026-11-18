#!/usr/bin/env bash
# The weekly restore rehearsal (issue #226): it must PROVE a backup restores, and fail visibly when it does not. A good repository
# passes; a missing or corrupted manifest, a secret-looking line, and a restore that is broken on purpose (runs held-back code,
# writes the wrong content, follows a link, sets setuid) each fail with a plain sentence; a poisoned repository passes only because
# the protections hold. After every run the scratch folder is gone and nothing outside it changed. Temp HOME only: never the real one.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
lib="$here/packaging/libexec"
T="$(mktemp -d)"; trap 'rm -rf "$T"' EXIT
export HOME="$T/home" XDG_RUNTIME_DIR="$T/run" GIT_CONFIG_SYSTEM=/dev/null GIT_CONFIG_GLOBAL="$T/gitconfig"
unset XDG_DATA_HOME
mkdir -p "$HOME" "$XDG_RUNTIME_DIR"
case "$HOME" in "$T"/*) ;; *) echo "FAIL: HOME is not in the sandbox: $HOME" >&2; exit 1 ;; esac
ls_bin="$here/target/release/lifestream"; [ -x "$ls_bin" ] || ls_bin="$here/target/debug/lifestream"
[ -x "$ls_bin" ] || { echo "build lifestream first (cargo build)" >&2; exit 2; }
export LIFESTREAM="$ls_bin"
fail() { echo "FAIL: $*" >&2; exit 1; }
git config --global user.email t@t.t; git config --global user.name t; git config --global init.defaultBranch main
. /etc/os-release; rel="applications/linux/${ID:-linux}/${VERSION_ID:-unknown}"
share="$T/share"; json="$share/hestia/rehearsal.json"

# a victim: the stand-in "real" home keys, and a folder a hostile link points at. Listed before and after everything.
mkdir -p "$HOME/.ssh" "$HOME/.config/environment.d" "$T/victim"
chmod 700 "$HOME/.ssh"; echo 'ssh-ed25519 VICTIM' > "$HOME/.ssh/authorized_keys"; echo 'victim key' > "$T/victim/id_ed25519"
echo 'my own bashrc' > "$HOME/.bashrc"
snap() { ( cd "$T" && { find home victim -printf '%p %y %m %s %T@ %l\n'; find home victim -type f -exec sha256sum {} +; } | LC_ALL=C sort | sha256sum ); }
world0="$(snap)"

mkrepo() { # a home repository (not yet committed): tree.toml, a capture list and a few captured files; prints its path
  local r c; r="$(mktemp -d "$T/repo.XXXXXX")"; c="$r/$rel"
  mkdir -p "$c/dotfiles" "$c/config/htop" "$c/config/gtk-3.0/sub" "$c/config/autostart"
  git init -q "$r"; : > "$r/tree.toml"
  printf '# capture list\n~/.bashrc\n~/.config/htop\n~/.config/gtk-3.0\n' > "$c/capture-paths.txt"
  echo 'alias ll="ls -l"' > "$c/dotfiles/.bashrc"; echo 'x=1' > "$c/dotfiles/.nvidia-settings-rc"
  echo 'hide_kernel_threads=1' > "$c/config/htop/htoprc"; echo run > "$c/config/htop/tool.sh"; chmod 755 "$c/config/htop/tool.sh"
  echo '[Settings]' > "$c/config/gtk-3.0/settings.ini"; echo s > "$c/config/gtk-3.0/sub/f"
  printf '[Desktop Entry]\nExec=/tmp/evil\n' > "$c/config/autostart/evil.desktop"
  echo "$r"
}
commit() { git -C "$1" add -A && git -C "$1" commit -qm "$2"; }
# run the rehearsal; sets rc and out. $1 repo, optional $2 libexec dir
go() { rm -rf "$share"; rc=0; out="$(bash "${2:-$lib}/rehearse.sh" --repo "$1" --share "$share" 2>&1)" || rc=$?; }
clean() { # the scratch folder is gone and nothing else moved
  [ -z "$(find "$XDG_RUNTIME_DIR" -mindepth 1 -not -name '*.lock')" ] || fail "$1: the scratch folder was left behind: $(ls -A "$XDG_RUNTIME_DIR")"
  [ "$(snap)" = "$world0" ] || fail "$1: something outside the scratch folder changed"
}
has() { grep -q "$1" "$json" || fail "$2 (rehearsal.json: $(cat "$json" 2>&1))"; }
hasnt() { if grep -q "$1" "$json"; then fail "$2 (rehearsal.json: $(cat "$json"))"; fi; }

# 1. a good repository passes, and the result is recorded
r="$(mkrepo)"; commit "$r" "capture"
go "$r"
[ "$rc" = 0 ] || fail "a good repository did not pass: $out $(cat "$json" 2>&1)"
has '"result":"passed"' "no passed result"; has '"problems":\[\]' "a passed result lists problems"
has "\"repo_commit\":\"$(git -C "$r" rev-parse --short HEAD)\"" "the result does not say which commit was rehearsed"
has '"at":[0-9][0-9]*,' "no time"
checked="$(sed -n 's/.*"checked":\([0-9]*\).*/\1/p' "$json")"; [ "${checked:-0}" -ge 7 ] || fail "too little was checked: $checked"
clean good
[ "$(cat "$HOME/.bashrc")" = "my own bashrc" ] || fail "the real bashrc changed"
# nothing to rehearse yet: no repository, no result, no failure
rm -rf "$share"; bash "$lib/rehearse.sh" --repo "$T/none" --share "$share" || fail "a missing repository must not be a failure"
[ ! -e "$json" ] || fail "a result was written with no repository"

# 2. a missing manifest, a corrupted manifest, a missing capture list, nothing committed: each fails with a plain sentence
r="$(mkrepo)"; commit "$r" c; git -C "$r" rm -q tree.toml; git -C "$r" commit -qm "lose the manifest"
go "$r"; [ "$rc" = 1 ] || fail "a missing manifest did not fail"; has '"result":"failed"' "not recorded as failed"; has "tree.toml" "the problem does not name the manifest"; clean missing-manifest
r="$(mkrepo)"; echo 'x=[' > "$r/tree.toml"; commit "$r" c
go "$r"; [ "$rc" = 1 ] || fail "a corrupted manifest did not fail"; has "tree.toml" "a corrupted manifest is not reported"; clean corrupt-manifest
r="$(mkrepo)"; rm "$r/$rel/capture-paths.txt"; commit "$r" c
go "$r"; [ "$rc" = 1 ] || fail "a missing capture list did not fail"; has "capture list" "no sentence about the capture list"; clean no-list
r="$(mkrepo)"
# shellcheck disable=SC2088 # a literal "~/" is what capture-paths.txt holds
printf '~/../etc/passwd\n' > "$r/$rel/capture-paths.txt"; commit "$r" c
go "$r"; [ "$rc" = 1 ] || fail "a hostile capture list did not fail"; clean bad-list
r="$T/empty"; git init -q "$r"
go "$r"; [ "$rc" = 1 ] || fail "a repository with nothing committed did not fail"; has "nothing committed" "no plain sentence"; clean empty
# only what is committed counts: an uncommitted fix does not rescue a broken HEAD
r="$(mkrepo)"; echo 'x=[' > "$r/tree.toml"; commit "$r" c; : > "$r/tree.toml"
go "$r"; [ "$rc" = 1 ] || fail "the working files were rehearsed instead of the pushed commit"; clean head-only

# 3. a secret-looking line fails, and the result never repeats it
r="$(mkrepo)"; echo "token = ghp_abcdefghijklmnopqrstuvwxyz0123456789" >> "$r/$rel/config/htop/htoprc"; commit "$r" c
go "$r"; [ "$rc" = 1 ] || fail "a secret in the repository did not fail the rehearsal"
has "secret" "no sentence about the secret"; hasnt "ghp_" "the result repeats the secret"
case "$out" in *ghp_*) fail "the output repeats the secret" ;; esac
clean secret

# 4. a poisoned repository passes only because every protection holds, and nothing outside the scratch folder moves
r="$(mkrepo)"; c="$r/$rel"
mkdir -p "$c/dotfiles/.ssh" "$c/dotfiles/.gnupg" "$c/config/environment.d" "$c/config/keyrings" "$c/config/fish"
echo 'ssh-ed25519 ATTACKER' > "$c/dotfiles/.ssh/authorized_keys"; echo evil > "$c/dotfiles/.gnupg/gpg.conf"
printf '[core]\n\tsshCommand = /tmp/evil\n' > "$c/dotfiles/.gitconfig"
echo 'LD_PRELOAD=/tmp/evil.so' > "$c/config/environment.d/10.conf"; echo x > "$c/config/keyrings/login.keyring"; echo /tmp/evil > "$c/config/fish/config.fish"
ln -s "$T/victim/id_ed25519" "$c/config/htop/htoprc2"; ln -s "$T/victim" "$c/config/gtk-3.0/outside"; ln -s "$HOME/.ssh" "$c/dotfiles/.cache-link"
echo 'setuid' > "$c/config/htop/setuid.sh"; chmod 4755 "$c/config/htop/setuid.sh"
commit "$r" "poisoned"
go "$r"; [ "$rc" = 0 ] || fail "the poisoned repository failed although every protection held: $out $(cat "$json" 2>&1)"
clean poisoned

# 5. the same checks catch a restore that is broken on purpose (the rehearsal is what notices a restore that is no longer safe)
broken() { # NAME sed-expression: a copy of the shipped scripts with one protection removed
  local d="$T/lib-$1"; rm -rf "$d"; cp -r "$lib" "$d"; sed -i "$2" "$d/bootstrap.sh"
  if cmp -s "$lib/bootstrap.sh" "$d/bootstrap.sh"; then fail "the break for '$1' did not apply (update the test)"; fi
  go "$r" "$d"; [ "$rc" = 1 ] || fail "a restore broken on purpose ($1) still passed the rehearsal"; has '"result":"failed"' "$1 not recorded"; clean "$1"
}
broken runs-held-back-code 's|^    if is_executable_config "\$rel" \&\& \[ "\$EXEC" != 1 \]; then review+=.*$|    true|'
broken wrong-content 's|run cp -- "\$src" "\$dest"; run chmod|run cp -- /etc/hostname "$dest"; run chmod|'
broken follows-links 's|if \[ -L "\$src" \] \|\| \[ ! -f "\$src" \]; then|if [ ! -f "$src" ]; then|'
broken setuid-mode 's|run chmod "\$(mode_of "\$src")" -- "\$dest"|run chmod 4755 -- "$dest"|'

# 6. the scratch folder is made private
grep -q 'chmod 700 "\$T"' "$lib/rehearse.sh" || fail "the scratch folder is not made private"
echo "rehearsal ok"
