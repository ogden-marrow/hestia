# Changelog

All notable changes. Dates are 2026-11-11 unless noted. This project has not cut a release yet; everything is under Unreleased.

## Unreleased

### Removed
- **The standalone launcher window** (`hestia --launch`, its toggle and sizing): the Shell extension owns Alt+Space. Search inside the app and `hestia --serve` (the engine behind the Shell bar) are unchanged; the desktop entry's "Search…" action opens the app's Search screen.

### Added
- **Folders** (Explore): a real tree of your home folder. Expand and collapse, breadcrumbs, select; rename or move a file with Undo, through the same journaled mover as filing. Nothing is ever deleted. Moving whole folders waits for an engine API.
- **Applications** (Explore): every installed app and game in one list with its icon: desktop entries (native, Flatpak, Snap, your own), Steam shortcuts and installed Steam games from Steam's own manifests. Search, All / Games / Apps, group by kind or source, sort by name or most used; click to open.
- **A calm backup status**: the header says "Backed up 12 minutes ago" on every screen; when it is not true, Today shows "Your backup needs you" with one plain action (back up now, or sign in).
- **Undo for filing** (engine): every batch of moves is written down; `lifestream file --undo` puts the last batch back, one file at
  a time, never overwriting, never leaving your home folder, and leaves what it could not restore to be retried.
- **Sign in with GitHub** (engine and screens): device-flow sign-in, find, adopt or create your private home repository.
  The token lives only in the system keyring and is refused if there is none. Needs the project's GitHub Client ID to go live.
- **Machine watcher shipped with Hestia**: captures packages, snaps, flatpaks, config, dotfiles and desktop settings every
  few minutes when the computer is quiet, one commit per kind of change, and pushes to your private repository.
- **Safe restore** (`bootstrap.sh`): a dry run by default; copies instead of linking, never overwrites, and holds back
  anything that runs by itself until you ask.
- **Design system and redesigned Welcome, Today, sidebar and wording**; the launcher shows why an app did not open.
- **Release packaging**: one script builds the tarball and `.deb` with checksums and a signed build attestation.
- **CI**: the whole workspace is lint-clean (clippy with warnings as errors) and so are the shell scripts (shellcheck), clean-computer end-to-end test, release rehearsal, dependency-source check, privacy check, uninstall test.

### Fixed
- **Capture coverage** (#173): the capture list now survives an OS upgrade (a new version folder starts from the nearest other version's list). Symlinked
  dotfiles (a stow layout) are captured by their target's content, never as a link, when the target is inside your home folder and
  allowed by the never-capture list; others are listed in `SKIPPED.txt`. A cloned folder such as `~/.config/nvim` is stored as its files, without
  its `.git`. Third-party apt sources in the newer `.sources` format are captured and restored like `.list` files.
- **The machine watcher no longer fails quietly** (#166-#172). A failed commit, push or capture is a `watch.json` result that starts
  `FAILED` with the first line of the reason, and the unit exits 1; offline, signed out, repository gone and repository public are told
  apart. A push rejected because another computer pushed first is fetched and rebased. Files over 50 MB and unreadable files are
  skipped and listed in `SKIPPED.txt`. A possible secret in an unsent commit is redone from what GitHub has. File names with spaces,
  accents, apostrophes, `#` or a newline are handled. A refused mass deletion can be confirmed (`touch .accept-smaller-capture`) and a
  changed capture list resets the check. A stale `index.lock` is cleared, a hung push times out (`TimeoutStartSec`, fetch/push
  timeouts), a second run reports it was refused (exit 75), and waiting for a quiet computer ends after 6 hours.
  The engine's `secret-scan` streams files, takes folders, prints `-z` NUL-separated results and exits 2 when it could not look.

### Security
- Secret scanner and deny-list, red-team round 3 (#175): JSON-quoted keys, bare `PASSWORD=`/`DB_PASS=`/`MYSQL_PWD=` values, `--password=`, libpq and rclone
  lines, `curl -u`, Slack, Stripe, SendGrid, JWT, Authorization headers, Azure, WireGuard and kubernetes keys, base64-wrapped private keys. The push
  scan no longer skips added lines starting `++` or merge commits. The deny-list now covers credential files, browser, chat and password-manager
  profiles, shell histories, VPN files and `id_dsa`, and refuses a parent folder (`~/.config`, `~/.docker`) that holds any of them.
- **Restore hardened against a poisoned home repository** (#174): file by file with a `.bak` of anything different (no more skipping whole existing folders), a fixed list of names, never `~/.ssh`, `~/.gnupg` or the keyring, shell startup files, `.gitconfig`, `environment.d`, fish/zsh/bash config held back until `--allow-executable`, file modes restored, honest counts in the summary.
- One shared secret scanner (modern token formats, short quoted or exported passwords, binaries) gates capture, push and
  publish; secret lines are left out of the backup, private keys drop the file.
- Pushes go only to a private GitHub repository of yours, checked on the address git will really use.
- One list of paths that are never captured, read by capture, the Machine screen and the starter manifest.
- File names reach the local model only as cleaned data; the model cannot choose a destination.
- Capture can no longer delete a live file; the mover refuses symlinks, overwrites and hidden destinations.
- Clipboard history is off by default, filters codes and secrets, is readable only by you and deleted when turned off.
- The never-capture list also applies to files inside captured folders (for example a file-manager bookmarks file).
- Desktop settings are captured by allowlist; the launcher ignores toggles while the screen is locked.
- CI actions are pinned to commits; the release workflow reads by default and only its job can write.
- See `docs/THREAT_MODEL.md` for every finding, its status and the release gate.

### Changed
- **One home-repository setting**: the `home=` line in `~/.config/hestia/settings` is the only place that says where your home
  repository is. The app, the watcher, the nightly scan and the restore script all read it; `install.sh` only seeds it. The path
  baked into the systemd units and the `environment.d` file are gone.
- "Answer similar folders together" is off until you choose it.
- Auto-push is on by default (`HESTIA_AUTOPUSH=0` turns it off).

### Known limits
- No release has been cut, so signing and attestation are unexercised.
- The `.deb` needs `hestia-setup` run once per person to turn on the background backup (a package can only place files under /usr).
- Not yet tried on a clean OS image with a real desktop session (`docs/VISION_FIT.md`).
