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
shell: `lifestream tree --move FROM TO [--apply]`. `move_dir` / `make_dir` (`lifestream folder move|new`) are the journaled, undoable
versions (`lifestream file --undo` takes the last one back). The classifier's label set is the built-in
kinds plus every kind in `tree.toml` (`bundle::kinds_for`), and a changed tree makes cached
folder suggestions stale, so `lifestream suggest` always works from the latest tree.

## Is the backup working? (`lifestream backup-state`)

```
lifestream backup-state [--share DIR] [--mark-notified]
```

The one policy behind the desktop notification and the tray icon; the app can show the same state. Reads `<share>/hestia/watch.json`, `watch-history.jsonl` and `notified.json` (`--share` defaults to `$XDG_DATA_HOME` or `~/.local/share`) and prints **one JSON line** (keys in this alphabetical order):

```
{"failing_runs":3,"last_ok":"2026-11-18T01:00:00Z","notify":true,"reason":"Your backup can't reach GitHub. Check your internet connection. It has not worked for about 7 hours.","since":"2026-11-18T02:00:00Z","state":"failing"}
```

- `state`: `ok`; `failing` (the latest runs, ignoring routine "waiting for a quieter moment" skips, are `FAILED`); `stale` (no successful run for more than 36 hours); `unknown` (missing, empty or unreadable files, or no run yet).
- `since`: RFC3339 start of the problem (first failing run, or 36 hours after the last success for `stale`); `null` when ok or unknown. `failing_runs`: how many failed runs in a row (0 unless failing). `last_ok`: RFC3339 or `null`.
- `reason`: one plain sentence for a person, or `null` when ok or unknown. It is chosen from a fixed list by the kind of failure (offline, signed out, repository gone or public, SSO, a possible secret, anything else) plus how long, never copied from the watcher's text: no paths, no tokens.
- `notify`: true only when the state is `failing` or `stale`, has lasted 6 hours, this episode (its `since`) was not already told, and `notifications=off` is not set in `~/.config/hestia/settings`.
- `--mark-notified`: record the current episode as told (so `notify` turns false for it), or, when the state is `ok`, forget the told episode. It exits **10** (after printing) when it just forgot an episode that had been told and notifications are on, the one moment to say "working again"; otherwise 0.
- Read-only without `--mark-notified`; never prints file contents or paths.
- `rehearsal`: the last restore rehearsal, from `<share>/hestia/rehearsal.json` (written weekly by `rehearse.sh`, see below): `{"age_days":3,"at":"2026-11-15T03:12:00Z","result":"passed"}`. `result` is `passed`, `failed`, or `never` (no readable result yet: then `at` and `age_days` are `null`). `age_days` is whole days since `at` (never negative). Only the time and the verdict are read; the file's sentences are never copied. It never changes `state` or `notify`. When `reason` is not `null` and the last check failed or is older than 14 days, `reason` gets a **second line** (separated by `\n`): `Your last restore check failed: ...` or `Your backup has not been checked by a restore for more than two weeks.` A healthy backup keeps `reason: null` whatever the rehearsal says; the app should show `rehearsal` itself ("Last restore check: passed 3 days ago", or "failed", or nothing for `never`).

### The restore rehearsal (`rehearse.sh`, weekly)

```
rehearse.sh [--repo DIR] [--share DIR]      # installed beside watch.sh; exit 0 passed, 1 failed, 75 already running
```

Copies the home repository's HEAD into a private scratch folder, runs `bootstrap.sh` against a fake home there (dry run, then `--apply --only dotfiles,config`; never packages or desktop settings), verifies the result and deletes the scratch folder. Never writes the real home. Writes `<share>/hestia/rehearsal.json` (`--share` defaults to `$XDG_DATA_HOME` or `~/.local/share`):

```
{"at":1795022888,"result":"passed","checked":14,"problems":[],"repo_commit":"7aefb6a"}
```

`at` is epoch seconds (like `watch.json`); `problems` are fixed plain sentences (at most 20; no paths, contents or secrets); `repo_commit` is the short commit rehearsed, or `null`. With no home repository yet it exits 0 and writes nothing.

## The feed: what happened and who decided (`lifestream feed`)

```
lifestream feed [--since DUR|TIMESTAMP] [--limit N] [--out STREAM]
```

The one reader behind the Stream screen. Read-only; prints **one JSON object per line, newest first**, and nothing else on stdout.

- `--since`: `90s`, `30m`, `2h`, `7d`, `1w`, or an RFC3339 timestamp (`2026-11-18T09:00:00Z`). An unreadable value exits 1 with a message on stderr.
- `--limit`: default 100, clamped to 1..=1000. The whole output never exceeds 512 KB (it stops at a whole line).
- `--out`: the stream folder (default `~/.local/share/lifestream/stream`); the journal is `<stream>/moves.jsonl`, the undo log `<stream>/undone.jsonl`, the watcher history `$XDG_DATA_HOME/hestia/watch-history.jsonl` (default `~/.local/share/hestia/`).
- Exit 0 even when a source is missing or has damaged lines (those are skipped); an empty feed prints nothing.

Each item:

| field | meaning |
|---|---|
| `id` | stable string for this event (a key for the UI; not a hash) |
| `at` | RFC3339 UTC, whole seconds |
| `kind` | `filed`, `moved`, `renamed`, `folder_created`, `undone`, `backup_saved`, `backup_failed`. Reserved, not emitted yet because nothing records them: `question_asked`, `question_answered`, `scan` |
| `actor` | `hestia_rule`, `model`, `you`, `watcher`, or `unknown` (a journal line from before actors were recorded whose record is gone) |
| `reason` | one plain sentence: `rule: pdf downloads`, `both models agreed: invoice`, `you answered: Taxes`, `you pressed Undo` |
| `summary` | plain words, no jargon or hashes: `Filed 3 files into Documents/Finance`, `Renamed the folder Old to Older`, `Backup saved 12 changes` |
| `count` | files or folders in the item (for `backup_saved` the commits; for a repeating `backup_failed` how many runs) |
| `paths` | up to 5, relative to the home folder, never hidden or outside it |
| `undoable` | `true` for `filed`, `moved`, `renamed`, `folder_created` while the journal still holds the batch |
| `batch` | for those items the id to give `lifestream file --undo --batch ID` (one JSON line back: `{"event":"undone","restored":N,"skipped":[...]}`); `null` otherwise |

One journal batch can give several items when it holds different deciders; each keeps the same `batch`, and undoing the batch undoes them all. `file --undo` with no `--batch` still takes back the newest batch. An undone batch leaves the journal and appears as an `undone` item instead. A backup that fails again and again with the same words is one `backup_failed` item at its first run; runs that changed nothing and routine "waiting for a quieter moment" runs are not items.

Example:

```json
{"id":"2026-11-18T09:12:01.123456789Z:0","at":"2026-11-18T09:12:01Z","kind":"filed","actor":"hestia_rule","reason":"rule: pdf downloads","summary":"Filed 3 files into Documents/Finance","count":3,"paths":["Documents/Finance/a.pdf","Documents/Finance/b.pdf","Documents/Finance/c.pdf"],"undoable":true,"batch":"2026-11-18T09:12:01.123456789Z"}
```

The journal format (`moves.jsonl`) gained `actor` and `reason` on every line; older lines without them are read as the person's own (`you`) for folder changes, and for files from the record that was filed, otherwise `unknown`.
