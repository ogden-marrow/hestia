//! The directory tree you design (`tree.toml`) and the mover that files into it.
//!
//! The tree is plain data: `inbox` folders whose loose files get filed, and `folder`s that
//! say where a file of a given kind/tag/name belongs. The classifier never writes a path; it
//! only picks a kind. This module turns kind into a destination and moves the file, and it
//! never overwrites, never touches secrets, repos, symlinks, open files or low-confidence files.

use crate::{find_manifest, load_all, looks_secret, sensitive_paths, sha256_file, tilde, write_all, Record};
use anyhow::{bail, Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

/// A rule-tier record is trusted for moving only at or above this confidence.
const MIN_CONFIDENCE: f32 = 0.9;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Folder {
    /// relative to $HOME, e.g. "Documents/Vehicles/Saab"
    pub path: String,
    /// kind prefixes this folder accepts ("video/screen-recording", "document")
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kind: Vec<String>,
    /// tags a file must all have
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tag: Vec<String>,
    /// regex on the file name
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// file into a YYYY subfolder (the file's own year)
    #[serde(default, skip_serializing_if = "is_false")]
    pub by_year: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Tree {
    /// folders (relative to $HOME) whose loose files are filed; nested folders are never split up
    #[serde(default)]
    pub inbox: Vec<String>,
    #[serde(default, rename = "folder")]
    pub folders: Vec<Folder>,
}

fn under(p: &str, root: &str) -> bool {
    p == root || p.strip_prefix(root).is_some_and(|r| r.starts_with('/'))
}

/// A place files may be filed into: inside the home folder, never hidden. Hidden folders (`.config/autostart`,
/// `.local/share/applications`, `.ssh`) are where a moved file would run at login or unlock something,
/// so no tree, inbox or domain may name one.
pub fn rel_ok(p: &str) -> bool {
    !p.is_empty()
        && !p.starts_with('/')
        && !p.starts_with('~')
        && !p.contains(['\\', '\0'])
        && !p.chars().any(|c| c.is_control())
        && !p.split('/').any(|c| c.is_empty() || c.starts_with('.'))
}

impl Tree {
    pub fn load(path: &Path) -> Result<Tree> {
        let t: Tree = toml::from_str(&fs::read_to_string(path).with_context(|| path.display().to_string())?)
            .with_context(|| format!("parsing {}", path.display()))?;
        t.validate()?;
        Ok(t)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        self.validate()?;
        let tmp = path.with_extension("toml.tmp");
        fs::write(&tmp, toml::to_string_pretty(self)?)?;
        fs::rename(tmp, path)?;
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        for p in self.inbox.iter().chain(self.folders.iter().map(|f| &f.path)) {
            if !rel_ok(p) {
                bail!("bad path {p:?}: use a visible folder inside your home folder (no hidden folders, no .. or empty parts)");
            }
        }
        for f in &self.folders {
            if let Some(re) = &f.name {
                Regex::new(re).with_context(|| format!("folder {}: bad name regex", f.path))?;
            }
        }
        Ok(())
    }

    /// Drag and drop: rename a folder and everything under it (children carry their paths with them).
    pub fn move_folder(&mut self, from: &str, to: &str) -> Result<()> {
        if !rel_ok(from) || !rel_ok(to) {
            bail!("bad path: use a path relative to your home folder");
        }
        if under(to, from) {
            bail!("cannot move a folder into itself");
        }
        let remap = |p: &str| -> Option<String> { if under(p, from) { Some(format!("{to}{}", &p[from.len()..])) } else { None } };
        let moved: Vec<String> = self.folders.iter().filter_map(|f| remap(&f.path)).collect();
        if self.folders.iter().any(|f| !under(&f.path, from) && moved.contains(&f.path)) {
            bail!("{to} already exists in the tree");
        }
        for f in &mut self.folders {
            if let Some(n) = remap(&f.path) {
                f.path = n;
            }
        }
        for i in &mut self.inbox {
            if let Some(n) = remap(i) {
                *i = n;
            }
        }
        Ok(())
    }

    /// Accept a suggestion (or add any folder). Refuses a path that is already a folder.
    pub fn add_folder(&mut self, f: Folder) -> Result<()> {
        if self.folders.iter().any(|x| x.path == f.path) {
            bail!("{} is already in the tree", f.path);
        }
        self.folders.push(f);
        self.validate()
    }

    /// Every directory the tree implies (each folder and its parents), parents first.
    pub fn dirs(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for f in &self.folders {
            let mut acc = String::new();
            for part in f.path.split('/') {
                if !acc.is_empty() {
                    acc.push('/');
                }
                acc.push_str(part);
                if !out.contains(&acc) {
                    out.push(acc.clone());
                }
            }
        }
        out.sort();
        out
    }

    /// The first folder that accepts this file (a folder with no criteria only gives structure).
    pub fn folder_for(&self, rec: &Record, name: &str) -> Option<&Folder> {
        self.folders.iter().find(|f| {
            if f.kind.is_empty() && f.tag.is_empty() && f.name.is_none() {
                return false;
            }
            (f.kind.is_empty() || f.kind.iter().any(|k| rec.kind == *k || rec.kind.starts_with(&format!("{k}/"))))
                && f.tag.iter().all(|t| rec.tags.contains(t))
                && f.name.as_ref().is_none_or(|re| Regex::new(re).is_ok_and(|r| r.is_match(name)))
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Move {
    pub id: String,
    pub from: PathBuf,
    pub to: PathBuf,
    pub size: u64,
}

#[derive(Default, Debug)]
pub struct Plan {
    pub moves: Vec<Move>,
    /// (reason, path): everything deliberately left in place
    pub skipped: Vec<(&'static str, String)>,
}

/// Paths that must never be read or moved (the sensitive list in manifest/rules.toml).
pub fn excluded(repo_dir: &Path) -> Result<Vec<PathBuf>> {
    sensitive_paths(&repo_dir.join("manifest/rules.toml"), &crate::home()?)
}

/// What the filer would do right now, without touching anything: for a live preview in the app.
pub fn preview(recs: &[Record], tree: &Tree, repo_dir: &Path) -> Result<Plan> {
    let home = crate::home()?;
    let excluded = sensitive_paths(&repo_dir.join("manifest/rules.toml"), &home)?;
    Ok(plan(recs, tree, &home, &excluded, &open_files()))
}

/// Files any process has open right now (best effort, Linux /proc).
pub fn open_files() -> HashSet<PathBuf> {
    let mut set = HashSet::new();
    if let Ok(procs) = fs::read_dir("/proc") {
        for p in procs.flatten() {
            if let Ok(fds) = fs::read_dir(p.path().join("fd")) {
                for fd in fds.flatten() {
                    if let Ok(t) = fs::read_link(fd.path()) {
                        set.insert(t);
                    }
                }
            }
        }
    }
    set
}

fn trusted(r: &Record) -> bool {
    r.status == "filed" && r.kind != "project/git" && (r.by.tier == "human" || (r.by.tier == "rule" && r.by.confidence >= MIN_CONFIDENCE))
}

fn with_suffix(name: &str, tag: &str) -> String {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => format!("{stem}-{tag}.{ext}"),
        _ => format!("{name}-{tag}"),
    }
}

/// Decide every move without touching the disk (except reading metadata and hashing on name clashes).
pub fn plan(recs: &[Record], tree: &Tree, home: &Path, excluded: &[PathBuf], open: &HashSet<PathBuf>) -> Plan {
    let mut plan = Plan::default();
    let mut reserved: HashSet<PathBuf> = HashSet::new();
    let inbox: Vec<String> = tree.inbox.iter().map(|i| format!("~/{i}")).collect();
    for r in recs.iter().filter(|r| r.status != "gone") {
        for src in &r.sources {
            let Some((dir, name)) = src.rsplit_once('/') else { continue };
            if !inbox.iter().any(|i| i == dir) {
                continue; // not loose in an inbox: units and curated folders stay whole
            }
            let skip = |plan: &mut Plan, why: &'static str| plan.skipped.push((why, src.clone()));
            if !trusted(r) {
                skip(&mut plan, "not filed with enough confidence");
                continue;
            }
            let from = home.join(src.trim_start_matches("~/"));
            if looks_secret(name) || excluded.iter().any(|e| from.starts_with(e)) {
                skip(&mut plan, "sensitive");
                continue;
            }
            match fs::symlink_metadata(&from) {
                Ok(m) if m.is_file() => {}
                Ok(_) => {
                    skip(&mut plan, "not a regular file");
                    continue;
                }
                Err(_) => {
                    skip(&mut plan, "missing");
                    continue;
                }
            }
            if open.contains(&from) {
                skip(&mut plan, "open right now");
                continue;
            }
            let Some(folder) = tree.folder_for(r, name) else {
                skip(&mut plan, "no folder accepts it");
                continue;
            };
            let mut dest_dir = home.join(&folder.path);
            if folder.by_year && r.mtime.len() >= 4 {
                dest_dir.push(&r.mtime[0..4]);
            }
            let mut to = dest_dir.join(name);
            if to == from {
                continue;
            }
            if to.exists() || reserved.contains(&to) {
                if to.exists() && fs::metadata(&to).is_ok_and(|m| m.len() == r.size) && sha256_file(&to).ok().as_deref() == Some(r.id.as_str()) {
                    skip(&mut plan, "identical file already there (duplicate)");
                    continue;
                }
                to = dest_dir.join(with_suffix(name, r.id.trim_start_matches("sha256:").get(..8).unwrap_or("dup")));
                if to.exists() || reserved.contains(&to) {
                    skip(&mut plan, "name collision");
                    continue;
                }
            }
            reserved.insert(to.clone());
            plan.moves.push(Move { id: r.id.clone(), from, to, size: r.size });
        }
    }
    plan
}

/// Only a plain file may be moved, and only into a real directory: a symlink at either end could
/// redirect the move somewhere the plan never meant (a sensitive folder, another disk).
fn check_ends(from: &Path, to: &Path) -> Result<()> {
    let m = fs::symlink_metadata(from).with_context(|| format!("{} is gone", from.display()))?;
    if !m.file_type().is_file() {
        bail!("{} is not a plain file (symlink or special): not moved", from.display());
    }
    if let Some(d) = to.parent() {
        if fs::symlink_metadata(d).map(|m| m.file_type().is_symlink()).unwrap_or(false) {
            bail!("{} is a symlink: not moved into it", d.display());
        }
    }
    Ok(())
}

/// Same-filesystem: hard-link then unlink (atomic, refuses to overwrite). Otherwise copy,
/// verify the hash, then remove the original.
pub fn move_file(from: &Path, to: &Path) -> Result<()> {
    if let Some(d) = to.parent() {
        fs::create_dir_all(d)?;
    }
    check_ends(from, to)?;
    match fs::hard_link(from, to) {
        Ok(()) => {
            fs::remove_file(from)?;
            Ok(())
        }
        Err(e) if e.kind() == ErrorKind::AlreadyExists => bail!("{} already exists", to.display()),
        Err(_) => copy_move(from, to),
    }
}

pub fn copy_move(from: &Path, to: &Path) -> Result<()> {
    check_ends(from, to)?;
    let tmp = to.with_file_name(format!(".{}.lifestream-part", to.file_name().and_then(|n| n.to_str()).unwrap_or("file")));
    let before = sha256_file(from)?;
    // The temp is created exclusively, so a pre-planted file or link at that name is an error.
    {
        let mut src = fs::File::open(from)?;
        let mut dst = fs::OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        std::io::copy(&mut src, &mut dst)?;
    }
    if before != sha256_file(&tmp)? || before != sha256_file(from)? {
        let _ = fs::remove_file(&tmp);
        bail!("copy of {} did not verify (or it changed while copying)", from.display());
    }
    if let Ok(m) = fs::metadata(from).and_then(|m| m.modified()) {
        let _ = fs::File::options().write(true).open(&tmp).and_then(|f| f.set_modified(m));
    }
    // Publish without replacing: hard_link fails if `to` appeared in the meantime.
    let linked = fs::hard_link(&tmp, to);
    let _ = fs::remove_file(&tmp);
    match linked {
        Ok(()) => {}
        Err(e) if e.kind() == ErrorKind::AlreadyExists => bail!("{} already exists", to.display()),
        Err(e) => return Err(e.into()),
    }
    fs::remove_file(from)?;
    Ok(())
}

/// Do the moves, then record them in the stream (new path in `sources`, old path in `was`).
/// Returns (moved, failures).
pub fn apply(plan: &Plan, stream: &Path, home: &Path) -> Result<(usize, Vec<String>)> {
    let mut done: Vec<&Move> = Vec::new();
    let mut failed = Vec::new();
    for m in &plan.moves {
        match move_file(&m.from, &m.to) {
            Ok(()) => done.push(m),
            Err(e) => failed.push(format!("{}: {e}", m.from.display())),
        }
    }
    let mut recs = load_all(stream)?;
    for m in &done {
        let (old, new) = (tilde(&m.from, home), tilde(&m.to, home));
        if let Some(r) = recs.iter_mut().find(|r| r.id == m.id) {
            if let Some(s) = r.sources.iter_mut().find(|s| **s == old) {
                *s = new;
            }
            if !r.was.contains(&old) {
                r.was.push(old);
            }
        }
    }
    write_all(stream, &recs)?;
    journal_batch(stream, home, &done)?;
    Ok((done.len(), failed))
}

// ---- undo: every batch of moves is written down, so the last one can be put back ----------------------------------

fn journal_path(stream: &Path) -> PathBuf {
    stream.join("moves.jsonl")
}

/// The journal is `<stream>/moves.jsonl`, one JSON object per line; lines that share a `batch` (the time) are undone together.
/// Three kinds, told apart by `kind` (a line without one is a file move):
///   file   {"batch","id","from","to"}               `apply` moved a file from `from` to `to` (paths as `~/...`)
///   dir    {"batch","kind":"dir","from","to"}       `move_dir` renamed a folder
///   mkdir  {"batch","kind":"mkdir","to"}            `make_dir` created an empty folder
/// A journal is a file on disk, so undo treats every line as untrusted.
fn journal_append(stream: &Path, lines: &[serde_json::Value]) -> Result<()> {
    if lines.is_empty() {
        return Ok(());
    }
    let mut body = String::new();
    for l in lines {
        body.push_str(&l.to_string());
        body.push('\n');
    }
    fs::create_dir_all(stream)?;
    use std::io::Write;
    fs::OpenOptions::new().create(true).append(true).open(journal_path(stream))?.write_all(body.as_bytes())?;
    Ok(())
}

fn new_batch() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, true)
}

/// One line per file `apply` moved, all sharing one batch id.
fn journal_batch(stream: &Path, home: &Path, done: &[&Move]) -> Result<()> {
    let batch = new_batch();
    let lines: Vec<_> = done.iter().map(|m| serde_json::json!({"batch": batch, "id": m.id, "from": tilde(&m.from, home), "to": tilde(&m.to, home)})).collect();
    journal_append(stream, &lines)
}

#[derive(Debug, Default, PartialEq)]
pub struct UndoReport {
    pub restored: usize,
    /// (path as `~/…`, why it was left where it is)
    pub skipped: Vec<(String, String)>,
}

/// `~/a/b` -> /home/x/a/b, only if it is a plain, non-hidden path under the home folder (a journal is a file on disk:
/// treat it as untrusted, like everything else the mover reads).
fn under_home(home: &Path, t: &str) -> Option<PathBuf> {
    let rel = t.strip_prefix("~/")?;
    rel_ok(rel).then(|| home.join(rel))
}

/// Put the files of the LAST batch of moves back where they came from, one at a time. It never overwrites (a file
/// already at the old path stops that one), never leaves the home folder, and leaves anything it could not restore in
/// the journal so asking again retries it. The stream is updated to match what happened on disk.
pub fn undo_last(stream: &Path, home: &Path) -> Result<UndoReport> {
    let text = fs::read_to_string(journal_path(stream)).unwrap_or_default();
    let lines: Vec<serde_json::Value> = text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
    let Some(batch) = lines.last().and_then(|v| v["batch"].as_str()).map(String::from) else { return Ok(UndoReport::default()) };
    let mut recs = load_all(stream)?;
    let mut report = UndoReport::default();
    let mut stream_changed = false;
    let mut settled: Vec<usize> = Vec::new();
    for (i, v) in lines.iter().enumerate().rev() {
        if v["batch"].as_str() != Some(batch.as_str()) {
            continue;
        }
        let kind = v["kind"].as_str().unwrap_or("file");
        let (Some(to_s), from_s) = (v["to"].as_str(), v["from"].as_str().unwrap_or("")) else { continue };
        let skip = |why: &str, rep: &mut UndoReport| rep.skipped.push((to_s.to_string(), why.to_string()));
        let Some(to) = under_home(home, to_s) else {
            skip("not a plain path inside your home folder", &mut report);
            continue;
        };
        if kind == "mkdir" {
            match fs::symlink_metadata(&to) {
                Ok(m) if m.file_type().is_dir() => {}
                Ok(_) => { skip("it is not a plain folder any more", &mut report); continue }
                Err(_) => { skip("it is no longer there", &mut report); continue }
            }
            if real_dirs(home, &to).is_err() {
                skip("a symlink is on the way to it", &mut report);
                continue;
            }
            // rmdir, never recursive: a folder that gained anything since is the person's now
            match fs::remove_dir(&to) {
                Ok(()) => report.restored += 1,
                Err(e) if e.kind() == ErrorKind::DirectoryNotEmpty => skip("kept: no longer empty", &mut report),
                Err(e) => { skip(&e.to_string(), &mut report); continue }
            }
            settled.push(i); // removed, or deliberately kept: either way there is nothing left to retry
            continue;
        }
        let Some(from) = under_home(home, from_s) else {
            skip("not a plain path inside your home folder", &mut report);
            continue;
        };
        if kind == "dir" {
            match fs::symlink_metadata(&to) {
                Ok(m) if m.file_type().is_dir() => {}
                Ok(_) => { skip("it is not a plain folder any more", &mut report); continue }
                Err(_) => { skip("it is no longer there", &mut report); continue }
            }
            match rename_dir(home, &to, &from) {
                Ok(()) => {
                    report.restored += 1;
                    settled.push(i);
                    stream_changed |= retarget(&mut recs, to_s, from_s);
                }
                Err(e) => skip(&e.to_string(), &mut report),
            }
            continue;
        }
        let Some(id) = v["id"].as_str() else { continue };
        match fs::symlink_metadata(&to) {
            Ok(m) if m.file_type().is_file() => {}
            Ok(_) => { skip("it is not a plain file any more", &mut report); continue }
            Err(_) => { skip("it is no longer there", &mut report); continue }
        }
        if fs::symlink_metadata(&from).is_ok() {
            skip("something is already at the old place", &mut report);
            continue;
        }
        match move_file(&to, &from) {
            Ok(()) => {
                report.restored += 1;
                settled.push(i);
                stream_changed = true;
                if let Some(r) = recs.iter_mut().find(|r| r.id == id) {
                    if let Some(s) = r.sources.iter_mut().find(|s| s.as_str() == to_s) {
                        *s = from_s.to_string();
                    }
                    r.was.retain(|w| w != from_s);
                }
            }
            Err(e) => skip(&e.to_string(), &mut report),
        }
    }
    if !settled.is_empty() {
        if stream_changed {
            write_all(stream, &recs)?;
        }
        let keep: String = text.lines().enumerate().filter(|(i, _)| !settled.contains(i)).map(|(_, l)| format!("{l}\n")).collect();
        let tmp = journal_path(stream).with_extension("jsonl.tmp");
        fs::write(&tmp, keep)?;
        fs::rename(&tmp, journal_path(stream))?;
    }
    Ok(report)
}

/// Every part of `p` below the home folder must be a real folder: a symlink anywhere on the way down could point the
/// operation somewhere the person never meant (the home folder itself may be reached through a link; below it nothing may).
fn real_dirs(home: &Path, p: &Path) -> Result<()> {
    let rel = p.strip_prefix(home).with_context(|| format!("{} is outside your home folder", p.display()))?;
    let mut cur = home.to_path_buf();
    for c in rel.components() {
        cur.push(c);
        let m = fs::symlink_metadata(&cur).with_context(|| format!("{} does not exist", cur.display()))?;
        if !m.file_type().is_dir() {
            bail!("{} is a symlink or not a folder", cur.display());
        }
    }
    Ok(())
}

/// A folder and everything in it must live on one device: a mount point inside would make the rename fail, so
/// anything that is not one filesystem is refused.
fn one_device(root: &Path, dev: u64) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in fs::read_dir(&d).with_context(|| format!("cannot read {}", d.display()))? {
            let p = e?.path();
            let m = fs::symlink_metadata(&p)?;
            if m.file_type().is_dir() {
                if m.dev() != dev {
                    bail!("{} is on another disk or a mount point: not moved", p.display());
                }
                stack.push(p);
            }
        }
    }
    Ok(())
}

/// The one way a folder is renamed: both ends real folders (no symlink on the way), the destination does not exist,
/// the source is not a mount point and holds no other device, and the destination is on the same device. A rename, never
/// copy and delete. (Linux lets rename replace an EMPTY destination folder in a race; it can never replace one with files.)
fn rename_dir(home: &Path, src: &Path, dst: &Path) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    if src == home || dst.starts_with(src) {
        bail!("cannot move {} into itself or move your home folder", src.display());
    }
    real_dirs(home, src)?;
    let parent = dst.parent().context("no destination folder")?;
    real_dirs(home, parent).context("the destination's folder must already exist")?;
    if fs::symlink_metadata(dst).is_ok() {
        bail!("{} already exists: not merging or overwriting", dst.display());
    }
    let dev = fs::symlink_metadata(src)?.dev();
    if fs::symlink_metadata(src.parent().context("no parent")?)?.dev() != dev || fs::symlink_metadata(parent)?.dev() != dev {
        bail!("{} is a mount point or the destination is on another disk: not moved", src.display());
    }
    one_device(src, dev)?;
    fs::rename(src, dst).with_context(|| format!("moving {} to {}", src.display(), dst.display()))
}

/// Swap the `old` prefix for `new` in every stream path; says whether anything changed.
fn retarget(recs: &mut [Record], old: &str, new: &str) -> bool {
    let mut changed = false;
    for r in recs {
        for s in r.sources.iter_mut().chain(r.was.iter_mut()) {
            if under(s, old) {
                *s = format!("{new}{}", &s[old.len()..]);
                changed = true;
            }
        }
    }
    changed
}

/// Rename a real folder (paths relative to home), journal it so it can be undone, and rewrite every stream path under it.
/// Refuses what `rename_dir` refuses, hidden or odd paths, a move into itself, and sensitive paths (also a folder that
/// contains one). The journal line is written only after the rename succeeded.
pub fn move_dir(home: &Path, from: &str, to: &str, excluded: &[PathBuf], stream: &Path) -> Result<()> {
    if !rel_ok(from) || !rel_ok(to) {
        bail!("use visible folders inside your home folder (no hidden folders, no .. or empty parts)");
    }
    if under(to, from) {
        bail!("cannot move a folder into itself");
    }
    let (src, dst) = (home.join(from), home.join(to));
    if excluded.iter().any(|e| src.starts_with(e) || dst.starts_with(e) || e.starts_with(&src)) {
        bail!("{from} is or holds a sensitive path: not moved");
    }
    rename_dir(home, &src, &dst)?;
    let (old, new) = (format!("~/{from}"), format!("~/{to}"));
    journal_append(stream, &[serde_json::json!({"batch": new_batch(), "kind": "dir", "from": old, "to": new})])?;
    let mut recs = load_all(stream)?;
    if retarget(&mut recs, &old, &new) {
        write_all(stream, &recs)?;
    }
    Ok(())
}

/// Create exactly one new folder (path relative to home): its parent must already be a real folder, it must not exist,
/// and it is journaled so undo can remove it again, but only while it is still empty.
pub fn make_dir(home: &Path, path: &str, stream: &Path) -> Result<()> {
    if !rel_ok(path) {
        bail!("use a visible folder inside your home folder (no hidden folders, no .. or empty parts)");
    }
    let dir = home.join(path);
    real_dirs(home, dir.parent().context("no parent folder")?).context("the folder to put it in must already exist")?;
    match fs::create_dir(&dir) {
        Ok(()) => {}
        Err(e) if e.kind() == ErrorKind::AlreadyExists => bail!("{path} already exists"),
        Err(e) => return Err(e).with_context(|| format!("creating {path}")),
    }
    journal_append(stream, &[serde_json::json!({"batch": new_batch(), "kind": "mkdir", "to": format!("~/{path}")})])
}

/// Rename a real folder (the disk half of a drag and drop) and rewrite every stream path under it.
/// Never merges into an existing folder, never touches sensitive paths, same disk only.
pub fn move_on_disk(home: &Path, from: &str, to: &str, excluded: &[PathBuf], stream: &Path) -> Result<()> {
    let (src, dst) = (home.join(from), home.join(to));
    if excluded.iter().any(|e| src.starts_with(e) || dst.starts_with(e)) {
        bail!("{from} is a sensitive path: not moved");
    }
    if !src.exists() {
        return Ok(()); // only structure so far
    }
    if dst.exists() {
        bail!("{to} already exists: not merging");
    }
    if let Some(d) = dst.parent() {
        fs::create_dir_all(d)?;
    }
    fs::rename(&src, &dst).with_context(|| format!("moving {from} to {to} (same disk only)"))?;
    let mut recs = load_all(stream)?;
    if retarget(&mut recs, &format!("~/{from}"), &format!("~/{to}")) {
        write_all(stream, &recs)?;
    }
    Ok(())
}

pub fn print_plan(plan: &Plan, home: &Path) {
    let mut by_dest: std::collections::BTreeMap<String, (usize, u64)> = Default::default();
    for m in &plan.moves {
        let d = m.to.parent().map(|p| tilde(p, home)).unwrap_or_default();
        let e = by_dest.entry(d).or_default();
        e.0 += 1;
        e.1 += m.size;
    }
    for m in &plan.moves {
        println!("  {}  ->  {}", tilde(&m.from, home), tilde(&m.to, home));
    }
    println!("\n{} file(s) to move:", plan.moves.len());
    for (d, (n, sz)) in &by_dest {
        println!("  {n:5}  {:>9}  {d}", crate::human(*sz));
    }
    let mut why: std::collections::BTreeMap<&str, usize> = Default::default();
    for (w, _) in &plan.skipped {
        *why.entry(*w).or_default() += 1;
    }
    println!("\nleft in place:");
    for (w, n) in &why {
        println!("  {n:5}  {w}");
    }
}

/// `lifestream file`: dry run unless `apply`.
pub fn cmd_file(tree_path: Option<PathBuf>, stream: &Path, manifest: Option<PathBuf>, apply_it: bool, undo: bool) -> Result<()> {
    let home = crate::home()?;
    if undo {
        // One JSON line, like the other engine commands: {"event":"undone","restored":N,"skipped":[{"path","why"}]}
        let r = undo_last(stream, &home)?;
        println!("{}", serde_json::json!({"event": "undone", "restored": r.restored, "skipped": r.skipped.iter().map(|(p, w)| serde_json::json!({"path": p, "why": w})).collect::<Vec<_>>()}));
        return Ok(());
    }
    let tree_path = tree_path.or_else(|| crate::query::find_up("tree.toml")).context("cannot find tree.toml (run inside your home repo or pass --tree)")?;
    let tree = Tree::load(&tree_path)?;
    let Some(manifest) = manifest.or_else(find_manifest) else {
        bail!("cannot find manifest/rules.toml (sensitive-path list); refusing to move files without it");
    };
    let excluded = sensitive_paths(&manifest, &home)?;
    let recs = load_all(stream)?;
    let p = plan(&recs, &tree, &home, &excluded, &open_files());
    print_plan(&p, &home);
    if !apply_it {
        println!("\n(dry run: nothing moved; pass --apply to move)");
        return Ok(());
    }
    let (n, failed) = apply(&p, stream, &home)?;
    println!("\nmoved {n}; {} failed", failed.len());
    for f in &failed {
        println!("  FAILED {f}");
    }
    if !failed.is_empty() {
        bail!("{} move(s) failed", failed.len());
    }
    Ok(())
}

/// `lifestream folder move FROM TO`. One JSON line, exit 1 if refused:
/// {"event":"moved","from","to"} or {"event":"failed","kind":"refused","message"}
pub fn cmd_folder_move(from: &str, to: &str, stream: &Path, manifest: Option<PathBuf>) -> Result<()> {
    let home = crate::home()?;
    let res = manifest.or_else(find_manifest).context("cannot find manifest/rules.toml (sensitive-path list); refusing to move folders without it")
        .and_then(|m| sensitive_paths(&m, &home))
        .and_then(|excluded| move_dir(&home, from, to, &excluded, stream));
    folder_event(res, serde_json::json!({"event": "moved", "from": from, "to": to}))
}

/// `lifestream folder new PATH`: {"event":"created","path"} or the same "failed" event.
pub fn cmd_folder_new(path: &str, stream: &Path) -> Result<()> {
    folder_event(make_dir(&crate::home()?, path, stream), serde_json::json!({"event": "created", "path": path}))
}

fn folder_event(res: Result<()>, ok: serde_json::Value) -> Result<()> {
    match res {
        Ok(()) => println!("{ok}"),
        Err(e) => {
            println!("{}", serde_json::json!({"event": "failed", "kind": "refused", "message": format!("{e:#}")}));
            std::process::exit(1);
        }
    }
    Ok(())
}

/// `lifestream tree`: show the tree; with `apply`, create the folders.
pub fn cmd_tree(tree_path: Option<PathBuf>, apply_it: bool, mv: Vec<String>) -> Result<()> {
    let home = crate::home()?;
    let tree_path = tree_path.or_else(|| crate::query::find_up("tree.toml")).context("cannot find tree.toml (run inside your home repo or pass --tree)")?;
    let mut tree = Tree::load(&tree_path)?;
    if let [from, to] = mv.as_slice() {
        tree.move_folder(from, to)?;
        println!("{from} -> {to} (with children)");
        if apply_it {
            let manifest = find_manifest().context("cannot find manifest/rules.toml")?;
            move_on_disk(&home, from, to, &sensitive_paths(&manifest, &home)?, &crate::stream_dir(None)?)?;
            tree.save(&tree_path)?;
        } else {
            println!("(dry run: tree.toml and disk unchanged)");
        }
    }
    println!("inbox: {}", tree.inbox.join(", "));
    for d in tree.dirs() {
        let p = home.join(&d);
        let mark = if p.is_dir() { " " } else { "+" };
        println!("  {mark} ~/{d}");
        if apply_it {
            fs::create_dir_all(&p)?;
        }
    }
    if !apply_it {
        println!("\n(+ = would be created; pass --apply to create)");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn hidden_and_odd_destinations_are_refused() {
        for p in [".config/autostart", ".local/share/applications", ".ssh", "Documents/.hidden", "Documents/..", "Documents//x", "/etc", "~/x", "a\\b", "a\nb", "", ".", "Docs/./x"] {
            assert!(!super::rel_ok(p), "should refuse {p:?}");
        }
        for p in ["Documents", "Pictures/2026/Trips", "Games/Dark Souls", "Software/app-bundles"] {
            assert!(super::rel_ok(p), "should allow {p:?}");
        }
        let bad_inbox = super::Tree { inbox: vec![".ssh".into()], ..Default::default() };
        assert!(bad_inbox.validate().is_err());
        let bad_folder = super::Tree { folders: vec![super::Folder { path: ".config/autostart".into(), ..Default::default() }], ..Default::default() };
        assert!(bad_folder.validate().is_err());
    }
    use super::*;
    use crate::By;

    fn rec(id: &str, kind: &str, src: &str, size: u64, tier: &str, conf: f32) -> Record {
        Record {
            id: id.into(), first_seen: "n".into(), mtime: "2025-09-15T00:00:00Z".into(), kind: kind.into(), size,
            sources: vec![src.into()], tags: vec![], summary: None,
            by: By { tier: tier.into(), name: "t".into(), confidence: conf },
            status: "filed".into(), store: None, future_at: None, was: vec![],
        }
    }

    fn tree() -> Tree {
        Tree {
            inbox: vec!["Downloads".into()],
            folders: vec![
                Folder { path: "Documents/Papers".into(), kind: vec!["document".into()], by_year: true, ..Default::default() },
                Folder { path: "Pictures/Saved".into(), kind: vec!["image".into()], ..Default::default() },
            ],
        }
    }

    fn sandbox(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ls-tree-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(d.join("Downloads")).unwrap();
        d
    }

    fn file(home: &Path, rel: &str, body: &[u8]) -> String {
        let p = home.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, body).unwrap();
        format!("sha256:{}", &sha256_file(&p).unwrap()["sha256:".len()..])
    }

    #[test]
    fn plans_by_kind_and_year_and_leaves_units_alone() {
        let h = sandbox("kind");
        let a = file(&h, "Downloads/a.pdf", b"a");
        let b = file(&h, "Downloads/b.png", b"b");
        let c = file(&h, "Downloads/App/c.pdf", b"c");
        let recs = vec![rec(&a, "document/pdf", "~/Downloads/a.pdf", 1, "rule", 1.0), rec(&b, "image", "~/Downloads/b.png", 1, "rule", 1.0), rec(&c, "document/pdf", "~/Downloads/App/c.pdf", 1, "rule", 1.0)];
        let p = plan(&recs, &tree(), &h, &[], &HashSet::new());
        let to: Vec<_> = p.moves.iter().map(|m| m.to.strip_prefix(&h).unwrap().to_str().unwrap().to_string()).collect();
        assert_eq!(to, vec!["Documents/Papers/2025/a.pdf", "Pictures/Saved/b.png"]);
        assert!(p.skipped.is_empty(), "nested folder is not even looked at");
        fs::remove_dir_all(h).unwrap();
    }

    #[test]
    fn skips_sensitive_secret_symlink_open_lowconf_and_unmatched() {
        let h = sandbox("skip");
        let ids: Vec<String> = ["s.pdf", "k.pdf", "l.pdf", "o.pdf", "w.pdf", "u.xyz"].iter().map(|n| file(&h, &format!("Downloads/{n}"), n.as_bytes())).collect();
        fs::create_dir_all(h.join("Downloads/sec")).unwrap();
        std::os::unix::fs::symlink(h.join("Downloads/s.pdf"), h.join("Downloads/link.pdf")).unwrap();
        let mut recs = vec![
            rec(&ids[0], "document/pdf", "~/Downloads/s.pdf", 5, "rule", 1.0),
            rec(&ids[1], "document/pdf", "~/Downloads/.env", 5, "rule", 1.0),
            rec("x", "document/pdf", "~/Downloads/link.pdf", 5, "rule", 1.0),
            rec(&ids[3], "document/pdf", "~/Downloads/o.pdf", 5, "rule", 1.0),
            rec(&ids[4], "document/pdf", "~/Downloads/w.pdf", 5, "rule", 0.5),
            rec(&ids[5], "weird", "~/Downloads/u.xyz", 5, "rule", 1.0),
        ];
        recs[0].sources[0] = "~/Downloads/s.pdf".into();
        let excluded = vec![h.join("Downloads/s.pdf")];
        let open: HashSet<PathBuf> = [h.join("Downloads/o.pdf")].into();
        let p = plan(&recs, &tree(), &h, &excluded, &open);
        assert!(p.moves.is_empty(), "{:?}", p.moves);
        let why: Vec<&str> = p.skipped.iter().map(|s| s.0).collect();
        assert_eq!(why, vec!["sensitive", "sensitive", "not a regular file", "open right now", "not filed with enough confidence", "no folder accepts it"]);
        let mut pend = rec(&ids[0], "document/pdf", "~/Downloads/s.pdf", 5, "human", 1.0);
        pend.status = "pending".into();
        assert!(plan(&[pend], &tree(), &h, &[], &HashSet::new()).moves.is_empty());
        fs::remove_dir_all(h).unwrap();
    }

    #[test]
    fn collisions_get_a_deterministic_suffix_never_overwrite() {
        let h = sandbox("coll");
        let a = file(&h, "Downloads/x.png", b"new");
        file(&h, "Pictures/Saved/x.png", b"old");
        let r = rec(&a, "image", "~/Downloads/x.png", 3, "rule", 1.0);
        let p = plan(std::slice::from_ref(&r), &tree(), &h, &[], &HashSet::new());
        let want = format!("x-{}.png", &a["sha256:".len()..]["".len()..][..8]);
        assert_eq!(p.moves[0].to.file_name().unwrap().to_str().unwrap(), want);
        assert_eq!(fs::read(h.join("Pictures/Saved/x.png")).unwrap(), b"old");
        // identical content already filed: reported, not duplicated
        file(&h, "Pictures/Saved/x.png", b"new");
        let p2 = plan(&[r], &tree(), &h, &[], &HashSet::new());
        assert!(p2.moves.is_empty());
        assert_eq!(p2.skipped[0].0, "identical file already there (duplicate)");
        fs::remove_dir_all(h).unwrap();
    }

    #[test]
    fn apply_moves_updates_stream_and_rerun_is_idempotent() {
        let h = sandbox("apply");
        let a = file(&h, "Downloads/a.png", b"pic");
        let stream = h.join("stream");
        write_all(&stream, &[rec(&a, "image", "~/Downloads/a.png", 3, "rule", 1.0)]).unwrap();
        let recs = load_all(&stream).unwrap();
        let p = plan(&recs, &tree(), &h, &[], &HashSet::new());
        let (n, failed) = apply(&p, &stream, &h).unwrap();
        assert_eq!((n, failed.len()), (1, 0));
        assert!(!h.join("Downloads/a.png").exists() && h.join("Pictures/Saved/a.png").exists());
        let r = &load_all(&stream).unwrap()[0];
        assert_eq!((r.sources.clone(), r.was.clone()), (vec!["~/Pictures/Saved/a.png".to_string()], vec!["~/Downloads/a.png".to_string()]));
        let again = plan(&load_all(&stream).unwrap(), &tree(), &h, &[], &HashSet::new());
        assert!(again.moves.is_empty() && again.skipped.is_empty());
        fs::remove_dir_all(h).unwrap();
    }

    #[test]
    fn move_refuses_symlinks_at_either_end() {
        use std::os::unix::fs::symlink;
        let p = &sandbox("symlinks");
        fs::write(p.join("real"), b"x").unwrap();
        symlink(p.join("real"), p.join("link")).unwrap();
        assert!(move_file(&p.join("link"), &p.join("out")).is_err());
        assert!(p.join("link").exists() && !p.join("out").exists());
        fs::create_dir(p.join("dir")).unwrap();
        symlink(p.join("dir"), p.join("dirlink")).unwrap();
        assert!(move_file(&p.join("real"), &p.join("dirlink/real")).is_err());
        assert!(p.join("real").exists());
        assert!(copy_move(&p.join("real"), &p.join("dirlink/real")).is_err());
    }

    #[test]
    fn move_refuses_to_overwrite_and_cross_device_copy_verifies() {
        let h = sandbox("mv");
        file(&h, "Downloads/a.bin", b"data");
        file(&h, "out/a.bin", b"keep");
        assert!(move_file(&h.join("Downloads/a.bin"), &h.join("out/a.bin")).is_err());
        assert_eq!(fs::read(h.join("out/a.bin")).unwrap(), b"keep");
        assert!(h.join("Downloads/a.bin").exists());
        // the copy path (used across devices)
        copy_move(&h.join("Downloads/a.bin"), &h.join("out/b.bin")).unwrap();
        assert_eq!(fs::read(h.join("out/b.bin")).unwrap(), b"data");
        assert!(!h.join("Downloads/a.bin").exists());
        assert!(copy_move(&h.join("out/b.bin"), &h.join("out/a.bin")).is_err());
        fs::remove_dir_all(h).unwrap();
    }

    #[test]
    fn drag_and_drop_carries_children_and_refuses_bad_moves() {
        let mut t = Tree { inbox: vec![], folders: vec![
            Folder { path: "Documents/Vehicles".into(), ..Default::default() },
            Folder { path: "Documents/Vehicles/Saab".into(), kind: vec!["document".into()], ..Default::default() },
            Folder { path: "Archive".into(), ..Default::default() },
        ] };
        assert!(t.move_folder("Documents/Vehicles", "Documents/Vehicles/Inner").is_err());
        assert!(t.move_folder("Documents/Vehicles", "Archive").is_err());
        t.move_folder("Documents/Vehicles", "Archive/Vehicles").unwrap();
        let paths: Vec<_> = t.folders.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, vec!["Archive/Vehicles", "Archive/Vehicles/Saab", "Archive"]);
        assert!(t.add_folder(Folder { path: "Archive".into(), ..Default::default() }).is_err());
        t.add_folder(Folder { path: "Games/Wii".into(), ..Default::default() }).unwrap();
    }

    #[test]
    fn disk_move_rewrites_stream_and_never_merges_or_touches_sensitive() {
        let h = sandbox("dnd");
        let id = file(&h, "Documents/Saab/a.bin", b"a");
        let stream = h.join("stream");
        write_all(&stream, &[rec(&id, "document", "~/Documents/Saab/a.bin", 1, "rule", 1.0)]).unwrap();
        fs::create_dir_all(h.join("Taken")).unwrap();
        assert!(move_on_disk(&h, "Documents/Saab", "Taken", &[], &stream).is_err(), "no merge");
        assert!(move_on_disk(&h, "Documents/Saab", "Vehicles/Saab", &[h.join("Documents")], &stream).is_err(), "sensitive");
        move_on_disk(&h, "Documents/Saab", "Vehicles/Saab", &[], &stream).unwrap();
        assert!(h.join("Vehicles/Saab/a.bin").exists() && !h.join("Documents/Saab").exists());
        let r = &load_all(&stream).unwrap()[0];
        assert_eq!((r.sources[0].as_str(), r.was.len()), ("~/Vehicles/Saab/a.bin", 0));
        move_on_disk(&h, "Nothing/Here", "X", &[], &stream).unwrap();
        fs::remove_dir_all(h).unwrap();
    }

    #[test]
    fn tree_validates_dirs_and_roundtrips() {
        let t = tree();
        assert_eq!(t.dirs(), vec!["Documents", "Documents/Papers", "Pictures", "Pictures/Saved"]);
        let bad = Tree { inbox: vec!["../x".into()], ..Default::default() };
        assert!(bad.validate().is_err());
        let badre = Tree { folders: vec![Folder { path: "a".into(), name: Some("(".into()), ..Default::default() }], ..Default::default() };
        assert!(badre.validate().is_err());
        let f = std::env::temp_dir().join(format!("ls-tree-rt-{}.toml", std::process::id()));
        t.save(&f).unwrap();
        assert_eq!(Tree::load(&f).unwrap(), t);
        fs::remove_file(f).unwrap();
    }
    fn applied(tag: &str, names: &[&str]) -> (PathBuf, PathBuf) {
        let h = sandbox(tag);
        let stream = h.join("stream");
        let recs: Vec<Record> = names.iter().map(|n| { let id = file(&h, &format!("Downloads/{n}"), n.as_bytes()); rec(&id, "image", &format!("~/Downloads/{n}"), n.len() as u64, "rule", 1.0) }).collect();
        write_all(&stream, &recs).unwrap();
        (h, stream)
    }

    fn move_one(h: &Path, stream: &Path, name: &str) {
        let p = plan(&load_all(stream).unwrap(), &tree(), h, &[], &HashSet::new());
        let only = Plan { moves: p.moves.into_iter().filter(|m| m.from.ends_with(name)).collect(), skipped: vec![] };
        assert_eq!(apply(&only, stream, h).unwrap().0, 1);
    }

    #[test]
    fn undo_puts_the_file_back_and_the_stream_agrees() {
        let (h, stream) = applied("undo1", &["a.png"]);
        move_one(&h, &stream, "a.png");
        assert!(h.join("Pictures/Saved/a.png").exists() && !h.join("Downloads/a.png").exists());
        let r = undo_last(&stream, &h).unwrap();
        assert_eq!((r.restored, r.skipped.len()), (1, 0), "{r:?}");
        assert!(h.join("Downloads/a.png").exists() && !h.join("Pictures/Saved/a.png").exists());
        let rec = &load_all(&stream).unwrap()[0];
        assert_eq!((rec.sources.clone(), rec.was.len()), (vec!["~/Downloads/a.png".to_string()], 0));
        assert_eq!(undo_last(&stream, &h).unwrap(), UndoReport::default(), "nothing left to undo");
        fs::remove_dir_all(h).unwrap();
    }

    #[test]
    fn undo_never_overwrites_and_remembers_what_it_could_not_do() {
        let (h, stream) = applied("undo2", &["a.png"]);
        move_one(&h, &stream, "a.png");
        fs::write(h.join("Downloads/a.png"), b"a different file the person made since").unwrap();
        let r = undo_last(&stream, &h).unwrap();
        assert_eq!(r.restored, 0);
        assert!(r.skipped.iter().any(|(p, why)| p.ends_with("a.png") && why.contains("already")), "{r:?}");
        assert_eq!(fs::read(h.join("Downloads/a.png")).unwrap(), b"a different file the person made since");
        assert!(h.join("Pictures/Saved/a.png").exists(), "the filed copy stays");
        fs::remove_file(h.join("Downloads/a.png")).unwrap();
        assert_eq!(undo_last(&stream, &h).unwrap().restored, 1, "asking again retries what was left");
        // a file the person has since moved or deleted is reported, not invented
        let (h2, stream2) = applied("undo3", &["b.png"]);
        move_one(&h2, &stream2, "b.png");
        fs::remove_file(h2.join("Pictures/Saved/b.png")).unwrap();
        let r = undo_last(&stream2, &h2).unwrap();
        assert!(r.restored == 0 && r.skipped[0].1.contains("no longer there"), "{r:?}");
        fs::remove_dir_all(h).unwrap();
        fs::remove_dir_all(h2).unwrap();
    }

    #[test]
    fn undo_takes_back_only_the_last_batch() {
        let (h, stream) = applied("undo4", &["a.png", "b.png"]);
        move_one(&h, &stream, "a.png");
        move_one(&h, &stream, "b.png");
        assert_eq!(undo_last(&stream, &h).unwrap().restored, 1);
        assert!(h.join("Downloads/b.png").exists() && h.join("Pictures/Saved/a.png").exists(), "only the later batch comes back");
        assert_eq!(undo_last(&stream, &h).unwrap().restored, 1);
        assert!(h.join("Downloads/a.png").exists());
        fs::remove_dir_all(h).unwrap();
    }

    #[test]
    fn undo_treats_the_journal_as_untrusted() {
        let (h, stream) = applied("undo5", &["a.png"]);
        move_one(&h, &stream, "a.png");
        let j = journal_path(&stream);
        let id = load_all(&stream).unwrap()[0].id.clone();
        let batch = "9999-12-31T00:00:00.000000000Z";
        fs::create_dir_all(h.join(".ssh")).unwrap();
        let mut evil = String::new();
        for (from, to) in [("~/.ssh/authorized_keys", "~/Pictures/Saved/a.png"), ("/etc/cron.d/x", "~/Pictures/Saved/a.png"), ("~/Downloads/../.ssh/k", "~/Pictures/Saved/a.png"), ("~/Downloads/x", "/etc/passwd")] {
            evil.push_str(&format!("{}\n", serde_json::json!({"batch": batch, "id": id, "from": from, "to": to})));
        }
        use std::io::Write;
        fs::OpenOptions::new().append(true).open(&j).unwrap().write_all(evil.as_bytes()).unwrap();
        let r = undo_last(&stream, &h).unwrap();
        assert_eq!(r.restored, 0);
        assert_eq!(r.skipped.len(), 4, "{r:?}");
        assert!(h.join("Pictures/Saved/a.png").exists() && !h.join(".ssh/authorized_keys").exists(), "nothing moved outside the rules");
        fs::remove_dir_all(h).unwrap();
    }

    // ---- folders: move_dir, make_dir and their undo (always under a temp HOME, never the real one) ----

    fn home_box(tag: &str) -> (PathBuf, PathBuf) {
        let h = sandbox(tag);
        assert!(h.starts_with(std::env::temp_dir()), "tests only ever touch the temp dir");
        let h = fs::canonicalize(h).unwrap();
        (h.clone(), h.join("stream"))
    }

    #[test]
    fn move_dir_renames_rewrites_the_stream_and_undo_puts_it_back() {
        let (h, stream) = home_box("dir1");
        let id = file(&h, "Documents/Saab/a.bin", b"a");
        write_all(&stream, &[rec(&id, "document", "~/Documents/Saab/a.bin", 1, "rule", 1.0)]).unwrap();
        fs::create_dir_all(h.join("Vehicles")).unwrap();
        move_dir(&h, "Documents/Saab", "Vehicles/Saab", &[], &stream).unwrap();
        assert!(h.join("Vehicles/Saab/a.bin").exists() && !h.join("Documents/Saab").exists());
        assert_eq!(load_all(&stream).unwrap()[0].sources, vec!["~/Vehicles/Saab/a.bin".to_string()]);
        let r = undo_last(&stream, &h).unwrap();
        assert_eq!((r.restored, r.skipped.len()), (1, 0), "{r:?}");
        assert!(h.join("Documents/Saab/a.bin").exists() && !h.join("Vehicles/Saab").exists());
        assert_eq!(load_all(&stream).unwrap()[0].sources, vec!["~/Documents/Saab/a.bin".to_string()]);
        assert_eq!(undo_last(&stream, &h).unwrap(), UndoReport::default());
        fs::remove_dir_all(h).unwrap();
    }

    #[test]
    fn move_dir_refusals_leave_everything_in_place() {
        use std::os::unix::fs::symlink;
        let (h, stream) = home_box("dir2");
        file(&h, "Documents/Saab/a.bin", b"a");
        file(&h, "Taken/keep.bin", b"keep");
        file(&h, "Documents/Saab/Inner/b.bin", b"b");
        fs::create_dir_all(h.join("Sensitive/Deep")).unwrap();
        fs::create_dir_all(h.join(".ssh")).unwrap();
        fs::create_dir_all(h.join("EmptyDest")).unwrap();
        symlink(h.join("Documents"), h.join("DocsLink")).unwrap();
        symlink(h.join("Documents/Saab"), h.join("SaabLink")).unwrap();
        symlink(h.join("Taken"), h.join("TakenLink")).unwrap();
        let ex = [h.join("Sensitive/Deep")];
        for (from, to, why) in [
            ("Documents/Saab", "Taken", "overwrite of an existing destination"),
            ("Documents/Saab", "EmptyDest", "replacing an existing empty destination"),
            ("Documents/Saab", "Nowhere/Saab", "destination parent missing"),
            ("Documents/Saab", "TakenLink/Saab", "destination parent is a symlink"),
            ("SaabLink", "Moved", "source is a symlink"),
            ("DocsLink/Saab", "Moved", "an ancestor of the source is a symlink"),
            ("Documents/Saab", "DocsLink/Moved", "an ancestor of the destination is a symlink"),
            ("Documents/Saab", ".hidden", "hidden destination"),
            ("Documents/Saab", ".ssh/Saab", "hidden destination folder"),
            ("Documents/Saab", "Documents/Saab", "onto itself"),
            ("Documents/Saab", "Documents/Saab/Inner/Saab", "into its own descendant"),
            ("Documents/Saab", "Documents/Saab/Inner", "into itself"),
            ("Documents", "Documents/Saab/Inner/D", "an ancestor into its own descendant"),
            ("Documents/Saab", "../Escaped", "outside the home folder"),
            ("Documents/Saab", "/tmp/Escaped", "absolute path"),
            ("", "Moved", "the home folder itself"),
            (".", "Moved", "the home folder as a dot"),
            (".ssh", "Moved", "a hidden source"),
            ("Sensitive", "Moved", "a folder that holds a sensitive path"),
            ("Documents/Saab", "Sensitive/Deep/Saab", "into a sensitive path"),
            ("Missing", "Moved", "a source that is not there"),
            ("Taken/keep.bin", "Moved", "a file, not a folder"),
        ] {
            assert!(move_dir(&h, from, to, &ex, &stream).is_err(), "{why}: {from} -> {to}");
        }
        assert!(h.join("Documents/Saab/a.bin").exists() && h.join("Documents/Saab/Inner/b.bin").exists());
        assert_eq!(fs::read(h.join("Taken/keep.bin")).unwrap(), b"keep");
        assert!(h.join("EmptyDest").is_dir() && !h.join("EmptyDest/a.bin").exists());
        assert!(!h.join("Moved").exists() && !h.join("Documents/Saab/Inner/Saab").exists());
        assert!(!journal_path(&stream).exists(), "a refusal writes nothing to the journal");
        fs::remove_dir_all(h).unwrap();
    }

    #[test]
    fn a_folder_holding_another_device_is_refused() {
        use std::os::unix::fs::MetadataExt;
        let (h, _) = home_box("dev");
        file(&h, "A/B/C/x", b"x");
        let dev = fs::metadata(h.join("A")).unwrap().dev();
        assert!(one_device(&h.join("A"), dev).is_ok());
        assert!(one_device(&h.join("A"), dev + 1).is_err(), "a different device anywhere below fails closed");
        fs::remove_dir_all(h).unwrap();
    }

    #[test]
    fn make_dir_creates_one_folder_and_refuses_the_rest() {
        use std::os::unix::fs::symlink;
        let (h, stream) = home_box("mk1");
        fs::create_dir_all(h.join("Documents")).unwrap();
        symlink(h.join("Documents"), h.join("Link")).unwrap();
        make_dir(&h, "Documents/New", &stream).unwrap();
        assert!(h.join("Documents/New").is_dir());
        for (p, why) in [
            ("Documents/New", "exists"),
            ("Documents", "exists"),
            ("Link", "exists as a symlink"),
            ("Link/Sub", "symlink parent"),
            ("Nope/Sub", "parent missing"),
            (".hidden", "hidden"),
            ("Documents/.hidden", "hidden"),
            ("../Out", "outside home"),
            ("/tmp/Out", "absolute"),
            ("", "empty"),
            ("a//b", "empty part"),
        ] {
            assert!(make_dir(&h, p, &stream).is_err(), "{why}: {p}");
        }
        assert!(!h.join("Documents/.hidden").exists() && !h.join("Nope").exists() && !h.join("Documents/Sub").exists());
        assert_eq!(fs::read_to_string(journal_path(&stream)).unwrap().lines().count(), 1, "only the success is journaled");
        fs::remove_dir_all(h).unwrap();
    }

    #[test]
    fn undo_of_mkdir_removes_an_empty_folder_and_keeps_one_that_gained_a_file() {
        let (h, stream) = home_box("mk2");
        make_dir(&h, "Fresh", &stream).unwrap();
        let r = undo_last(&stream, &h).unwrap();
        assert_eq!((r.restored, r.skipped.len()), (1, 0), "{r:?}");
        assert!(!h.join("Fresh").exists());
        make_dir(&h, "Used", &stream).unwrap();
        fs::write(h.join("Used/mine.txt"), b"mine").unwrap();
        let r = undo_last(&stream, &h).unwrap();
        assert_eq!(r.restored, 0);
        assert_eq!(r.skipped, vec![("~/Used".to_string(), "kept: no longer empty".to_string())]);
        assert_eq!(fs::read(h.join("Used/mine.txt")).unwrap(), b"mine");
        assert_eq!(undo_last(&stream, &h).unwrap(), UndoReport::default(), "a kept folder is settled, not retried forever");
        fs::remove_dir_all(h).unwrap();
    }

    #[test]
    fn undo_of_a_folder_move_never_overwrites_and_can_be_retried() {
        let (h, stream) = home_box("dir3");
        file(&h, "Documents/Saab/a.bin", b"a");
        move_dir(&h, "Documents/Saab", "Saab", &[], &stream).unwrap();
        file(&h, "Documents/Saab/other.bin", b"made since");
        let r = undo_last(&stream, &h).unwrap();
        assert_eq!(r.restored, 0);
        assert!(r.skipped[0].1.contains("already exists"), "{r:?}");
        assert!(h.join("Saab/a.bin").exists() && h.join("Documents/Saab/other.bin").exists());
        fs::remove_dir_all(h.join("Documents/Saab")).unwrap();
        assert_eq!(undo_last(&stream, &h).unwrap().restored, 1, "asking again retries");
        assert!(h.join("Documents/Saab/a.bin").exists());
        fs::remove_dir_all(h).unwrap();
    }

    #[test]
    fn undo_takes_back_mixed_kinds_one_batch_at_a_time() {
        let (h, stream) = home_box("mix");
        make_dir(&h, "Box", &stream).unwrap();
        move_dir(&h, "Box", "Crate", &[], &stream).unwrap();
        assert_eq!(undo_last(&stream, &h).unwrap().restored, 1);
        assert!(h.join("Box").is_dir() && !h.join("Crate").exists());
        assert_eq!(undo_last(&stream, &h).unwrap().restored, 1);
        assert!(!h.join("Box").exists());
        fs::remove_dir_all(h).unwrap();
    }

    #[test]
    fn undo_of_folders_treats_the_journal_as_untrusted() {
        use std::os::unix::fs::symlink;
        let (h, stream) = home_box("tamper");
        fs::create_dir_all(h.join(".ssh/keys")).unwrap();
        fs::create_dir_all(h.join("Real/Empty")).unwrap();
        symlink(h.join("Real"), h.join("Link")).unwrap();
        let outside = std::env::temp_dir().join(format!("ls-outside-{}", std::process::id()));
        fs::create_dir_all(&outside).unwrap();
        let batch = "9999-12-31T00:00:00.000000000Z";
        let out_name = outside.file_name().unwrap().to_str().unwrap();
        let mut evil = String::new();
        for l in [
            serde_json::json!({"batch": batch, "kind": "mkdir", "to": "/etc"}),
            serde_json::json!({"batch": batch, "kind": "mkdir", "to": "~/.ssh/keys"}),
            serde_json::json!({"batch": batch, "kind": "mkdir", "to": "~/Real/../.ssh/keys"}),
            serde_json::json!({"batch": batch, "kind": "mkdir", "to": format!("~/../{out_name}")}),
            serde_json::json!({"batch": batch, "kind": "mkdir", "to": "~/Link/Empty"}),
            serde_json::json!({"batch": batch, "kind": "dir", "from": "~/.ssh/new", "to": "~/Real/Empty"}),
            serde_json::json!({"batch": batch, "kind": "dir", "from": "/etc/x", "to": "~/Real/Empty"}),
            serde_json::json!({"batch": batch, "kind": "dir", "from": "~/Moved", "to": "~/Link/Empty"}),
            serde_json::json!({"batch": batch, "kind": "dir", "from": "~/Moved", "to": "/etc"}),
        ] {
            evil.push_str(&format!("{l}\n"));
        }
        fs::create_dir_all(&stream).unwrap();
        fs::write(journal_path(&stream), evil).unwrap();
        let r = undo_last(&stream, &h).unwrap();
        assert_eq!(r.restored, 0, "{r:?}");
        assert_eq!(r.skipped.len(), 9, "{r:?}");
        assert!(h.join(".ssh/keys").is_dir() && h.join("Real/Empty").is_dir() && outside.is_dir() && !h.join(".ssh/new").exists() && !h.join("Moved").exists());
        fs::remove_dir_all(h).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }
}
