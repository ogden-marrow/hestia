#!/usr/bin/env bash
# Render Hestia's own headless snapshots across the look matrix. Never captures the screen.
#   ui/scripts/snap-matrix.sh OUT_DIR [panes] [looks] [sizes]
# Defaults: every pane x {dark,light} x {1.0,1.5 text} x {960x640,1440x900}.
# Snapshots render the real data on this machine, so keep OUT_DIR outside the repository.
set -euo pipefail
out=${1:?usage: snap-matrix.sh OUT_DIR [panes] [looks] [sizes]}
panes=${2:-"welcome welcome-home welcome-key today questions time files history machine settings settings-general settings-scanning ask"}
looks=${3:-"dark light dark,1.5 light,1.5 dark,orange,airy"}
sizes=${4:-"960x640 1440x900"}
bin=${HESTIA_BIN:-target/debug/hestia}
mkdir -p "$out"
for p in $panes; do for l in $looks; do for z in $sizes; do
  HESTIA_LOOK="$l" timeout 90 "$bin" --snap "$out/$p--${l//,/_}--$z.png" --scene "$p" --size "$z" >/dev/null 2>&1 \
    || echo "failed: $p $l $z" >&2
done; done; done
echo "wrote $(ls "$out" | wc -l) snapshots to $out"
