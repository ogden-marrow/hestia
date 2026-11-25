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
mutate "the folder mover replaces an existing destination" lifestream/src/tree.rs 'if fs::symlink_metadata(dst).is_ok() {' 'if false {' rs move_dir_refusals
mutate "a group move journals each item as its own batch" lifestream/src/tree.rs 'serde_json::json!({"batch": batch, "kind": kind,' 'serde_json::json!({"batch": new_batch(), "kind": kind,' rs a_group_of_files_and_folders
mutate "a group move goes ahead past a refused item"      lifestream/src/tree.rs 'if !refused.is_empty() {
        let more' 'if refused.len() > 1000 {
        let more' rs one_refused_item_refuses_the_whole_group
mutate "a group move that fails midway leaves the rest moved" lifestream/src/tree.rs '.filter(|&i| move_item(home, &home.join(&items[i].1)' '.filter(|&i| true || move_item(home, &home.join(&items[i].1)' rs a_failure_midway
mutate "undoing a new folder deletes what was put in it"  lifestream/src/tree.rs 'fs::remove_dir(&to) {' 'fs::remove_dir_all(&to) {' rs undo_of_mkdir
mutate "the feed lists a path outside the home folder"      lifestream/src/feed.rs 'if tree::under_home(home, to_s).is_none() {' 'if false {' rs a_line_pointing_outside_home
mutate "the feed has no cap on its output"                 lifestream/src/feed.rs 'if out.len() + line.len() + 1 > OUTPUT_CAP {' 'if false {' rs output_is_capped
mutate "watcher pushes to a repo Hestia cannot vouch for" packaging/libexec/watch.sh 'out=$("$LS" home check-private "$slug" 2>&1) ||' 'out=$(true) ||' bash tests/watcher.sh
mutate "capture no longer refuses a mass deletion"         packaging/libexec/capture.sh '[ "$before" -ge 4 ]' '[ "$before" -ge 4000000 ]' bash tests/watcher.sh
mutate "deny-list is not applied inside captured folders"  packaging/libexec/capture.sh 'if [ "${#inside[@]}" -gt 0 ]; then' 'if [ 1 -eq 0 ]; then' bash tests/watcher.sh
mutate "a secret line is left in a shell file"             packaging/libexec/watch.sh 'sed -i "${n}s/.*/# [Hestia left this line out: it looked like a secret]/" "$f"' 'true' bash tests/watcher.sh
mutate "capture keeps secret-named files whole"            packaging/libexec/watch.sh 'for f in "${!drop[@]}"; do [ -f "$f" ] && rm -f -- "$f" && withheld=$((withheld+1)) && left_out withheld_secret_file "$(home_of "$f")"; done' 'true' bash tests/watcher.sh
mutate "a backup failing for minutes already raises a notification" lifestream/src/backup.rs 'age >= NOTIFY_AFTER && notified' 'notified' bash tests/watcher-notify.sh
mutate "the same failing backup is announced on every run" lifestream/src/backup.rs 'notified != Some(since)' 'true' bash tests/watcher-notify.sh
mutate "a computer with no verified private copy reads as backed up" lifestream/src/backup.rs 'return if now - first > GRACE { Local::OnlyHere' 'return if false { Local::OnlyHere' rs a_fresh_repository_with_no_remote
mutate "commits that never reach the private copy for days read as ok" lifestream/src/backup.rs 'Some(oldest) if now - oldest > GRACE =>' 'Some(oldest) if now - oldest > GRACE * 1000 =>' rs commits_unpushed_for_over_a_day
mutate "a repository nobody verified counts as private"    lifestream/src/backup.rs 'v["private"].as_bool() == Some(true) && v["slug"]' 'v["slug"]' rs a_remote_nobody_verified
mutate "local_only never raises its one notification"      lifestream/src/backup.rs 'let notify = notifications && notified_episode(dir) != Some(since);' 'let notify = false;' bash tests/watcher-notify.sh
mutate "a commit that failed once blocks every later run"   packaging/libexec/watch.sh 'g reset -q -- "$rel" machines' 'true' bash tests/watcher-failures.sh
mutate "the left-out list records the secret line it withheld" packaging/libexec/watch.sh '[ "$k" = 0 ] || left_out withheld_secret_lines "$(home_of "$f")" "\"count\":$k"' '[ "$k" = 0 ] || left_out withheld_secret_lines "$(home_of "$f")" "\"count\":$k,\"was\":\"$(grep -h ghp_ "$HOME/$(home_of "$f")" | head -1 | tr -d "\"\\\\")\""' bash tests/watcher-failures.sh
mutate "left out: a private-by-rule file is offered for inclusion" lifestream/src/leftout.rs 'can_include: matches!(why, "too_large" | "unreadable")' 'can_include: matches!(why, "too_large" | "unreadable" | "private_by_rule")' rs private_by_rule_is_never_includable
mutate "left out: a path outside the home folder is shown"  lifestream/src/leftout.rs "&& !p.starts_with(['/', '~'])" '' rs unsafe_paths_are_null
mutate "uninstall keeps the clipboard history"            uninstall.sh 'rm -f "$share/hestia/clipboard.jsonl"' 'true' bash tests/uninstall.sh
mutate "bootstrap restores code that runs by itself"       packaging/libexec/bootstrap.sh 'if is_executable_config "$rel" && [ "$EXEC" != 1 ]; then review+=("$kind/$rel"); continue; fi' 'true' bash tests/bootstrap.sh
mutate "the rehearsal no longer notices restored code that runs by itself" packaging/libexec/rehearse.sh 'if is_executable_config "$name" || is_executable_config "$rel"; then' 'if false; then' bash tests/rehearse.sh
mutate "the rehearsal accepts restored content that differs from the repository" packaging/libexec/rehearse.sh 'if [ "$(sha256sum < "$src")" != "$(sha256sum < "$dest")" ]; then' 'if false; then' bash tests/rehearse.sh
mutate "an update runs while a backup holds its lock"     lifestream/src/update.rs 'Err(TryLockError::WouldBlock) => return Err(busy),' 'Err(TryLockError::WouldBlock) => continue,' bash tests/update.sh
mutate "an update switches to a build that does not start" lifestream/src/update.rs 'if install_sh(out, &src, &prefix, &["--smoke", &d], None).is_none() {' 'if false {' bash tests/update.sh
mutate "updates keep every old build"                      lifestream/src/update.rs 'if !kept.contains(' 'if false && !kept.contains(' bash tests/update.sh

echo; echo "killed $killed, survived $survived"
[ "$survived" = 0 ]
