#!/usr/bin/env bash
# A release, rehearsed: build the release files with the SAME script the release workflow runs, verify their checksums,
# install from the tarball onto a computer that has no compiler, and look inside the .deb. Scratch HOME with stand-ins
# for every system command: it never touches a real session. (Build first: cargo build --release --locked.)
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
T="$(mktemp -d)"; trap 'rm -rf "$T"' EXIT
fail() { echo "FAIL: $*" >&2; exit 1; }
out="$T/out"; bash "$here/packaging/build-release.sh" 0.0.0-rehearsal "$out" >/dev/null || fail "the release script failed"
for f in hestia-linux-x86_64.tar.gz hestia-linux-x86_64.tar.gz.sha256 hestia_0.0.0-rehearsal_amd64.deb hestia_0.0.0-rehearsal_amd64.deb.sha256; do [ -s "$out/$f" ] || fail "$f was not produced"; done
( cd "$out" && sha256sum -c ./*.sha256 >/dev/null ) || fail "a checksum does not match its file"
# a flipped byte must be caught
cp "$out/hestia-linux-x86_64.tar.gz" "$T/bad.tgz"; printf 'x' >> "$T/bad.tgz"
[ "$(sha256sum "$T/bad.tgz" | cut -d' ' -f1)" != "$(cut -d' ' -f1 "$out/hestia-linux-x86_64.tar.gz.sha256")" ] || fail "a modified file still matches its checksum"

# the tarball: what a person who downloads it gets
mkdir "$T/x"; tar -xzf "$out/hestia-linux-x86_64.tar.gz" -C "$T/x"; pkg="$T/x/hestia-linux-x86_64"
for f in bin/hestia bin/hestia-tray bin/lifestream install.sh uninstall.sh LICENSE README.md packaging/libexec/watch.sh packaging/systemd/hestia-watch.timer; do [ -e "$pkg/$f" ] || fail "the tarball lacks $f"; done
for b in hestia hestia-tray lifestream; do [ -x "$pkg/bin/$b" ] || fail "bin/$b is not executable"; done
find "$pkg" -type f \( -name '*.rs' -o -name Cargo.toml \) | grep -q . && fail "the tarball carries source it does not need"

# install from the tarball on a computer with NO compiler: PATH holds only stand-ins and the system's own tools
export HOME="$T/home" XDG_DATA_HOME="$T/home/.local/share" PREFIX="$T/home/.local" HESTIA_HOME="$T/home/Home"; mkdir -p "$HOME" "$T/bin"
for c in systemctl pkill gsettings gtk-update-icon-cache update-desktop-database; do printf '#!/bin/sh\necho "%s $*" >> "%s/calls.log"\n' "$c" "$T" > "$T/bin/$c"; chmod +x "$T/bin/$c"; done
for c in bash sh env sed grep cp mv rm mkdir cat printf install chmod find sort tr cut head tail dirname basename readlink ln date python3 glib-compile-schemas tar mktemp ls; do p="$(command -v "$c" 2>/dev/null || true)"; [ -n "$p" ] && ln -sf "$p" "$T/bin/$c"; done
PATH="$T/bin" command -v cargo >/dev/null 2>&1 && fail "the rehearsal machine has a compiler, so it proves nothing"
( cd "$pkg" && PATH="$T/bin" bash ./install.sh >"$T/install.log" 2>&1 ) || { cat "$T/install.log" >&2; fail "install.sh from the tarball failed without a compiler"; }
for b in hestia hestia-tray lifestream; do [ -x "$PREFIX/bin/$b" ] || fail "$b was not installed from the tarball"; done
"$PREFIX/bin/lifestream" --version >/dev/null || fail "the installed lifestream does not run"
grep -q "ExecStart=$PREFIX/libexec/hestia/watch.sh" "$HOME/.config/systemd/user/hestia-watch.service" || fail "the installed watch unit is wrong"
( cd "$pkg" && PATH="$T/bin" bash ./uninstall.sh >/dev/null ) || fail "uninstall from the tarball failed"
[ ! -e "$PREFIX/bin/lifestream" ] || fail "uninstall left a program behind"

# the .deb: what apt would install
deb="$out/hestia_0.0.0-rehearsal_amd64.deb"
dpkg-deb --info "$deb" | grep -q "Package: hestia" || fail "the .deb has no package name"
dpkg-deb --info "$deb" | grep -q "Version: 0.0.0-rehearsal" || fail "the .deb has the wrong version"
list="$(dpkg-deb --contents "$deb")"
for f in usr/bin/hestia usr/bin/hestia-tray usr/bin/lifestream usr/share/applications/app.hestia.Hestia.desktop usr/share/icons/hicolor/scalable/apps/app.hestia.Hestia.svg usr/share/metainfo/app.hestia.Hestia.metainfo.xml usr/share/gnome-shell/extensions/hestia-clipboard@hestia.app/extension.js; do echo "$list" | grep -q "$f" || fail "the .deb lacks $f"; done
dpkg-deb --fsys-tarfile "$deb" | tar -xO ./usr/share/applications/app.hestia.Hestia.desktop | grep -q '@HOME@' && fail "the .deb's desktop entry has an unfilled placeholder"
dpkg-deb --fsys-tarfile "$deb" | tar -xO ./usr/share/applications/app.hestia.Hestia.desktop | grep -q '^Icon=app.hestia.Hestia$' || fail "the .deb's desktop entry has the wrong icon"
echo "release rehearsal ok"
