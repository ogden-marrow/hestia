#!/usr/bin/env bash
# home-repo.sh — where is the home repository? ONE answer, read from the app's settings file (`home=` in
# ~/.config/hestia/settings), which is what the app shows and what signing in, adopting or creating a repository writes.
# Absolute, `~/…` or relative to the home folder. If nothing is set: ~/Home. Source this; it only defines a function.
hestia_home_repo() {
  local f="$HOME/.config/hestia/settings" v   # exactly where the app and install.sh keep it
  v="$(sed -n 's/^home=//p' "$f" 2>/dev/null | head -1)"
  # shellcheck disable=SC2088 # a literal "~/" is what the setting contains
  case "$v" in
    "")    echo "$HOME/Home" ;;
    "~/"*) echo "$HOME/${v#\~/}" ;;
    /*)    echo "$v" ;;
    *)     echo "$HOME/$v" ;;
  esac
}
