#!/usr/bin/env bash
# The machine watcher when things go wrong (issues #166-#172): every failure is a visible result that starts with
# FAILED, a stuck backup unsticks itself, and nothing hangs. Hostile on purpose: a fake HOME, fake gh, a fake git remote
# (a bare repository in a temp folder) and a fake "GitHub URL" for it. Never touches the real HOME or any real repository.
# shellcheck disable=SC2088 # a literal "~/" is what capture-paths.txt holds
# shellcheck disable=SC2254 # want() takes a glob on purpose
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
w="$here/packaging/libexec/watch.sh"
T="$(mktemp -d)"; trap 'chmod -R u+rwX "$T" 2>/dev/null; rm -rf "$T"' EXIT
# KEEP_GOING=1 lists every failing check instead of stopping at the first (to see which checks an old build fails)
bad=0
fail() { echo "FAIL: $*" >&2; bad=$((bad+1)); [ -z "${KEEP_GOING:-}" ] && exit 1; return 0; }
# the sandbox guard: every write below happens under $T, and the home is never a real one
export HOME="$T/home"
guard() { case "$HOME" in "$T"/*) ;; *) fail "HOME is not in the sandbox: $HOME" ;; esac; case "$HOME" in /home/*|/root*) fail "HOME looks real: $HOME" ;; esac; }
mkdir -p "$HOME"; guard
export GIT_CONFIG_SYSTEM=/dev/null GIT_CONFIG_GLOBAL="$HOME/.gitconfig" XDG_RUNTIME_DIR="$T" HESTIA_NOW=1
ls_bin="$here/target/release/lifestream"; [ -x "$ls_bin" ] || ls_bin="$here/target/debug/lifestream"
[ -x "$ls_bin" ] || { echo "build lifestream first (cargo build)" >&2; exit 2; }
export LIFESTREAM="$ls_bin"
. /etc/os-release; rel="applications/linux/${ID:-linux}/${VERSION_ID:-unknown}"

# fake GitHub: git asks where "origin" pushes (answer: a GitHub URL) but really talks to the local bare repository;
# gh says private unless told otherwise; a push can be made to hang.
real_git="$(command -v git)"; mkdir -p "$T/bin"
cat > "$T/bin/git" <<EOF
#!/usr/bin/env bash
for a in "\$@"; do
  [ "\$a" = get-url ] && { echo https://github.com/someone/private-home.git; exit 0; }
  [ "\$a" = push ] && [ "\${FAKE_HANG:-}" = 1 ] && exec sleep 30
done
exec "$real_git" "\$@"
EOF
cat > "$T/bin/gh" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "${FAKE_GH:-true}"; exit "${FAKE_GH_RC:-0}"
EOF
# a Hestia that is signed in, whose check-private says whatever FAKE_CP says
cat > "$T/fake-ls" <<EOF
#!/usr/bin/env bash
case "\$1 \$2" in
  "auth status") echo '{"event":"status","signed_in":true,"user":"someone"}' ;;
  "home check-private") echo "\$FAKE_CP"; exit 1 ;;
  *) exec "$ls_bin" "\$@" ;;
esac
EOF
chmod +x "$T/bin/git" "$T/bin/gh" "$T/fake-ls"
export PATH="$T/bin:$PATH"

n=0
fresh() { # a new sandbox: home, a home repository that is already pushed once, and the bare "GitHub" side
  n=$((n+1)); S="$T/s$n"; mkdir -p "$S/home"; export HOME="$S/home" XDG_RUNTIME_DIR="$S" GIT_CONFIG_GLOBAL="$S/home/.gitconfig"; guard
  git config --global user.email t@t.t; git config --global user.name t
  git init -q --bare -b main "$S/remote.git"
  git init -q -b main "$S/repo"; touch "$S/repo/tree.toml"
  git -C "$S/repo" add -A; git -C "$S/repo" commit -qm init
  git -C "$S/repo" remote add origin "$S/remote.git"; git -C "$S/repo" push -q origin main
  r="$S/repo"; unset FAKE_HANG FAKE_GH FAKE_GH_RC FAKE_CP
}
paths() { mkdir -p "$r/$rel"; printf '%s\n' "$@" > "$r/$rel/capture-paths.txt"; }   # what to capture, instead of the default list
watch() { rc=0; bash "$w" "$r" || rc=$?; }
result() { sed -E 's/.*"result":"(.*)"\}$/\1/' "$HOME/.local/share/hestia/watch.json"; }
want() { case "$(result)" in $1) ;; *) fail "$2 (result: $(result))" ;; esac; }
tracked() { git -C "$r" ls-files --error-unmatch -- "$1" >/dev/null 2>&1; }
clean() { [ -z "$(git -C "$r" status --porcelain -uall -- "$rel")" ]; }
tok="ghp_$(printf 'a1B2c3D4%.0s' 1 2 3 4 5)"

# ---- #166: a commit that fails is a FAILED result with the reason, never "ok"
fresh; paths '~/.bashrc'; echo 'export A=1' > "$HOME/.bashrc"
git -C "$r" config commit.gpgsign true; git -C "$r" config gpg.program /bin/false
watch; [ "$rc" = 1 ] || fail "a failed commit must exit 1 (got $rc)"
want 'FAILED: git commit failed: *' "a commit that could not be signed was not reported as FAILED"
want '*gpg*' "the result must carry the first line of git's complaint"
fresh; paths '~/.bashrc'; echo 'export A=1' > "$HOME/.bashrc"
git config --global --unset user.email; git config --global --unset user.name; git config --global user.useConfigOnly true
watch; want 'FAILED: git commit failed: *' "a commit without a git identity was not reported as FAILED"
fresh; paths '~/.bashrc'; echo 'export A=1' > "$HOME/.bashrc"; watch; want 'ok:*' "a good run must be ok"
echo 'export B=2' >> "$HOME/.bashrc"; printf 'garbage' > "$r/.git/index"
watch; want 'FAILED: *' "a corrupt index was not reported as FAILED"
# one canonical path for the reason: the result is one valid JSON line with the quotes in it made safe
grep -q '^{"at":[0-9]*,"result":"FAILED: [^"]*"}$' "$HOME/.local/share/hestia/watch.json" || fail "watch.json is not one clean JSON line"

# ---- #167: another computer pushed first: fetch, rebase, push; a real conflict is FAILED and leaves no half rebase
fresh; paths '~/.bashrc'; echo 'export A=1' > "$HOME/.bashrc"; watch; want 'ok:*pushed*' "the first push failed"
git clone -q "$S/remote.git" "$S/other"; mkdir -p "$S/other/applications/linux/other/1"
echo hi > "$S/other/applications/linux/other/1/note.txt"; git -C "$S/other" add -A; git -C "$S/other" commit -qm "other computer"; git -C "$S/other" push -q origin main
echo 'export B=2' >> "$HOME/.bashrc"; watch
want 'ok:*pushed*' "a push rejected because another computer pushed first was not fixed by fetch and rebase"
[ -z "$(git -C "$r" log origin/main..HEAD --oneline)" ] || fail "commits are still unpushed"
git -C "$S/remote.git" show main:applications/linux/other/1/note.txt >/dev/null || fail "the other computer's file was lost"
echo 'export C=3' >> "$HOME/.bashrc"
mkdir -p "$S/other/$rel/dotfiles"; git -C "$S/other" pull -q --rebase origin main; echo "different" > "$S/other/$rel/dotfiles/.bashrc"
git -C "$S/other" add -A; git -C "$S/other" commit -qm "same file"; git -C "$S/other" push -q origin main
watch; want 'FAILED: could not combine*' "a rebase conflict was not reported as FAILED"
[ ! -d "$r/.git/rebase-merge" ] && [ ! -d "$r/.git/rebase-apply" ] || fail "a half-finished rebase was left behind"

# ---- #168: one huge file is skipped and listed, not pushed; the scanner tells "could not look" from "found one"
fresh; paths '~/.config/big' '~/.bashrc'; echo 'export A=1' > "$HOME/.bashrc"
mkdir -p "$HOME/.config/big"; head -c 5000 /dev/zero | tr '\0' 'x' > "$HOME/.config/big/huge.bin"; echo small > "$HOME/.config/big/small.txt"
HESTIA_MAX_FILE_BYTES=1000 watch
want 'ok:*' "a big file must not fail the whole capture"
[ ! -e "$r/$rel/config/big/huge.bin" ] || fail "a file over the size cap was captured"
tracked "$rel/config/big/small.txt" || fail "the small file next to the big one was lost"
grep -q 'huge.bin' "$r/$rel/SKIPPED.txt" || fail "the skipped big file is not listed in SKIPPED.txt"
want '*skipped*' "the result does not say a file was skipped"
clean || fail "tree not clean"
set +e; "$ls_bin" secret-scan "$T/does-not-exist" >/dev/null 2>&1; rc=$?; set -e
[ "$rc" = 2 ] || fail "a scanner that could not read a file must exit 2, not 1 (a finding) (got $rc)"
truncate -s 64M "$T/sparse"
set +e; ( ulimit -v 48000; "$ls_bin" secret-scan "$T/sparse" >/dev/null 2>&1 ); rc=$?; set -e
[ "$rc" = 0 ] || fail "scanning a 64 MB file must stream, not read it all into memory (exit $rc)"
rm -f "$T/sparse"

# ---- #169: a file name with a newline plus a token is withheld; an unsent commit that holds a secret is redone clean
fresh; paths '~/.config/odd'; mkdir -p "$HOME/.config/odd"
printf 'token=%s\n' "$tok" > "$HOME/.config/odd/$(printf 'x\ny.txt')"; echo ok > "$HOME/.config/odd/plain.txt"
watch; want 'ok:*pushed*' "a secret in a file with a newline in its name blocked the push or was committed (see the note)"
want '*withheld*' "the withheld file was not counted"
grep -q "$tok" <<<"$(git -C "$S/remote.git" log -p --all)" && fail "the secret reached the remote"
tracked "$rel/config/odd/plain.txt" || fail "the ordinary file next to it was lost"
out="$("$ls_bin" secret-scan -z "$HOME/.config/odd" 2>/dev/null | tr '\0' '|')" || true
case "$out" in "$HOME/.config/odd/x"$'\n'"y.txt|1|github token|"*) ;; *) false ;; esac || fail "secret-scan -z must print path, line and rule NUL-separated (got: $out)"
fresh; paths '~/.bashrc'; echo 'export A=1' > "$HOME/.bashrc"
printf 'token=%s\n' "$tok" > "$r/leak.txt"; git -C "$r" add -A; git -C "$r" commit -qm "bad"; git -C "$r" rm -q leak.txt; git -C "$r" commit -qm "delete it"
watch; want 'ok:*pushed*' "a secret-flagged unsent commit blocked the pushes for good"
grep -q "$tok" <<<"$(git -C "$r" log -p --all)" && fail "the secret is still in the history"
want '*started again*' "the result does not say what was redone"

# ---- #170: (a) a mass deletion needs one way to confirm, and a changed capture list resets the baseline
many() { mkdir -p "$HOME/.config/many"; for i in 1 2 3 4 5 6; do echo "f$i" > "$HOME/.config/many/f$i"; done; }
fresh; paths '~/.config/many' '~/.bashrc'; echo 'export A=1' > "$HOME/.bashrc"; many; watch; want 'ok:*' "setup"
rm -rf "$HOME/.config/many"; watch; want 'FAILED: capture failed: REFUSED*touch*' "the refusal does not say how to confirm"
tracked "$rel/config/many/f1" || fail "the refusal deleted the old copy"
touch "$r/$rel/.accept-smaller-capture"; watch; want 'ok:*' "after confirming, the capture is still refused"
[ ! -e "$r/$rel/config/many/f1" ] || fail "the confirmed smaller capture was not made"
[ ! -e "$r/$rel/.accept-smaller-capture" ] || fail "the confirmation is not used up"
fresh; paths '~/.config/many' '~/.bashrc'; echo 'export A=1' > "$HOME/.bashrc"; many; watch
paths '~/.bashrc'; rm -rf "$HOME/.config/many"; watch; want 'ok:*' "changing the capture list must reset the mass-deletion baseline"
# (b) one unreadable file is skipped and listed, the rest is captured
if [ "$(id -u)" != 0 ]; then
  fresh; paths '~/.config/many'; many; chmod 000 "$HOME/.config/many/f3"; watch
  chmod 600 "$HOME/.config/many/f3"
  want 'ok:*' "one unreadable file failed the whole capture"
  tracked "$rel/config/many/f1" || fail "the readable files were not captured ($(result); $(cat "$HOME/.local/share/hestia/watch.err"); $(git -C "$r" ls-files | tr "\n" " "))"
  grep -q 'f3' "$r/$rel/SKIPPED.txt" || fail "the unreadable file is not listed"
else echo "skipped: running as root, chmod 000 does not stop root"; fi
# (c) an apostrophe, a # or a space in a path, and a trailing comment
fresh; paths "~/it's.conf  # a note" '~/a#b.conf' '~/spaced name.conf' '# ~/commented'
echo 1 > "$HOME/it's.conf"; echo 2 > "$HOME/a#b.conf"; echo 3 > "$HOME/spaced name.conf"; echo 4 > "$HOME/commented"
watch; want 'ok:*' "a path with an apostrophe or # killed the capture"
for f in "it's.conf" "a#b.conf" "spaced name.conf"; do tracked "$rel/dotfiles/$f" || fail "$f was not captured"; done
[ ! -e "$r/$rel/dotfiles/commented" ] || fail "a commented-out path was captured"

# ---- #171: names with spaces or non-ASCII letters are committed at once
fresh; paths '~/.config/odd' '~/.bashrc'; echo 'export A=1' > "$HOME/.bashrc"; mkdir -p "$HOME/.config/odd"; echo a > "$HOME/.config/odd/first"; watch
echo b > "$HOME/.config/odd/häl lö.conf"; echo c > "$HOME/.config/odd/with space"; watch
tracked "$rel/config/odd/häl lö.conf" && tracked "$rel/config/odd/with space" || fail "a file with a space or an accent stayed untracked"
clean || fail "tree not clean after files with unusual names"

# ---- #172: a stale index.lock, a hung push, a held lock, endless waiting, and the reason a repository is refused
fresh; paths '~/.bashrc'; echo 'export A=1' > "$HOME/.bashrc"; watch
touch "$r/.git/index.lock"; echo 'export B=2' >> "$HOME/.bashrc"; watch; want 'git is busy*' "a fresh index.lock must be respected"
touch -d '1 hour ago' "$r/.git/index.lock"; watch; want 'ok:*' "a stale index.lock wedged the watcher"
[ ! -e "$r/.git/index.lock" ] || fail "the stale lock was not removed"
echo 'export C=3' >> "$HOME/.bashrc"; start=$SECONDS; FAKE_HANG=1 HESTIA_NET_TIMEOUT=2 watch
[ $((SECONDS - start)) -lt 20 ] || fail "a hung push held the watcher for $((SECONDS - start)) s"
want 'FAILED: push timed out*' "a hung push was not reported as timed out"
grep -q '^TimeoutStartSec=' "$here/packaging/systemd/hestia-watch.service" || fail "the watch service has no TimeoutStartSec"
grep -q '^SuccessExitStatus=75' "$here/packaging/systemd/hestia-watch.service" || fail "a second run that finds the lock held must not make the unit fail"
# a second run while one is running says so instead of letting "Capture now" claim it captured
( exec 8>"$S/hestia-watch.lock"; flock 8; sleep 6 ) & sleep 1
watch; [ "$rc" = 75 ] || fail "a run that finds the lock held must exit 75 (got $rc)"
want 'busy: another capture*' "the result does not say another capture is running"
wait
# quiet-gating has an upper bound
mkdir -p "$S/psi"; echo 'some avg10=90.00 avg60=90.00 avg300=90.00 total=1' > "$S/psi/cpu"
unset HESTIA_NOW; export HESTIA_PSI_DIR="$S/psi" HESTIA_MAX_WAIT=3600
date +%s > "$HOME/.local/share/hestia/last-capture"; watch; want 'busy: waiting*' "a busy computer must wait"
echo $(( $(date +%s) - 7200 )) > "$HOME/.local/share/hestia/last-capture"; watch; want 'ok:*' "waiting for a quiet moment must have an upper bound"
unset HESTIA_PSI_DIR HESTIA_MAX_WAIT; export HESTIA_NOW=1
# why a repository is refused: four different things to do about it
cp_state() { # what the engine says -> what the person is told
  fresh; paths '~/.bashrc'; echo 'export A=1' > "$HOME/.bashrc"
  FAKE_CP="$1" LIFESTREAM="$T/fake-ls" watch; want "FAILED: not pushed: $2*" "check-private said $1"
}
cp_state '{"event":"failed","kind":"offline"}' 'offline'
cp_state '{"event":"failed","kind":"other","message":"you are not signed in to GitHub"}' 'signed out'
cp_state '{"event":"failed","kind":"other","message":"that repository was not found"}' 'the repository is gone'
cp_state '{"event":"failed","kind":"other","message":"that repository is public"}' 'the repository is public'
fresh; paths '~/.bashrc'; echo 'export A=1' > "$HOME/.bashrc"; FAKE_GH=false watch
want 'FAILED: not pushed: the repository is public*' "the GitHub CLI path does not say the repository is public"
# ---- #173: the capture list survives an OS upgrade; symlinked dotfiles are captured by content; a nested git repository is
#      stored as files; deb822 apt sources are captured
osd="applications/linux/${ID:-linux}"
fresh; mkdir -p "$r/$osd/0.1" "$r/$osd/0.2"
printf '%s\n' '~/.bashrc' > "$r/$osd/0.1/capture-paths.txt"; printf '%s\n' '~/.bashrc' '~/.myrc' > "$r/$osd/0.2/capture-paths.txt"
echo a > "$HOME/.bashrc"; echo b > "$HOME/.myrc"; watch; want 'ok:*' "setup for the carried-forward list"
grep -qx '~/.myrc' "$r/$rel/capture-paths.txt" || fail "an OS upgrade lost the person's added path (the list was re-seeded from the default)"
tracked "$rel/dotfiles/.myrc" || fail "the carried-forward path was not captured"
echo c > "$HOME/.third"; echo '~/.third' >> "$r/$rel/capture-paths.txt"; watch
grep -qx '~/.third' "$r/$rel/capture-paths.txt" || fail "an existing list was overwritten"

fresh; paths '~/.bashrc' '~/.outside' '~/.mylink' '~/.tokrc' '~/.config/stowed'
mkdir -p "$HOME/dots" "$HOME/.config/stowed"
echo 'export A=1' > "$HOME/dots/bashrc"; ln -s "$HOME/dots/bashrc" "$HOME/.bashrc"
ln -s /etc/hostname "$HOME/.outside"
printf 'machine x login y\n' > "$HOME/.netrc"; ln -s "$HOME/.netrc" "$HOME/.mylink"
printf 'token=%s\n' "$tok" > "$HOME/dots/tokrc"; ln -s "$HOME/dots/tokrc" "$HOME/.tokrc"
echo 'inner' > "$HOME/dots/inner.conf"; ln -s "$HOME/dots/inner.conf" "$HOME/.config/stowed/inner.conf"; ln -s /etc/hostname "$HOME/.config/stowed/out.conf"
watch; want 'ok:*' "symlinked dotfiles broke the capture"
[ -f "$r/$rel/dotfiles/.bashrc" ] && [ ! -L "$r/$rel/dotfiles/.bashrc" ] && grep -q 'A=1' "$r/$rel/dotfiles/.bashrc" || fail "a symlinked dotfile was not captured by content"
git -C "$r" ls-files -s -- "$rel/dotfiles/.bashrc" | grep -q '^120000' && fail "a symlink was committed as a symlink blob"
[ ! -e "$r/$rel/dotfiles/.outside" ] || fail "a symlink to a file outside the home folder was captured"
[ ! -e "$r/$rel/dotfiles/.mylink" ] || fail "a symlink to a never-captured file was captured"
grep -q '.outside' "$r/$rel/SKIPPED.txt" && grep -q '.mylink' "$r/$rel/SKIPPED.txt" || fail "the refused symlinks are not listed in SKIPPED.txt"
grep -q "$tok" <<<"$(git -C "$S/remote.git" log -p --all)" && fail "a secret behind a symlink reached the remote"
grep -q inner "$r/$rel/config/stowed/inner.conf" && [ ! -L "$r/$rel/config/stowed/inner.conf" ] || fail "a symlink inside a captured folder was not captured by content"
[ ! -e "$r/$rel/config/stowed/out.conf" ] || fail "a symlink inside a folder that leaves the home folder was captured"
if git -C "$r" ls-files -s | grep -q '^120000'; then fail "a symlink blob is in the repository"; fi

fresh; paths '~/.config/nvim'; mkdir -p "$HOME/.config/nvim/lua"; echo 'vim.o.number=true' > "$HOME/.config/nvim/init.lua"
git init -q "$HOME/.config/nvim"; echo x > "$HOME/.config/nvim/lua/a.lua"
watch; want 'ok:*' "a nested git repository broke the capture"
tracked "$rel/config/nvim/init.lua" && tracked "$rel/config/nvim/lua/a.lua" || fail "the files of a captured git clone were not stored"
[ ! -e "$r/$rel/config/nvim/.git" ] || fail "the clone's .git was copied"
git -C "$r" ls-files -s | grep -q '^160000' && fail "a nested repository became a gitlink with no content"

fresh; paths '~/.bashrc'; echo a > "$HOME/.bashrc"; mkdir -p "$T/apt$n"
printf 'Types: deb\nURIs: https://example.com/repo\nSuites: stable\nComponents: main\n' > "$T/apt$n/ext.sources"
echo 'deb https://example.com/old stable main' > "$T/apt$n/ext.list"; echo 'Types: deb' > "$T/apt$n/ubuntu.sources"
HESTIA_APT_SOURCES_DIR="$T/apt$n" watch; want 'ok:*' "setup for apt sources"
tracked "$rel/apt-sources/ext.sources" || fail "a deb822 .sources file was not captured"
tracked "$rel/apt-sources/ext.list" || fail "a .list file was not captured"
tracked "$rel/apt-sources/ubuntu.sources" && fail "the distribution's own sources file was captured"
[ "$bad" = 0 ] || exit 1
echo "watcher failures ok"
