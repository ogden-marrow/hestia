#!/usr/bin/env bash
# The dconf filter keeps restorable settings and drops everything that remembers what you did.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
f="$here/packaging/libexec/dconf-filter.sh"
fail() { echo "FAIL: $*" >&2; exit 1; }
in=$(cat <<'EOF'
[org/gnome/desktop/interface]
color-scheme='prefer-dark'
font-name='Ubuntu Sans 11'
recent-files-note='x'

[org/gnome/desktop/background]
picture-uri='file:///usr/share/backgrounds/a.jpg'

[org/gnome/portal/filechooser/org.gnome.Firefox]
last-folder-path='/home/someone/Taxes'

[org/gnome/nautilus/window-state]
initial-size=(100, 100)

[org/gnome/nautilus/preferences]
default-folder-viewer='list-view'

[org/gnome/papers/default]
recent-uris=['file:///home/someone/secret.pdf']

[org/gnome/clocks/state/window]
location='Paris'

[org/gnome/evolution-data-server]
network-monitor-gio-name=''

[org/gnome/desktop/notifications/application/discord]
enable=true

[org/gnome/shell]
favorite-apps=['firefox.desktop']
last-selected-power-profile='balanced'

[org/gnome/settings-daemon/plugins/media-keys]
www=['<Super>b']
EOF
)
out=$(printf '%s\n' "$in" | bash "$f")
for keep in "color-scheme" "picture-uri" "default-folder-viewer" "favorite-apps" "www="; do echo "$out" | grep -q "$keep" || fail "dropped a setting worth restoring: $keep"; done
for gone in "Taxes" "secret.pdf" "Paris" "clocks" "filechooser" "window-state" "papers" "evolution" "discord" "recent-files-note" "last-selected"; do echo "$out" | grep -q "$gone" && fail "kept something private: $gone"; done
[ -z "$(printf '' | bash "$f")" ] || fail "empty input must give empty output"
echo "dconf filter ok"
