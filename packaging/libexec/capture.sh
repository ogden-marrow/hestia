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
# timestamps change by themselves and say nothing about your settings
{ dconf dump / 2>/dev/null || true; } | { grep -vE '(^|-)timestamp=' || true; } > dconf.ini

# --- dotfiles and config, from capture-paths.txt --------------------------
# Each line is a path under ~. ~/.config/X goes to config/X, other ~/X to dotfiles/X.
# Secret-looking paths are refused even if listed.
rm -rf dotfiles config && mkdir -p dotfiles config
while IFS= read -r line; do
  p="${line%%#*}"; p="$(echo "$p" | xargs)"
  [ -z "$p" ] && continue
  case "$p" in "~/"*) rel="${p#\~/}" ;; *) echo "skip (not under ~): $p" >&2; continue ;; esac
  case "$rel" in
    .ssh*|.gnupg*|.config/gh*|.config/Claude*|.config/helium*|.config/goa-1.0*|.config/evolution*|*..*|*password*|*secret*|*token*|*.pem|*.key|*id_rsa*|*id_ed25519*)
      echo "REFUSED (secret-looking): $p" >&2; continue ;;
  esac
  [ -e ~/"$rel" ] || continue
  case "$rel" in .config/*) dest="config/${rel#.config/}" ;; *) dest="dotfiles/$rel" ;; esac
  mkdir -p "$(dirname "$dest")"
  cp -a ~/"$rel" "$dest"
done < capture-paths.txt

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
