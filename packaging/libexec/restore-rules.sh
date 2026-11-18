#!/usr/bin/env bash
# restore-rules.sh — what a restore may write, held back or refuse. ONE list, read by bootstrap.sh (which enforces it) and
# rehearse.sh (which checks the outcome against it). Source this; it only defines functions.
# Runs code by itself, or changes what runs: held back unless --allow-executable. PATH is relative to ~ (dotfiles) or ~/.config.
# A poisoned repository can use any of these to run its code (LD_PRELOAD in environment.d, core.sshCommand in .gitconfig).
is_executable_config() {
  case "$1" in
    .bashrc|.bash_profile|.bash_login|.bash_logout|.profile|.zshrc|.zshenv|.zprofile|.zlogin|.zlogout|.gitconfig) return 0 ;;
    autostart|autostart/*|systemd|systemd/*|environment.d|environment.d/*|fish|fish/*|zsh|zsh/*|bash|bash/*|git|git/*) return 0 ;;
    */autostart/*|*/systemd/*|*/environment.d/*) return 0 ;;
  esac
  return 1
}
# Never restored, with or without --allow-executable: keys and the keyring.
is_denied() { case "$1" in .ssh|.ssh/*|.gnupg|.gnupg/*|gnupg|gnupg/*|*keyring*|gh|gh/*) return 0 ;; esac; return 1; }
# The only top-level names restore will write. Everything else in the repository is ignored and counted as skipped.
is_allowed() {
  case "$1" in
    .nvidia-settings-rc|mimeapps.list|user-dirs.dirs|monitors.xml|gtk-3.0|gtk-4.0|mpv|htop|nautilus|powerlimit|johncast|hestia) return 0 ;;
  esac
  is_executable_config "$1"
}
