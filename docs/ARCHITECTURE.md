# Hestia: vision and architecture

Status: kept current with the code (last updated 2026-11-12; `docs/VISION_FIT.md` has the per-force scorecard and `docs/THREAT_MODEL.md` every security finding). Where the code does not yet match the vision, this document says so.

## The idea

Your computer is a pile of things that matter: documents, photos, how your system is set up, the apps you installed, the machines you own. Today that pile is scattered, half backed up, and impossible to rebuild. Hestia treats the whole computer as **code in one private git repository**, organises it for you, and tells you each morning what it did and what it is unsure about.

You sit down, see what happened overnight, answer a few questions, and never hunt for a folder. A stranger can install it and within a day have a backed-up, organised computer without editing a file by hand.

Hestia is a **public project with no personal data**. Your own setup lives in your **private home repository**, which tracks Hestia as a submodule, so the tool is versioned by the thing it manages.

## Nine forces (from `docs/VISION.md`)

1. Your computer, as code.
2. It organises itself and asks only when unsure (Lifestreams: Freeman and Gelernter).
3. Files and folders are one thing, derived from editable domains.
4. Every machine is known and can be rebuilt from the repository.
5. Private by construction: sensitive paths never read, pushes only to a private repo, stopped by any secret.
6. Never destructive: it moves and copies, never deletes; everything is inspectable and undoable.
7. Lean: one way to do each thing.
8. It feels like a good native app.
9. Hestia is public; your data is not.

## Shape of the system

Two repositories, one machine.

- **hestia** (public): the application and its engine.
- **home** (private): your documents, `machines/<name>/machine.toml`, `applications/<os>/<version>/`, your folder tree and domains, with `hestia` as a submodule.

Five pieces run on your computer.

| Piece | What it is | Where |
|---|---|---|
| **Engine** | The brain. Ingests files into a stream, classifies them, plans and undoes moves, scans for secrets, talks to a local model, signs in to GitHub (token only in the system keyring) and finds, adopts or creates the home repo. No UI. Also a CLI (`lifestream`) whose engine commands print one JSON event per line. | `lifestream/` |
| **App** | The native window (Rune, GPU). Today, Questions, Files, History, Machine, Settings, and the launcher. Calls the engine; never does slow work on the UI thread (jobs layer). | `ui/` |
| **Tray** | Small always-on presence: shows state, lets you open the app. | `tray/` |
| **Watcher** | A systemd user timer (every few minutes, a setting) that, only when the computer is quiet, captures this machine's state (packages, allowlisted config, dotfiles, an allowlisted slice of desktop settings) into the home repo, commits it and pushes. Shipped inside Hestia, not copied into your repo; finds the repository from the one `home=` setting. | `packaging/libexec/` |
| **Launcher** | One search box for screens, apps, clipboard and files. A GNOME Shell extension draws it; the engine answers over a pipe. | `packaging/gnome-extension/` |

## How things flow

1. **Arrive.** New files appear in watched roots (Downloads, Documents, Pictures, Videos). A scheduled scan hashes them and writes one record per file into the **stream** (a plain, append-friendly store keyed by content hash).
2. **Classify, three tiers.** Deterministic rules first (data-driven, `rules.toml`). Then two small **local** models each suggest a label; disagreement, not self-reported confidence, decides what to ask. Everything else is left alone.
3. **Ask in bundles.** The unit of question is a folder, not a file. Questions screen: one card at a time, best guess is primary, Skip is always there.
4. **Move, safely.** A plan is built, shown, confirmed, then applied: same-disk hard-link-then-unlink or verified copy; never overwrites; refuses symlinks and hidden destinations; records old and new path in the stream and in a journal, so `lifestream file --undo` (the app's "Put them back") reverses the last batch without overwriting anything.
5. **Capture the machine.** The watcher writes a fingerprint and a snapshot of how the machine is set up, one commit per kind of change.
6. **Back up.** The watcher scans everything it captures with the shared secret scanner (a secret line is left out of the copy, a private key drops the file) and the one never-capture list, commits, and pushes automatically, only to a private GitHub repository of yours checked on the address git will really use, and only if the new commits scan clean. `HESTIA_AUTOPUSH=0` turns pushing off.
7. **Find.** Search by name, by meaning (local embeddings of metadata, never contents), by time.
7a. **Tell the person who never looks.** `lifestream backup-state` is the one policy for "is the backup working and does anyone need to be told": from `watch.json` and `watch-history.jsonl` it says `ok`, `failing`, `stale` (no success for 36 hours) or `unknown`, a plain sentence, and `notify` (a problem has lasted 6 hours and was not told yet). Deciding, sending and remembering live in the engine too (`backup::tell_in`, `backup-state --notify`): at most one `notify-send`, then the episode is recorded (`notified.json`); it clears when the backup works again, with one quiet "working again". Two callers share it: the watcher after every run, and the tray every minute (and once at start, before it looks for a tray host), so a stopped or dead watcher timer is still told (#252); a lock (`notified.lock`) keeps the two from telling one episode twice. The tray is the always-on check because it already runs for the whole graphical session (enabled by setup, `Restart=always`) and already reads the same state, and a desktop notification only reaches a person in that session anyway. The tray turns the flame warm and puts the sentence in its tooltip from the same code. `notifications=off` in the settings file turns the notice off. A backup only on the same disk is not a backup: `backup-state` also reads the home repository offline (its push target and commit times) and the watcher's last private check (`private-check.json`); no verified private push target is `local_only` after a 24-hour grace from the first commit, and an unpushed commit older than a day is `failing`. Both notify once per episode; the watcher's own failures win.
7b. **Prove it restores.** Once a week `hestia-rehearse.timer` runs `rehearse.sh`: HEAD of the home repository is copied into a private scratch folder, the one restore (`bootstrap.sh`, whose allow, deny and hold-back lists live in `restore-rules.sh`) runs against a fake home there as a dry run and then for real (file steps only), and the outcome is checked: manifest and capture list parse, every placed file has the repository's hash and a safe mode, nothing that runs by itself or any key was written, nothing outside the fake home moved, the secret scanner is clean. The verdict is `<share>/hestia/rehearsal.json` (`passed|failed`, a count and fixed plain sentences). `lifestream backup-state` reports it as `rehearsal` and adds a second line to the reason of a backup that already needs you when the last check failed or is older than 14 days; it never turns an ok backup into a failing one. The scratch folder is deleted however the run ends.
7c. **Show the gaps.** What the backup leaves out is recorded by one writer (`left-out.sh`): `capture.sh` writes `<share>/hestia/skipped.jsonl` (too large, unreadable, private by the never-capture list, links outside the home folder or to a private file) and `watch.sh` writes `withheld.jsonl` (secret-looking lines and files the scanner held back). Each line is a reason, a home-relative path, when it was first left out, and numbers only, never a line or contents; the lists stay on this computer. `SKIPPED.txt` in the repository stays the note for a person reading the repository. `lifestream left-out` is the one read-only reader behind Computer > Backup "Left out and why" (a plain sentence per item, `can_include` only where a fix by the person brings it in), and `backup-state` carries the counts as `left_out`. There is no "back this up anyway": that would change the deny policy.
7d. **Update itself.** Settings > About asks `lifestream update check` (one JSON line: the installed and available build ids, whether it is newer, up to ten plain CHANGELOG lines, or a plain error) and its Update button runs `lifestream update apply`, which streams progress events. Hestia is installed from a git checkout today: `install.sh` records the checkout in `install.json` and puts every build in its own folder (`<prefix>/lib/hestia/builds/`), with the programs and scripts as links through `current`. An update refuses while a backup or restore check holds its lock or when the checkout has changes of its own, fast-forwards the checkout from the public repository (the only network use: a `git fetch` over https without credentials), then runs the one installer's three steps (`install.sh --stage`, `--smoke`, `--activate`); the new build is switched on by one atomic link rename only after it starts, and exactly one older build is kept for `lifestream update rollback`. Signed releases replace the fetch later (#244).
8. **Tell the story.** `lifestream feed` is the one reader of what happened and who decided it, newest first: the move journal (every line now says `actor`, one of Hestia's rules, the models or you, and a one-sentence `reason`), a small log of undos, and the watcher's run history (`watch-history.jsonl`, the last 200 runs). It is read-only, skips any line it cannot trust, and is what the Stream screen shows; each item that moved files carries the `batch` that `lifestream file --undo --batch ID` takes back. Questions and scans leave no log yet, so they are not in the feed.

## Trust boundaries

`docs/THREAT_MODEL.md` is the authority (29 threats, each with its mitigation, status and evidence, plus the release gate). In short:

- Nothing leaves the machine except commits to the person's own private GitHub repository, after a scan, over https, with a token that lives only in the system keyring. Hestia has no account system and no server.
- The model is local only (loopback host, no redirects), and file names reach it only as cleaned data it is told to treat as data.
- The mover and capture can neither overwrite nor delete the person's files; restore is a dry run by default and holds back anything that runs by itself.
- The repository is untrusted input when it is restored from; the watcher and scripts ship with Hestia, never from the repository.
- Open items: signed releases are untested until the first real tag (#12), and the maintainer's public name and email appear on GitHub merge commits by choice (#142).

## Where we honestly are

- **Built and working:** stream and scan, rules, two-model bundling, domains, tree and mover with undo, search, home repository create/adopt/find through GitHub sign-in (engine and screens), machine fingerprint, the watcher with secret scanning and verified private pushes, safe restore, the launcher, jobs layer and status strip, design system, a redesigned Welcome, Today and sidebar, release packaging with attestation, CI that includes a clean-computer end-to-end test.
- **Not live yet:** GitHub sign-in needs the project's OAuth App Client ID (an owner step); until then the welcome says it is not set up.
- **Partly:** the UI milestone (many screens still to migrate to the design system), and trying it on a real clean OS image with a real session.
- **Not started:** the stream as the *front door* (a live, time-ordered Today with substreams, reminders and search by time). This is the heart of the original idea and the biggest gap between the vision and the product; it waits on decision #81.
- **Debt:** two private repositories (`home` and the older `homebase`) and a duplicate older copy of the code in homebase: see the plan in #7 (owner decisions #82 and #83).

## Principles for building it

- One way to do each thing; no legacy or compatibility code.
- Every slow action is a job with a button that disables and says what it is doing.
- Never destructive; confirm anything outward; everything undoable.
- Real process: issues and milestones, a branch per issue, CI, tests, no mainlining.

## Open decisions (owner)

Tracked as issues in the "M0 Decisions" milestone, each with options and a recommendation: **#81** Today as the live stream or a briefing; **#82** what lives in the home repository; **#83** one home repository for all machines; **#84** who the user is; **#85** the autonomy dial and an activity log with undo; **#86** what Hestia should feel like.
