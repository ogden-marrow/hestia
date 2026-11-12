#!/usr/bin/env bash
# bootstrap.sh [home-repo] [--apply] [--only steps] [--allow-executable]
# Rebuild this computer from what watch.sh captured into the home repository.
#
#   (default)   DRY RUN: prints every step and changes nothing.
#   --apply     do it.
#   --only a,b  run only these steps. Steps: packages dotfiles config settings sources
#   --allow-executable
#               also restore things that run by themselves (autostart entries, systemd user units, custom
#               keyboard-shortcut commands). Without it they are listed for you to read first and left alone.
#
# Safe by design:
#  * It never deletes. A file that is already there is moved aside to NAME.bak.<time>, never overwritten.
#  * Dotfiles and config are COPIED, never linked, so a live file never points into the repository.
#  * Anything that can run code on login is not restored unless you say so with --allow-executable.
#  * Third-party apt sources (they add software you did not review) only run when named with --only sources.
set -uo pipefail
repo=""; APPLY=0; ONLY=""; EXEC=0
while [ $# -gt 0 ]; do
  case "$1" in
    --apply) APPLY=1 ;;
    --only) ONLY="${2:-}"; shift ;;
    --allow-executable) EXEC=1 ;;
    -h|--help) sed -n '2,16p' "$0"; exit 0 ;;
    -*) echo "unknown option: $1" >&2; exit 2 ;;
    *) repo="$1" ;;
  esac; shift
done
. "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/home-repo.sh"
repo="${repo:-$(hestia_home_repo)}"
[ -f "$repo/tree.toml" ] || { echo "not a Hestia home repository: $repo" >&2; exit 2; }
. /etc/os-release
here="$repo/applications/linux/${ID:-linux}/${VERSION_ID:-unknown}"
[ -d "$here" ] || { echo "nothing captured for ${PRETTY_NAME:-this system} in $repo yet (looked for $here)" >&2; exit 2; }

say()  { printf '\n== %s\n' "$*"; }
run()  { if [ "$APPLY" = 1 ]; then echo "+ $*"; "$@"; else echo "[dry run] $*"; fi; }
want() { if [ -n "$ONLY" ]; then [[ ",$ONLY," == *",$1,"* ]]; else [ "$1" != sources ]; fi; }
fail=(); review=()
stamp="$(date +%Y%m%d%H%M%S)"

# Runs code by itself on login or in the background: held back unless --allow-executable.
is_executable_config() {
  case "$1" in
    autostart/*|systemd/*|*/autostart/*|*/systemd/*) return 0 ;;
  esac
  return 1
}

# put a captured file or folder at DEST: copy, and move anything already there aside
place() { # src dest
  local src="$1" dest="$2"
  if [ -e "$dest" ] || [ -L "$dest" ]; then
    if [ -f "$dest" ] && [ ! -L "$dest" ] && cmp -s "$src" "$dest" 2>/dev/null; then echo "already the same: $dest"; return; fi
    run mv -- "$dest" "$dest.bak.$stamp"
  fi
  run mkdir -p -- "$(dirname "$dest")"
  run cp -a -- "$src" "$dest"
}

if want packages; then
  say "packages: apt ($(wc -l < "$here/apt-manual.txt" 2>/dev/null || echo 0)), snap, flatpak"
  if [ -s "$here/apt-manual.txt" ]; then
    if [ "$APPLY" = 1 ]; then
      xargs -a "$here/apt-manual.txt" sudo apt-get install -y --no-install-recommends \
        || while read -r p; do [ -n "$p" ] && { sudo apt-get install -y "$p" || fail+=("apt:$p"); }; done < "$here/apt-manual.txt"
    else echo "[dry run] apt-get install the $(wc -l < "$here/apt-manual.txt") packages in apt-manual.txt"; fi
  fi
  while read -r name chan flag; do
    [ -n "${name:-}" ] || continue
    run sudo snap install "$name" --channel="${chan:-stable}" ${flag:-} || fail+=("snap:$name")
  done < "$here/snap.txt" 2>/dev/null
  while read -r app origin; do
    [ -n "${app:-}" ] || continue
    run flatpak install -y "${origin:-flathub}" "$app" || fail+=("flatpak:$app")
  done < "$here/flatpak.txt" 2>/dev/null
fi

if want dotfiles; then
  say "dotfiles (copied into your home folder; existing files are moved aside, never overwritten)"
  for f in "$here"/dotfiles/.[!.]*; do
    [ -e "$f" ] || continue
    place "$f" "$HOME/$(basename "$f")"
  done
fi

if want config; then
  say "config (~/.config)"
  for d in "$here"/config/*; do
    [ -e "$d" ] || continue
    name="$(basename "$d")"
    if is_executable_config "$name/" && [ "$EXEC" != 1 ]; then review+=("config/$name"); continue; fi
    if [ -e "$HOME/.config/$name" ]; then echo "skip (already there, not changed): ~/.config/$name"; else place "$d" "$HOME/.config/$name"; fi
  done
  # autostart and systemd entries may also live under a captured folder's own name; report anything that would run by itself
  if [ "$EXEC" != 1 ]; then
    while IFS= read -r f; do review+=("${f#"$here"/}"); done < <(find "$here/config" "$here/dotfiles" -type f \( -path '*/autostart/*' -o -path '*/systemd/*' \) 2>/dev/null)
  fi
fi

if want settings; then
  say "desktop settings (dconf)"
  if [ -s "$here/dconf.ini" ]; then
    # A custom keyboard shortcut runs a command. Those sections are held back unless --allow-executable.
    safe="$(mktemp)"; trap 'rm -f "$safe"' EXIT
    awk -v allow="$EXEC" -v out="$safe" -v rev="$safe.review" '
      function flush() { if (n > 0) { if (bad && allow != 1) print sect >> rev; else printf "%s", buf >> out } buf=""; n=0; bad=0 }
      /^\[/ { flush(); sect=$0; buf=$0 "\n"; n=1; next }
      { buf = buf $0 "\n"; n++; if ($0 ~ /^(command|binding-command|exec)[[:space:]]*=/) bad=1 }
      END { flush() }' "$here/dconf.ini"
    [ -s "$safe.review" ] && while IFS= read -r s; do review+=("settings $s (runs a command)"); done < "$safe.review"
    if [ "$APPLY" = 1 ]; then dconf load / < "$safe" && echo "settings restored"; else echo "[dry run] dconf load / (with command-running sections removed)"; fi
    rm -f "$safe.review"
  fi
fi

if want sources; then
  say "third-party apt sources (adds software you did not review here)"
  for l in "$here"/apt-sources/*.list "$here"/apt-sources/*.sources; do
    [ -e "$l" ] && run sudo install -m 0644 "$l" "/etc/apt/sources.list.d/$(basename "$l")"
  done
  run sudo apt-get update
fi

say "summary"
if [ ${#review[@]} -gt 0 ]; then
  echo "Held back because they run by themselves. Read them, then re-run with --allow-executable:"
  printf '  - %s\n' "${review[@]}" | sort -u
fi
if [ ${#fail[@]} -gt 0 ]; then printf 'failed: %s\n' "${fail[@]}"; else echo "no failures"; fi
[ "$APPLY" = 1 ] || echo "(dry run: nothing was changed. Re-run with --apply to do it.)"
