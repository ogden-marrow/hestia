#!/usr/bin/env bash
# Updating Hestia from the checkout it was installed from (#265): `lifestream update check`, `apply` and `rollback`, in a
# scratch HOME against a local bare repository standing in for the public origin. It never touches the real HOME, the real
# checkout or any home repository: every path below is inside one scratch folder, and the test refuses to run otherwise.
# The build is stubbed to keep CI fast: each commit of the stand-in repository carries bin/ (the way a release does), whose
# lifestream runs the engine under test and whose hestia can be made to fail its start-up check. (Build first:
# cargo build --release --locked. tests/release-rehearsal.sh covers real programs through the same install, smoke and rollback.)
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
engine="$here/target/release/lifestream"
[ -x "$engine" ] || { echo "build first: cargo build --release --locked" >&2; exit 2; }
T="$(mktemp -d)"; trap 'rm -rf "$T"' EXIT
export HOME="$T/home" XDG_DATA_HOME="$T/home/.local/share" XDG_CONFIG_HOME="$T/home/.config" XDG_RUNTIME_DIR="$T/run" PREFIX="$T/home/.local" HESTIA_HOME="$T/home/Home"
case "$HOME" in "$T"/*) ;; *) echo "refusing to run: HOME is not inside the scratch folder" >&2; exit 2 ;; esac
mkdir -p "$HOME" "$T/bin" "$XDG_RUNTIME_DIR"
fail() { echo "FAIL: $*" >&2; exit 1; }
for c in systemctl pkill gsettings gtk-update-icon-cache update-desktop-database notify-send; do printf '#!/bin/sh\necho "%s $*" >> "%s/calls.log"\n' "$c" "$T" > "$T/bin/$c"; done
chmod +x "$T"/bin/*
clean_path="$(printf '%s' "$PATH" | tr ':' '\n' | grep -v '/\.local/bin$' | paste -sd:)"
export PATH="$T/bin:$clean_path"
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t.t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t.t
git config --global init.defaultBranch main
LS="$PREFIX/bin/lifestream"
field() { printf '%s' "$1" | python3 -c 'import json,sys; v=json.load(sys.stdin)[sys.argv[1]]; print(json.dumps(v) if not isinstance(v,str) else v)' "$2"; }
id() { git -C "$1" rev-parse --short=8 HEAD; }
builds() { find "$PREFIX/lib/hestia/builds" -mindepth 1 -maxdepth 1 | wc -l; }
build_of() { readlink -f "$PREFIX/lib/hestia/$1" | sed 's/.*-//'; }   # the build id at the end of the folder the link names
installed() { field "$(cat "$XDG_DATA_HOME/hestia/install.json")" build_id; }

# ---- the stand-in public repository: install.sh, packaging and a CHANGELOG, plus a prebuilt bin/ instead of a build
dev="$T/dev"; mkdir -p "$dev/bin"
cp -r "$here/install.sh" "$here/uninstall.sh" "$here/packaging" "$dev/"
printf '#!/bin/sh\nexec "%s" "$@"\n' "$engine" > "$dev/bin/lifestream"
printf '#!/bin/sh\nexit 0\n' > "$dev/bin/hestia"; cp "$dev/bin/hestia" "$dev/bin/hestia-tray"; chmod +x "$dev"/bin/*
printf '# Changelog\n\n## Unreleased\n\n### Added\n- **The first thing** (#1): it starts.\n' > "$dev/CHANGELOG.md"
git -C "$dev" init -q && git -C "$dev" add -A && git -C "$dev" commit -qm A
git init -q --bare "$T/origin.git"; git -C "$dev" push -q "$T/origin.git" main
git clone -q "$T/origin.git" "$T/checkout"; src="$T/checkout"
case "$src" in "$T"/*) ;; *) fail "the checkout is outside the scratch folder" ;; esac
export HESTIA_UPDATE_ORIGIN="$T/origin.git"   # the test stand-in for the public repository's address
push() { git -C "$dev" add -A && git -C "$dev" commit -qm "$1" && git -C "$dev" push -q "$T/origin.git" main; }

bash "$src/install.sh" >"$T/install.log" 2>&1 || { cat "$T/install.log" >&2; fail "install from the checkout failed"; }
A="$(id "$src")"
[ "$(installed)" = "$A" ] || fail "install.json does not record the installed build"

# 1. nothing new: not newer, no changes, no error
out="$("$LS" update check)"
[ "$(field "$out" newer)" = false ] && [ "$(field "$out" available)" = "$A" ] && [ "$(field "$out" installed)" = "$A" ] && [ "$(field "$out" error)" = null ] && [ "$(field "$out" source)" = checkout ] || fail "check with nothing new: $out"
[ "$(field "$out" changes)" = "[]" ] || fail "check with nothing new lists changes: $out"
date -d "$(field "$out" checked_at)" >/dev/null 2>&1 || fail "checked_at is not a time: $out"

# 2. a newer build on origin: newer, with the CHANGELOG lines in between, plain and without hashes; the checkout is untouched
printf -- '- **Something new** (#999): it does a thing. And then more words.\n' >> "$dev/CHANGELOG.md"; push B
B="$(id "$dev")"
out="$("$LS" update check)"
[ "$(field "$out" newer)" = true ] && [ "$(field "$out" available)" = "$B" ] || fail "check did not see the newer build: $out"
[ "$(field "$out" changes)" = '["Something new: it does a thing."]' ] || fail "check's changes are not the plain CHANGELOG lines: $out"
case "$(field "$out" changes)" in *"$B"*|*"$A"*) fail "a commit hash is in the changes: $out" ;; esac
[ "$(id "$src")" = "$A" ] && [ -z "$(git -C "$src" status --porcelain)" ] || fail "check changed the checkout's working tree"

# 3. offline: a plain sentence, nothing available, not newer
git -C "$src" remote set-url origin "$T/nowhere.git"
out="$(HESTIA_UPDATE_ORIGIN="$T/nowhere.git" "$LS" update check)"
[ "$(field "$out" available)" = null ] && [ "$(field "$out" newer)" = false ] || fail "offline check: $out"
case "$(field "$out" error)" in "Could not reach the Hestia repository"*) ;; *) fail "offline check has no plain error: $out" ;; esac
git -C "$src" remote set-url origin "$T/origin.git"

# 4. an origin that is not the public Hestia repository is refused, by check and by apply
out="$(HESTIA_UPDATE_ORIGIN= "$LS" update check)"
case "$(field "$out" error)" in *"does not point at the public Hestia repository"*) ;; *) fail "check accepted a non-public origin: $out" ;; esac
out="$(HESTIA_UPDATE_ORIGIN= "$LS" update apply)" && fail "apply accepted a non-public origin"
case "$out" in *"does not point at the public Hestia repository"*) ;; *) fail "apply's refusal is not plain: $out" ;; esac

# 5. a backup or a restore check holds its lock: apply refuses and changes nothing (not even a fetch)
for lock in hestia-watch hestia-rehearse; do
  out="$(flock "$XDG_RUNTIME_DIR/$lock.lock" "$LS" update apply)" && fail "apply ran while $lock held its lock"
  case "$out" in *'"event":"failed"'*"is running right now"*) ;; *) fail "apply under $lock: $out" ;; esac
  case "$out" in *'"step"'*) fail "apply started working while $lock held its lock: $out" ;; esac
  [ "$(id "$src")" = "$A" ] && [ "$(installed)" = "$A" ] || fail "apply changed something while $lock held its lock"
done

# 6. the checkout has changes of its own: apply refuses and leaves them alone
echo "mine" >> "$src/CHANGELOG.md"
out="$("$LS" update apply)" && fail "apply ran over local changes"
case "$out" in *"has changes of its own"*) ;; *) fail "apply's refusal of local changes is not plain: $out" ;; esac
[ "$(tail -1 "$src/CHANGELOG.md")" = mine ] && [ "$(id "$src")" = "$A" ] || fail "apply touched the local changes"
git -C "$src" checkout -q -- CHANGELOG.md

# 7. apply: fetch, build, test, switch in that order, then updated; the checkout is fast-forwarded; the old build is kept
: > "$T/calls.log"
out="$("$LS" update apply)" || fail "apply failed: $out $(tail -5 "$XDG_DATA_HOME/hestia/update.log" 2>&1)"
[ "$(printf '%s\n' "$out" | grep -o '"step":"[a-z]*"' | uniq | sed 's/"step"://; s/"//g' | paste -sd,)" = "fetch,build,test,switch" ] || fail "the steps are not fetch, build, test, switch: $out"
[ "$(printf '%s\n' "$out" | tail -1)" = "{\"event\":\"updated\",\"from\":\"$A\",\"to\":\"$B\"}" ] || fail "no updated event from A to B: $out"
[ "$(id "$src")" = "$B" ] && [ "$(installed)" = "$B" ] || fail "the checkout or install.json is not at the new build"
[ "$(build_of current)" = "$B" ] && [ "$(build_of previous)" = "$A" ] && [ "$(builds)" = 2 ] || fail "after an update: current $(build_of current), previous $(build_of previous), $(builds) builds"
"$LS" --version >/dev/null || fail "the updated lifestream does not run"
grep -q "restart hestia-watch.timer" "$T/calls.log" && fail "the watch timer was restarted although its unit did not change"
grep -q "try-restart hestia-tray.service" "$T/calls.log" || fail "the tray was not restarted onto the new build"
[ "$(field "$("$LS" update check)" newer)" = false ] || fail "check still says newer after the update"

# 8. rollback: back to A, and the build it leaves is kept (so it could go forward again)
out="$("$LS" update rollback)" || fail "rollback failed: $out"
[ "$(printf '%s\n' "$out" | tail -1)" = "{\"event\":\"rolled_back\",\"from\":\"$B\",\"to\":\"$A\"}" ] || fail "no rolled_back event: $out"
[ "$(build_of current)" = "$A" ] && [ "$(build_of previous)" = "$B" ] && [ "$(installed)" = "$A" ] || fail "rollback did not switch back to A"
"$LS" --version >/dev/null || fail "the rolled-back lifestream does not run"
[ "$(field "$("$LS" update check)" newer)" = true ] || fail "after a rollback, check should offer the newer build again"

# 9. another update keeps exactly one previous build; a changed timer unit is restarted
printf '# changed\n' >> "$dev/packaging/systemd/hestia-watch.timer"; push C
C="$(id "$dev")"; : > "$T/calls.log"
out="$("$LS" update apply)" || fail "the second apply failed: $out"
[ "$(build_of current)" = "$C" ] && [ "$(build_of previous)" = "$A" ] && [ "$(builds)" = 2 ] || fail "more than one previous build kept, or the wrong one: $(ls "$PREFIX/lib/hestia/builds")"
grep -q "restart hestia-watch.timer" "$T/calls.log" || fail "the watch timer's unit changed but the timer was not restarted"

# 10. the new build fails its start-up check: the old build stays installed and the failed one is removed
printf '#!/bin/sh\nexit 1\n' > "$dev/bin/hestia"; push D
out="$("$LS" update apply)" && fail "apply switched to a build that does not start"
case "$(printf '%s\n' "$out" | tail -1)" in *'"event":"failed"'*"did not start properly"*) ;; *) fail "a failed start-up check is not a plain failure: $out" ;; esac
case "$out" in *'"step":"switch"'*) fail "apply reached the switch with a build that does not start" ;; esac
[ "$(build_of current)" = "$C" ] && [ "$(installed)" = "$C" ] && [ "$(builds)" = 2 ] || fail "a failed update changed the installed build or left its folder behind"
"$LS" --version >/dev/null || fail "the installed lifestream stopped working after a failed update"

# 11. uninstall removes every build and install.json, and leaves the checkout alone
bash "$src/uninstall.sh" >/dev/null 2>&1 || fail "uninstall failed"
[ ! -e "$PREFIX/lib/hestia" ] && [ ! -e "$XDG_DATA_HOME/hestia/install.json" ] || fail "uninstall left builds or install.json"
[ -d "$src/.git" ] || fail "uninstall touched the checkout"
echo "update ok"
