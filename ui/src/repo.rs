//! A git repository as the app sees it: commits, what each one changed, the diff of a file.
//! Shells out to `git` (read-only commands), so it shows exactly what git itself reports.
//! Submodules are ignored in status/diff: scanning all of them costs ~250 ms; ignoring them ~7 ms.

use std::path::Path;
use std::process::Command;

/// The pseudo-commit for uncommitted changes in the working tree.
pub const WORKTREE: &str = "WORKTREE";

#[derive(Clone, Debug, PartialEq)]
pub struct Commit {
    pub hash: String,
    pub short: String,
    pub subject: String,
    pub when: String,
    pub author: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    pub path: String,
    pub add: u32,
    pub del: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Add,
    Del,
    Ctx,
    Hunk,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DiffLine {
    pub kind: Kind,
    pub old: Option<u32>,
    pub new: Option<u32>,
    pub text: String,
}

fn git(dir: &Path, args: &[&str]) -> String {
    Command::new("git").arg("-C").arg(dir).args(args).output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default()
}

/// "main ahead 1" style one-liner.
pub fn status_line(dir: &Path) -> String {
    git(dir, &["status", "-sb", "--porcelain", "--ignore-submodules"]).lines().next().unwrap_or("").trim_start_matches("## ").to_string()
}

/// Changes whenever HEAD or the working tree changes, so the screen knows to reload.
pub fn stamp(dir: &Path) -> String {
    format!("{}|{}", git(dir, &["rev-parse", "HEAD"]).trim(), git(dir, &["status", "--porcelain", "--ignore-submodules"]).len())
}

pub fn dirty(dir: &Path) -> bool {
    !git(dir, &["status", "--porcelain", "--ignore-submodules"]).trim().is_empty()
}

pub fn log(dir: &Path, n: usize) -> Vec<Commit> {
    let n = n.to_string();
    git(dir, &["log", "-n", &n, "--format=%H%x1f%h%x1f%s%x1f%ar%x1f%an"]).lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\u{1f}').collect();
            (f.len() == 5).then(|| Commit { hash: f[0].into(), short: f[1].into(), subject: f[2].into(), when: f[3].into(), author: f[4].into() })
        })
        .collect()
}

pub fn changes(dir: &Path, hash: &str) -> Vec<Change> {
    let out = if hash == WORKTREE { git(dir, &["diff", "--numstat", "--ignore-submodules", "HEAD"]) } else { git(dir, &["show", "--numstat", "--format=", hash]) };
    out.lines()
        .filter_map(|l| {
            let mut p = l.splitn(3, '\t');
            let (a, d, path) = (p.next()?, p.next()?, p.next()?);
            Some(Change { path: path.to_string(), add: a.parse().unwrap_or(0), del: d.parse().unwrap_or(0) })
        })
        .collect()
}

/// Parses unified-diff text into numbered lines (everything from the first hunk on).
pub fn parse_diff(text: &str, cap: usize) -> Vec<DiffLine> {
    let (mut old, mut new) = (0u32, 0u32);
    let mut out = Vec::new();
    let mut in_hunk = false;
    for l in text.lines() {
        if let Some(rest) = l.strip_prefix("@@ ") {
            // @@ -a,b +c,d @@ context
            let nums: Vec<&str> = rest.split_whitespace().take(2).collect();
            let start = |s: &str| s.trim_start_matches(['-', '+']).split(',').next().and_then(|n| n.parse::<u32>().ok()).unwrap_or(1);
            if nums.len() == 2 {
                old = start(nums[0]);
                new = start(nums[1]);
            }
            in_hunk = true;
            out.push(DiffLine { kind: Kind::Hunk, old: None, new: None, text: l.to_string() });
        } else if !in_hunk || l.starts_with('\\') {
            continue;
        } else if let Some(t) = l.strip_prefix('+') {
            out.push(DiffLine { kind: Kind::Add, old: None, new: Some(new), text: t.to_string() });
            new += 1;
        } else if let Some(t) = l.strip_prefix('-') {
            out.push(DiffLine { kind: Kind::Del, old: Some(old), new: None, text: t.to_string() });
            old += 1;
        } else {
            out.push(DiffLine { kind: Kind::Ctx, old: Some(old), new: Some(new), text: l.strip_prefix(' ').unwrap_or(l).to_string() });
            old += 1;
            new += 1;
        }
        if out.len() >= cap {
            break;
        }
    }
    out
}

pub fn diff(dir: &Path, hash: &str, path: &str, cap: usize) -> Vec<DiffLine> {
    let text = if hash == WORKTREE { git(dir, &["diff", "--ignore-submodules", "HEAD", "--", path]) } else { git(dir, &["show", "--format=", hash, "--", path]) };
    parse_diff(&text, cap)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_lines_are_numbered_from_the_hunk_header() {
        let text = "diff --git a/x b/x\nindex 1..2\n--- a/x\n+++ b/x\n@@ -10,3 +10,3 @@ fn f()\n keep\n-old\n+new\n tail\n";
        let d = parse_diff(text, 100);
        assert_eq!(d[0].kind, Kind::Hunk);
        assert_eq!((d[1].old, d[1].new, d[1].kind), (Some(10), Some(10), Kind::Ctx));
        assert_eq!((d[2].old, d[2].new, d[2].kind), (Some(11), None, Kind::Del));
        assert_eq!((d[3].old, d[3].new, d[3].kind), (None, Some(11), Kind::Add));
        assert_eq!((d[4].old, d[4].new), (Some(12), Some(12)));
        assert_eq!(d.len(), 5, "file headers are skipped");
    }

    #[test]
    fn a_real_repo_round_trips_commits_changes_and_diffs() {
        let d = std::env::temp_dir().join(format!("ls-repo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let run = |a: &[&str]| { Command::new("git").arg("-C").arg(&d).args(a).output().unwrap(); };
        run(&["init", "-q"]);
        run(&["config", "user.email", "t@t"]);
        run(&["config", "user.name", "t"]);
        std::fs::write(d.join("a.txt"), "one\ntwo\n").unwrap();
        run(&["add", "."]);
        run(&["commit", "-qm", "first"]);
        std::fs::write(d.join("a.txt"), "one\nTWO\nthree\n").unwrap();
        run(&["commit", "-qam", "second"]);
        assert!(!dirty(&d));
        let l = log(&d, 10);
        assert_eq!((l.len(), l[0].subject.as_str()), (2, "second"));
        let c = changes(&d, &l[0].hash);
        assert_eq!(c, vec![Change { path: "a.txt".into(), add: 2, del: 1 }]);
        let lines = diff(&d, &l[0].hash, "a.txt", 100);
        assert!(lines.iter().any(|x| x.kind == Kind::Add && x.text == "three"));
        std::fs::write(d.join("a.txt"), "changed\n").unwrap();
        assert!(dirty(&d));
        assert_eq!(changes(&d, WORKTREE).len(), 1);
        std::fs::remove_dir_all(d).unwrap();
    }
}
