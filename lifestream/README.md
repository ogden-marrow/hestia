# lifestream (phase 0)

Read-only ingest and deterministic filing. See the repository README.

```bash
cargo build --release   # from the repository root
target/release/lifestream scan ~/Documents ~/Pictures --dry-run   # preview
target/release/lifestream scan ~/Documents ~/Pictures                      # write the stream
target/release/lifestream report                                           # stats on the stream
```

More commands (phase 1a):

```bash
lifestream find game save --tag backup --year 2026   # search: words match path, kind, tags (newest first)
lifestream brief                                    # the morning briefing text
lifestream views list                               # saved views in your home repo's views/*.toml, with counts
lifestream views render                             # each view as a folder of symlinks
```

The app is Hestia, a separate Rune app in `../ui` (`hestia`): briefing header, views with counts,
the stream, a detail pane, a word filter, ask-by-meaning, and **teach** (type `kind [tags]`, e.g.
`document/manual help`; stored as a *human* decision that rules and models never overwrite).
It edits only stream records, never your files.

Meaning search (phase 1b, needs the local model from Ollama):

```bash
lifestream embed                       # embed names/tags of ~1.7k records (17 s, incremental)
lifestream ask dark souls save games   # ranked by meaning, not by words
```

- Only **names, paths, kinds and tags** are embedded, never file contents, and only by a
  model on this machine: any non-loopback host is refused. Derived files (thumbnails)
  are skipped unless `--all`. Vectors live next to the stream
  (`~/.local/share/lifestream/emb-*.f32`), not in git.

- **Views** are saved queries (`views/*.toml`): the paper's *substreams*. `views render`
  builds them as folders of **symlinks** under `~/.local/share/lifestream/views`
  (change with `--into`). It never moves or copies a file, only deletes symlinks it
  made, and refuses to touch a view folder that contains anything else.
- **Vanished files are history, not erased:** status becomes `gone` and the old
  paths are kept in `was`, so you can still search for what existed.

- **Read-only:** files are opened read-only; nothing is written inside scanned
  folders (`--out` inside a root is refused). Verified by fingerprinting folders
  before and after a 33 GB scan.
- **Fails closed:** needs `manifest/rules.toml` (the sensitive-path list). Without
  it, it refuses to scan. Listed sensitive paths and secret-looking file names
  (`.env`, `*.pem`, `id_rsa`, `*.kdbx`...) are never read, hashed or recorded.
- **Stream:** one JSON record per distinct file (id = sha256), sharded by the
  file's own month: `<out>/YYYY/MM.jsonl`. Default `<out>` is
  `~/.local/share/lifestream/stream`, outside the repo on purpose: it holds my
  file names. Commit it only when I decide to.
- **Rules** live in `rules.toml` (data, not code). Files no rule matches become
  `pending`: that list is the future AI / ask-me queue.
- **Incremental:** unchanged files (path+size+mtime) are not re-hashed; later AI or
  human decisions are never overwritten by a rule result; vanished files become `gone`.
- Git repos are recorded as one `project/git` entry (id = origin URL), so duplicate
  clones collapse into one record.

## Filing into your directory tree (`tree.toml`)

`tree.toml` (repo root) is the tree you design: `inbox` folders whose loose files get
filed, and `folder`s saying where a kind/tag/name belongs (`by_year` adds a YYYY level).
Hestia edits it; the nightly flow reads it.

- `lifestream tree [--apply]` shows the tree; `--apply` creates the folders.
- `lifestream file [--apply]` plans every move (dry run by default) and prints per-folder
  counts and what stays. `--apply` moves.
- Moves never overwrite (a clash gets a `-<hash8>` suffix, an identical file is reported as a
  duplicate), use hard-link+unlink on one disk, or copy + hash verify + remove across disks,
  and the stream record gets the new path in `sources` and the old in `was`.
- Left in place: nested folders (units are never taken apart), git repos, symlinks, secret
  names and `manifest/rules.toml` sensitive paths, files open right now, and anything not
  `filed` by a human or a rule at confidence >= 0.9.
- The nightly timer does not move files yet: add `ExecStartPost=... lifestream file --apply`
  to `lifestream-nightly.service` after a supervised first run.

**Editing the tree.** `Tree::move_folder(from, to)` is drag and drop (children carry their
paths); `move_on_disk` renames the real folder and rewrites stream paths (never merges, never
touches sensitive paths, same disk only); `Tree::add_folder` accepts a suggestion. From the
shell: `lifestream tree --move FROM TO [--apply]`. The classifier's label set is the built-in
kinds plus every kind in `tree.toml` (`bundle::kinds_for`), and a changed tree makes cached
folder suggestions stale, so `lifestream suggest` always works from the latest tree.
