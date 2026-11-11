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

fn rel_ok(p: &str) -> bool {
    !p.is_empty() && !p.starts_with('/') && !p.starts_with('~') && !p.split('/').any(|c| c.is_empty() || c == "." || c == "..")
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
                bail!("bad path {p:?}: use a path relative to your home folder, no .. or empty parts");
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

/// Same-filesystem: hard-link then unlink (atomic, refuses to overwrite). Otherwise copy,
/// verify the hash, then remove the original.
pub fn move_file(from: &Path, to: &Path) -> Result<()> {
    if let Some(d) = to.parent() {
        fs::create_dir_all(d)?;
    }
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
    if to.exists() {
        bail!("{} already exists", to.display());
    }
    let tmp = to.with_file_name(format!(".{}.lifestream-part", to.file_name().and_then(|n| n.to_str()).unwrap_or("file")));
    fs::copy(from, &tmp)?;
    let ok = sha256_file(from)? == sha256_file(&tmp)?;
    if !ok {
        let _ = fs::remove_file(&tmp);
        bail!("copy of {} did not verify", from.display());
    }
    if let Ok(m) = fs::metadata(from).and_then(|m| m.modified()) {
        let _ = fs::File::options().write(true).open(&tmp).and_then(|f| f.set_modified(m));
    }
    fs::rename(&tmp, to)?;
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
    Ok((done.len(), failed))
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
    let (old, new) = (format!("~/{from}"), format!("~/{to}"));
    let swap = |s: &mut String| {
        if under(s, &old) {
            *s = format!("{new}{}", &s[old.len()..]);
        }
    };
    let mut recs = load_all(stream)?;
    for r in &mut recs {
        r.sources.iter_mut().for_each(swap);
        r.was.iter_mut().for_each(swap);
    }
    write_all(stream, &recs)?;
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
pub fn cmd_file(tree_path: Option<PathBuf>, stream: &Path, manifest: Option<PathBuf>, apply_it: bool) -> Result<()> {
    let home = crate::home()?;
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
        let p = plan(&[r.clone()], &tree(), &h, &[], &HashSet::new());
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
}
