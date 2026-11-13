#!/usr/bin/env bash
# capture.sh <capture-dir> — snapshot this machine's state into a folder of the home repository.
# Safe to re-run; it overwrites the generated files. Review `git diff` after.
# It copies ONLY an allowlist of config. Secrets (ssh keys, gpg, tokens,
# keyrings, browser profiles, mail, online accounts) are never copied.
set -euo pipefail
dir="${1:?usage: capture.sh <capture-dir>}"
cd "$dir"

echo "capture: $(. /etc/os-release && echo "$PRETTY_NAME")"

# --- packages -------------------------------------------------------------
apt-mark showmanual | sort > apt-manual.txt

# Snap apps only; skip bases/runtimes that snapd pulls in automatically.
snap list 2>/dev/null | awk 'NR>1' | while read -r name _ver _rev track _pub notes; do
  case "$name" in
    bare|core*|snapd|gnome-*|gtk-common-themes|mesa-*|ffmpeg-*|gaming-graphics-*|prompting-client|desktop-security-center) continue ;;
  esac
  flag=""; [[ "$notes" == *classic* ]] && flag=" --classic"
  echo "$name ${track%%/*}$flag"
done | sort > snap.txt

flatpak list --app --columns=application,origin 2>/dev/null | sort > flatpak.txt || : > flatpak.txt
{ find ~/.cargo/bin -mindepth 1 -maxdepth 1 -printf '%f\n' 2>/dev/null || true; } | grep -Ev '^(cargo|cargo-.*|clippy-driver|rls|rust-.*|rustc|rustdoc|rustfmt|rustup)$' | sort > cargo-bins.txt || : > cargo-bins.txt
docker images --format '{{.Repository}}:{{.Tag}}' 2>/dev/null | sort > docker-images.txt || : > docker-images.txt

# --- apt third-party sources (keys must be re-fetched; see bootstrap.sh) ----
# Both formats: one-line *.list and deb822 *.sources. ubuntu.sources / debian.sources are the distribution's own and
# belong to the release, so they are not captured (restoring one onto another release would break apt).
rm -f apt-sources/*.list apt-sources/*.sources
srcdir="${HESTIA_APT_SOURCES_DIR:-/etc/apt/sources.list.d}"
for f in "$srcdir"/*.list "$srcdir"/*.sources; do
  case "${f##*/}" in ubuntu.sources|debian.sources) continue ;; esac
  [ -e "$f" ] && cp "$f" apt-sources/
done

# --- GNOME / dconf ---------------------------------------------------------
# Only an allowlist of sections is kept (see dconf-filter.sh): `dconf dump /` also holds recent documents,
# last-used folders, clock locations and account data, which are private and not settings.
# timestamps change by themselves and say nothing about your settings
libexec="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
{ dconf dump / 2>/dev/null || true; } | { grep -vE '(^|-)timestamp=' || true; } | "$libexec/dconf-filter.sh" > dconf.ini

# --- dotfiles and config, from capture-paths.txt --------------------------
# Each line is a path under ~. ~/.config/X goes to config/X, other ~/X to dotfiles/X.
# Secret-looking paths are refused even if listed.
# Built beside the old copy and swapped in only when it is complete, so a failure never leaves the repository
# with files missing. A live file that resolves into this repository (a bootstrap symlink) is copied from what it
# points at, so capturing it can never delete it. A capture that would drop most of what was captured before is refused,
# unless the capture list itself changed or you say it is fine (touch the file the message names).
# A file that cannot be read, or is bigger than HESTIA_MAX_FILE_BYTES (50 MB: GitHub refuses 100 MB), is skipped, not fatal,
# and is listed in SKIPPED.txt so it is never silently missing.
repo_root="$(git -C "$dir" rev-parse --show-toplevel 2>/dev/null || echo "$dir")"; repo_root="$(readlink -f "$repo_root")"
new=".capture-new.$$"; rm -rf "$new"; mkdir -p "$new/dotfiles" "$new/config"
skipped="$new.skipped"; : > "$skipped"
trap 'rm -rf "$new" "$skipped" "$new.err"' EXIT
home_rel() { case "$1" in "$new/config/"*) printf '.config/%s' "${1#"$new/config/"}" ;; "$new/dotfiles/"*) printf '%s' "${1#"$new/dotfiles/"}" ;; esac; }
# A symlink target is captured only when it is a real file inside the home folder that the deny-list allows.
home_real="$(readlink -f ~)"
link_ok() { # <resolved path>
  case "$1" in "$home_real"/*) ;; *) return 1 ;; esac
  [ -e "$1" ] || return 1
  "${LIFESTREAM:-lifestream}" deny-path "${1#"$home_real"/}" >/dev/null 2>&1
}
fix_links() { # <copied tree> <where it came from>: replace each symlink in the copy by its target's content, or drop and list it
  local tree="$1" orig="$2" l r t
  while IFS= read -r -d '' l; do
    r="${l#"$tree"}"; t="$(readlink -f "$orig$r" 2>/dev/null || true)"
    rm -f -- "$l"
    if [ -n "$t" ] && [ -f "$t" ] && link_ok "$t"; then cp -p -- "$t" "$l"
    else printf 'symlink not captured (target is not a file inside the home folder, or never captured): ~/%s\n' "$(home_rel "$l" | tr '\n' '?')" >> "$skipped"; fi
  done < <(find "$tree" -type l -print0)
}
while IFS= read -r line; do
  # a # starts a comment only at the start of a line or after a space: a path may hold one. Quotes are ordinary characters.
  p="${line%%[[:space:]]#*}"; p="${p#"${p%%[![:space:]]*}"}"; p="${p%"${p##*[![:space:]]}"}"
  case "$p" in ""|"#"*) continue ;; esac
  # shellcheck disable=SC2088 # a literal "~/" is what the capture list contains
  case "$p" in "~/"*) rel="${p#\~/}" ;; *) echo "skip (not under ~): $p" >&2; continue ;; esac
  # one deny-list for everything (lifestream deny-path); if it cannot be asked, nothing is captured from home
  if ! "${LIFESTREAM:-lifestream}" deny-path "$rel" >/dev/null 2>&1; then
    echo "REFUSED (never captured): $p" >&2; continue
  fi
  [ -e ~/"$rel" ] || continue
  case "$rel" in .config/*) dest="$new/config/${rel#.config/}" ;; *) dest="$new/dotfiles/$rel" ;; esac
  mkdir -p "$(dirname "$dest")"
  real="$(readlink -f ~/"$rel")"
  case "$real" in
    "$repo_root"/*) cp -aL "$real" "$dest" ;;  # symlinked into this repository: copy what it points at
    *) if [ -L ~/"$rel" ]; then
         # a symlinked dotfile (a stow layout): capture what it points at, never the link. The target must be inside the home
         # folder and pass the deny-list; otherwise it is skipped and listed.
         if link_ok "$real"; then
           stage="$new.link"; rm -rf "$stage"; mkdir -p "$stage"
           tar --exclude=.git -C "$(dirname "$real")" -cf - -- "$(basename "$real")" 2>/dev/null | tar -C "$stage" -xpf - \
             && mv "$stage/$(basename "$real")" "$dest"
           rm -rf "$stage"; [ -d "$dest" ] && fix_links "$dest" "$real"
         else
           printf 'symlink not captured (target outside the home folder or never captured): ~/%s\n' "$rel" >> "$skipped"
         fi
         continue
       fi
       # tar, not cp: it carries on past a file it cannot read (cp stops at the first), and says which.
       # --exclude=.git: a nested git repository (a cloned ~/.config/nvim) is stored as its files, never as a gitlink.
       pre="${rel%/*}/"; [ "$pre" = "$rel/" ] && pre=""
       tar --exclude=.git -C "$HOME/$pre" -cf - -- "${rel##*/}" 2>"$new.err" | tar -C "$(dirname "$dest")" -xpf - || {
         [ "${PIPESTATUS[1]}" = 0 ] || exit 1
         grep -v '^tar: Exiting with failure' "$new.err" | while IFS= read -r e; do
           e="${e#tar: }"; printf '%s\n' "${e//$'\n'/?}" | sed -E "s#^(.*): Cannot [a-z ]+: (.*)\$#unreadable (\2): ~/$pre\1#" >> "$skipped"
         done; }
       ;;
  esac
  # links INSIDE a captured folder are resolved the same way
  [ -d "$dest" ] && fix_links "$dest" ~/"$rel"
done < capture-paths.txt
# one big file must not wedge every later push: it is left out and listed
max="${HESTIA_MAX_FILE_BYTES:-52428800}"
while IFS= read -r -d '' f; do
  rm -f -- "$f"; printf 'too big (over %s MB): ~/%s\n' "$((max / 1048576))" "$(home_rel "$f" | tr '\n' '?')" >> "$skipped"
done < <(find "$new" -type f -size +"${max}"c -print0)
# The deny-list also applies to what is INSIDE a captured folder (a bookmarks file in ~/.config/gtk-3.0, a token file in an app's
# folder): every captured file is asked about by its home-relative path, and a refused one is dropped from the COPY only. If the
# list cannot be asked, nothing is captured at all.
mapfile -d '' -t captured < <(find "$new" -type f -print0)
inside=()
for f in "${captured[@]}"; do inside+=("$(home_rel "$f")"); done
if [ "${#inside[@]}" -gt 0 ]; then
  rc=0; refused=$("${LIFESTREAM:-lifestream}" deny-path "${inside[@]}" 2>/dev/null) || rc=$?   # exit 1 means "something was refused": expected
  [ "$rc" -le 1 ] || { echo "cannot ask the deny-list (lifestream deny-path): nothing captured" >&2; exit 1; }
  while IFS= read -r h; do
    [ -n "$h" ] || continue
    case "$h" in .config/*) rm -f -- "$new/config/${h#.config/}" ;; *) rm -f -- "$new/dotfiles/$h" ;; esac
    echo "REFUSED inside a captured folder (never captured): ~/$h" >&2
  done <<< "$refused"
fi
before=$( { find dotfiles config -type f 2>/dev/null || true; } | wc -l); after=$( { find "$new/dotfiles" "$new/config" -type f 2>/dev/null || true; } | wc -l)
accept=".accept-smaller-capture"
if [ "$before" -ge 4 ] && [ "$((after * 2))" -lt "$before" ] && [ ! -e "$accept" ] \
   && git -C "$dir" diff --quiet HEAD -- capture-paths.txt 2>/dev/null && git -C "$dir" ls-files --error-unmatch capture-paths.txt >/dev/null 2>&1; then
  echo "REFUSED: this capture has $after files where the last had $before; keeping the last one. If that is right, run: touch '$dir/$accept'" >&2
  exit 1
fi
rm -f "$accept"
rm -rf dotfiles config && mv "$new/dotfiles" dotfiles && mv "$new/config" config
if [ -s "$skipped" ]; then sort -u "$skipped" > SKIPPED.txt; else rm -f SKIPPED.txt; fi

# --- identity & hardware notes --------------------------------------------
{
  echo "os: $(. /etc/os-release && echo "$PRETTY_NAME")"
  echo "kernel: $(uname -r)"
  echo "desktop: ${XDG_CURRENT_DESKTOP:-?} / ${XDG_SESSION_TYPE:-?}"
  echo "hardware: $(hostnamectl 2>/dev/null | awk -F': *' '/Hardware (Vendor|Model)/{printf "%s ", $2}')"
  echo "nvidia: $(nvidia-smi --query-gpu=driver_version --format=csv,noheader 2>/dev/null | head -1)"
} > MACHINE.txt

# Secrets: this script copies, it does not judge. watch.sh runs the one shared scanner (lifestream secret-scan)
# over everything copied before anything is committed, and withholds any file that looks sensitive.

echo "done. $(wc -l < apt-manual.txt) apt, $(wc -l < snap.txt) snap, $(wc -l < flatpak.txt) flatpak, $(wc -l < dconf.ini) dconf lines"
