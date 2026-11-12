#!/usr/bin/env bash
# build-release.sh <version> <out-dir> — turn the programs in target/release into the files a release publishes:
#   hestia-linux-x86_64.tar.gz (+ .sha256)   programs, install.sh, packaging, licence: installs without a compiler
#   hestia_<version>_amd64.deb (+ .sha256)   the same for apt
# The release workflow runs this, and tests/release-rehearsal.sh runs the same script, so what is rehearsed is what ships.
# Build first: cargo build --release --locked
set -euo pipefail
version="${1:?usage: build-release.sh <version> <out-dir>}"; out="$(mkdir -p "${2:?usage: build-release.sh <version> <out-dir>}" && cd "$2" && pwd)"
here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
for b in hestia hestia-tray lifestream; do [ -x "$here/target/release/$b" ] || { echo "build-release: $b is not built (cargo build --release --locked)" >&2; exit 1; }; done
work="$(mktemp -d)"; trap 'rm -rf "$work"' EXIT

# ---- tarball
name=hestia-linux-x86_64
mkdir -p "$work/$name/bin"
cp "$here"/target/release/{hestia,hestia-tray,lifestream} "$work/$name/bin/"
cp -r "$here/packaging" "$here/install.sh" "$here/uninstall.sh" "$here/LICENSE" "$here/README.md" "$work/$name/"
for f in CHANGELOG.md THIRD_PARTY.md; do [ -f "$here/$f" ] && cp "$here/$f" "$work/$name/"; done
tar --sort=name --owner=0 --group=0 --numeric-owner -czf "$out/$name.tar.gz" -C "$work" "$name"
( cd "$out" && sha256sum "$name.tar.gz" > "$name.tar.gz.sha256" )

# ---- .deb
d="$work/hestia_${version}_amd64"
install -Dm755 "$here"/target/release/hestia "$here"/target/release/hestia-tray "$here"/target/release/lifestream -t "$d/usr/bin"
install -Dm644 "$here/packaging/desktop/app.hestia.Hestia.svg" -t "$d/usr/share/icons/hicolor/scalable/apps"
install -Dm644 "$here/packaging/desktop/app.hestia.Hestia-symbolic.svg" -t "$d/usr/share/icons/hicolor/symbolic/apps"
install -Dm644 "$here/packaging/desktop/app.hestia.Hestia.metainfo.xml" -t "$d/usr/share/metainfo"
sed 's|@HOME@/.local/bin/||g' "$here/packaging/desktop/app.hestia.Hestia.desktop" | sed 's|^Icon=.*|Icon=app.hestia.Hestia|;/^TryExec/d' > "$work/app.desktop"
install -Dm644 "$work/app.desktop" "$d/usr/share/applications/app.hestia.Hestia.desktop"
mkdir -p "$d/usr/share/gnome-shell/extensions"
cp -r "$here/packaging/gnome-extension/hestia-clipboard@hestia.app" "$d/usr/share/gnome-shell/extensions/"
glib-compile-schemas "$d/usr/share/gnome-shell/extensions/hestia-clipboard@hestia.app/schemas"
mkdir -p "$d/DEBIAN"
printf 'Package: hestia\nVersion: %s\nSection: utils\nPriority: optional\nArchitecture: amd64\nMaintainer: Hestia authors <noreply@example.invalid>\nDescription: The hearth of your computer: files, history and time\n' "$version" > "$d/DEBIAN/control"
dpkg-deb --build --root-owner-group "$d" "$out/hestia_${version}_amd64.deb" >/dev/null
( cd "$out" && sha256sum "hestia_${version}_amd64.deb" > "hestia_${version}_amd64.deb.sha256" )
ls -1 "$out"
