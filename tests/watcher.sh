#!/usr/bin/env bash
# The machine watcher on a brand-new home repository: first capture, then one commit per change, and no
# crash when nothing is set up. Runs in a scratch HOME; never touches the real one.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
w="$here/packaging/libexec/watch.sh"
T="$(mktemp -d)"; trap 'rm -rf "$T"' EXIT
mkdir -p "$T/home" "$T/repo"
export HOME="$T/home" XDG_RUNTIME_DIR="$T"
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

# the push target is the URL git will really use: a pushInsteadOf to somewhere else is refused
git remote add origin https://github.com/someone/private-home.git
git config "url.file://$T/elsewhere.pushInsteadOf" https://github.com/someone/private-home.git
echo 'export BAR=2' >> "$T/home/.bashrc"; run
grep -q 'not pushed' "$T/home/.local/share/hestia/watch.json" || fail "a rewritten push target was not refused"
echo "watcher ok"
