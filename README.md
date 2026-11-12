# Hestia

**The hearth of your computer.** Hestia files your documents for you, shows your computer's
configuration and history, and lets you travel back in time through your files. Files are read only by models
running on your machine. The one thing that leaves it is your own home repository, pushed to a private
repository you choose.

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
deleted; every move is logged, and Hestia refuses anything risky instead of guessing.

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
is fine, in small commits. Before committing it scans what it captured and withholds any file that looks
like a secret (the original is never touched). It pushes only to a private GitHub repository, checked on the
URL git will actually use, and only if the new commits scan clean; otherwise it says why and does not push.
Set `HESTIA_AUTOPUSH=0` to turn pushing off.

### Rebuilding a computer

`~/.local/libexec/hestia/bootstrap.sh <home-repo>` (path under your install prefix) restores a computer from what
the watcher captured. It is a **dry run** until you add `--apply`, never deletes (anything already there is moved
to `NAME.bak.<time>`), copies dotfiles and config instead of linking them into the repository, and holds back
anything that runs by itself (autostart entries, systemd user units, custom keyboard-shortcut commands) until you
have read the list and re-run with `--allow-executable`. Third-party apt sources only run with `--only sources`.

## The launcher

A GNOME Shell extension (`packaging/gnome-extension`) draws the **Alt+Space** launcher inside the shell,
so it is centred, instant and closes when you click away. It finds apps, files, things you copied and
Hestia itself. The same extension can keep a text-only clipboard history; it is off until you turn it on in Settings.

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

From a release tarball (no compiler needed). Check the download first:

```bash
sha256sum -c hestia-linux-x86_64.tar.gz.sha256          # the file is what was published
gh attestation verify hestia-linux-x86_64.tar.gz --repo ogden-marrow/hestia   # and it was built by this repository's workflow
tar xf hestia-linux-x86_64.tar.gz && cd hestia-linux-x86_64
./install.sh
```

The `.deb` installs the app, the launcher extension and the icons, but not the background watcher and its schedule;
use the tarball for the full setup. `./uninstall.sh` removes what `install.sh` put on the computer and leaves your
home repository and files alone (`--purge` also deletes Hestia's own settings and data).

From source:

```bash
git clone <this repository> && cd hestia
./install.sh
```

`install.sh` puts the programs in `~/.local/bin`, installs the desktop entry, icons, the GNOME
extension and the systemd user timers (overnight scan at about 02:30, morning notification at 07:30),
and sets the `home` setting if it is not set yet. Choose its first value with `HESTIA_HOME`:

```bash
HESTIA_HOME=~/Home ./install.sh
```

Log out and in once so the launcher and the setting take effect.

## First run

Open Hestia from the app grid. The welcome tour signs you in to GitHub and finds or creates your private home
repository. There is no Hestia account and no Hestia server: GitHub only stores your own private repository, and the
token lives only in your system keyring. (Sign-in needs a build that includes the project's GitHub Client ID; without
it the welcome says so and lets you continue.) It then asks which folders to watch, sets the overnight and morning
schedule, and offers the launcher; clipboard history stays off until you turn it on. Then try:

```bash
lifestream scan ~/Documents ~/Pictures --dry-run   # preview, writes nothing
lifestream scan ~/Documents ~/Pictures             # write the stream
lifestream brief                                   # the morning briefing
```

## Configuration

| Setting | Where | Default |
|---|---|---|
| Home repository | the `home` setting in `~/.config/hestia/settings`: the one place the app, the watcher and the restore script read (absolute, `~/…` or a folder under your home folder). `install.sh` seeds it from `HESTIA_HOME` only if it is not set; signing in, adopting or creating a repository writes it | `~/Home` |
| Hestia's public repository | the `hestia_repo` setting (URL used for the submodule) | unset |
| App settings | `~/.config/hestia/settings`, edited in Settings | |
| Stream | `~/.local/share/lifestream/stream` | |

## Privacy

Only names, paths, kinds and tags are embedded, never file contents, and only by a model on this
machine: any non-loopback model host is refused. Paths you mark sensitive in your home repository's
`manifest/rules.toml` are never read, moved or sent to any model. The clipboard history is **off until you
turn it on**. It is text only, kept in a file only you can read, skips password managers and anything that looks
like a key, token or one-time code (a filter, not a guarantee: do not rely on it for secrets), drops old entries on
a timer, and is deleted when you turn it off.

## Build and test

```bash
cargo build --release
cargo test --release
```

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Licensed under the [MIT License](LICENSE).
