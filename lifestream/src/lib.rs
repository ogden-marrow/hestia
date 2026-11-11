//! lifestream, phase 0: read-only ingest + deterministic filing.
//!
//! Walks the roots you give it, hashes files, classifies them with the rules in
//! rules.toml, and records one entry per distinct file in a time-ordered stream
//! (JSON Lines shards by the file's own month). It NEVER writes inside the
//! scanned folders and skips every path the sensitive list covers.

pub mod auth;
pub mod bundle;
pub mod domains;
pub mod home;
pub mod machine;
pub mod query;
pub mod rules;
pub mod secrets;
pub mod semantic;
pub mod system;
pub mod tree;

use anyhow::{bail, Context, Result};
use chrono::{DateTime, SecondsFormat, Utc};
use rayon::prelude::*;
use rules::RuleSet;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::SystemTime;

const DEFAULT_RULES: &str = include_str!("../rules.toml");
/// Directories that are regenerable noise: never descended into.
const IGNORE_DIRS: &[&str] = &[
    "node_modules", "target", ".cache", "__pycache__", ".venv", "venv", ".gradle", "steamapps",
];

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct By {
    pub tier: String,
    pub name: String,
    pub confidence: f32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Record {
    pub id: String,
    pub first_seen: String,
    pub mtime: String,
    pub kind: String,
    pub size: u64,
    pub sources: Vec<String>,
    /// paths this file used to have (kept when it vanishes, so history stays searchable)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub was: Vec<String>,
    pub tags: Vec<String>,
    pub summary: Option<String>,
    pub by: By,
    /// filed | pending | gone
    pub status: String,
    pub store: Option<String>,
    pub future_at: Option<String>,
}

enum Item {
    File { path: PathBuf, size: u64, mtime: SystemTime },
    Repo { path: PathBuf },
}

struct Walker<'a> {
    excluded: &'a [PathBuf],
    items: Vec<Item>,
    skipped_sensitive: u64,
    skipped_ignored: u64,
    unreadable: u64,
}

impl Walker<'_> {
    fn is_excluded(&self, p: &Path) -> bool {
        self.excluded.iter().any(|e| p.starts_with(e))
    }

    fn walk(&mut self, dir: &Path) {
        if self.is_excluded(dir) {
            self.skipped_sensitive += 1;
            return;
        }
        // A git checkout is one unit: record the project, don't descend.
        if dir.join(".git").exists() {
            self.items.push(Item::Repo { path: dir.to_path_buf() });
            return;
        }
        let rd = match fs::read_dir(dir) {
            Ok(r) => r,
            Err(_) => {
                self.unreadable += 1;
                return;
            }
        };
        for e in rd.flatten() {
            let p = e.path();
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_symlink() {
                continue;
            }
            if ft.is_dir() {
                let name = e.file_name().to_string_lossy().into_owned();
                if IGNORE_DIRS.contains(&name.as_str()) {
                    self.skipped_ignored += 1;
                    continue;
                }
                self.walk(&p);
            } else if ft.is_file() {
                if self.is_excluded(&p) || looks_secret(&e.file_name().to_string_lossy()) {
                    self.skipped_sensitive += 1;
                    continue;
                }
                if is_transient(&e.file_name().to_string_lossy()) {
                    self.skipped_ignored += 1;
                    continue;
                }
                match e.metadata() {
                    Ok(m) => self.items.push(Item::File {
                        path: p,
                        size: m.len(),
                        mtime: m.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                    }),
                    Err(_) => self.unreadable += 1,
                }
            }
        }
    }
}

/// Scratch files a running program rewrites constantly (database write-ahead logs, journals, locks):
/// never indexed, since they would churn the stream on every scan.
pub fn is_transient(name: &str) -> bool {
    let n = name.to_lowercase();
    ["-wal", "-shm", "-journal"].iter().any(|x| n.ends_with(x)) || n.ends_with(".lock") || n.ends_with(".tmp") || n.ends_with('~')
}

/// File names that look like they hold secrets: never indexed, hashed or read.
pub fn looks_secret(name: &str) -> bool {
    let n = name.to_lowercase();
    if n == ".env" || n == ".netrc" || n == ".pgpass" || n == ".git-credentials" || n == "credentials.json" {
        return true;
    }
    if n.starts_with(".env.") && !(n.ends_with(".example") || n.ends_with(".sample") || n.ends_with(".template")) {
        return true;
    }
    if n.ends_with(".env") || n.starts_with("secrets.") || n.starts_with("secret.") {
        return true;
    }
    if let Some(ext) = n.rsplit('.').next().filter(|_| n.contains('.')) {
        if ["pem", "key", "p12", "pfx", "kdbx", "gpg", "keystore", "jks"].contains(&ext) {
            return true;
        }
    }
    // private keys: id_rsa, id_ed25519, foo_ed25519 ... but not the public halves
    if !n.ends_with(".pub")
        && (n.starts_with("id_rsa") || n.starts_with("id_ed25519") || n.starts_with("id_ecdsa")
            || n.ends_with("_ed25519") || n.ends_with("_rsa"))
    {
        return true;
    }
    false
}

pub fn home() -> Result<PathBuf> {
    let h = std::env::var("HOME").context("HOME is not set")?;
    Ok(fs::canonicalize(h)?)
}

pub fn tilde(p: &Path, home: &Path) -> String {
    match p.strip_prefix(home) {
        Ok(rest) => format!("~/{}", rest.to_string_lossy()),
        Err(_) => p.to_string_lossy().into_owned(),
    }
}

fn rfc3339(t: SystemTime) -> String {
    DateTime::<Utc>::from(t).to_rfc3339_opts(SecondsFormat::Secs, true)
}

pub(crate) fn find_manifest() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        let cand = dir.join("manifest/rules.toml");
        if cand.is_file() {
            return Some(cand);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Paths (absolute) that must never be read: `sensitive = true` rules + [secrets].
pub(crate) fn sensitive_paths(manifest: &Path, home: &Path) -> Result<Vec<PathBuf>> {
    let v: toml::Value = toml::from_str(&fs::read_to_string(manifest)?)
        .with_context(|| format!("parsing {}", manifest.display()))?;
    let mut out = Vec::new();
    if let Some(rules) = v.get("rule").and_then(|r| r.as_array()) {
        for r in rules {
            if r.get("sensitive").and_then(|s| s.as_bool()) == Some(true) {
                if let Some(p) = r.get("path").and_then(|p| p.as_str()) {
                    out.push(home.join(p));
                }
            }
        }
    }
    if let Some(paths) = v.get("secrets").and_then(|s| s.get("paths")).and_then(|p| p.as_array()) {
        for p in paths.iter().filter_map(|p| p.as_str()) {
            out.push(home.join(p));
        }
    }
    Ok(out)
}

pub(crate) fn sha256_file(p: &Path) -> std::io::Result<String> {
    let mut f = fs::File::open(p)?; // read-only
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(format!("sha256:{:x}", h.finalize()))
}

fn git_origin(repo: &Path) -> Option<String> {
    let cfg = fs::read_to_string(repo.join(".git/config")).ok()?;
    let mut in_origin = false;
    for line in cfg.lines() {
        let l = line.trim();
        if l.starts_with('[') {
            in_origin = l == "[remote \"origin\"]";
        } else if in_origin {
            if let Some(rest) = l.strip_prefix("url") {
                if let Some(v) = rest.trim().strip_prefix('=') {
                    let v = v.trim().trim_end_matches('/');
                    return Some(v.strip_suffix(".git").unwrap_or(v).to_string());
                }
            }
        }
    }
    None
}

pub fn load_all(out: &Path) -> Result<Vec<Record>> {
    let mut recs = Vec::new();
    if !out.exists() {
        return Ok(recs);
    }
    for year in fs::read_dir(out)?.flatten() {
        if !year.path().is_dir() {
            continue;
        }
        for f in fs::read_dir(year.path())?.flatten() {
            let p = f.path();
            if p.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            for (i, line) in fs::read_to_string(&p)?.lines().enumerate() {
                if line.trim().is_empty() {
                    continue;
                }
                let r: Record = serde_json::from_str(line)
                    .with_context(|| format!("{}:{}: bad record", p.display(), i + 1))?;
                recs.push(r);
            }
        }
    }
    Ok(recs)
}

pub fn shard_of(r: &Record) -> (String, String) {
    let m = &r.mtime;
    if m.len() >= 7 {
        (m[0..4].to_string(), m[5..7].to_string())
    } else {
        ("0000".into(), "00".into())
    }
}

/// Rewrites shards from the full record set; only touches files whose content changed.
pub fn write_all(out: &Path, recs: &[Record]) -> Result<usize> {
    let mut shards: BTreeMap<(String, String), Vec<&Record>> = BTreeMap::new();
    for r in recs {
        shards.entry(shard_of(r)).or_default().push(r);
    }
    let mut written = 0;
    for ((y, m), mut rs) in shards {
        rs.sort_by(|a, b| a.id.cmp(&b.id));
        let mut body = String::new();
        for r in rs {
            body.push_str(&serde_json::to_string(r)?);
            body.push('\n');
        }
        let dir = out.join(&y);
        fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{m}.jsonl"));
        if fs::read_to_string(&path).ok().as_deref() == Some(body.as_str()) {
            continue;
        }
        let tmp = dir.join(format!("{m}.jsonl.tmp"));
        fs::write(&tmp, body)?;
        fs::rename(&tmp, &path)?;
        written += 1;
    }
    Ok(written)
}

pub fn cmd_scan(
    roots: Vec<PathBuf>,
    out: Option<PathBuf>,
    rules_path: Option<PathBuf>,
    manifest: Option<PathBuf>,
    dry_run: bool,
) -> Result<()> {
    let home = home()?;
    let out = out.unwrap_or_else(|| home.join(".local/share/lifestream/stream"));
    let mut canon = Vec::new();
    for r in &roots {
        canon.push(fs::canonicalize(r).with_context(|| format!("root {}", r.display()))?);
    }
    for r in &canon {
        if out.starts_with(r) {
            bail!("--out ({}) is inside scanned root {}: refusing to write there", out.display(), r.display());
        }
    }
    let manifest = match manifest.or_else(find_manifest) {
        Some(m) => m,
        None => bail!("cannot find manifest/rules.toml (sensitive-path list). Run from inside your home repo or pass --manifest. Refusing to scan without it."),
    };
    let excluded = sensitive_paths(&manifest, &home)?;
    let rules_text = match &rules_path {
        Some(p) => fs::read_to_string(p)?,
        None => DEFAULT_RULES.to_string(),
    };
    let ruleset = RuleSet::parse(&rules_text)?;

    eprintln!("sensitive paths excluded: {}", excluded.len());
    let mut w = Walker { excluded: &excluded, items: Vec::new(), skipped_sensitive: 0, skipped_ignored: 0, unreadable: 0 };
    for r in &canon {
        w.walk(r);
    }
    eprintln!("found {} items; hashing…", w.items.len());

    let existing = load_all(&out)?;
    let first_seen_by_id: HashMap<&str, &str> =
        existing.iter().map(|r| (r.id.as_str(), r.first_seen.as_str())).collect();
    // incremental: reuse the id when path+size+mtime are unchanged
    let mut known: HashMap<(&str, u64, &str), &str> = HashMap::new();
    for r in &existing {
        for s in &r.sources {
            known.insert((s.as_str(), r.size, r.mtime.as_str()), r.id.as_str());
        }
    }

    let now = rfc3339(SystemTime::now());
    let done = AtomicUsize::new(0);
    let total = w.items.len();
    let results: Vec<Result<Record, String>> = w
        .items
        .par_iter()
        .map(|item| {
            let n = done.fetch_add(1, Ordering::Relaxed) + 1;
            if n.is_multiple_of(2000) {
                eprintln!("  {n}/{total}");
            }
            match item {
                Item::Repo { path } => {
                    let src = tilde(path, &home);
                    let id = match git_origin(path) {
                        Some(o) => format!("repo:{o}"),
                        None => format!("repo:local:{src}"),
                    };
                    let mtime = fs::metadata(path.join(".git")).and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
                    let mut tags: BTreeSet<String> = ["project", "git"].iter().map(|s| s.to_string()).collect();
                    add_auto_tags(&mut tags, &src, &rfc3339(mtime));
                    Ok(Record {
                        id,
                        first_seen: now.clone(),
                        mtime: rfc3339(mtime),
                        kind: "project/git".into(),
                        size: 0,
                        sources: vec![src],
                        tags: tags.into_iter().collect(),
                        summary: None,
                        by: By { tier: "rule".into(), name: "git-repo".into(), confidence: 1.0 },
                        status: "filed".into(),
                        store: None,
                        future_at: None,
                        was: vec![],
                    })
                }
                Item::File { path, size, mtime } => {
                    let src = tilde(path, &home);
                    let m = rfc3339(*mtime);
                    let id = match known.get(&(src.as_str(), *size, m.as_str())) {
                        Some(id) => (*id).to_string(),
                        None => match sha256_file(path) {
                            Ok(h) => h,
                            Err(_) => return Err(src), // keep its path alive: unreadable is not deleted
                        },
                    };
                    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
                    let o = ruleset.classify(&src, &name, &ext);
                    let mut tags = o.tags;
                    add_auto_tags(&mut tags, &src, &m);
                    let (kind, by, status) = match (&o.kind, &o.kind_rule) {
                        (Some(k), Some(r)) => (k.clone(), By { tier: "rule".into(), name: r.clone(), confidence: 1.0 }, "filed"),
                        _ => (
                            format!("unknown/{}", if ext.is_empty() { "none" } else { &ext }),
                            By { tier: "rule".into(), name: "none".into(), confidence: 0.0 },
                            "pending",
                        ),
                    };
                    Ok(Record {
                        id,
                        first_seen: now.clone(),
                        mtime: m,
                        kind,
                        size: *size,
                        sources: vec![src],
                        tags: tags.into_iter().collect(),
                        summary: None,
                        by,
                        status: status.into(),
                        store: None,
                        future_at: None,
                        was: vec![],
                    })
                }
            }
        })
        .collect();

    let mut unhashable_paths: HashSet<String> = HashSet::new();
    let mut ok_results: Vec<Record> = Vec::new();
    for r in results {
        match r {
            Ok(rec) => ok_results.push(rec),
            Err(p) => {
                unhashable_paths.insert(p);
            }
        }
    }
    let unhashable = unhashable_paths.len() as u64;

    // fold duplicates (same id) into one record with several sources
    let mut fresh: BTreeMap<String, Record> = BTreeMap::new();
    for r in ok_results {
        match fresh.get_mut(&r.id) {
            Some(e) => {
                for s in r.sources {
                    if !e.sources.contains(&s) {
                        e.sources.push(s);
                    }
                }
                if r.mtime < e.mtime {
                    e.mtime = r.mtime;
                }
            }
            None => {
                fresh.insert(r.id.clone(), r);
            }
        }
    }

    // merge with the existing stream
    let root_tildes: Vec<String> = canon.iter().map(|r| tilde(r, &home)).collect();
    let under_roots = |s: &str| root_tildes.iter().any(|r| s == r || s.starts_with(&format!("{r}/")));
    let mut seen: HashSet<&str> = fresh.values().flat_map(|r| r.sources.iter().map(|s| s.as_str())).collect();
    seen.extend(unhashable_paths.iter().map(|s| s.as_str()));
    let mut merged: BTreeMap<String, Record> = BTreeMap::new();
    for mut r in existing.clone() {
        let (keep, removed): (Vec<String>, Vec<String>) =
            r.sources.iter().cloned().partition(|s| !under_roots(s) || seen.contains(s.as_str()));
        r.sources = keep;
        for s in removed {
            if !r.was.contains(&s) {
                r.was.push(s);
            }
        }
        merged.insert(r.id.clone(), r);
    }
    for (id, mut f) in fresh {
        if let Some(old) = merged.remove(&id) {
            let human_or_ai = old.by.tier != "rule";
            let mut sources = old.sources.clone();
            for s in &f.sources {
                if !sources.contains(s) {
                    sources.push(s.clone());
                }
            }
            f.sources = sources;
            f.first_seen = first_seen_by_id.get(id.as_str()).map(|s| s.to_string()).unwrap_or(f.first_seen);
            if human_or_ai {
                // never overwrite later-phase (AI/human) decisions with a rule result
                f.kind = old.kind;
                f.tags = old.tags;
                f.summary = old.summary;
                f.by = old.by;
                f.status = old.status;
            }
            f.was = old.was.into_iter().filter(|w| !f.sources.contains(w)).collect();
            f.store = old.store;
            f.future_at = old.future_at;
        }
        merged.insert(id, f);
    }
    for r in merged.values_mut() {
        if r.sources.is_empty() {
            r.status = "gone".into();
        } else if r.status == "gone" {
            r.status = "filed".into();
        }
    }
    let all: Vec<Record> = merged.into_values().collect();

    eprintln!(
        "skipped: {} sensitive, {} regenerable dirs, {} unreadable, {} unhashable",
        w.skipped_sensitive, w.skipped_ignored, w.unreadable, unhashable
    );
    print_stats(&all);
    if dry_run {
        println!("\n(dry run: nothing written)");
    } else {
        let n = write_all(&out, &all)?;
        println!("\nwrote {n} shard file(s) under {}", out.display());
    }
    Ok(())
}

/// Record a human decision for one or more files: kind + tags, filed by "human", never overwritten by
/// rules. Re-reads the stream from disk so a scan that ran meanwhile is never clobbered.
pub fn teach(stream: &Path, ids: &[String], kind: &str, tags: &[String]) -> Result<Vec<Record>> {
    let ok = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_' | ':' | '.'));
    let kind = kind.to_lowercase();
    let tags: Vec<String> = tags.iter().map(|t| t.to_lowercase()).collect();
    if !ok(&kind) || !tags.iter().all(|t| ok(t)) {
        bail!("use letters, digits and / - _ : . only (e.g. document/invoice finance)");
    }
    let mut recs = load_all(stream)?;
    let mut n = 0;
    for r in recs.iter_mut().filter(|r| ids.contains(&r.id) && r.status != "gone") {
        r.kind = kind.clone();
        for t in &tags {
            if !r.tags.contains(t) {
                r.tags.push(t.clone());
            }
        }
        r.tags.sort();
        r.by = By { tier: "human".into(), name: "me".into(), confidence: 1.0 };
        r.status = "filed".into();
        n += 1;
    }
    if n == 0 {
        bail!("no matching file on disk any more");
    }
    write_all(stream, &recs)?;
    Ok(recs)
}

fn add_auto_tags(tags: &mut BTreeSet<String>, src: &str, mtime: &str) {
    if mtime.len() >= 4 {
        tags.insert(format!("year:{}", &mtime[0..4]));
    }
    if let Some(rest) = src.strip_prefix("~/") {
        if let Some(top) = rest.split('/').next() {
            tags.insert(format!("in:{top}"));
        }
    }
}

pub fn human(n: u64) -> String {
    const U: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 { format!("{n} B") } else { format!("{v:.1} {}", U[i]) }
}

pub fn print_stats(all: &[Record]) {
    let live: Vec<&Record> = all.iter().filter(|r| r.status != "gone").collect();
    let count = |s: &str| all.iter().filter(|r| r.status == s).count();
    println!("\n== stream: {} records ({} filed, {} pending, {} gone)", all.len(), count("filed"), count("pending"), count("gone"));
    let total: u64 = live.iter().map(|r| r.size).sum();
    println!("   distinct content: {}", human(total));

    let mut by_kind: BTreeMap<&str, (usize, u64)> = BTreeMap::new();
    for r in &live {
        let e = by_kind.entry(r.kind.as_str()).or_default();
        e.0 += 1;
        e.1 += r.size;
    }
    let mut kinds: Vec<_> = by_kind.into_iter().collect();
    kinds.sort_by(|a, b| b.1 .1.cmp(&a.1 .1).then(b.1 .0.cmp(&a.1 .0)));
    println!("\n== by kind (top 20 by size)");
    for (k, (n, sz)) in kinds.iter().take(20) {
        println!("   {k:28} {n:7} files {:>10}", human(*sz));
    }

    let mut pend: BTreeMap<&str, (usize, u64)> = BTreeMap::new();
    for r in live.iter().filter(|r| r.status == "pending") {
        let e = pend.entry(r.kind.as_str()).or_default();
        e.0 += 1;
        e.1 += r.size;
    }
    let mut pend: Vec<_> = pend.into_iter().collect();
    pend.sort_by_key(|a| std::cmp::Reverse(a.1 .0));
    println!("\n== pending: no rule matched (top 15; the future AI/ask queue)");
    for (k, (n, sz)) in pend.iter().take(15) {
        println!("   {k:28} {n:7} files {:>10}", human(*sz));
    }

    let mut dupes: Vec<(&Record, u64)> = live
        .iter()
        .filter(|r| r.sources.len() > 1 && r.kind != "project/git")
        .map(|r| (*r, r.size * (r.sources.len() as u64 - 1)))
        .collect();
    dupes.sort_by_key(|a| std::cmp::Reverse(a.1));
    let wasted: u64 = dupes.iter().map(|d| d.1).sum();
    println!("\n== duplicate files: {} groups, {} reclaimable", dupes.len(), human(wasted));
    for (r, w) in dupes.iter().take(8) {
        println!("   {:>10} x{}  {}", human(*w), r.sources.len(), r.sources[0]);
    }

    let repos: Vec<&&Record> = live.iter().filter(|r| r.kind == "project/git").collect();
    let multi: Vec<_> = repos.iter().filter(|r| r.sources.len() > 1).collect();
    println!("\n== git projects: {} ({} cloned more than once)", repos.len(), multi.len());
    for r in multi.iter().take(10) {
        println!("   x{}  {}", r.sources.len(), r.id.trim_start_matches("repo:"));
    }
}

pub fn stream_dir(out: Option<PathBuf>) -> Result<PathBuf> {
    Ok(match out {
        Some(o) => o,
        None => home()?.join(".local/share/lifestream/stream"),
    })
}

pub fn views_dir(dir: Option<PathBuf>) -> Result<PathBuf> {
    dir.or_else(|| query::find_up("views"))
        .context("cannot find a views/ folder (run from inside your home repo or pass --dir)")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tilde_shortens_home() {
        assert_eq!(tilde(Path::new("/h/a/b"), Path::new("/h")), "~/a/b");
        assert_eq!(tilde(Path::new("/x/y"), Path::new("/h")), "/x/y");
    }

    #[test]
    fn auto_tags_year_and_top_dir() {
        let mut t = BTreeSet::new();
        add_auto_tags(&mut t, "~/Downloads/a/b.pdf", "2025-09-15T00:00:00Z");
        assert!(t.contains("year:2025") && t.contains("in:Downloads"));
    }

    #[test]
    fn shard_uses_file_month() {
        let r = Record {
            id: "x".into(), first_seen: "n".into(), mtime: "2025-09-15T00:00:00Z".into(), kind: "k".into(),
            size: 0, sources: vec![], tags: vec![], summary: None,
            by: By { tier: "rule".into(), name: "n".into(), confidence: 1.0 },
            status: "filed".into(), store: None, future_at: None,
                        was: vec![],
        };
        assert_eq!(shard_of(&r), ("2025".into(), "09".into()));
    }

    #[test]
    fn secret_looking_names_are_caught() {
        for n in [".env", ".env.local", "freellmapi.env", "server.pem", "x.KEY", "id_rsa", "id_ed25519",
                  "backup_ed25519", "creds.kdbx", ".netrc", "secrets.json"] {
            assert!(looks_secret(n), "{n} should be secret");
        }
        for n in [".env.example", "id_ed25519.pub", "MainGitKey.pub", "monkey.png", "keynote.txt", "notes.md", "turkey"] {
            assert!(!looks_secret(n), "{n} should be fine");
        }
    }

    #[test]
    fn teach_merges_onto_disk_and_rejects_junk() {
        let d = std::env::temp_dir().join(format!("ls-teach-lib-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        let mk = |id: &str| Record {
            id: id.into(), first_seen: "n".into(), mtime: "2025-09-15T00:00:00Z".into(), kind: "unknown/htm".into(),
            size: 1, sources: vec!["~/a".into()], tags: vec![], summary: None,
            by: By { tier: "rule".into(), name: "none".into(), confidence: 0.0 },
            status: "pending".into(), store: None, future_at: None, was: vec![],
        };
        write_all(&d, &[mk("x"), mk("y")]).unwrap();
        assert!(teach(&d, &["x".into()], "bad kind!", &[]).is_err());
        let recs = teach(&d, &["x".into()], "Document/Manual", &["Help".into()]).unwrap();
        let x = recs.iter().find(|r| r.id == "x").unwrap();
        assert_eq!((x.kind.as_str(), x.by.tier.as_str(), x.status.as_str()), ("document/manual", "human", "filed"));
        assert_eq!(x.tags, vec!["help".to_string()]);
        assert_eq!(recs.iter().find(|r| r.id == "y").unwrap().status, "pending");
        assert!(teach(&d, &["nope".into()], "a/b", &[]).is_err());
        let recs = teach(&d, &["x".into(), "y".into()], "a/b", &[]).unwrap();
        assert!(recs.iter().all(|r| r.by.tier == "human"), "one answer covers a whole folder");
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn transient_scratch_files_are_recognised() {
        for n in ["freeapi.db-wal", "freeapi.db-shm", "x.sqlite-journal", "a.lock", "b.TMP", "notes.txt~"] {
            assert!(is_transient(n), "{n}");
        }
        for n in ["freeapi.db", "wallpaper.png", "lockfile.md", "shm.txt"] {
            assert!(!is_transient(n), "{n}");
        }
    }

    #[test]
    fn human_sizes() {
        assert_eq!(human(0), "0 B");
        assert_eq!(human(1536), "1.5 KB");
    }

    #[test]
    fn sha256_known_vector() {
        let d = std::env::temp_dir().join(format!("ls-test-{}", std::process::id()));
        fs::create_dir_all(&d).unwrap();
        let f = d.join("abc.txt");
        fs::write(&f, b"abc").unwrap();
        assert_eq!(
            sha256_file(&f).unwrap(),
            "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn walker_skips_excluded_and_git_repos() {
        let d = std::env::temp_dir().join(format!("ls-walk-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(d.join("secret")).unwrap();
        fs::create_dir_all(d.join("proj/.git")).unwrap();
        fs::create_dir_all(d.join("ok")).unwrap();
        fs::write(d.join("secret/pw.txt"), b"x").unwrap();
        fs::write(d.join("proj/code.rs"), b"x").unwrap();
        fs::write(d.join("ok/a.txt"), b"x").unwrap();
        let excl = vec![d.join("secret")];
        let mut w = Walker { excluded: &excl, items: vec![], skipped_sensitive: 0, skipped_ignored: 0, unreadable: 0 };
        w.walk(&d);
        let files = w.items.iter().filter(|i| matches!(i, Item::File { .. })).count();
        let repos = w.items.iter().filter(|i| matches!(i, Item::Repo { .. })).count();
        assert_eq!((files, repos, w.skipped_sensitive), (1, 1, 1));
        fs::remove_dir_all(d).unwrap();
    }
}
