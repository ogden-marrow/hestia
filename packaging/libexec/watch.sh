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

withheld_note=""
note() { printf '{"at":%s,"result":"%s%s"}\n' "$(date +%s)" "$1" "$withheld_note" > "$state/watch.json"; }

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

# ---- the one secret scanner (lifestream secret-scan). No scanner, no commit: fail closed.
LS="${LIFESTREAM:-$(command -v lifestream 2>/dev/null || echo "$HOME/.local/bin/lifestream")}"
[ -x "$LS" ] || { note "nothing committed: the secret scanner is missing"; exit 1; }

# ---- capture at the lowest priority
nice -n 19 ionice -c 3 "$libexec/capture.sh" "$here" >/dev/null 2>"$state/watch.err" || { note "capture failed: see watch.err"; exit 1; }

# ---- withhold anything that looks like a secret. Nothing is ever changed on this computer: only the captured
# copy. A shell startup file keeps everything except the lines that look like secrets (each becomes a comment);
# any other file with a hit is left out whole, and so is any file holding a private key. The status lists counts,
# never values.
withheld=0; redacted=0
declare -A drop redact
scan_out=$(find "$here" -type f -not -path '*/.git/*' -print0 | xargs -0 -r "$LS" secret-scan 2>/dev/null || true)
while IFS= read -r l; do
  [[ "$l" =~ ^(.*):([0-9]+):\ (.*)$ ]] || continue
  f="${BASH_REMATCH[1]}"; n="${BASH_REMATCH[2]}"; r="${BASH_REMATCH[3]}"
  case "$f" in
    */dotfiles/.bashrc|*/dotfiles/.profile|*/dotfiles/.bash_profile|*/dotfiles/.bash_logout|*/dotfiles/.zshrc|*/dotfiles/.zprofile|*/dotfiles/.zshenv|*/dotfiles/.zlogout)
      if [ "$r" = "private key" ]; then drop["$f"]=1; else redact["$f"]+=" $n"; fi ;;
    *) drop["$f"]=1 ;;
  esac
done <<< "$scan_out"
for f in "${!drop[@]}"; do [ -f "$f" ] && rm -f -- "$f" && withheld=$((withheld+1)); done
for f in "${!redact[@]}"; do
  [ -f "$f" ] && [ -z "${drop[$f]:-}" ] || continue
  for n in $(printf '%s\n' ${redact[$f]} | sort -un); do
    sed -i "${n}s/.*/# [Hestia left this line out: it looked like a secret]/" "$f" && redacted=$((redacted+1))
  done
done
[ "$withheld" -gt 0 ] || [ "$redacted" -gt 0 ] && withheld_note="; $withheld file(s) and $redacted line(s) withheld: possible secrets"

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
# Files that are new since the first capture (the desktop-settings file once it is allowed, the capture list,
# anything a later Hestia starts capturing) would otherwise stay untracked for ever: commit them, scanned already.
mapfile -t fresh < <(git -C "$repo" ls-files --others --exclude-standard -- "$rel" | grep -E "^$rel/[^/]+$" || true)
if [ "${#fresh[@]}" -gt 0 ]; then
  git -C "$repo" add -- "${fresh[@]}" 2>/dev/null
  names=$(printf '%s\n' "${fresh[@]}" | sed "s#^$rel/##" | head -3 | paste -sd, -)
  git -C "$repo" commit -q -m "machine: new files captured ($names)" -m "Captured automatically when the computer was quiet." -- "${fresh[@]}" && made=$((made+1))
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

# ---- push automatically, but only to a private GitHub repo (checked on the URL git will really push to,
# after pushurl and insteadOf rewrites) and only if the shared scanner finds nothing in the new commits
push_ok() {
  git -C "$repo" remote get-url origin >/dev/null 2>&1 || { note "ok: $made commit(s) (no remote yet)"; return 1; }
  local url slug; url=$(git -C "$repo" remote get-url --push origin)
  case "$url" in https://github.com/*|git@github.com:*) ;; *) note "not pushed: remote is not a GitHub repository"; return 1;; esac
  slug=$(printf '%s' "$url" | sed -E 's#^(https://github.com/|git@github.com:)##; s#\.git$##')
  [ "$(gh repo view "$slug" --json isPrivate --jq .isPrivate 2>/dev/null)" = "true" ] || { note "not pushed: remote is not a confirmed private repo"; return 1; }
  local range="origin/main..HEAD"; git -C "$repo" rev-parse origin/main >/dev/null 2>&1 || range="all"
  "$LS" secret-scan --repo "$repo" --range "$range" >/dev/null 2>&1 || { note "not pushed: possible secret in the new commits (run: lifestream secret-scan --repo $repo --range $range)"; return 1; }
}
# Auto-push is on by default; set HESTIA_AUTOPUSH=0 to turn it off.
if [ "${HESTIA_AUTOPUSH:-1}" = 0 ]; then
  note "ok: $made commit(s) (auto-push is off)"
elif push_ok; then
  if [ -n "$(git -C "$repo" log origin/main..HEAD --oneline 2>/dev/null)" ]; then
    if git -C "$repo" push -q origin HEAD 2>>"$state/watch.err"; then note "ok: $made commit(s), pushed"; else note "ok: $made commit(s); push failed: see watch.err"; fi
  else note "ok: $made commit(s), up to date"; fi
fi
