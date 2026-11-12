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

## Trust boundaries

`docs/THREAT_MODEL.md` is the authority (21 threats, each with its mitigation, status and evidence, plus the release gate). In short:

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
