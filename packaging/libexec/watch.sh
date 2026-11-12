#!/usr/bin/env bash
# watch.sh [home-repo] — keep the machine capture current, in small commits, only when the computer is quiet.
# Run by hestia-watch.timer. Captures, then commits each kind of change on its own with a plain
# message, then pushes to the private origin. It only touches the capture files.
# The result is one line in watch.json: it starts with "ok" only when everything asked of it worked, and with
# "FAILED" (and the first line of what went wrong) otherwise. Exit 1 means FAILED; 75 means another run holds the lock.
set -uo pipefail
export PATH="$HOME/.cargo/bin:$PATH"
libexec="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
. "$libexec/home-repo.sh"
repo="${1:-$(hestia_home_repo)}"   # the one setting the app shows; an argument is for tests
# nothing to do until Hestia has made the home repository
[ -f "$repo/tree.toml" ] && [ -e "$repo/.git" ] || exit 0
. /etc/os-release
rel="applications/linux/${ID:-linux}/${VERSION_ID:-unknown}"
here="$repo/$rel"
mkdir -p "$here/apt-sources"
[ -e "$here/capture-paths.txt" ] || cp "$libexec/capture-paths.default" "$here/capture-paths.txt"
state="${XDG_DATA_HOME:-$HOME/.local/share}/hestia"; mkdir -p "$state"

withheld_note=""
note() { # one line of JSON; quotes, backslashes and control characters in the text are made safe
  local t="${1//\\/\\\\}"; t="${t//\"/\\\"}"; t="$(printf '%s' "$t" | tr -d '\000-\037')"
  printf '{"at":%s,"result":"%s%s"}\n' "$(date +%s)" "$t" "$withheld_note" > "$state/watch.json"
}
die() { note "FAILED: $1"; exit 1; }
first() { local l; l="$(printf '%s\n' "$1" | sed -n '/[^[:space:]]/{p;q}')"; printf '%s' "${l:-no details}"; }   # first non-blank line

# one at a time. A second run says so (the app must not report "captured" when nothing ran).
exec 9>"${XDG_RUNTIME_DIR:-/tmp}/hestia-watch.lock"
flock -n 9 || { note "busy: another capture is still running, so nothing new was captured"; exit 75; }

# ---- is the computer quiet enough? (pressure stall info: the share of the last 10 s anything waited)
# Waiting is never for ever: after HESTIA_MAX_WAIT seconds (6 h) since the last capture it runs anyway.
psi="${HESTIA_PSI_DIR:-/proc/pressure}"; power="${HESTIA_POWER_DIR:-/sys/class/power_supply}"
busy() { awk -v max="$2" '/^some/ {split($2,a,"="); exit !(a[2] > max)}' "$psi/$1" 2>/dev/null; }
overdue=0; [ "$(( $(date +%s) - $(cat "$state/last-capture" 2>/dev/null || echo 0) ))" -gt "${HESTIA_MAX_WAIT:-21600}" ] && overdue=1
if [ "${HESTIA_NOW:-0}" != 1 ] && [ "$overdue" = 0 ]; then
  if busy cpu 25 || busy io 25; then note "busy: waiting for a quieter moment"; exit 0; fi
  for ac in "$power"/A*/online; do [ -r "$ac" ] && on_ac=$(cat "$ac"); done
  cap=$(cat "$power"/BAT*/capacity 2>/dev/null | head -1)
  if [ "${on_ac:-1}" = "0" ] && [ "${cap:-100}" -lt 25 ]; then note "battery low: skipped"; exit 0; fi
fi
# git leaves index.lock behind when it is killed. Ours is the only run, so one that is old is stale.
lock="$(git -C "$repo" rev-parse --git-path index.lock 2>/dev/null)"; case "$lock" in /*) ;; *) lock="$repo/$lock" ;; esac
if [ -e "$lock" ]; then
  [ -z "$(find "$lock" -mmin +10 2>/dev/null)" ] && { note "git is busy: skipped"; exit 0; }
  rm -f -- "$lock"
fi

# ---- the one secret scanner (lifestream secret-scan). No scanner, no commit: fail closed.
LS="${LIFESTREAM:-$(command -v lifestream 2>/dev/null || echo "$HOME/.local/bin/lifestream")}"
[ -x "$LS" ] || die "nothing committed: the secret scanner is missing"

export LIFESTREAM="$LS"   # capture.sh asks the engine's one deny-list which paths it may copy
# ---- capture at the lowest priority
nice -n 19 ionice -c 3 "$libexec/capture.sh" "$here" >/dev/null 2>"$state/watch.err" || die "capture failed: $(first "$(tac "$state/watch.err")")"
date +%s > "$state/last-capture"

# ---- withhold anything that looks like a secret. Nothing is ever changed on this computer: only the captured
# copy. A shell startup file keeps everything except the lines that look like secrets (each becomes a comment);
# any other file with a hit is left out whole, and so is any file holding a private key. The status lists counts,
# never values. The scanner prints path, line and rule each ended by a NUL, so any file name is read back whole.
withheld=0; redacted=0
declare -A drop redact
scan_rc=0; "$LS" secret-scan -z "$here" > "$state/scan.out" 2>"$state/scan.err" || scan_rc=$?
[ "$scan_rc" -le 1 ] || die "the secret scanner could not finish, so nothing was committed: $(first "$(cat "$state/scan.err")")"
while IFS= read -r -d '' f && IFS= read -r -d '' n && IFS= read -r -d '' r; do
  case "$f" in
    */dotfiles/.bashrc|*/dotfiles/.profile|*/dotfiles/.bash_profile|*/dotfiles/.bash_logout|*/dotfiles/.zshrc|*/dotfiles/.zprofile|*/dotfiles/.zshenv|*/dotfiles/.zlogout)
      if [ "$r" = "private key" ]; then drop["$f"]=1; else redact["$f"]+=" $n"; fi ;;
    *) drop["$f"]=1 ;;
  esac
done < "$state/scan.out"
for f in "${!drop[@]}"; do [ -f "$f" ] && rm -f -- "$f" && withheld=$((withheld+1)); done
for f in "${!redact[@]}"; do
  [ -f "$f" ] && [ -z "${drop[$f]:-}" ] || continue
  # shellcheck disable=SC2086 # the recorded line numbers are a space-separated list: splitting is the point
  for n in $(printf '%s\n' ${redact[$f]} | sort -un); do
    sed -i "${n}s/.*/# [Hestia left this line out: it looked like a secret]/" "$f" && redacted=$((redacted+1))
  done
done
{ [ "$withheld" -gt 0 ] || [ "$redacted" -gt 0 ]; } && withheld_note="; $withheld file(s) and $redacted line(s) withheld: possible secrets"
if [ -s "$here/SKIPPED.txt" ]; then withheld_note="$withheld_note; $(wc -l < "$here/SKIPPED.txt") file(s) skipped: unreadable or too big (see SKIPPED.txt)"; fi

# ---- who is this machine (machines/<name>/machine.toml)
"$LS" machine --repo "$repo" --write >/dev/null 2>>"$state/watch.err" || true

# ---- git, where every failure is a result. g runs a git command that must work; nothing is hidden.
g() { local e; e=$(git -C "$repo" "$@" 2>&1 >/dev/null) || die "git $1 failed: $(first "$e")"; }
has_staged() { local e rc=0; e=$(git -C "$repo" diff --cached --quiet -- "$@" 2>&1) || rc=$?; [ "$rc" -le 1 ] || die "git diff failed: $(first "$e")"; [ "$rc" = 1 ]; }
# changed files under the given paths, NUL-separated (-z: a name with a space, an accent or a newline is still one name)
changed() { git -C "$repo" ls-files -z -m -d -o --exclude-standard -- "$@" | sort -zu; }
count() { tr -cd '\0' | wc -c; }
firstnames() { tr '\0' '\n' | sed "s#^$rel/##" | head -3 | paste -sd, -; }

# git refuses a path it has never seen: keep only paths that hold files or are tracked
live() { for p in "$@"; do if [ -n "$(find "$repo/$p" -type f 2>/dev/null | head -1)" ] || [ -n "$(git -C "$repo" ls-files -- "$p")" ]; then printf '%s\n' "$p"; fi; done; }

commit_area() { # kind-of-change (for the reader of the call), message, paths...
  local msg="$2"; shift 2
  mapfile -t paths < <(live "$@"); set -- "${paths[@]}"; [ $# -gt 0 ] || return 0
  g add -A -- "$@"
  has_staged "$@" || return 0
  g commit -q -m "machine: $msg" -m "Captured automatically when the computer was quiet." -- "$@"; made=$((made+1))
}
# files directly in the capture folder that no other step owns (the capture list, the skipped list, anything a later Hestia starts capturing)
commit_strays() { # words for the message, then the ls-files flags that select them
  local words="$1" f; shift
  local top=(); mapfile -d '' -t all < <(git -C "$repo" ls-files -z "$@" --exclude-standard -- "$rel" | sort -zu)
  for f in "${all[@]}"; do [[ "$f" == "$rel"/* && "${f#"$rel"/}" != */* ]] && top+=("$f"); done
  [ "${#top[@]}" -gt 0 ] || return 0
  g add -A -- "${top[@]}"
  has_staged "${top[@]}" || return 0
  g commit -q -m "machine: $words ($(printf '%s\0' "${top[@]}" | firstnames))" -m "Captured automatically when the computer was quiet." -- "${top[@]}"; made=$((made+1))
}
made=0
# ---- one commit per kind of change
commit_all() {
  # the first capture of a computer is one commit; after that, one per kind of change
  if [ -z "$(git -C "$repo" ls-files -- "$rel" | head -1)" ]; then
    g add -A -- "$rel"
    if has_staged "$rel"; then
      g commit -q -m "machine: first capture of this computer" -m "Captured automatically when the computer was quiet." -- "$rel"; made=$((made+1))
    fi
  fi
  local f p add del what a r m sections n files
  for f in apt-manual.txt snap.txt flatpak.txt cargo-bins.txt docker-images.txt; do
    p="$rel/$f"; g add -A -- "$p"
    has_staged "$p" || continue
    add=$(git -C "$repo" diff --cached -U0 -- "$p" | grep -cE '^\+[^+]'); del=$(git -C "$repo" diff --cached -U0 -- "$p" | grep -cE '^-[^-]')
    what="${f%.txt}"; what="${what%-manual}"
    a=$(git -C "$repo" diff --cached -U0 -- "$p" | grep -E '^\+[^+]' | sed 's/^+//;s/ .*//' | head -3 | paste -sd, -)
    r=$(git -C "$repo" diff --cached -U0 -- "$p" | grep -E '^-[^-]' | sed 's/^-//;s/ .*//' | head -3 | paste -sd, -)
    m="$what"; [ "$add" -gt 0 ] && m="$m +$add ($a)"; [ "$del" -gt 0 ] && m="$m -$del ($r)"
    g commit -q -m "machine: $m" -m "Captured automatically when the computer was quiet." -- "$p"; made=$((made+1))
  done
  sections=$(git -C "$repo" diff HEAD -U0 -- "$rel/dconf.ini" | grep -E '^[+-]\[' | sed -E 's/^[+-]\[//;s/\]//' | sort -u | head -3 | paste -sd, -)
  n=$(git -C "$repo" diff HEAD -U0 -- "$rel/dconf.ini" | grep -cE '^[+-][^+-]')
  [ "$n" -gt 0 ] && commit_area dconf "GNOME settings, $n lines changed${sections:+ ($sections)}" "$rel/dconf.ini"
  n=$(changed "$rel/config" "$rel/dotfiles" | count); files=$(changed "$rel/config" "$rel/dotfiles" | firstnames)
  [ "$n" -gt 0 ] && commit_area config "config, $n files changed ($files)" "$rel/config" "$rel/dotfiles"
  n=$(changed "$rel/apt-sources" | count)
  [ "$n" -gt 0 ] && commit_area apt "apt sources changed" "$rel/apt-sources"
  commit_area machine "machine fingerprint or hardware notes changed" "$rel/MACHINE.txt" machines
  commit_strays "new files captured" -o
  commit_strays "capture files changed" -m -d
  # nothing may be left behind: a commit that failed quietly is the worst kind of backup
  git -C "$repo" status --porcelain -z -uall -- "$rel" machines > "$state/left"
  [ ! -s "$state/left" ] || die "$(count < "$state/left") change(s) could not be committed (first: $(tr '\0' '\n' < "$state/left" | head -1 | cut -c4-))"
}
commit_all

# ---- push automatically, but only to a private GitHub repo (checked on the URL git will really push to,
# after pushurl and insteadOf rewrites) and only if the shared scanner finds nothing in the new commits
# Signed in to Hestia (the token lives in the system keyring)? Then Hestia vouches for the repository and git gets the
# token from the keyring; otherwise the GitHub CLI does both, as before.
signed_in() { "$LS" auth status --json 2>/dev/null | grep -q '"signed_in":true'; }
creds=()
# the network never holds the lock for ever: a stalled transfer is cut off
gitnet() { timeout "${HESTIA_NET_TIMEOUT:-600}" git -C "$repo" "${creds[@]}" -c http.lowSpeedLimit=1000 -c http.lowSpeedTime=60 "$@" 2>&1 >/dev/null; }
netdie() { # what, output of the failed command, its exit status
  [ "$3" = 124 ] && die "$1 timed out: GitHub did not answer, will retry"
  case "$2" in *"Could not resolve"*|*"unable to access"*|*"Connection"*|*"Network is unreachable"*) die "offline: $1 could not reach GitHub, will retry" ;; esac
  die "$1 failed: $(first "$2")"
}
# why is a repository not usable? offline, signed out, gone and public are different things to do about
why_not() { # the engine's JSON events (or gh's words), one string
  case "$1" in
    *'"kind":"offline"'*|*"resolve host"*|*"dial tcp"*|*"timeout"*) echo "offline: could not ask GitHub, will retry" ;;
    *"not signed in"*|*"HTTP 401"*|*"gh auth login"*|*"authentication"*) echo "signed out: sign in to GitHub again in Hestia" ;;
    *"was not found"*|*"HTTP 404"*|*"Could not resolve to a Repository"*) echo "the repository is gone: GitHub says it does not exist" ;;
    *"is public"*) echo "the repository is public: nothing is sent to a public repository" ;;
    *'"kind":"sso"'*) echo "your organisation needs you to approve Hestia (SSO) on GitHub" ;;
    *) echo "remote is not a confirmed private repo of yours" ;;
  esac
}
publish() {
  git -C "$repo" remote get-url origin >/dev/null 2>&1 || { note "ok: $made commit(s) (no remote yet)"; return 0; }
  local url slug out e rc=0; url=$(git -C "$repo" remote get-url --push origin)
  case "$url" in https://github.com/*|git@github.com:*) ;; *) die "not pushed: remote is not a GitHub repository" ;; esac
  slug=$(printf '%s' "$url" | sed -E 's#^(https://github.com/|git@github.com:)##; s#\.git$##')
  if signed_in; then
    out=$("$LS" home check-private "$slug" 2>&1) || die "not pushed: $(why_not "$out")"
    case "$LS" in *\'*) die "not pushed: the Hestia program path has a quote in it" ;; esac
    creds=(-c credential.helper= -c "credential.helper=!'$LS' auth git-credential")
  else
    out=$(gh repo view "$slug" --json isPrivate --jq .isPrivate 2>&1) || true
    [ "$out" = "true" ] || { [ "$out" = "false" ] && out="is public"; die "not pushed: $(why_not "$out")"; }
  fi
  # another computer may have pushed since: fetch, then put this computer's commits on top (the paths are different per computer)
  e=$(gitnet fetch -q origin main) || { rc=$?; case "$e" in *"couldn't find remote ref"*) ;; *) netdie "fetch" "$e" "$rc" ;; esac; }
  if git -C "$repo" rev-parse -q --verify origin/main >/dev/null && ! git -C "$repo" merge-base --is-ancestor origin/main HEAD; then
    e=$(git -C "$repo" rebase --autostash origin/main 2>&1 >/dev/null) || { git -C "$repo" rebase --abort >/dev/null 2>&1; die "could not combine with the other computer's changes: $(first "$e")"; }
  fi
  # a possible secret in a commit that was never pushed: start again from what GitHub has (the working files were just scanned and cleaned)
  local range="origin/main..HEAD"; git -C "$repo" rev-parse origin/main >/dev/null 2>&1 || range="all"
  scan_range() { rc=0; e=$("$LS" secret-scan --repo "$repo" --range "$range" 2>&1 >/dev/null) || rc=$?; [ "$rc" -le 1 ] || die "the secret scanner could not finish, so nothing was pushed: $(first "$e")"; [ "$rc" = 0 ]; }
  if ! scan_range; then
    [ "$range" != all ] || die "not pushed: possible secret in the unsent commits (run: lifestream secret-scan --repo $repo --range all)"
    g reset -q origin/main
    commit_all
    scan_range || die "not pushed: possible secret in the new commits (run: lifestream secret-scan --repo $repo --range $range)"
    withheld_note="$withheld_note; unsent commits held a possible secret: started again from what is on GitHub"
  fi
  if [ -z "$(git -C "$repo" log origin/main..HEAD --oneline 2>/dev/null)" ] && git -C "$repo" rev-parse -q --verify origin/main >/dev/null; then note "ok: $made commit(s), up to date"; return 0; fi
  e=$(gitnet push -q origin HEAD) || { rc=$?; printf '%s\n' "$e" >> "$state/watch.err"; netdie "push" "$e" "$rc"; }
  note "ok: $made commit(s), pushed"
}
# Auto-push is on by default; set HESTIA_AUTOPUSH=0 to turn it off.
if [ "${HESTIA_AUTOPUSH:-1}" = 0 ]; then note "ok: $made commit(s) (auto-push is off)"; else publish; fi
