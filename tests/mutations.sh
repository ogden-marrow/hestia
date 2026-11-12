#!/usr/bin/env bash
# Do the tests for Hestia's most important protections really fail when the protection is broken?
# Each mutation breaks one line in a scratch COPY of the repository (a git worktree; your checkout is never touched), runs the
# test that should notice, and expects it to FAIL. A mutation that survives means a protection is not really tested.
# Not part of CI (it rebuilds a few times). Run it before a release or after touching a protection:  bash tests/mutations.sh
set -uo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
W="$(mktemp -d)"; trap 'git -C "$here" worktree remove --force "$W" >/dev/null 2>&1; rm -rf "$W"' EXIT
git -C "$here" worktree add --detach -q "$W" HEAD || exit 2   # the committed state: commit before you run this
cd "$W" || exit 2
export CARGO_TARGET_DIR="${MUTATION_TARGET:-$HOME/.cache/hestia-mutations-target}"
echo "building a baseline (the first run compiles everything)..." >&2
cargo build --release -p lifestream >/dev/null 2>&1 || { echo "baseline does not build" >&2; exit 2; }
mkdir -p "$W/target/release"; ln -sf "$CARGO_TARGET_DIR/release/lifestream" "$W/target/release/lifestream"
survived=0; killed=0
mutate() { # description  file  old  new  test-command...
  local desc="$1" file="$2" old="$3" new="$4"; shift 4
  git -C "$W" checkout -q -- . 2>/dev/null
  python3 - "$W/$file" "$old" "$new" <<'PY' || { echo "SKIP  $desc (the line it breaks is gone: update tests/mutations.sh)"; return; }
import sys
p, old, new = sys.argv[1:4]
s = open(p).read()
if old not in s: sys.exit(1)
open(p, "w").write(s.replace(old, new, 1))
PY
  case "$file" in *.rs) cargo build --release -p lifestream >/dev/null 2>&1 || { echo "SKIP  $desc (the mutant does not compile)"; return; } ;; esac
  if ( cd "$W" && "$@" ) >/dev/null 2>&1; then echo "SURVIVED  $desc"; survived=$((survived+1)); else echo "killed    $desc"; killed=$((killed+1)); fi
}
rs() { cargo test -p lifestream "$1"; }

mutate "secret scanner stops seeing GitHub tokens"        lifestream/src/secrets.rs 'gh[pousr]_[A-Za-z0-9]{30,}' 'gh[pousr]_[A-Za-z0-9]{3000,}' rs modern_token_formats
mutate "deny-list forgets ~/.ssh"                          lifestream/src/deny.rs '(".ssh", "SSH keys and known hosts"),' '' rs deny
mutate "push gate accepts a repo that is not private"      lifestream/src/ghrepo.rs 'Ok(v) if v["private"].as_bool() == Some(true) => {
            emit(out, json!({"event": "private"' 'Ok(v) if v["private"].as_bool() != Some(false) => {
            emit(out, json!({"event": "private"' rs check_private
mutate "mover follows symlinks"                            lifestream/src/tree.rs 'if !m.file_type().is_file() {
        bail!("{} is not a plain file' 'if false {
        bail!("{} is not a plain file' rs move_refuses_symlinks
mutate "the sign-in prints the token"                      lifestream/src/auth.rs 'emit(out, json!({"event": "approved", "user": user}));' 'emit(out, json!({"event": "approved", "user": user, "t": token}));' rs signs_in_stores
mutate "hostile file names are not cleaned"                lifestream/src/bundle.rs 'c.is_control()
            ||' 'false
            ||' rs hostile_names
mutate "direction-changing characters are not removed from names" lifestream/src/bundle.rs "'\\u{202A}'..='\\u{202E}' | " '' rs hostile_names
mutate "undo accepts a journal that leaves the home folder" lifestream/src/tree.rs 'rel_ok(rel).then(|| home.join(rel))' 'Some(home.join(rel))' rs undo_treats_the_journal
mutate "watcher pushes to a repo Hestia cannot vouch for"  packaging/libexec/watch.sh 'out=$("$LS" home check-private "$slug" 2>&1) ||' 'out=$(true) ||' bash tests/watcher.sh
mutate "capture no longer refuses a mass deletion"         packaging/libexec/capture.sh '[ "$before" -ge 4 ]' '[ "$before" -ge 4000000 ]' bash tests/watcher.sh
mutate "deny-list is not applied inside captured folders"  packaging/libexec/capture.sh 'if [ "${#inside[@]}" -gt 0 ]; then' 'if [ 1 -eq 0 ]; then' bash tests/watcher.sh
mutate "a secret line is left in a shell file"             packaging/libexec/watch.sh 'sed -i "${n}s/.*/# [Hestia left this line out: it looked like a secret]/" "$f"' 'true' bash tests/watcher.sh
mutate "capture keeps secret-named files whole"            packaging/libexec/watch.sh 'for f in "${!drop[@]}"; do [ -f "$f" ] && rm -f -- "$f" && withheld=$((withheld+1)); done' 'true' bash tests/watcher.sh
mutate "uninstall keeps the clipboard history"             uninstall.sh 'rm -f "$share/hestia/clipboard.jsonl"' 'true' bash tests/uninstall.sh
mutate "bootstrap restores code that runs by itself"       packaging/libexec/bootstrap.sh 'if is_executable_config "$name/" && [ "$EXEC" != 1 ]; then review+=("config/$name"); continue; fi' 'true' bash tests/bootstrap.sh

echo; echo "killed $killed, survived $survived"
[ "$survived" = 0 ]
