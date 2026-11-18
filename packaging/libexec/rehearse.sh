#!/usr/bin/env bash
# rehearse.sh [--repo DIR] [--share DIR] — prove the backup can be restored. A backup nobody has restored is a hope.
# Run weekly by hestia-rehearse.timer. It copies the home repository's HEAD into a private scratch folder, runs the restore
# (bootstrap.sh, the one restore) against a FAKE home folder there, first as a dry run and then with --apply, and checks:
#   the manifest (tree.toml) and the capture list parse; every file the restore placed exists with the repository's content and
#   mode (no setuid, no group/other write); nothing was written outside the fake home (a sentinel folder and the repository copy
#   are listed before and after, and so are the key and startup files of the REAL home); everything that runs by itself was held
#   back; no ~/.ssh, ~/.gnupg or keyring was written; no link was followed or restored; nothing in the restored files looks like
#   a secret. The real home folder is never a restore target and is never written.
# The result is <share>/hestia/rehearsal.json: {"at":EPOCH,"result":"passed|failed","checked":N,"problems":[plain sentences],
# "repo_commit":"abc1234"|null}. Problems are fixed sentences: no paths, no file contents, no secrets.
# Exit 0 passed, 1 failed, 75 another rehearsal is running. Nothing to rehearse yet (no home repository): exit 0, nothing written.
# --share is the data folder (default $XDG_DATA_HOME or ~/.local/share), like `lifestream backup-state --share`.
set -uo pipefail
libexec="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
. "$libexec/home-repo.sh"
. "$libexec/restore-rules.sh"
repo=""; share=""
while [ $# -gt 0 ]; do
  case "$1" in
    --repo) repo="${2:-}"; shift ;;
    --share) share="${2:-}"; shift ;;
    -h|--help) sed -n '2,15p' "$0"; exit 0 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac; shift
done
REAL_HOME="$HOME"
repo="${repo:-$(hestia_home_repo)}"
share="${share:-${XDG_DATA_HOME:-$HOME/.local/share}}"
[ -e "$repo/.git" ] || exit 0   # nothing to rehearse until Hestia has made the home repository

exec 9>"${XDG_RUNTIME_DIR:-/tmp}/hestia-rehearse.lock"
flock -n 9 || exit 75

# the scratch folder: private, and gone when this ends, however it ends
base="${XDG_RUNTIME_DIR:-/tmp}"; [ -d "$base" ] && [ -w "$base" ] || base=/tmp
T="$(mktemp -d "$base/hestia-rehearse.XXXXXX")" || { echo "rehearse: cannot make a scratch folder" >&2; exit 1; }
chmod 700 "$T"
trap 'rm -rf -- "$T"' EXIT
mkdir -p "$T/repo" "$T/home" "$T/outside/.ssh" "$T/tmp"

problems=(); checked=0
problem() { local p; for p in "${problems[@]}"; do [ "$p" = "$1" ] && return; done; problems+=("$1"); }

LS="${LIFESTREAM:-$(command -v lifestream 2>/dev/null || echo "$HOME/.local/bin/lifestream")}"

# a listing of a folder that changes when any file in it is added, removed, changed or re-linked (names, kinds, modes, link targets, content)
listing() { [ -d "$1" ] || { echo "(absent)"; return; }; ( cd "$1" && { find . -printf '%p %y %m %l\n'; find . -type f -exec sha256sum {} +; } | LC_ALL=C sort ); }
# the real home's keys and startup files: names, kinds, sizes, modes and change times
real_guard() {
  local p
  for p in .ssh .gnupg .bashrc .profile .bash_profile .zshrc .gitconfig .config/environment.d .config/autostart .config/systemd .local/share/keyrings; do
    find "$REAL_HOME/$p" -printf '%p %y %s %m %T@ %C@\n' 2>/dev/null
  done | LC_ALL=C sort
}
echo "canary" > "$T/outside/canary"; echo "canary key" > "$T/outside/.ssh/authorized_keys"

finish() { # writes the result and exits
  local result=passed rc=0 at commit esc json i first=1 out
  [ ${#problems[@]} -eq 0 ] || { result=failed; rc=1; }
  at="$(date +%s)"
  commit="$(git -C "$repo" rev-parse --short HEAD 2>/dev/null)"
  out="$share/hestia"; mkdir -p "$out" 2>/dev/null
  json="{\"at\":$at,\"result\":\"$result\",\"checked\":$checked,\"problems\":["
  for i in "${!problems[@]}"; do
    [ "$i" -lt 20 ] || break
    esc="${problems[$i]//\\/\\\\}"; esc="${esc//\"/\\\"}"
    [ "$first" = 1 ] || json="$json,"; first=0; json="$json\"$esc\""
  done
  json="$json],\"repo_commit\":"; if [ -n "$commit" ]; then json="$json\"$commit\"}"; else json="${json}null}"; fi
  printf '%s\n' "$json" > "$out/rehearsal.json.tmp" && mv -f "$out/rehearsal.json.tmp" "$out/rehearsal.json" || rc=1
  if [ "$rc" = 0 ]; then echo "restore rehearsal passed ($checked checked)"; else printf 'restore rehearsal FAILED: %s\n' "${problems[0]:-could not write the result}"; fi
  exit "$rc"
}

# ---- 1. the repository as pushed: HEAD, as plain files, in the scratch folder (git archive: no .git, no hooks, links stay links)
if ! git -C "$repo" rev-parse -q --verify HEAD >/dev/null 2>&1; then
  problem "The home repository has nothing committed yet, so there is nothing to restore."; finish
fi
( umask 022; git -C "$repo" archive --format=tar HEAD | tar -x --no-same-owner -C "$T/repo" ) 2>/dev/null \
  || { problem "The home repository could not be copied out of git."; finish; }
[ -x "$LS" ] || { problem "The secret scanner is missing, so the check could not run."; finish; }

. /etc/os-release 2>/dev/null
here="$T/repo/applications/linux/${ID:-linux}/${VERSION_ID:-unknown}"

# ---- 2. the manifest and the capture list parse
if [ -f "$T/repo/tree.toml" ] && "$LS" tree --tree "$T/repo/tree.toml" >/dev/null 2>&1; then checked=$((checked+1))
else problem "The home repository's tree.toml (its manifest) is missing or does not parse."; fi
if [ ! -d "$here" ]; then
  problem "Nothing was captured in the repository for this computer's system version."
elif [ ! -s "$here/capture-paths.txt" ]; then
  problem "The capture list (capture-paths.txt) is missing or empty."
else
  bad=0; n=0
  while IFS= read -r line || [ -n "$line" ]; do
    case "$line" in ""|"#"*) continue ;; esac
    n=$((n+1))
    # shellcheck disable=SC2088 # a literal "~/" is what capture-paths.txt holds
    case "$line" in "~/"*) ;; *) bad=$((bad+1)) ;; esac
    case "$line" in *..*|*[[:cntrl:]]*) bad=$((bad+1)) ;; esac
  done < "$here/capture-paths.txt"
  if [ "$bad" = 0 ] && [ "$n" -gt 0 ]; then checked=$((checked+1)); else problem "The capture list has no usable entries, or has an entry that is not a path under the home folder."; fi
fi
# without these there is nothing to restore from: stop with what is known
[ ${#problems[@]} -eq 0 ] || finish

# ---- 3. restore into the fake home, as a dry run, then for real. Only the file steps run: packages and desktop settings
# change the machine itself and are never part of a rehearsal. A clean environment: nothing here can resolve to the real home.
before_outside="$(listing "$T/outside")"; before_repo="$(listing "$T/repo")"; before_real="$(real_guard)"; before_fake="$(listing "$T/home")"
restore() { env -i PATH="$PATH" LANG=C HOME="$T/home" TMPDIR="$T/tmp" bash "$libexec/bootstrap.sh" "$T/repo" "$@" 2>&1; }
out="$(restore)"; rc=$?
if [ "$rc" -ne 0 ]; then problem "The restore dry run did not finish."; finish; fi
case "$out" in *"dry run: nothing was changed"*) checked=$((checked+1)) ;; *) problem "The restore did not say that it was only a dry run." ;; esac
[ "$(listing "$T/home")" = "$before_fake" ] || problem "The restore dry run changed files."
: > "$T/record"
out="$(restore --apply --only dotfiles,config --record "$T/record")"; rc=$?
if [ "$rc" -ne 0 ]; then problem "The restore did not finish."; finish; fi
case "$out" in *"no failures"*) ;; *) problem "The restore reported failures." ;; esac

# ---- 4. what was restored is exactly what the repository holds
placed=0
mode_of() { printf '%o' $(( 8#$(stat -c %a -- "$1") )); }
declare -A is_placed
while IFS= read -r -d '' src && IFS= read -r -d '' dest; do
  placed=$((placed+1)); is_placed["$dest"]=1
  case "$dest" in "$T/home"/*) ;; *) problem "The restore wrote a file outside the fake home folder."; continue ;; esac
  if [ -L "$src" ]; then problem "The restore copied from a link in the repository."; continue; fi
  if [ -L "$dest" ] || [ ! -f "$dest" ]; then problem "A restored file is missing or is a link."; continue; fi
  if [ "$(sha256sum < "$src")" != "$(sha256sum < "$dest")" ]; then problem "A restored file does not have the content the repository holds."; continue; fi
  m="$(mode_of "$dest")"
  if [ $(( 8#$m & 8#7022 )) -ne 0 ]; then problem "A restored file has setuid, setgid or group/other write."; continue; fi
  if [ "$m" != "$(printf '%o' $(( 8#$(stat -c %a -- "$src") & 8#755 )))" ]; then problem "A restored file does not have the mode the repository holds."; continue; fi
  checked=$((checked+1))
done < "$T/record"
[ "$placed" -gt 0 ] || problem "The restore had nothing to put back: no file in the repository could be restored."

# ---- 5. nothing else appeared, and nothing there should not be
while IFS= read -r -d '' p; do
  rel="${p#"$T/home"/}"
  if [ -L "$p" ]; then problem "A link appeared in the restored files."; continue; fi
  if [ -d "$p" ]; then
    [ $(( 8#$(mode_of "$p") & 8#7022 )) -eq 0 ] || problem "A restored folder has setuid, setgid or group/other write."
    continue
  fi
  [ -n "${is_placed[$p]:-}" ] || problem "A file appeared that the restore did not report placing."
  name="$rel"; case "$rel" in .config/*) name="${rel#.config/}" ;; esac
  if is_executable_config "$name" || is_executable_config "$rel"; then problem "Something that runs by itself was restored without permission."; fi
  if is_denied "$name" || is_denied "$rel"; then problem "A key, keyring or login file was restored."; fi
  case "/$rel" in */.ssh/*|*/.gnupg/*|*keyring*) problem "A key or keyring folder was written." ;; esac
done < <(find "$T/home" -mindepth 1 -print0)

# ---- 6. nothing outside the fake home changed: the sentinel folder, the repository copy, and the real home's keys and startup files
[ "$(listing "$T/outside")" = "$before_outside" ] || problem "The restore changed a folder outside the fake home."
[ "$(listing "$T/repo")" = "$before_repo" ] || problem "The restore changed the repository copy."
[ "$(real_guard)" = "$before_real" ] || problem "Something in the real home folder's keys or startup files changed while the check ran."

# ---- 7. the shared secret scanner finds nothing in the repository's captured files or in what was restored
for d in "$here" "$T/home"; do
  rc=0; "$LS" secret-scan "$d" >/dev/null 2>&1 || rc=$?
  [ "$rc" -le 1 ] || { problem "The secret scanner could not finish."; continue; }
  [ "$rc" = 0 ] || problem "A captured file looks like it holds a secret."
done
finish
