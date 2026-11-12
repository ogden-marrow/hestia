#!/usr/bin/env bash
# bootstrap.sh [home-repo] [--apply] [--only steps] [--allow-executable]
# Rebuild this computer from what watch.sh captured into the home repository.
#
#   (default)   DRY RUN: prints every step and changes nothing.
#   --apply     do it.
#   --only a,b  run only these steps. Steps: packages dotfiles config settings sources
#   --allow-executable
#               also restore things that run by themselves or change what runs (shell startup files, .gitconfig,
#               environment.d, autostart entries, systemd user units, custom keyboard-shortcut
#               commands). Without it they are listed for you to read first and left alone.
#
# Safe by design:
#  * It never deletes. A file that is already there and different is moved aside to NAME.bak.<time>, never overwritten.
#  * Restore is file by file from a fixed list of names, with the file modes. ~/.ssh, ~/.gnupg and the keyring are never touched.
#  * Dotfiles and config are COPIED, never linked, so a live file never points into the repository.
#  * Anything that can run code or change what runs (shell startup files, .gitconfig, environment.d, autostart,
#    systemd user units, fish/zsh/bash config) is not restored unless you say so with --allow-executable.
#  * Third-party apt sources (they add software you did not review) only run when named with --only sources.
set -uo pipefail
repo=""; APPLY=0; ONLY=""; EXEC=0
while [ $# -gt 0 ]; do
  case "$1" in
    --apply) APPLY=1 ;;
    --only) ONLY="${2:-}"; shift ;;
    --allow-executable) EXEC=1 ;;
    -h|--help) sed -n '2,11p' "$0"; exit 0 ;;
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

# Runs code by itself, or changes what runs: held back unless --allow-executable. PATH is relative to ~ (dotfiles) or ~/.config.
# A poisoned repository can use any of these to run its code (LD_PRELOAD in environment.d, core.sshCommand in .gitconfig).
is_executable_config() {
  case "$1" in
    .bashrc|.bash_profile|.bash_login|.bash_logout|.profile|.zshrc|.zshenv|.zprofile|.zlogin|.zlogout|.gitconfig) return 0 ;;
    autostart|autostart/*|systemd|systemd/*|environment.d|environment.d/*|fish|fish/*|zsh|zsh/*|bash|bash/*|git|git/*) return 0 ;;
    */autostart/*|*/systemd/*|*/environment.d/*) return 0 ;;
  esac
  return 1
}
# Never restored, with or without --allow-executable: keys and the keyring.
is_denied() { case "$1" in .ssh|.ssh/*|.gnupg|.gnupg/*|gnupg|gnupg/*|*keyring*|gh|gh/*) return 0 ;; esac; return 1; }
# The only top-level names restore will write. Everything else in the repository is ignored and counted as skipped.
is_allowed() {
  case "$1" in
    .nvidia-settings-rc|mimeapps.list|user-dirs.dirs|monitors.xml|gtk-3.0|gtk-4.0|mpv|htop|nautilus|powerlimit|johncast|hestia) return 0 ;;
  esac
  is_executable_config "$1"
}
n_new=0; n_same=0; n_bak=0; n_skip=0
mode_of() { printf '%o' $(( 8#$(stat -c %a -- "$1") & 8#755 )); } # keeps owner bits and private modes, drops setuid/setgid and group/other write

# make DEST (a directory) exist, giving each directory it creates the mode of its source twin
ensure_dir() { # srcdir destdir srcroot
  [ -d "$2" ] && return
  if [ "$1" = "$3" ]; then run mkdir -p -- "$2"; return; fi
  ensure_dir "$(dirname "$1")" "$(dirname "$2")" "$3"
  run mkdir -- "$2"; run chmod "$(mode_of "$1")" -- "$2"
}

# restore one captured file at DEST: copy with its mode; if a different file is there, keep it as NAME.bak.<time> first
place_file() { # src dest srcroot
  local src="$1" dest="$2"
  case "$(realpath -m -- "$dest")" in "$HOME"/*) ;; *) echo "refused (outside your home folder): $dest"; n_skip=$((n_skip+1)); return ;; esac
  if [ -L "$src" ] || [ ! -f "$src" ]; then echo "skipped (not a plain file): $src"; n_skip=$((n_skip+1)); return; fi
  if [ -f "$dest" ] && [ ! -L "$dest" ] && cmp -s "$src" "$dest" 2>/dev/null; then n_same=$((n_same+1)); return; fi
  if [ -e "$dest" ] || [ -L "$dest" ]; then run mv -- "$dest" "$dest.bak.$stamp"; n_bak=$((n_bak+1)); fi
  ensure_dir "$(dirname "$src")" "$(dirname "$dest")" "$3"
  run cp -- "$src" "$dest"; run chmod "$(mode_of "$src")" -- "$dest"
  n_new=$((n_new+1))
}

# restore everything captured under SRC-ROOT/NAME into DEST-ROOT/NAME, file by file
restore_top() { # kind srcroot destroot name
  local kind="$1" root="$2" droot="$3" name="$4" f rel
  if is_denied "$name"; then echo "refused (never restored): $name"; n_skip=$((n_skip+1)); return; fi
  if ! is_allowed "$name"; then echo "skipped (not on the restore list): $name"; n_skip=$((n_skip+1)); return; fi
  while IFS= read -r -d '' f; do
    rel="${f#"$root"/}"
    if is_denied "$rel"; then echo "refused (never restored): $rel"; n_skip=$((n_skip+1)); continue; fi
    if is_executable_config "$rel" && [ "$EXEC" != 1 ]; then review+=("$kind/$rel"); continue; fi
    place_file "$f" "$droot/$rel" "$root"
  done < <(find "$root/$name" \( -type f -o -type l \) -print0 2>/dev/null)
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
    # shellcheck disable=SC2086 # ${flag:-} is "" or "--classic": it must vanish when empty, so it is deliberately unquoted
    run sudo snap install "$name" --channel="${chan:-stable}" ${flag:-} || fail+=("snap:$name")
  done < "$here/snap.txt" 2>/dev/null
  while read -r app origin; do
    [ -n "${app:-}" ] || continue
    run flatpak install -y "${origin:-flathub}" "$app" || fail+=("flatpak:$app")
  done < "$here/flatpak.txt" 2>/dev/null
fi

if want dotfiles; then
  say "dotfiles (copied into your home folder with their modes; a different existing file is kept as NAME.bak.<time>)"
  for f in "$here"/dotfiles/.[!.]*; do
    [ -e "$f" ] || [ -L "$f" ] || continue
    restore_top dotfiles "$here/dotfiles" "$HOME" "$(basename "$f")"
  done
fi

if want config; then
  say "config (~/.config)"
  for d in "$here"/config/*; do
    [ -e "$d" ] || [ -L "$d" ] || continue
    restore_top config "$here/config" "$HOME/.config" "$(basename "$d")"
  done
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
echo "files: $n_new restored, $n_same already identical, $n_bak replaced (old copy kept as .bak), $n_skip skipped"
if [ ${#review[@]} -gt 0 ]; then
  echo "Held back because they run by themselves. Read them, then re-run with --allow-executable:"
  printf '  - %s\n' "${review[@]}" | sort -u
fi
if [ ${#fail[@]} -gt 0 ]; then printf 'failed: %s\n' "${fail[@]}"; else echo "no failures"; fi
[ "$APPLY" = 1 ] || echo "(dry run: nothing was changed. Re-run with --apply to do it.)"
