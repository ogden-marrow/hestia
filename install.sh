#!/usr/bin/env bash
# Installs Hestia for the current user: programs, desktop entry, icons, the GNOME Shell extension and
# the systemd user timers. Works from a release tarball (bin/ next to this script) or a source checkout.
#
#   HESTIA_HOME=~/Home ./install.sh      # the first home repository setting (default ~/Home); after that the app owns it
#   PREFIX=~/.local ./install.sh         # programs go to $PREFIX/bin (default ~/.local)
#
# Every build gets its own folder, $PREFIX/lib/hestia/builds/<date>-<build id>/ (programs and scripts); $PREFIX/bin/* and
# $PREFIX/libexec/hestia are links through $PREFIX/lib/hestia/current, so switching builds is one atomic rename and the build
# before is kept for rollback (`lifestream update rollback`). A plain ./install.sh runs the three steps below in order;
# `lifestream update apply` (the app's Update button) runs them one at a time, so it can show progress and stop before
# anything installed changes:
#   ./install.sh --stage           build (or take bin/) into a NEW build folder and print its path; nothing installed changes
#   ./install.sh --smoke DIR       check that the programs in that folder start
#   ./install.sh --activate DIR    make it the installed build: desktop entry, icons, extension, links, units, install.json
set -euo pipefail
shopt -s inherit_errexit
here="$(cd "$(dirname "$0")" && pwd)"
prefix="${PREFIX:-$HOME/.local}"
home_repo="${HESTIA_HOME:-$HOME/Home}"
share="${XDG_DATA_HOME:-$HOME/.local/share}"
pk="$here/packaging"
builds="$prefix/lib/hestia/builds"
mode="${1:-all}"; build=""
case "$mode" in
  all|--stage) ;;
  --smoke|--activate) build="${2:?usage: install.sh $mode <build folder>}" ;;
  -h|--help) sed -n '2,15p' "$0"; exit 0 ;;
  *) echo "install.sh: unknown option '$mode' (see --help)" >&2; exit 2 ;;
esac

# These paths are written into systemd units and a shell command. Refuse characters that could change what a unit
# does (quotes, backslash, $, %, newlines, spaces) rather than try to escape them.
for v in "$prefix" "$home_repo" "$here" ${build:+"$build"}; do
  case "$v" in
    *[!A-Za-z0-9_./~+@:,=-]*) echo "install.sh: '$v' has a character that is not safe to put in a systemd unit (letters, digits and _ . / ~ + @ : , = - only). Choose another place." >&2; exit 1 ;;
  esac
done
if [ -n "$build" ]; then
  ok=0
  case "$build" in "$builds"/*/*|"$builds"/.*|*/..|*/../*) ;; "$builds"/*) [ -x "$build/bin/lifestream" ] && ok=1 ;; esac
  [ "$ok" = 1 ] || { echo "install.sh: '$build' is not a build folder under $builds" >&2; exit 1; }
fi

# ---- stage: a new build folder. stdout is only its path (cargo talks on stderr).
stage() {
  local id source dir target
  if [ -e "$here/.git" ]; then
    # the same build id the app shows (ui/build.rs): the commit, and +local when tracked files have changes of their own
    source=checkout; id="$(git -C "$here" rev-parse --short=8 HEAD)"
    [ -z "$(git -C "$here" status --porcelain --untracked-files=no)" ] || id="$id+local"
  else
    source=release; id="$(tr -cd 'A-Za-z0-9.+-' < "$here/BUILD" 2>/dev/null || true)"; id="${id:-unknown}"
  fi
  dir="$builds/$(date +%Y%m%d-%H%M%S)-$id"; [ ! -e "$dir" ] || dir="$dir-$$"
  staging="$dir.partial"; rm -rf "$staging"; mkdir -p "$staging/bin"
  trap 'rm -rf "$staging"' EXIT   # a build that fails leaves nothing behind
  if [ -d "$here/bin" ]; then
    install -m755 "$here"/bin/{hestia,hestia-tray,lifestream} "$staging/bin/"
  else
    (cd "$here" && cargo build --release --locked) >&2
    target="${CARGO_TARGET_DIR:-$here/target}"; case "$target" in /*) ;; *) target="$here/$target" ;; esac
    install -m755 "$target"/release/{hestia,hestia-tray,lifestream} "$staging/bin/"
  fi
  # the machine capture scripts ship with Hestia, never from the home repository; the unit templates go beside them
  cp -r "$pk/libexec" "$staging/libexec" && chmod +x "$staging"/libexec/*.sh
  cp -r "$pk/systemd" "$staging/libexec/systemd"
  printf '{"build_id":"%s","source":"%s","source_dir":"%s"}\n' "$id" "$source" "$here" > "$staging/build-info.json"
  mv "$staging" "$dir"; trap - EXIT
  echo "$dir"
}

# ---- smoke: do the new programs start? (hestia --list-scenes needs no display)
smoke() {
  local v
  v="$(timeout 30 "$1/bin/lifestream" --version 2>/dev/null)" && [ "${v%% *}" = lifestream ] || { echo "install.sh: the new lifestream does not start" >&2; return 1; }
  timeout 60 "$1/bin/hestia" --list-scenes >/dev/null 2>&1 || { echo "install.sh: the new hestia does not start" >&2; return 1; }
}

# ---- activate: what lives outside the build folders, then the switch (links, install.json, units) by the new build's own engine
activate() {
  # desktop entry, metainfo, icons
  mkdir -p "$share/applications" "$share/metainfo" "$share/icons/hicolor/scalable/apps" "$share/icons/hicolor/symbolic/apps"
  cp "$pk/desktop/app.hestia.Hestia.metainfo.xml" "$share/metainfo/"
  sed -e "s|@HOME@|$HOME|g" -e "s|\.local/bin|${prefix#"$HOME"/}/bin|g" "$pk/desktop/app.hestia.Hestia.desktop" > "$share/applications/app.hestia.Hestia.desktop"
  cp "$pk/desktop/app.hestia.Hestia.svg" "$share/icons/hicolor/scalable/apps/"
  cp "$pk/desktop/app.hestia.Hestia-symbolic.svg" "$share/icons/hicolor/symbolic/apps/"
  # sized PNG copies beside the SVG, for shells that prefer a raster at a fixed size (optional: needs python3-gi)
  python3 - "$pk/desktop/app.hestia.Hestia.svg" "$share" <<'PY' 2>/dev/null || true
import sys, os, gi
gi.require_version('GdkPixbuf', '2.0')
from gi.repository import GdkPixbuf
for n in (48, 64, 128, 256, 512):
    dest = os.path.join(sys.argv[2], f"icons/hicolor/{n}x{n}/apps")
    os.makedirs(dest, exist_ok=True)
    GdkPixbuf.Pixbuf.new_from_file_at_size(sys.argv[1], n, n).savev(os.path.join(dest, "app.hestia.Hestia.png"), "png", [], [])
PY
  gtk-update-icon-cache -q -f -t "$share/icons/hicolor" 2>/dev/null || true
  update-desktop-database "$share/applications" 2>/dev/null || true

  # the launcher and clipboard recorder: one GNOME Shell extension (Hestia's welcome tour turns it on)
  rm -rf "$share/gnome-shell/extensions/hestia-clipboard@hestia.app"
  mkdir -p "$share/gnome-shell/extensions"
  cp -r "$pk/gnome-extension/hestia-clipboard@hestia.app" "$share/gnome-shell/extensions/"
  glib-compile-schemas "$share/gnome-shell/extensions/hestia-clipboard@hestia.app/schemas" 2>/dev/null || echo "note: glib-compile-schemas not found; install libglib2.0-bin" >&2

  # the links, install.json, and the per-person part (the home repository setting, the systemd user units and enabling them):
  # setup-user.sh, which a .deb install runs too (`hestia-setup`); the switch runs the new build's copy after moving the links
  HESTIA_HOME="$home_repo" "$1/bin/lifestream" update switch --prefix "$prefix" "$1"
}

case "$mode" in
  --stage) stage ;;
  --smoke) smoke "$build" ;;
  --activate) activate "$build" ;;
  all)
    d="$(stage)"
    smoke "$d" || { rm -rf "$d"; echo "install.sh: nothing was changed; the installed Hestia is the one you had" >&2; exit 1; }
    activate "$d"
    echo "Hestia is installed. Log out and in once so the launcher takes effect." ;;
esac
