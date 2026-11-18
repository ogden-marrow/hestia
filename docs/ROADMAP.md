# Roadmap

Written 2026-11-18 as the product plan. It answers: where are we, what is next, what is blocked on a person, and how do we know we are done. `VISION.md` says why; `VISION_FIT.md` scores how close we are; this file says what to do next. GitHub milestones and issues hold the work; this file explains the order.

## What Hestia is, in one paragraph

Your whole computer as code in a private git repository: settings, programs, applications, documents and machines. It watches quietly, backs up without being asked, files new things where they belong (asking only when unsure), never deletes, and can undo anything. There is no Hestia account or online service; your own private GitHub repository is the only place data goes, and only after you sign in.

## Where we are (2026-11-18)

| Area | State |
|---|---|
| Backup engine | Working and live on the owner's machine: allowlisted capture, secret scanner, deny-list, push gate to a verified-private repository, fetch and rebase, timeouts, visible failures, restore hardening, 15 protections proven by mutation tests (`tests/mutations.sh`). |
| Safety | Three red-team rounds done; every high finding fixed with a test that fails on the old code. Open: residual risks listed in `THREAT_MODEL.md` (T2, T20, T21). |
| Organising | Classifier and file mover with journal and undo; folder move, new folder and undo in the engine; folder tree in the app with rename, move, make folder, undo, and drop files onto folders. |
| App shape | Stream / Library / Computer / Settings, header backup chip, applications and games list with real icons and Steam covers, History as saved changes, plain-language copy, one button style. Latest critic round: panes score 4.5 to 7.5 out of 10. None is first-party quality yet. |
| Install | Tested hostile in CI and in a rehearsal of the release script. Not tested on a clean OS image. Sign-in works against a stub; the real flow needs the project's GitHub OAuth App Client ID. |
| Release | Nothing published. Needs the owner's explicit go. |

## Principles for choosing work

1. **Trust before beauty.** A beautiful app that loses a file or leaks a secret is a failed product. Anything that touches files, git, the network or secrets goes first.
2. **Visible beats correct-but-silent.** A failure the person cannot see is a failure we have not fixed (`THREAT_MODEL` T22, T23).
3. **One way to do each thing.** Every action has one canonical place (see `feedback` in the maintainer notes); duplicates are bugs.
4. **Real data, own renders.** UI is judged on `--snap` renders and on the owner's real data, never personal data in the repository.
5. **Small PRs, green CI, merge fast.** Every PR carries its test, threat-model line and changelog entry.

## Order of work

### Now: M1c Library and Computer, and M1 Trustworthy UI
Goal: the owner opens Hestia and it feels like a native app that does the job: list my applications, let me organise my folders, show me what happened.
- **Applications**: verify launching on the real machine (#235), non-Steam games (#233), recents and favourites (#234).
- **Computer**: drag to move, inline rename, multi-select (#231), "file these for me" inside the tree (#232).
- **Rules** as a real editor (#230, the worst pane).
- **One adaptive layout system**: reading width, grids, two-pane breakpoints (#236).
- **Accessibility pass** (keyboard, focus, screen reader, contrast) (#241).
- Existing M1 issues remain the quality bar; many are done or superseded and were closed on 2026-11-18.
- **Exit test**: critic round with every pane at 8 or above on 960, 1280 and 1920 in light and dark, and the owner says "yes".

### Next: M5 Everything backed up, provably
Goal: the owner never has to wonder whether the backup works.
- Desktop notification and tray state when a backup keeps failing (#225): done in the engine, watcher and tray; the app should show `lifestream backup-state` too.
- Weekly restore rehearsal in a scratch folder (#226).
- "Left out and why": skipped, withheld and denied files shown (#227).
- Coverage check: warn when important config is not captured (#228).
- Repository size guard (#229).
- **Exit test**: break each of network, token, remote and disk on purpose; each produces one clear message and recovers on its own when fixed; a restore rehearsal passes and is shown in the app.

### Then: M1b The stream is the heart
Goal: the Stream is a live, time-ordered story of what happened, with who decided and Undo.
- Engine: events with an actor and a reason per batch (one canonical reader) (#237).
- UI: feed grouped by day with inline Undo; saved views later.
- **Exit test**: an hour of activity reads like a story and every item that moved files has Undo.

### Then: M2 Fresh install works end to end, and M4 Release
- Register the OAuth App; verify sign-in, adopt or create the home repository on a fresh account (#238, #110).
- Clean-machine rehearsal on a VM from the tarball and the `.deb` (#239, #6).
- User guide, README with real screenshots, troubleshooting (#242).
- First signed release (owner's go), then verify the artifacts on a clean machine (#244).
- **Exit test**: a stranger installs from the release page and, within a day, has a backed-up, organised computer without reading source.

### Later: M6 After the first release
- Second machine: join an existing home repository and restore with a preview (#243).
- Other distributions and desktops, and the iCloud bridge via a Mac as scoped in the maintainer's notes.

## What is blocked on a person

| Blocker | Who | What unblocks it |
|---|---|---|
| GitHub OAuth App Client ID | Owner | Sign in to GitHub in the built-in browser; Hestia fills the form; no client secret. |
| Which repository is the home repository (#82, #83), and collapsing the old workspace | Owner | Say Option A (`~/Home`) or B. Plan in the private collapse plan. Nothing in the old workspace is touched without the owner's go. |
| First release | Owner | Explicit go; steps in `PUBLISHING.md`. |
| Local model choice | Owner's Ollama agent | A recommendation that fits fully on the GPU; Hestia only shows it and runs it (#240). |
| Real launch check of a Steam game and an app | Owner | Click one in the real window after reinstalling. |

## Risks we are watching

- **UI quality plateau.** Critic scores rose about one point per round; the structural fixes (shared formatter, surfaces, type scale, layout system) are what move panes past 7. Mitigation: do the systemic work before more per-pane polish.
- **Silent backup gaps.** Fixed the known ones; the "Left out and why" and coverage check close the class.
- **Scanner is a filter, not a guarantee** (T2). Mitigation: private repository gate, withhold rules, restore holds back executable config, and showing the person what was left out.
- **CI queue time.** One runner at a time slows merges; keep PRs small and non-overlapping on `ui/src/main.rs` (it is the usual conflict point).
- **Bus factor on taste.** The owner does not review code; critics and the owner's reaction on real renders are the check. Keep the research notes in `design-research/` current.

## How progress is measured

- Critic scores per pane per round, kept in `design-research/critic-log.md`.
- `VISION_FIT.md` scorecard, updated when a milestone closes.
- Mutation check (`tests/mutations.sh`) green before every release.
- CI green on `main` at all times.
- The owner's own words after reinstalling.

## Cadence

- Every merge: main CI checked, issues closed with the PR that did the work.
- Weekly: triage the issue list against this file; close what is done, merge what overlaps, file what the latest critic round or red team found.
- Before a release: red-team round, UI board, mutation check, clean-machine rehearsal, owner's go.
