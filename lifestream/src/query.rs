//! Phase 1: query the stream, define views (saved queries), render them as
//! folders of symlinks, and produce the morning briefing text.

use crate::{human, Record};
use anyhow::{bail, Context, Result};
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

/// A saved query ("substream"). Also the shape of a views/*.toml file.
#[derive(Debug, Deserialize, Default, Clone)]
pub struct Query {
    pub name: Option<String>,
    pub description: Option<String>,
    /// every whitespace-separated word must appear in path, kind or tags
    pub text: Option<String>,
    #[serde(default)]
    pub tags_all: Vec<String>,
    #[serde(default)]
    pub tags_any: Vec<String>,
    #[serde(default)]
    pub exclude_tags: Vec<String>,
    pub kind_prefix: Option<String>,
    pub year: Option<String>,
    pub status: Option<String>,
    pub min_size_mb: Option<u64>,
}

impl Query {
    pub fn matches(&self, r: &Record) -> bool {
        match &self.status {
            Some(s) => {
                if &r.status != s {
                    return false;
                }
            }
            None => {
                if r.status == "gone" {
                    return false;
                }
            }
        }
        if let Some(k) = &self.kind_prefix {
            if !r.kind.starts_with(k.as_str()) {
                return false;
            }
        }
        if let Some(y) = &self.year {
            if !r.mtime.starts_with(y.as_str()) {
                return false;
            }
        }
        if let Some(mb) = self.min_size_mb {
            if r.size < mb * 1024 * 1024 {
                return false;
            }
        }
        if !self.tags_all.iter().all(|t| r.tags.contains(t)) {
            return false;
        }
        if !self.tags_any.is_empty() && !self.tags_any.iter().any(|t| r.tags.contains(t)) {
            return false;
        }
        if self.exclude_tags.iter().any(|t| r.tags.contains(t)) {
            return false;
        }
        if let Some(t) = &self.text {
            let hay = format!("{} {} {} {}", r.sources.join(" "), r.was.join(" "), r.kind, r.tags.join(" ")).to_lowercase();
            if !t.to_lowercase().split_whitespace().all(|w| hay.contains(w)) {
                return false;
            }
        }
        true
    }
}

/// Newest first: the stream reads from the present backwards.
pub fn sorted<'a>(recs: &'a [Record], q: &Query) -> Vec<&'a Record> {
    let mut v: Vec<&Record> = recs.iter().filter(|r| q.matches(r)).collect();
    v.sort_by(|a, b| b.mtime.cmp(&a.mtime).then(a.id.cmp(&b.id)));
    v
}

pub fn find(recs: &[Record], q: &Query, limit: usize) {
    let hits = sorted(recs, q);
    for r in hits.iter().take(limit) {
        let extra = if r.sources.len() > 1 { format!(" (+{} copies)", r.sources.len() - 1) } else { String::new() };
        let src = r.sources.first().or(r.was.first()).map(|s| s.as_str()).unwrap_or("(no source)");
        println!("{} {:>9} {:7} {:22} {}{}", &r.mtime[..10], human(r.size), r.status, r.kind, src, extra);
    }
    let total: u64 = hits.iter().map(|r| r.size).sum();
    println!("-- {} match(es), {} total{}", hits.len(), human(total), if hits.len() > limit { format!(" (showing {limit})") } else { String::new() });
}

pub fn load_views(dir: &Path) -> Result<Vec<Query>> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .with_context(|| format!("views dir {}", dir.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("toml"))
        .collect();
    files.sort();
    let mut views = Vec::new();
    for f in files {
        let q: Query = toml::from_str(&fs::read_to_string(&f)?).with_context(|| format!("parsing {}", f.display()))?;
        match &q.name {
            Some(n) if !n.is_empty() && !n.contains('/') && !n.starts_with('.') => views.push(q),
            _ => bail!("{}: view needs a plain `name` (no slashes, not hidden)", f.display()),
        }
    }
    Ok(views)
}

/// Look for `rel` from the working directory upwards, then in the home repository the one `home=` setting names (a
/// background service has no useful working directory).
pub fn find_up(rel: &str) -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        let c = dir.join(rel);
        if c.exists() {
            return Some(c);
        }
        if !dir.pop() {
            break;
        }
    }
    crate::settings_home().map(|h| h.join(rel)).filter(|p| p.exists())
}

fn expand(src: &str, home: &Path) -> PathBuf {
    match src.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None => PathBuf::from(src),
    }
}

/// Renders each view as a folder of symlinks under `into`. Only ever deletes
/// symlinks it finds there; bails if a view folder holds anything else.
pub fn render(views: &[Query], recs: &[Record], into: &Path, home: &Path) -> Result<()> {
    fs::create_dir_all(into)?;
    for v in views {
        let name = v.name.as_deref().unwrap_or_default();
        let dir = into.join(name);
        fs::create_dir_all(&dir)?;
        for e in fs::read_dir(&dir)?.flatten() {
            let p = e.path();
            if e.file_type()?.is_symlink() {
                fs::remove_file(&p)?;
            } else {
                bail!("{} contains a real file/folder ({}); refusing to touch it", dir.display(), p.display());
            }
        }
        let mut used: HashSet<String> = HashSet::new();
        let mut n = 0usize;
        for r in sorted(recs, v) {
            let Some(src) = r.sources.iter().map(|s| expand(s, home)).find(|p| p.exists()) else { continue };
            let base = src.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into());
            let mut link = base.clone();
            let mut k = 1;
            while !used.insert(link.clone()) {
                k += 1;
                link = match base.rsplit_once('.') {
                    Some((stem, ext)) if !stem.is_empty() => format!("{stem}~{k}.{ext}"),
                    _ => format!("{base}~{k}"),
                };
            }
            std::os::unix::fs::symlink(&src, dir.join(&link))?;
            n += 1;
        }
        println!("  {name:20} {n:6} links  {}", v.description.as_deref().unwrap_or(""));
    }
    Ok(())
}

/// The morning briefing: what the system did and what it needs from me.
pub fn brief(recs: &[Record], since_hours: i64) -> String {
    let cutoff = Utc::now() - Duration::hours(since_hours);
    let new: Vec<&Record> = recs
        .iter()
        .filter(|r| DateTime::parse_from_rfc3339(&r.first_seen).map(|t| t.with_timezone(&Utc) >= cutoff).unwrap_or(false))
        .collect();
    let live = |r: &&&Record| r.status != "gone";
    let filed = new.iter().filter(live).filter(|r| r.status == "filed").count();
    let pending: Vec<&&Record> = new.iter().filter(|r| r.status == "pending").collect();
    let gone = recs.iter().filter(|r| r.status == "gone").count();

    let mut out = String::new();
    out.push_str(&format!(
        "Overnight: {} new, {} filed, {} need you{}.\n",
        new.len(),
        filed,
        pending.len(),
        if gone > 0 { format!(", {gone} vanished") } else { String::new() }
    ));
    let mut fk: BTreeMap<&str, usize> = BTreeMap::new();
    for r in new.iter().filter(|r| r.status == "filed") {
        *fk.entry(r.kind.as_str()).or_default() += 1;
    }
    let mut fk: Vec<_> = fk.into_iter().collect();
    fk.sort_by_key(|x| std::cmp::Reverse(x.1));
    if !fk.is_empty() {
        out.push_str("  filed: ");
        out.push_str(&fk.iter().take(5).map(|(k, n)| format!("{n} {k}")).collect::<Vec<_>>().join(", "));
        out.push('\n');
    }
    let mut pk: BTreeMap<&str, usize> = BTreeMap::new();
    for r in &pending {
        *pk.entry(r.kind.as_str()).or_default() += 1;
    }
    let mut pk: Vec<_> = pk.into_iter().collect();
    pk.sort_by_key(|x| std::cmp::Reverse(x.1));
    if !pk.is_empty() {
        out.push_str("  need you: ");
        out.push_str(&pk.iter().take(5).map(|(k, n)| format!("{n} {k}")).collect::<Vec<_>>().join(", "));
        out.push('\n');
    }
    let wasted: u64 = recs
        .iter()
        .filter(|r| r.status != "gone" && r.sources.len() > 1 && r.kind != "project/git")
        .map(|r| r.size * (r.sources.len() as u64 - 1))
        .sum();
    if wasted > 0 {
        out.push_str(&format!("  duplicates: {} reclaimable\n", human(wasted)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::By;

    fn rec(id: &str, kind: &str, tags: &[&str], status: &str, mtime: &str, src: &str) -> Record {
        Record {
            id: id.into(),
            first_seen: "2026-11-09T00:00:00Z".into(),
            mtime: mtime.into(),
            kind: kind.into(),
            size: 1024,
            sources: vec![src.into()],
            tags: tags.iter().map(|s| s.to_string()).collect(),
            summary: None,
            by: By { tier: "rule".into(), name: "n".into(), confidence: 1.0 },
            status: status.into(),
            store: None,
            future_at: None,
            was: vec![],
        }
    }

    #[test]
    fn query_filters() {
        let r = rec("a", "image/raw", &["photo", "raw", "year:2026"], "filed", "2026-10-11T00:00:00Z", "~/Pictures/x.DNG");
        let q = |s: &str| toml::from_str::<Query>(s).unwrap();
        assert!(q("tags_all=['photo']").matches(&r));
        assert!(!q("tags_all=['photo','video']").matches(&r));
        assert!(q("tags_any=['video','raw']").matches(&r));
        assert!(!q("exclude_tags=['raw']").matches(&r));
        assert!(q("kind_prefix='image'").matches(&r));
        assert!(q("year='2026'").matches(&r));
        assert!(!q("year='2025'").matches(&r));
        assert!(q("text='pictures dng'").matches(&r));
        assert!(!q("text='nothere'").matches(&r));
        assert!(!q("status='pending'").matches(&r));
    }

    #[test]
    fn gone_hidden_unless_asked() {
        let r = rec("a", "x", &[], "gone", "2026-10-11T00:00:00Z", "~/a");
        assert!(!Query::default().matches(&r));
        assert!(toml::from_str::<Query>("status='gone'").unwrap().matches(&r));
    }

    #[test]
    fn newest_first() {
        let v = vec![
            rec("a", "k", &[], "filed", "2020-01-01T00:00:00Z", "~/a"),
            rec("b", "k", &[], "filed", "2026-01-01T00:00:00Z", "~/b"),
        ];
        let s = sorted(&v, &Query::default());
        assert_eq!(s[0].id, "b");
    }

    #[test]
    fn brief_counts() {
        let mut a = rec("a", "image", &[], "filed", "2026-10-11T00:00:00Z", "~/a");
        let mut b = rec("b", "unknown/zzz", &[], "pending", "2026-10-11T00:00:00Z", "~/b");
        let now = Utc::now().to_rfc3339();
        a.first_seen = now.clone();
        b.first_seen = now;
        let text = brief(&[a, b], 24);
        assert!(text.contains("2 new, 1 filed, 1 need you"), "{text}");
    }

    #[test]
    fn render_uses_unique_names_and_refuses_real_files() {
        let base = std::env::temp_dir().join(format!("ls-render-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let home = base.join("home");
        fs::create_dir_all(home.join("a")).unwrap();
        fs::create_dir_all(home.join("b")).unwrap();
        fs::write(home.join("a/same.txt"), b"1").unwrap();
        fs::write(home.join("b/same.txt"), b"2").unwrap();
        let recs = vec![
            rec("1", "document/text", &["document"], "filed", "2026-01-02T00:00:00Z", "~/a/same.txt"),
            rec("2", "document/text", &["document"], "filed", "2026-01-01T00:00:00Z", "~/b/same.txt"),
        ];
        let v = vec![toml::from_str::<Query>("name='docs'\ntags_all=['document']").unwrap()];
        let into = base.join("views");
        render(&v, &recs, &into, &home).unwrap();
        assert!(into.join("docs/same.txt").exists() && into.join("docs/same~2.txt").exists());
        // re-render is idempotent
        render(&v, &recs, &into, &home).unwrap();
        // a real file in a view folder makes it refuse
        fs::write(into.join("docs/precious.txt"), b"x").unwrap();
        assert!(render(&v, &recs, &into, &home).is_err());
        assert!(into.join("docs/precious.txt").exists());
        fs::remove_dir_all(base).unwrap();
    }
}
