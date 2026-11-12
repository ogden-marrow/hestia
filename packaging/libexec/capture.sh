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
ls ~/.cargo/bin 2>/dev/null | grep -Ev '^(cargo|cargo-.*|clippy-driver|rls|rust-.*|rustc|rustdoc|rustfmt|rustup)$' | sort > cargo-bins.txt || : > cargo-bins.txt
docker images --format '{{.Repository}}:{{.Tag}}' 2>/dev/null | sort > docker-images.txt || : > docker-images.txt

# --- apt third-party sources (keys must be re-fetched; see bootstrap.sh) ----
rm -f apt-sources/*.list apt-sources/*.sources
for f in /etc/apt/sources.list.d/*.list; do
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
# points at, so capturing it can never delete it. A capture that would drop most of what was captured before is refused.
repo_root="$(git -C "$dir" rev-parse --show-toplevel 2>/dev/null || echo "$dir")"; repo_root="$(readlink -f "$repo_root")"
new=".capture-new.$$"; rm -rf "$new"; mkdir -p "$new/dotfiles" "$new/config"
trap 'rm -rf "$new"' EXIT
while IFS= read -r line; do
  p="${line%%#*}"; p="$(echo "$p" | xargs)"
  [ -z "$p" ] && continue
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
    *) cp -a ~/"$rel" "$dest" ;;
  esac
done < capture-paths.txt
# The deny-list also applies to what is INSIDE a captured folder (a bookmarks file in ~/.config/gtk-3.0, a token file in an app's
# folder): every captured file is asked about by its home-relative path, and a refused one is dropped from the COPY only. If the
# list cannot be asked, nothing is captured at all.
mapfile -d '' -t captured < <(find "$new" -type f -print0)
inside=()
for f in "${captured[@]}"; do
  case "$f" in "$new/config/"*) inside+=(".config/${f#"$new/config/"}") ;; "$new/dotfiles/"*) inside+=("${f#"$new/dotfiles/"}") ;; esac
done
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
if [ "$before" -ge 4 ] && [ "$((after * 2))" -lt "$before" ]; then
  echo "REFUSED: this capture has $after files where the last had $before; keeping the last one. Check capture-paths.txt." >&2
  exit 1
fi
rm -rf dotfiles config && mv "$new/dotfiles" dotfiles && mv "$new/config" config

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
