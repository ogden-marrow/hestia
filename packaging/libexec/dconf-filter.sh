#!/usr/bin/env bash
# dconf-filter.sh — read `dconf dump /` on stdin, write only the settings worth restoring on stdout.
#
# An ALLOWLIST of sections (appearance, input, keyboard shortcuts, shell, extensions, power, terminal, file
# manager preferences). Everything else is dropped: last-used folders, recent documents, window state, clock
# locations, calendar, accounts, per-app notification lists. Inside the sections that are kept, any key whose
# name says it remembers what you did (recent, history, last-...) is dropped too. A setting that is not
# listed here is never captured; add to ALLOW on purpose.
awk '
  BEGIN {
    allow = "^\\[(org/gnome/desktop|org/gnome/shell|org/gnome/mutter|org/gnome/settings-daemon|org/gnome/nautilus/preferences|org/gnome/Ptyxis|org/gtk|com/ubuntu|desktop/ibus)(/|\\])"
    deny  = "^\\[(org/gnome/desktop/notifications/application|org/gnome/desktop/search-providers|org/gnome/desktop/privacy|org/gnome/shell/(app-switcher|world-clocks|weather|calendar|search-providers))"
    dropkey = "(recent|history|last-|-last|uris|geoloc|location|account|email)"
    keep = 0
  }
  /^\[/ { keep = ($0 ~ allow && $0 !~ deny); if (keep) { if (n++) print ""; print } ; next }
  keep {
    if ($0 ~ /^[A-Za-z0-9_.-]+=/) { key = $0; sub(/=.*/, "", key); if (tolower(key) ~ dropkey) next }
    if ($0 != "") print
  }'
