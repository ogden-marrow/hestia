#!/usr/bin/env bash
# watch.sh <home-repo> — keep the machine capture current, in small commits, only when the computer is quiet.
# Run by hestia-watch.timer. Captures, then commits each kind of change on its own with a plain
# message, then pushes to the private origin. It only touches the capture files.
set -uo pipefail
export PATH="$HOME/.cargo/bin:$PATH"
libexec="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="${1:-${HESTIA_HOME:-$HOME/Home}}"
# nothing to do until Hestia has made the home repository
[ -f "$repo/tree.toml" ] && [ -e "$repo/.git" ] || exit 0
. /etc/os-release
rel="applications/linux/${ID:-linux}/${VERSION_ID:-unknown}"
here="$repo/$rel"
mkdir -p "$here/apt-sources"
[ -e "$here/capture-paths.txt" ] || cp "$libexec/capture-paths.default" "$here/capture-paths.txt"
state="${XDG_DATA_HOME:-$HOME/.local/share}/hestia"; mkdir -p "$state"

note() { printf '{"at":%s,"result":"%s"}\n' "$(date +%s)" "$1" > "$state/watch.json"; }

# one at a time
exec 9>"${XDG_RUNTIME_DIR:-/tmp}/hestia-watch.lock"
flock -n 9 || exit 0

# ---- is the computer quiet enough? (pressure stall info: the share of the last 10 s anything waited)
busy() { awk -v max="$2" '/^some/ {split($2,a,"="); exit !(a[2] > max)}' "/proc/pressure/$1" 2>/dev/null; }
if [ "${HESTIA_NOW:-0}" != 1 ] && { busy cpu 25 || busy io 25; }; then note "busy: waiting for a quieter moment"; exit 0; fi
for ac in /sys/class/power_supply/A*/online; do [ -r "$ac" ] && on_ac=$(cat "$ac"); done
cap=$(cat /sys/class/power_supply/BAT*/capacity 2>/dev/null | head -1)
if [ "${HESTIA_NOW:-0}" != 1 ] && [ "${on_ac:-1}" = "0" ] && [ "${cap:-100}" -lt 25 ]; then note "battery low: skipped"; exit 0; fi
[ -e "$repo/.git/index.lock" ] && { note "git is busy: skipped"; exit 0; }

# ---- capture at the lowest priority
nice -n 19 ionice -c 3 "$libexec/capture.sh" "$here" >/dev/null 2>"$state/watch.err" || { note "capture failed: see watch.err"; exit 1; }

# ---- who is this machine (machines/<name>/machine.toml)
lifestream machine --repo "$repo" --write >/dev/null 2>>"$state/watch.err" || true

# git refuses a path it has never seen: keep only paths that hold files or are tracked
live() { for p in "$@"; do if [ -n "$(find "$repo/$p" -type f 2>/dev/null | head -1)" ] || [ -n "$(git -C "$repo" ls-files -- "$p")" ]; then printf '%s\n' "$p"; fi; done; }

# ---- one commit per kind of change
commit_area() { # label, message-builder-output, paths...
  local label="$1" msg="$2"; shift 2
  mapfile -t paths < <(live "$@"); set -- "${paths[@]}"; [ $# -gt 0 ] || return
  git -C "$repo" add -A -- "$@" 2>/dev/null
  git -C "$repo" diff --cached --quiet -- "$@" && return
  git -C "$repo" commit -q -m "machine: $msg" -m "Captured automatically when the computer was quiet." -- "$@" && made=$((made+1))
}
names() { # list +/- lines of a file diff as names, first few
  git -C "$repo" diff HEAD -U0 -- "$1" | grep -E "^$2" | grep -vE '^(\+\+\+|---)' | sed -E "s/^$2//" | cut -d' ' -f1 | head -3 | paste -sd, -
}
made=0
# the first capture of a computer is one commit; after that, one per kind of change
if [ -z "$(git -C "$repo" ls-files -- "$rel" | head -1)" ]; then
  git -C "$repo" add -A -- "$rel" 2>/dev/null
  git -C "$repo" commit -q -m "machine: first capture of this computer" -m "Captured automatically when the computer was quiet." -- "$rel" && made=$((made+1))
fi
for f in apt-manual.txt snap.txt flatpak.txt cargo-bins.txt docker-images.txt; do
  p="$rel/$f"; git -C "$repo" add -A -- "$p" 2>/dev/null
  git -C "$repo" diff --cached --quiet -- "$p" && continue
  add=$(git -C "$repo" diff --cached -U0 -- "$p" | grep -cE '^\+[^+]'); del=$(git -C "$repo" diff --cached -U0 -- "$p" | grep -cE '^-[^-]')
  what="${f%.txt}"; what="${what%-manual}"
  a=$(git -C "$repo" diff --cached -U0 -- "$p" | grep -E '^\+[^+]' | sed 's/^+//;s/ .*//' | head -3 | paste -sd, -)
  r=$(git -C "$repo" diff --cached -U0 -- "$p" | grep -E '^-[^-]' | sed 's/^-//;s/ .*//' | head -3 | paste -sd, -)
  m="$what"; [ "$add" -gt 0 ] && m="$m +$add ($a)"; [ "$del" -gt 0 ] && m="$m -$del ($r)"
  git -C "$repo" commit -q -m "machine: $m" -m "Captured automatically when the computer was quiet." -- "$p" && made=$((made+1))
done
sections=$(git -C "$repo" diff HEAD -U0 -- "$rel/dconf.ini" | grep -E '^[+-]\[' | sed -E 's/^[+-]\[//;s/\]//' | sort -u | head -3 | paste -sd, -)
changed=$(git -C "$repo" diff HEAD -U0 -- "$rel/dconf.ini" | grep -cE '^[+-][^+-]')
[ "$changed" -gt 0 ] && commit_area dconf "GNOME settings, $((changed)) lines changed${sections:+ ($sections)}" "$rel/dconf.ini"
files=$(git -C "$repo" status --porcelain -- "$rel" | grep -E " $rel/(config|dotfiles)/" | awk '{print $2}' | sed "s#^$rel/##" | head -3 | paste -sd, -)
n=$(git -C "$repo" status --porcelain -- "$rel" | grep -cE " $rel/(config|dotfiles)/")
[ "$n" -gt 0 ] && commit_area config "config, $n files changed ($files)" "$rel/config" "$rel/dotfiles"
n=$(git -C "$repo" status --porcelain -- "$rel/apt-sources" | wc -l)
[ "$n" -gt 0 ] && commit_area apt "apt sources changed" "$rel/apt-sources"
git -C "$repo" add -A -- "$rel/MACHINE.txt" machines 2>/dev/null
git -C "$repo" diff --cached --quiet -- "$rel/MACHINE.txt" machines || commit_area machine "machine fingerprint or hardware notes changed" "$rel/MACHINE.txt" machines

# ---- push automatically, but only to a private remote and only if nothing in the new commits looks like a secret
push_ok() {
  git -C "$repo" remote get-url origin >/dev/null 2>&1 || { note "ok: $made commit(s) (no remote yet)"; return 1; }
  local url slug; url=$(git -C "$repo" remote get-url origin)
  slug=$(printf '%s' "$url" | sed -E 's#^(https://github.com/|git@github.com:)##; s#\.git$##')
  [ "$(gh repo view "$slug" --json isPrivate --jq .isPrivate 2>/dev/null)" = "true" ] || { note "not pushed: remote is not a confirmed private repo"; return 1; }
  local range="origin/main..HEAD"; git -C "$repo" rev-parse origin/main >/dev/null 2>&1 || range="HEAD"
  if git -C "$repo" log -p --no-color "$range" 2>/dev/null | grep -E '^\+' | grep -qE 'BEGIN [A-Z ]*PRIVATE KEY|ghp_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{30,}|sk-[A-Za-z0-9]{32,}|AKIA[0-9A-Z]{16}|xox[baprs]-[A-Za-z0-9-]{20,}'; then
    note "not pushed: something that looks like a secret is in the new commits"; return 1
  fi
}
# Auto-push stays off until the shared secret scanner replaces the regexes above (red team: they caught 0 of 11
# common token formats). Turn on with HESTIA_AUTOPUSH=1 only after that fix lands.
if [ "${HESTIA_AUTOPUSH:-0}" != 1 ]; then
  note "ok: $made commit(s) (auto-push is off)"
elif push_ok; then
  if [ -n "$(git -C "$repo" log origin/main..HEAD --oneline 2>/dev/null)" ]; then
    if git -C "$repo" push -q origin HEAD 2>>"$state/watch.err"; then note "ok: $made commit(s), pushed"; else note "ok: $made commit(s); push failed: see watch.err"; fi
  else note "ok: $made commit(s), up to date"; fi
fi
