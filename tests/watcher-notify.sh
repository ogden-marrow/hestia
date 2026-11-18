#!/usr/bin/env bash
# A person who never opens Hestia must still find out when the backup keeps failing (issue #225): one gentle desktop
# notification once a problem has lasted 6 hours, never on the first blip, never twice for the same problem, quiet when
# healthy, and silent when notifications=off. A fake HOME in a temp folder, a notify-send stub that records its calls, a
# real watcher run that really fails (a commit that cannot be signed). Never touches the real HOME or any real repository.
# shellcheck disable=SC2088 # a literal "~/" is what capture-paths.txt holds
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
w="$here/packaging/libexec/watch.sh"
T="$(mktemp -d)"; trap 'rm -rf "$T"' EXIT
export HOME="$T/home" XDG_RUNTIME_DIR="$T" HESTIA_NOW=1 GIT_CONFIG_SYSTEM=/dev/null GIT_CONFIG_GLOBAL="$T/home/.gitconfig"
unset XDG_DATA_HOME
mkdir -p "$HOME"
case "$HOME" in "$T"/*) ;; *) echo "FAIL: HOME is not in the sandbox: $HOME" >&2; exit 1 ;; esac
ls_bin="$here/target/release/lifestream"; [ -x "$ls_bin" ] || ls_bin="$here/target/debug/lifestream"
[ -x "$ls_bin" ] || { echo "build lifestream first (cargo build)" >&2; exit 2; }
export LIFESTREAM="$ls_bin"
fail() { echo "FAIL: $*" >&2; exit 1; }
git config --global user.email t@t.t; git config --global user.name t
. /etc/os-release; rel="applications/linux/${ID:-linux}/${VERSION_ID:-unknown}"

# notify-send stub: records "title | body" (and the urgency) one line per call
mkdir -p "$T/bin"; calls="$T/calls"
cat > "$T/bin/notify-send" <<EOF
#!/usr/bin/env bash
u=""; [ "\$1" = -u ] && { u="\$2"; shift 2; }
[ "\$1" = -a ] && shift 2
printf '%s | %s | %s\n' "\$u" "\$1" "\$2" >> "$calls"
EOF
chmod +x "$T/bin/notify-send"; export PATH="$T/bin:$PATH"

share="$HOME/.local/share/hestia"; hist="$share/watch-history.jsonl"
now() { date +%s; }
ago() { # hours-ago result : one line of history (as the watcher writes it)
  mkdir -p "$share"; printf '{"at":%s,"result":"%s","commits":0}\n' "$(( $(now) - $1 * 3600 ))" "$2" >> "$hist"
}
n=0
fresh() { # a new home repository with one file to capture, an empty history, no notifications yet
  n=$((n+1)); S="$T/s$n"; mkdir -p "$S/home"; export HOME="$S/home" XDG_RUNTIME_DIR="$S" GIT_CONFIG_GLOBAL="$S/home/.gitconfig"
  case "$HOME" in "$T"/*) ;; *) fail "HOME is not in the sandbox: $HOME" ;; esac
  git config --global user.email t@t.t; git config --global user.name t
  share="$HOME/.local/share/hestia"; hist="$share/watch-history.jsonl"; calls="$S/calls"
  sed -i "s#>> \"[^\"]*\"\$#>> \"$calls\"#" "$T/bin/notify-send"; : > "$calls"
  r="$S/repo"; git init -q -b main "$r"; touch "$r/tree.toml"; git -C "$r" add -A; git -C "$r" commit -qm init
  mkdir -p "$r/$rel"; printf '%s\n' '~/.bashrc' > "$r/$rel/capture-paths.txt"
  echo 'export A=1' > "$HOME/.bashrc"
  # the first capture is made (and its record thrown away), so a later failure is a plain failure to commit a change
  bash "$w" "$r" || fail "the first capture did not work"; rm -rf "$share"; : > "$calls"
}
broken() { git -C "$r" config commit.gpgsign true; git -C "$r" config gpg.program /bin/false; }   # every commit fails
mended() { git -C "$r" config commit.gpgsign false; }
run() { echo "export N=$RANDOM" >> "$HOME/.bashrc"; rc=0; bash "$w" "$r" || rc=$?; }
sent() { grep -c . "$calls" || true; }
is() { local j; j="$(state)"; case "$j" in *"$1"*) ;; *) return 1 ;; esac; case "$j" in *"$2"*) ;; *) return 1 ;; esac; }
state() { "$ls_bin" backup-state --share "${1:-$HOME/.local/share}"; }

# ---- the engine's contract: one JSON line with exactly these fields
fresh; ago 1 "ok: 0 commit(s), up to date"
js="$(state)"; [ "$(printf '%s\n' "$js" | wc -l)" = 1 ] || fail "backup-state must print one line"
for k in state since reason failing_runs last_ok notify; do case "$js" in *"\"$k\":"*) ;; *) fail "no $k in: $js" ;; esac; done
is '"state":"ok"' '"notify":false' || fail "a run an hour ago is ok and quiet: $js"

# ---- ok never notifies (and says nothing about recovery when there was nothing to recover from)
fresh; run; [ "$rc" = 0 ] || fail "a good run must exit 0 (got $rc)"
[ "$(sent)" = 0 ] || fail "a healthy run sent a notification: $(cat "$calls")"
ago 30 "FAILED: x"; run   # an old, already-mended failure is history, not news
[ "$(sent)" = 0 ] || fail "a failure that is over sent a notification"

# ---- 5 hours of failing is not enough
fresh; broken; ago 10 "ok: 0 commit(s), up to date"; ago 5 "FAILED: offline"; run
[ "$rc" = 1 ] || fail "the run should have failed (rc $rc)"
[ "$(sent)" = 0 ] || fail "5 hours of failing already sent a notification"
is '"state":"failing"' '"notify":false' || fail "5 hours: failing but not yet worth a notification: $(state)"

# ---- 7 hours notifies, exactly once, however many runs follow; the words are plain
fresh; broken; ago 10 "ok: 0 commit(s), up to date"; ago 7 "FAILED: offline: push could not reach GitHub, will retry"; run
[ "$(sent)" = 1 ] || fail "7 hours of failing should send one notification (sent $(sent))"
run; run; run
[ "$(sent)" = 1 ] || fail "the same problem was announced again (sent $(sent))"
line="$(head -1 "$calls")"
case "$line" in "normal | Hestia: your backup needs you | "*) ;; *) fail "title or urgency is wrong: $line" ;; esac
body="${line#*| Hestia: your backup needs you | }"
[ -n "$body" ] || fail "the notification has no words"
case "$body" in */*|*"$HOME"*|*ghp_*|*"~"*) fail "the body must hold no path or token: $body" ;; esac
[ -f "$share/notified.json" ] || fail "the told episode was not remembered"

# ---- it works again: the episode is forgotten, with at most one quiet "working again"
mended; run; [ "$rc" = 0 ] || fail "the mended run should be ok (rc $rc: $(cat "$share/watch.json"))"
[ "$(sent)" = 2 ] || fail "expected the failure notice and one 'working again' (sent $(sent))"
case "$(tail -1 "$calls")" in "low | Hestia | Your backup is working again.") ;; *) fail "the recovery line is wrong: $(tail -1 "$calls")" ;; esac
[ ! -e "$share/notified.json" ] || fail "the episode was not cleared when the backup worked again"
run; [ "$(sent)" = 2 ] || fail "'working again' repeated"
# a new failure is a new episode and is told again after its own 6 hours
broken; : > "$hist"; ago 9 "ok: 0 commit(s), up to date"; ago 7 "FAILED: offline"; run
[ "$(sent)" = 3 ] || fail "a new failure after recovery was not announced (sent $(sent))"
run; [ "$(sent)" = 3 ] || fail "the second episode was announced twice"

# ---- stale: the timer stopped (no run at all) for more than 36 hours, and it matters after 6 more
fresh; ago 40 "ok: 0 commit(s), up to date"
is '"state":"stale"' '"notify":false' || fail "40 hours without a run: stale but not yet news: $(state)"
fresh; ago 43 "ok: 0 commit(s), up to date"
is '"state":"stale"' '"notify":true' || fail "43 hours without a run should be stale and news: $(state)"
case "$(state)" in *'"last_ok":"'*) ;; *) fail "stale must say when it last worked: $(state)" ;; esac
state | grep -q '"reason":"[^"/]*"' || fail "the stale sentence is not plain"
"$ls_bin" backup-state --share "$HOME/.local/share" --mark-notified >/dev/null
is '"state":"stale"' '"notify":false' || fail "stale told once must stay quiet: $(state)"

# ---- the setting: notifications=off means no notification at all
fresh; broken; mkdir -p "$HOME/.config/hestia"; printf 'scheme=dark\nnotifications=off\n' > "$HOME/.config/hestia/settings"
ago 10 "ok: 0 commit(s), up to date"; ago 7 "FAILED: offline"; run
[ "$(sent)" = 0 ] || fail "notifications=off still sent one"
is '"state":"failing"' '"notify":false' || fail "off: still failing, never notify: $(state)"

# ---- garbage or missing files: unknown, and never a notification
fresh; is '"state":"unknown"' '"notify":false' || fail "nothing written yet: $(state)"
mkdir -p "$share"; printf 'not json\n{"at":"x","result":3}\n\001\002' > "$hist"; printf '{{{{' > "$share/watch.json"
is '"state":"unknown"' '"notify":false' || fail "garbage files: $(state)"
printf '{"at":%s,"result":"FAILED: x"}\n' "$(( $(now) - 9 * 3600 ))" > "$share/watch.json"; : > "$hist"
case "$(state)" in *'"state":"failing"'*) ;; *) fail "watch.json alone is enough to see a failure: $(state)" ;; esac

# ---- the words never copy the watcher's text: a path and a token in the failure stay out of the sentence
fresh; ago 8 "FAILED: git commit failed: $HOME/.bashrc ghp_$(printf 'a1B2c3D4%.0s' 1 2 3 4 5)"
reason="$(state | sed -n 's/.*"reason":"\([^"]*\)".*/\1/p')"
[ -n "$reason" ] || fail "no reason: $(state)"
case "$reason" in */*|*"$HOME"*|*ghp_*) fail "the reason holds a path or token: $reason" ;; esac

echo "watcher notification checks passed"
