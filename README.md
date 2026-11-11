# Hestia

**The hearth of your computer.** Hestia files your documents for you, shows your computer's
configuration and history, and lets you travel back in time through your files. Everything stays on
your machine; nothing is uploaded and no file is read by anything that is not running locally.

Hestia is a native desktop app drawn on the GPU with [Rune](https://github.com/anomalyco/rune),
backed by **Lifestream**, a read-only ingest and filing engine, and a small menu bar icon. It is written
in Rust and targets Linux (Ubuntu with GNOME first).

<!-- screenshots: add to docs/images/ and link here -->

## The idea

Files are not filed by hand. Everything that arrives goes into a time-ordered **stream**. Overnight,
Lifestream classifies it with the cheapest tool that works: deterministic rules first, then local
embeddings, then a small local model, and finally it asks you. In the morning Hestia tells you what
happened ("214 files filed, 9 need you") and asks only about what it was unsure of. Every answer
teaches it, so the questions get rarer. Folders are views over the stream, and your files are never
deleted; moves are journaled in git so they can be undone.

Everything lives in your **home repository**, an ordinary private git repository (default `~/Home`)
that holds your domains, folder tree, rules, machine fingerprints and the history of all of it.
Hestia's welcome tour creates it for you and can add Hestia itself to it as a submodule, so the tool is
tracked by the thing it manages:

```
domains/        what each part of your computer is for (edit in Settings, Domains)
machines/       one fingerprint (machine.toml) per computer you use
applications/   what runs where and how it is set up, grouped by operating system
manifest/       what must never be read, and what is backed up where
tree.toml       where each kind of file is filed
hestia/         Hestia itself, as a submodule
```

## Domains

Your digital life is split into domains. Each one is a small, editable TOML file in
[`lifestream/defaults/domains`](lifestream/defaults/domains) describing what belongs there, its folder,
its shape, and how it is backed up:

| Domain | What it holds |
|---|---|
| Documents | letters, forms, manuals, statements, notes |
| Pictures | photos, screenshots, scans |
| Videos | recordings and clips |
| Music | songs, albums, podcasts |
| Applications | installers, packages, what is installed |
| Games | save backups, disc dumps, ROMs, mods, emulator data |
| Software engineering | projects, repositories, toolchains |
| Data | datasets, exports, archives |
| Machines | one fingerprint and capture per computer you use |

Domains drive the classifier, the folders on disk and the backup policy (small things go in git, large
ones to an encrypted store). The **Files** screen shows them together: each domain, its folders with
their rules, and the contents of the folder you pick. Change them in **Settings, Domains**.

## Machines

`lifestream machine --write` fingerprints the computer (distro, version, hardware) into
`machines/<name>/machine.toml` in your home repository, and the **Machine** screen compares what was
captured under `applications/<os>/<distro>/<version>` with what is installed now. A load-aware watcher
(`hestia-watch.timer`) re-captures every few minutes, only when the computer is quiet and the battery
is fine, in small commits. It never pushes.

## The launcher

A GNOME Shell extension (`packaging/gnome-extension`) draws the **Alt+Space** launcher inside the shell,
so it is centred, instant and closes when you click away. It finds apps, files, things you copied and
Hestia itself. The same extension records a text-only clipboard history. The welcome tour turns it on.

## What's in the box

| Crate / folder | |
|---|---|
| [`ui`](ui) (`hestia`) | the desktop app: Today, Search, Time, Files, History, Machine, Questions, System, Settings |
| [`tray`](tray) (`hestia-tray`) | the animated menu bar icon (StatusNotifierItem) |
| [`lifestream`](lifestream) | the engine and CLI: `scan`, `find`, `ask`, `brief`, `file`, `machine`, ... |
| [`packaging`](packaging) | GNOME Shell extension (Alt+Space launcher and clipboard history), systemd user units, desktop entry, icons |

## Install

Requirements: Linux with systemd, GNOME Shell 50 or newer for the launcher extension (the app itself
runs anywhere Rune does), and a Rust toolchain if you build from source. [Ollama](https://ollama.com) is
optional and only needed for meaning search and the local model tier.

From a release tarball:

```bash
tar xf hestia-linux-x86_64.tar.gz && cd hestia-linux-x86_64
./install.sh
```

From source:

```bash
git clone <this repository> && cd hestia
./install.sh
```

`install.sh` puts the programs in `~/.local/bin`, installs the desktop entry, icons, the GNOME
extension and the systemd user timers (overnight scan at about 02:30, morning notification at 07:30),
and records where your home repository is. Choose it with `HESTIA_HOME`:

```bash
HESTIA_HOME=~/Home ./install.sh
```

Log out and in once so the launcher and the setting take effect.

## First run

Open Hestia from the app grid. The welcome tour walks through naming your home repository (a folder
inside your home folder, like `Home`; created if missing, with an optional Hestia submodule), the folders
to watch, the overnight and morning schedule, and turning on the launcher and clipboard history. Then try:

```bash
lifestream scan ~/Documents ~/Pictures --dry-run   # preview, writes nothing
lifestream scan ~/Documents ~/Pictures             # write the stream
lifestream brief                                   # the morning briefing
```

## Configuration

| Setting | Where | Default |
|---|---|---|
| Home repository | `HESTIA_HOME` (absolute path; `install.sh` writes `~/.config/environment.d/hestia.conf`), else the `home` setting (folder name under your home folder) | `~/Home` |
| Hestia's public repository | the `hestia_repo` setting (URL used for the submodule) | unset |
| App settings | `~/.config/hestia/settings`, edited in Settings | |
| Stream | `~/.local/share/lifestream/stream` | |

## Privacy

Only names, paths, kinds and tags are embedded, never file contents, and only by a model on this
machine: any non-loopback model host is refused. Paths you mark sensitive in your home repository's
`manifest/rules.toml` are never read, moved or sent to any model. The clipboard history is text only
and skips password managers.

## Build and test

```bash
cargo build --release
cargo test --release
```

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Licensed under the [MIT License](LICENSE).
