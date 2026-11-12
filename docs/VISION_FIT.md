# Vision fit: where the product stands against `docs/VISION.md`

Updated 2026-11-11. Honest scorecard, one row per force. **Met** means a stranger could rely on it today; **Partial** means it works for the author but has named gaps; **Missing** means not built. Every gap links to an issue. Update this file in the same PR that changes a status.

| # | Force | Status | Evidence it exists | What is missing (issue) |
|---|---|---|---|---|
| 1 | Your computer, as code | **Partial** | The watcher captures packages, snaps, flatpaks, cargo/docker lists, apt sources, allowlisted config, dotfiles (secret lines left out) and an allowlisted slice of desktop settings, commits each kind of change separately and pushes to the private home repository (verified on a real machine, history scanned clean). `bootstrap.sh` restores it, dry-run by default. | What lives in the repo (documents? media?) is undecided (#82). A full install, capture, restore dry-run and uninstall into an empty home folder now runs in CI (`tests/install-e2e.sh`, system commands stubbed). A real clean OS image with a real session is still untested (#6). |
| 2 | It organises itself, asks only when unsure | **Partial** | Scan to stream, rules, two local models whose disagreement decides what to ask, questions per folder. | Today is a dashboard, not the live stream (#13, decision #81). Questions are not yet one-card-at-a-time or batch-confirmed (#20, #21). No reminders or substreams (#14, #15). Search by meaning/time not in the UI (#16). |
| 3 | Files and folders are one thing | **Partial** | Editable domains and a folder tree derived from them; the mover files by kind and year. | Domain editing UI is clipped and confusing (#31); Files screen still edits the tree instead of showing where things went (#26). |
| 4 | Every machine is known | **Partial** | Per-computer fingerprint (`machines/<name>`), per-OS-and-version capture folders, safe restore. | Linux/Ubuntu only. One-repo-for-many-machines is an open decision (#83). Install and capture are tested end to end in an empty home folder in CI; a real second machine has not been tried (#6). |
| 5 | Private by construction | **Partial, mostly met** | Threat model written (`docs/THREAT_MODEL.md`) with a release gate. One shared secret scanner (modern token formats, short quoted or exported passwords, binaries, case-insensitive) and one deny-list of paths that are never captured (`lifestream::deny`, read by capture, the Machine screen and the starter manifest) gates capture and push. Secret lines are left out, private keys drop the whole file. Push only to a private GitHub repo checked on the URL git really uses. Clipboard history off by default, filtered, 0600, pruned. Desktop settings by allowlist. GitHub token only in the system keyring, refused without one. Local-model host must be loopback. File names reach the model only as cleaned, shortened JSON data it is told to treat as data, and its answer is limited to the known kinds (#56). | A first release has not been cut, so signing and attestation are untested (#12). Pre-publish privacy check in CI (#61). |
| 6 | Never destructive | **Partial** | The mover never overwrites, follows no symlinks and refuses hidden destinations. Capture builds beside the old copy and swaps; it refuses mass deletions and never deletes a live file. Restore moves existing files aside. | 'File them now' has no undo and runs on the UI thread (#10). History shows raw diffs, not events with Undo (#27). |
| 7 | Lean | **Partial** | The gh-based publish path was removed. One scanner, one button style in progress, one status strip. | Two code trees (private homebase, public Hestia) have drifted (#7). Old windowed launcher still present (#8). The watcher keeps a GitHub-CLI fallback until Hestia sign-in is live (#110). |
| 8 | Feels like a good native app | **Partial** | Design system and tokens (`docs/DESIGN.md`), Welcome, Today, sidebar tint, calm wording, jobs layer with busy states, footer status strip. | Most of milestone M1 (#32 keyboard and focus, #33 errors, #34 one Button, #37 contrast, #39 scaling, #46 snapshot suite, and the ui-taste issues). Nothing has been seen in a live window by a second person yet. |
| 9 | Public project, no personal data | **Partial** | Public repository; a privacy check in CI (home paths, emails, the computer's user and host name, no-reply identity on every new commit); fixtures and snapshots no longer read the machine's real repository; test tokens are built at run time so scanners stay quiet. | The maintainer's public name and email on GitHub merge commits is an accepted choice (#142). A publishing checklist with the state of each check is in `docs/PUBLISHING.md`; licences are enforced in CI and listed in `THIRD_PARTY.md`. |

## What 'done' means: a stranger installs it and has a backed-up, organised computer within a day

| Step | Status |
|---|---|
| Install from a release without a compiler | Rehearsed in CI (`tests/release-rehearsal.sh`: the real release script, checksums, install on a machine with no compiler, the .deb); no release has been cut yet; the .deb lacks the background watcher (#148) |
| First-run sign-in with GitHub, find or create the home repository | Engine done (sign-in, find, adopt, create) and screens done (#110). Not live: needs the OAuth App Client ID. |
| Backups run by themselves | Met on the author's machine; the watch timer bug that stopped it is fixed. Install to first capture to uninstall is tested in an empty home folder in CI; a real clean machine is still untested (#6). |
| Organises downloads and asks a few questions | Partial (rows 2 and 3) |
| Restore a new computer from the repository | Partial: a dry run is tested in CI; a real restore onto a clean machine has never been run (#6) |

## Order of work this implies

1. Make sign-in live (Client ID), then test the whole first run on a clean machine (#6).
2. Finish the safety items (#61, then signed releases in #12).
3. Collapse the two code trees (#7), then the release (#12).
4. The UI milestone in parallel; the stream as the front door once #81 is decided.
