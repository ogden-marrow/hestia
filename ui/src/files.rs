//! A Finder-style view of a folder, kept current: names, kinds (from the stream when it knows
//! the file), sizes and dates, plus a fingerprint so the screen can notice changes.

use lifestream::Record;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub size: u64,
    pub modified: String,
    pub kind: String,
}

pub fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/".into()))
}

pub fn tilde(p: &Path) -> String {
    match p.strip_prefix(home()) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".into(),
        Ok(rest) => format!("~/{}", rest.to_string_lossy()),
        Err(_) => p.to_string_lossy().into_owned(),
    }
}

/// Path (as the stream writes it, `~/...`) -> kind, for every live file the stream knows.
pub fn kinds(recs: &[Record]) -> HashMap<String, String> {
    let mut m = HashMap::new();
    for r in recs.iter().filter(|r| r.status != "gone") {
        for s in &r.sources {
            m.insert(s.clone(), r.kind.clone());
        }
    }
    m
}

fn stamp(t: SystemTime) -> String {
    chrono::DateTime::<chrono::Local>::from(t).format("%b %-d, %Y, %-I:%M %p").to_string()
}

/// Folders first, then files, each A to Z; hidden files left out.
pub fn list(dir: &Path, kinds: &HashMap<String, String>, hidden: bool) -> Vec<Entry> {
    let mut out: Vec<Entry> = std::fs::read_dir(dir).into_iter().flatten().flatten()
        .filter(|e| hidden || !e.file_name().to_string_lossy().starts_with('.'))
        .filter_map(|e| {
            let md = e.metadata().ok()?;
            let path = e.path();
            let is_dir = md.is_dir();
            let kind = if is_dir {
                if path.join(".git").exists() { "project/git".to_string() } else { "folder".to_string() }
            } else {
                kinds.get(&tilde(&path)).cloned().unwrap_or_else(|| "not in the stream yet".into())
            };
            Some(Entry {
                name: e.file_name().to_string_lossy().into_owned(), path, is_dir, size: if is_dir { 0 } else { md.len() },
                modified: md.modified().map(stamp).unwrap_or_default(), kind,
            })
        })
        .collect();
    out.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_folders_first_and_hides_dotfiles() {
        let d = std::env::temp_dir().join(format!("ls-files-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("zdir")).unwrap();
        std::fs::write(d.join("b.txt"), "x").unwrap();
        std::fs::write(d.join("a.txt"), "xy").unwrap();
        std::fs::write(d.join(".hidden"), "x").unwrap();
        let mut k = HashMap::new();
        k.insert(tilde(&d.join("a.txt")), "document/text".to_string());
        let l = list(&d, &k, false);
        assert_eq!(l.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["zdir", "a.txt", "b.txt"]);
        assert_eq!(l[1].kind, "document/text");
        assert_eq!(l[2].kind, "not in the stream yet");
        std::fs::remove_dir_all(d).unwrap();
    }
}
