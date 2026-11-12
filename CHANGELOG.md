# Changelog

All notable changes. Dates are 2026-11-11 unless noted. This project has not cut a release yet; everything is under Unreleased.

## Unreleased

### Added
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
- **CI**: the whole workspace is lint-clean (clippy with warnings as errors), clean-computer end-to-end test, release rehearsal, dependency-source check, privacy check, uninstall test.

### Security
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
