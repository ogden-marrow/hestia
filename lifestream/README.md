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

The one policy behind the desktop notification and the tray icon; the app can show the same state. Reads `<share>/hestia/watch.json`, `watch-history.jsonl` and `notified.json` (`--share` defaults to `$XDG_DATA_HOME` or `~/.local/share`) (plus, offline, the home repository and `private-check.json`, see below) and prints **one JSON line** (keys in this alphabetical order):

```
{"failing_runs":3,"last_ok":"2026-11-18T01:00:00Z","notify":true,"reason":"Your backup can't reach GitHub. Check your internet connection. It has not worked for about 7 hours.","since":"2026-11-18T02:00:00Z","state":"failing"}
```

- `state`: `ok`; `failing` (the latest runs, ignoring routine "waiting for a quieter moment" skips, are `FAILED`; or, with a verified private push target, the oldest commit not yet on `origin/main` is more than 24 hours old); `stale` (no successful run for more than 36 hours); `local_only` (new in #256: the home repository has no push target Hestia has verified private, and its first commit is more than 24 hours old); `unknown` (missing, empty or unreadable files, no run yet, or a home repository git cannot read). New values may be added: treat an unknown value like `unknown`.
- How the home repository is judged (offline, cheap; only when the watcher is not already `failing` or `stale`, whose sentences win): the repository is `home=` from `~/.config/hestia/settings` (else `~/Home`), judged only once it has `tree.toml`, `.git` and a commit. Its push target (`git remote get-url --push origin`) counts as verified only when it is a `https://github.com/OWNER/NAME` or `git@github.com:OWNER/NAME` URL and `<share>/hestia/private-check.json` says `{"slug":"OWNER/NAME","private":true}` (case-insensitive). One writer in the engine (`backup::record_private`, written whole through a temporary file: `at`, `slug`, `private` only) records the last definite answer, from two places: sign-in (`lifestream home adopt` and `home create` record `true` once GitHub says the repository is private, so `local_only` clears right after sign-in without waiting for the watcher; `false` when GitHub says it is public or gone) and the watcher's private check before every push, which hands its answer to `lifestream home record-private OWNER/NAME true|false` (offline, needs no sign-in; `true` when Hestia (signed in) or `gh` vouched for the repository, `false` when it is public or gone). An offline, signed-out, redirected or otherwise unclear check records nothing. Fixed sentences: `local_only` is "Your backup is only on this computer. Sign in to keep a private copy online."; unpushed commits are `failing` with "Your newest changes have not reached your private copy for over a day." (`failing_runs` 0). `since` is 24 hours after the first commit (`local_only`) or after the oldest unpushed commit, and `notify` is true at once for that episode (the 24 hours were the wait), once, like any failure.
- `since`: RFC3339 start of the problem (first failing run, or 36 hours after the last success for `stale`); `null` when ok or unknown. `failing_runs`: how many failed runs in a row (0 unless failing). `last_ok`: RFC3339 or `null`.
- `reason`: one plain sentence for a person, or `null` when ok or unknown. It is chosen from a fixed list by the kind of failure (offline, signed out, repository gone or public, SSO, a possible secret, anything else) plus how long, never copied from the watcher's text: no paths, no tokens.
- `notify`: true only when the state is `failing`, `stale` or `local_only`, has lasted 6 hours (for `local_only` and unpushed commits: from `since`, which already includes the 24-hour grace), this episode (its `since`) was not already told, and `notifications=off` is not set in `~/.config/hestia/settings`.
- `--mark-notified`: record the current episode as told (so `notify` turns false for it), or, when the state is `ok`, forget the told episode. It exits **10** (after printing) when it just forgot an episode that had been told and notifications are on, the one moment to say "working again"; otherwise 0.
- Read-only without `--mark-notified`; never prints file contents or paths.
- `rehearsal`: the last restore rehearsal, from `<share>/hestia/rehearsal.json` (written weekly by `rehearse.sh`, see below): `{"age_days":3,"at":"2026-11-15T03:12:00Z","result":"passed"}`. `result` is `passed`, `failed`, or `never` (no readable result yet: then `at` and `age_days` are `null`). `age_days` is whole days since `at` (never negative). Only the time and the verdict are read; the file's sentences are never copied. It never changes `state` or `notify`. When `reason` is not `null` and the last check failed or is older than 14 days, `reason` gets a **second line** (separated by `\n`): `Your last restore check failed: ...` or `Your backup has not been checked by a restore for more than two weeks.` A healthy backup keeps `reason: null` whatever the rehearsal says; the app should show `rehearsal` itself ("Last restore check: passed 3 days ago", or "failed", or nothing for `never`).
- `left_out` (#227): how much the backup leaves out, from the same lists `lifestream left-out` reads (below): `{"by_why":{"link_outside_home":0,"link_to_private":0,"private_by_rule":3,"too_large":1,"unreadable":0,"withheld_secret_file":0,"withheld_secret_lines":1},"count":5}`. Every reason is always present (0 when none). Counts only, never a path. It never changes `state`, `reason` or `notify`: something left out on purpose is not a failing backup.

## Left out and why (`lifestream left-out`)

```
lifestream left-out [--share DIR] [--limit N]
```

The one reader behind Computer > Backup "Left out and why" (#227). Read-only; prints **one JSON object per line, newest first, then one summary line**, and nothing else on stdout. Exit 0 even when the lists are missing or damaged (bad lines are skipped); with nothing left out it prints only the summary.

- `--share`: the data folder (default `$XDG_DATA_HOME` or `~/.local/share`); the lists are `<share>/hestia/skipped.jsonl` (written by `capture.sh` on every capture) and `<share>/hestia/withheld.jsonl` (written by `watch.sh` after its secret scan), both through the one writer `packaging/libexec/left-out.sh`. They stay on this computer and are never committed.
- `--limit`: default 100, clamped to 1..=1000; the items never exceed 512 KB (stops at a whole line). The summary always counts everything, so `sum(counts) > items printed` means there is more.

Each item (keys in this order):

```
{"path":".config/big/huge.bin","why":"too_large","detail":"120 MB is over the 50 MB limit","since":"2026-11-25T09:00:00Z","can_include":true}
```

| field | meaning |
|---|---|
| `path` | relative to the home folder (`.bashrc`, `.config/gtk-3.0/bookmarks`; hidden components are normal here). `null` when it cannot be shown safely: not plainly relative (`..`, a leading `/` or `~`, an empty component), a control, direction-changing or replacement character, a backslash, or more than 240 characters. Never cleaned or cut. For a link, this is the link, never what it points to |
| `why` | `too_large`, `unreadable`, `private_by_rule`, `withheld_secret_lines`, `withheld_secret_file`, `link_outside_home`, `link_to_private`. New reasons may be added: show an unknown one with its `detail` |
| `detail` | one plain sentence chosen by `why` and built from numbers only: `120 MB is over the 50 MB limit`; `Hestia could not read it: it is missing, a broken link, or you do not have permission to open it`; `Never backed up: it holds private things (SSH keys and known hosts)` (the kind comes from the never-capture list; otherwise `Never backed up: its name or place says it holds private things (keys, sign-ins, passwords)`); `2 lines that looked like secrets were left out; the rest of the file is backed up`; `This file looked like it holds a secret, so all of it was left out`; `This is a link to something outside your home folder, and links are not followed there`; `This is a link to a private file, so it is not followed` |
| `since` | RFC3339 UTC: when this reason and path were first seen (carried over from run to run); `null` if unknown |
| `can_include` | `true` only for `too_large` and `unreadable`: the person can fix it (make it smaller, make it readable, fix the link) and the next capture takes it. Always `false` for `private_by_rule`, the two `withheld_*` reasons and the links |

Where each comes from: `too_large` (over `HESTIA_MAX_FILE_BYTES`, 50 MB); `unreadable` (tar could not read it, or a link inside the home folder that points at nothing or at a folder); `private_by_rule` (a capture-list entry or a file inside a captured folder that the never-capture list refuses, `lifestream deny-path`); `withheld_secret_lines` (lines of a shell startup file the scanner flagged, replaced in the copy by a comment); `withheld_secret_file` (any other captured file with a finding, or a shell file holding a private key; `path` is `null` for captured system files such as `dconf.ini` that are not in the home folder); `link_outside_home` / `link_to_private` (a symlink in the capture list or inside a captured folder whose target is outside the home folder, or a file inside it that the never-capture list refuses).

Summary, always last: `{"counts":{"link_outside_home":0,"link_to_private":1,"private_by_rule":3,"too_large":1,"unreadable":0,"withheld_secret_file":1,"withheld_secret_lines":1},"event":"summary"}` (every reason present).

Never printed: file contents, the withheld lines, their line numbers, what the scanner matched, a link's target. The lists are opened only if they are regular files (never through a link) and only their first 1 MB is read. There is no "back this up anyway" action: including a private or withheld file would change the deny policy and is not offered.

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

## Updating Hestia (`lifestream update`)

```
lifestream update check [--source DIR]
lifestream update apply
lifestream update rollback
```

What Settings > About calls (#265). Nothing here runs by itself and nothing is sent anywhere: the only network use is a
`git fetch` of the public repository.

**Where things are.** `install.sh` puts every build in its own folder, `<prefix>/lib/hestia/builds/<date>-<build id>/`
(`bin/`, `libexec/`, `build-info.json`). `<prefix>/lib/hestia/current` and `previous` are links to two of them; the
installed programs (`<prefix>/bin/{hestia,hestia-tray,lifestream}`) and scripts (`<prefix>/libexec/hestia`) are links
through `current`, so the systemd units never name a build and switching is one atomic rename. Exactly one older build is
kept. `$XDG_DATA_HOME/hestia/install.json` (default `~/.local/share/hestia/`) records the installed build:

```json
{"build_id":"9641b2ee","source":"checkout","source_dir":"/home/you/hestia","installed_at":"2026-11-25T19:55:01Z","prefix":"/home/you/.local"}
```

`build_id` is the same id the app shows (`ui/build.rs`: the short commit, `+local` when tracked files had changes of their
own). `source` is `checkout` (built from a git checkout) or `release` (a downloaded tarball, which carries a `BUILD` file).

### `check`: one JSON line, always exit 0

```json
{"installed":"9641b2ee","available":"ac5f019a","newer":true,"changes":["A weekly restore rehearsal: a backup nobody has restored is a hope."],"source":"checkout","checked_at":"2026-11-25T19:56:29Z","error":null}
```

- `installed`: the build id in install.json, or `null` if Hestia was not installed with install.sh.
- `available`: origin/main's build id after the fetch, or `null` (offline, refused, a release install).
- `newer`: origin/main has commits the installed build does not (false when they are the same or the installed build is ahead).
- `changes`: up to 10 plain one-line entries added to `CHANGELOG.md` between the two (markdown, issue numbers and commit
  hashes removed, first sentence, at most 160 characters); `[]` when not newer.
- `source`: `checkout` or `release`. For `release`, `available` is `null` and `error` says updates come with signed
  releases (the hook for #244 is `release_available` in `src/update.rs`).
- `checked_at`: RFC 3339, UTC.
- `error`: `null`, or one plain sentence for a person (offline; the folder is gone; its origin is not the public repository;
  not installed with install.sh; a release install). Never a path, a URL or git's own words.

For a checkout: the folder Hestia was installed from (or `--source DIR`) must be a git work tree whose `origin`, as git will
really fetch it (after `insteadOf`), is `https://github.com/ogden-marrow/hestia(.git)`. The fetch is
`git fetch --no-tags origin main` with no credential helper, no prompt (`GIT_TERMINAL_PROMPT=0`), no redirects, only the
https protocol (and local paths, for tests), killed after 20 seconds. It updates `origin/main` inside `.git` and never
touches the working tree.

### `apply` and `rollback`: one JSON event per line, exit 0 on success and 1 on failure

```json
{"event":"step","step":"fetch","pct":5}
{"event":"step","step":"build","pct":15}        (then more build events, up to 75, as cargo compiles)
{"event":"step","step":"test","pct":80}
{"event":"step","step":"switch","pct":90}
{"event":"updated","from":"9641b2ee","to":"ac5f019a"}
```

or, at any point, a last line `{"event":"failed","message":"<one plain sentence>"}`. Before any step it refuses (and
changes nothing) when: a backup (`watch.sh`) or a restore check (`rehearse.sh`) holds its lock (the same `flock` files in
`$XDG_RUNTIME_DIR`, held until the update ends, so neither starts during it), another update is running, Hestia was not
installed from a checkout, the folder's origin is not the public repository, it is not on `main`, or tracked files have
changes of their own. Then: fetch; fast-forward the checkout to `origin/main` (`git merge --ff-only`: never a reset, a merge
commit or a force; a checkout with commits of its own is refused); `install.sh --stage` builds into a new build folder
(cargo's output goes to `update.log` next to install.json); `install.sh --smoke` checks that `lifestream --version` and
`hestia --list-scenes` (headless) work, and a build that fails is deleted; `install.sh --activate` installs the desktop
entry, icons and extension and runs the new build's `lifestream update switch`, which moves the links, removes every build
except `current` and `previous`, writes install.json and refreshes the user units (`setup-user.sh` restarts a timer only when
its unit file changed; the tray is restarted onto the new build if it was running). Until the switch, the old build stays
installed and running. The app itself keeps running the old program until it is restarted.

`rollback` takes the same locks, emits `{"event":"step","step":"switch","pct":50}` and then
`{"event":"rolled_back","from":"ac5f019a","to":"9641b2ee"}`, or `failed` ("There is no earlier version to go back to."). It
swaps `current` and `previous`, so the build it leaves is kept and a later update or rollback can return to it. The desktop
entry, icons and extension stay as the newer build installed them. `uninstall.sh` removes every build folder and install.json.
