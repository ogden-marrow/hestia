# Hestia: vision and architecture

Status: draft for the owner's critique. Written 2026-11-11. Where the code does not yet match the vision, this document says so.

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
| **Engine** | The brain. Ingests files into a stream, classifies them, plans moves, scans for secrets, talks to a local model, manages the home repo. No UI. Also a CLI (`lifestream`). | `lifestream/` |
| **App** | The native window (Rune, GPU). Today, Questions, Files, History, Machine, Settings, and the launcher. Calls the engine; never does slow work on the UI thread (jobs layer). | `ui/` |
| **Tray** | Small always-on presence: shows state, lets you open the app. | `tray/` |
| **Watcher** | A systemd timer that every 15 minutes captures this machine's state (config, dotfiles, package lists) into the home repo and commits it. Shipped inside Hestia, not copied into your repo. | `packaging/libexec/` |
| **Launcher** | One search box for screens, apps, clipboard and files. A GNOME Shell extension draws it; the engine answers over a pipe. | `packaging/gnome-extension/` |

## How things flow

1. **Arrive.** New files appear in watched roots (Downloads, Documents, Pictures, Videos). A scheduled scan hashes them and writes one record per file into the **stream** (a plain, append-friendly store keyed by content hash).
2. **Classify, three tiers.** Deterministic rules first (data-driven, `rules.toml`). Then two small **local** models each suggest a label; disagreement, not self-reported confidence, decides what to ask. Everything else is left alone.
3. **Ask in bundles.** The unit of question is a folder, not a file. Questions screen: one card at a time, best guess is primary, Skip is always there.
4. **Move, safely.** A plan is built, shown, confirmed, then applied: same-disk hard-link-then-unlink or verified copy; never overwrites; refuses symlinks and hidden destinations; records old and new path in the stream so everything can be undone.
5. **Capture the machine.** The watcher writes a fingerprint and a snapshot of how the machine is set up, one commit per kind of change.
6. **Back up.** Commits go to your private remote. Before anything leaves the machine, the shared secret scanner reads it; a hit blocks the push. Auto-push is currently **off** until the scanner is wired into the watcher and the push target is verified private.
7. **Find.** Search by name, by meaning (local embeddings of metadata, never contents), by time.

## Trust boundaries (what the red team found, and what is fixed)

- Nothing leaves the machine except commits to a private repo, after a scan. *Fixed:* one scanner, modern token formats, binaries included, publish refuses a leaky history. *Open:* wire it into capture and push; verify the push target really is private (pushurl, insteadOf, forks); stop capturing `.bashrc` wholesale.
- The model is local only. *Fixed:* strict loopback check, no redirects, no user info.
- The mover can't be tricked. *Fixed:* hidden destinations, symlinks, check-then-move race.
- The home repo must not be able to run code on every machine. *Open (#48).*
- A machine rebuilt from the repo must not delete live files. *Open (#49).*
- The clipboard recorder is off by default and encrypted. *Open (#51).*

## Where we honestly are

- **Built and working:** stream and scan, rules, two-model bundling, domains, tree and mover, search, home repo creation and publish, machine fingerprint, watcher, launcher, jobs layer and status strip.
- **Partly:** safety (about half the findings fixed), UI states (design system just landed; screens not migrated), install (works on the author's machine, no clean-machine test).
- **Not started:** the stream as the *front door* (a live, time-ordered Today with substreams, reminders and search by time). This is the heart of the original idea and the biggest gap between the vision and the product.
- **Debt:** two code trees (private homebase and public Hestia) have drifted and must collapse into the public one; two settings name the home repo.

## Principles for building it

- One way to do each thing; no legacy or compatibility code.
- Every slow action is a job with a button that disables and says what it is doing.
- Never destructive; confirm anything outward; everything undoable.
- Real process: issues and milestones, a branch per issue, CI, tests, no mainlining.

## Questions for the owner

1. **Stream as front door.** Should Today be the live stream itself (events in time order, answered inline), or a briefing that summarises it? The Lifestreams paper argues for the first.
2. **Home repo scope.** Is the home repo everything (documents too) or only setup and metadata, with documents on cloud or external storage? Large media does not belong in git.
3. **Multiple machines.** Is one home repo the source of truth for all machines, with the Mac as the iCloud bridge, or is each machine its own writer?
4. **Who is the user?** The author only, or a stranger? "A stranger installs it" implies a much higher bar for onboarding, errors and support than a tool for one person.
5. **Autonomy dial.** How much should it do without asking, and how does a person see and undo it?
6. **Name and feel.** What should Hestia feel like beyond tidy: a hearth, a butler, an instrument?
