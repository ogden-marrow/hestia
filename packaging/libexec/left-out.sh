#!/usr/bin/env bash
# left-out.sh — the one writer of the "left out and why" records (#227). Source this; it only defines functions.
# capture.sh writes <state>/skipped.jsonl (too big, unreadable, private by rule, links) and watch.sh writes
# <state>/withheld.jsonl (secret lines and files the scanner held back); `lifestream left-out` is the one reader.
# <state> is $XDG_DATA_HOME/hestia (default ~/.local/share/hestia): on this computer only, never committed or pushed.
# One JSON object per line: {"why":"...","path":"<relative to the home folder>"|null,"since":<epoch>[,"bytes":N,"limit":N][,"count":N]}.
# Only a reason code, a path and numbers are ever written: never a file's contents, never a secret, never a message.
# A path with a control character is written as null (the reader cannot show it safely). `since` is when this
# (why, path) was first seen: carried over from the file being replaced, so a file left out for weeks says so.

# left_out_begin <final file>: start a new list beside the old one, remembering when each record was first seen
left_out_begin() {
  lo_final="$1"; lo_new="$1.new.$$"; lo_now="$(date +%s)"
  declare -gA lo_was=(); : > "$lo_new" || return 1
  local l re='^\{"why":"([a-z_]+)","path":(null|"([^"\\]|\\.)*"),"since":([0-9]+)'
  [ -f "$lo_final" ] && [ ! -L "$lo_final" ] || return 0
  while IFS= read -r l; do
    [[ $l =~ $re ]] && lo_was["${BASH_REMATCH[1]} ${BASH_REMATCH[2]}"]="${BASH_REMATCH[4]}"
  done < "$lo_final"
  return 0
}

# left_out <why> <path relative to home, or empty> [extra JSON members, e.g. "bytes":1,"limit":2]
left_out() {
  local p="null" s
  if [ -n "$2" ] && [[ "$2" != *[[:cntrl:]]* ]]; then s="${2//\\/\\\\}"; p="\"${s//\"/\\\"}\""; fi
  printf '{"why":"%s","path":%s,"since":%s%s}\n' "$1" "$p" "${lo_was["$1 $p"]:-$lo_now}" "${3:+,$3}" >> "$lo_new"
}

# left_out_end: the new list replaces the old (an empty list removes the file)
left_out_end() {
  if [ -s "$lo_new" ]; then sort -u "$lo_new" > "$lo_final.tmp.$$" && mv -f "$lo_final.tmp.$$" "$lo_final"; else rm -f "$lo_final"; fi
  rm -f "$lo_new"
}
