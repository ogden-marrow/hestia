#!/usr/bin/env bash
# The machine watcher on a brand-new home repository: first capture, then one commit per change, and no
# crash when nothing is set up. Runs in a scratch HOME; never touches the real one.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
w="$here/packaging/libexec/watch.sh"
T="$(mktemp -d)"; trap 'rm -rf "$T"' EXIT
mkdir -p "$T/home" "$T/repo"
# the watcher politely skips a busy computer; a CI runner that has just finished a build can look busy, so the test says "now"
export HOME="$T/home" XDG_RUNTIME_DIR="$T" HESTIA_NOW=1
git config --global user.email t@t.t; git config --global user.name t
ls_bin="$here/target/release/lifestream"; [ -x "$ls_bin" ] || ls_bin="$here/target/debug/lifestream"
[ -x "$ls_bin" ] || { echo "build lifestream first (cargo build)" >&2; exit 2; }
export LIFESTREAM="$ls_bin"
run() { bash "$w" "$T/repo"; }
fail() { echo "FAIL: $*" >&2; exit 1; }

# no home repository yet: do nothing, quietly
run || fail "should exit quietly without a home repository"

cd "$T/repo"; git init -q -b main .; touch tree.toml; git add -A; git commit -qm init
echo 'export PATH=$PATH' > "$T/home/.bashrc"

run
git log --format=%s | grep -q "first capture of this computer" || fail "no first capture commit"
[ -z "$(git status --short)" ] || fail "tree not clean after the first capture"
grep -q '"result":"ok' "$T/home/.local/share/hestia/watch.json" || fail "no status written"

echo 'export FOO=1' >> "$T/home/.bashrc"; run
git log -1 --format=%s | grep -q "config, 1 files changed" || fail "a changed dotfile is not its own commit"

before=$(git rev-list --count HEAD); run
[ "$(git rev-list --count HEAD)" = "$before" ] || fail "an unchanged machine must make no commit"
# a secret in a captured file is withheld: never committed, and the status says so
tok="ghp_$(printf 'a1B2c3D4%.0s' 1 2 3 4 5)"
echo "export GITHUB_TOKEN=$tok" >> "$T/home/.bashrc"; run
git log -p --all | grep -q "$tok" && fail "a secret was committed"
grep -q 'withheld' "$T/home/.local/share/hestia/watch.json" || fail "the status does not say a file was withheld"
grep -q "$tok" "$T/home/.local/share/hestia/watch.json" && fail "the status leaked the secret"
grep -q "$tok" "$T/home/.bashrc" || fail "the original file must be left alone"
# the rest of the shell file is still backed up; only the secret line is left out
cap=$(ls -d applications/linux/*/*)/dotfiles/.bashrc
[ -s "$cap" ] || fail "the shell file should be captured with the secret line left out, not dropped"
grep -q 'export PATH=' "$cap" || fail "ordinary lines of the shell file were lost"
grep -q 'looked like a secret' "$cap" || fail "the secret line was not replaced by a placeholder"
grep -q "$tok" "$cap" && fail "the captured copy still holds the secret"
# a private key anywhere in a shell file drops the whole captured copy
printf -- '-----BEGIN OPENSSH PRIVATE KEY-----\nabc\n' >> "$T/home/.bashrc"; run
[ ! -e "$cap" ] || fail "a shell file holding a private key must not be captured at all"

# the push target is the URL git will really use: a pushInsteadOf to somewhere else is refused
git remote add origin https://github.com/someone/private-home.git
git config "url.file://$T/elsewhere.pushInsteadOf" https://github.com/someone/private-home.git
echo 'export BAR=2' >> "$T/home/.bashrc"; run
grep -q 'not pushed' "$T/home/.local/share/hestia/watch.json" || fail "a rewritten push target was not refused"
# a live file that is a symlink into the repository is never deleted by capture (bootstrap layout)
git remote remove origin 2>/dev/null || true
echo 'export PATH=$PATH' > "$T/home/.bashrc"; run
rel=$(ls -d applications/linux/*/*)
cp "$rel/dotfiles/.bashrc" "$T/kept.bashrc"; rm -f "$T/home/.bashrc"; ln -s "$T/repo/$rel/dotfiles/.bashrc" "$T/home/.bashrc"
mark=$(git rev-parse HEAD)
run
[ -s "$rel/dotfiles/.bashrc" ] || fail "capture deleted a file that a live symlink points at"
git log --diff-filter=D --name-only --format= "$mark..HEAD" | grep -q 'dotfiles/.bashrc' && fail "a deletion of the live dotfile was committed"

# a capture that would drop most of what was captured before is refused, and the old copy stays
rm -f "$T/home/.bashrc"; cp "$T/kept.bashrc" "$T/home/.bashrc"
mkdir -p "$T/home/.config"
for f in .profile .bash_logout .nvidia-settings-rc .config/mimeapps.list .config/user-dirs.dirs; do echo "# $f" > "$T/home/$f"; done
run
rm -f "$T/home/.bashrc" "$T/home/.profile" "$T/home/.bash_logout" "$T/home/.nvidia-settings-rc" "$T/home/.config/mimeapps.list" "$T/home/.config/user-dirs.dirs"
run || true
[ -e "$rel/dotfiles/.profile" ] || fail "a mass deletion was not refused"
grep -q 'capture failed' "$T/home/.local/share/hestia/watch.json" || fail "the refusal was not reported"
# a file that appears in the capture folder after the first capture is committed, not left untracked
echo 'export PATH=$PATH' > "$T/home/.bashrc"
for f in .profile .bash_logout .nvidia-settings-rc .config/mimeapps.list .config/user-dirs.dirs; do echo "# $f" > "$T/home/$f"; done
rel=$(ls -d applications/linux/*/*)
echo "a new kind of capture" > "$rel/new-capture.txt"; run
git ls-files --error-unmatch "$rel/new-capture.txt" >/dev/null 2>&1 || fail "a new capture file stayed untracked"
git log -1 --format=%s | grep -q "new files captured (new-capture.txt)" || fail "the new file did not get its own plain commit"
[ -z "$(git status --short)" ] || fail "tree not clean after committing new capture files"
# signed in to Hestia: the repository must be vouched for by Hestia (a private repo of yours) before anything is sent
git config --unset-all "url.file://$T/elsewhere.pushInsteadOf" 2>/dev/null || true
git remote remove origin 2>/dev/null || true
git remote add origin https://github.com/someone/private-home.git
cat > "$T/fake-ls" <<EOF
#!/usr/bin/env bash
case "\$1 \$2" in
  "auth status") echo '{"event":"status","signed_in":true,"user":"someone"}' ;;
  "home check-private") exit 1 ;;
  *) exec "$ls_bin" "\$@" ;;
esac
EOF
chmod +x "$T/fake-ls"
echo 'export BAZ=3' >> "$T/home/.bashrc"
LIFESTREAM="$T/fake-ls" run
grep -q "confirmed private repo of yours" "$T/home/.local/share/hestia/watch.json" || fail "a repository Hestia could not vouch for was pushed to"
echo "watcher ok"
