//! Left out and why (#227): what the backup does NOT hold, and the reason, for Computer > Backup. The one reader
//! (`lifestream left-out`); `backup-state` counts the same list.
//!
//! Sources, both written by `packaging/libexec/left-out.sh` (the one writer) into `<share>/hestia/` on this computer only:
//! `skipped.jsonl` (by `capture.sh`: too large, unreadable, private by rule, links that are not followed) and
//! `withheld.jsonl` (by `watch.sh`: lines and files the secret scanner held back). Each line is
//! `{"why":"...","path":"<relative to home>"|null,"since":<epoch>,["bytes":N,"limit":N],["count":N]}`.
//!
//! Every line is untrusted: one that does not parse, has an unknown reason, or a reason the file may not hold, is
//! skipped. A path is printed only when it is plainly relative to the home folder (no `..`, no leading `/` or `~`, no
//! control or direction-changing characters, at most 240 characters); otherwise it is `null`, never cut or cleaned up.
//! The `detail` sentence is chosen by the reason and built from numbers only: nothing the files say is copied. Nothing
//! is opened but the two lists (never through a link), so no file contents and no secret can be printed.

use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::path::Path;

/// Every reason, in the order they are counted. New reasons may be added: a reader should show an unknown one plainly.
pub const WHYS: [&str; 7] = ["too_large", "unreadable", "private_by_rule", "withheld_secret_lines", "withheld_secret_file", "link_outside_home", "link_to_private"];
/// Which reasons each list may hold (a line in the wrong list is not believed).
const SKIPPED: (&str, &[&str]) = ("skipped.jsonl", &["too_large", "unreadable", "private_by_rule", "link_outside_home", "link_to_private"]);
const WITHHELD: (&str, &[&str]) = ("withheld.jsonl", &["withheld_secret_lines", "withheld_secret_file"]);
/// Only the first part of each list is read.
const READ_CAP: u64 = 1024 * 1024;
pub const DEFAULT_LIMIT: usize = 100;
pub const MAX_LIMIT: usize = 1000;
/// The items printed never exceed this many bytes (the summary line comes after, always).
const OUTPUT_CAP: usize = 512 * 1024;
const PATH_LEN_CAP: usize = 240;

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct Item {
    /// relative to the home folder; null when it cannot be shown safely
    pub path: Option<String>,
    pub why: &'static str,
    /// one plain sentence
    pub detail: String,
    /// RFC3339 UTC: when it was first left out; null if unknown
    pub since: Option<String>,
    /// could it be backed up after the person fixes something (shrink it, make it readable)? Never for private or secret kinds.
    pub can_include: bool,
    #[serde(skip)]
    at: i64,
}

/// A path that can be shown as it is, or None.
fn shown(p: &str) -> Option<String> {
    let odd = |c: char| c.is_control() || matches!(c, '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{FFFD}' | '\\');
    let ok = !p.is_empty()
        && p.chars().count() <= PATH_LEN_CAP
        && !p.starts_with(['/', '~'])
        && !p.chars().any(odd)
        && p.split('/').all(|c| !c.is_empty() && c != "." && c != "..");
    ok.then(|| p.to_string())
}

fn size(b: u64) -> String {
    const MB: u64 = 1024 * 1024;
    if b >= 1024 * MB {
        format!("{:.1} GB", b as f64 / (1024 * MB) as f64)
    } else if b >= MB {
        format!("{} MB", b / MB)
    } else {
        format!("{} KB", b.div_ceil(1024))
    }
}

/// The plain sentence for a reason. Numbers come from the line; every word is fixed here.
fn detail(why: &str, path: Option<&str>, v: &Value) -> String {
    match why {
        "too_large" => match (v["bytes"].as_u64().filter(|b| *b > 0), v["limit"].as_u64().filter(|l| *l > 0)) {
            (Some(b), Some(l)) => format!("{} is over the {} limit", size(b), size(l)),
            _ => "This file is over the size limit".into(),
        },
        "unreadable" => "Hestia could not read it: it is missing, a broken link, or you do not have permission to open it".into(),
        "private_by_rule" => match path.and_then(crate::deny::kind_of) {
            Some(k) => format!("Never backed up: it holds private things ({k})"),
            None => "Never backed up: its name or place says it holds private things (keys, sign-ins, passwords)".into(),
        },
        "withheld_secret_lines" => match v["count"].as_u64().filter(|n| *n > 0).map(|n| n.min(1_000_000)) {
            Some(1) => "1 line that looked like a secret was left out; the rest of the file is backed up".into(),
            Some(n) => format!("{n} lines that looked like secrets were left out; the rest of the file is backed up"),
            None => "Lines that looked like secrets were left out; the rest of the file is backed up".into(),
        },
        "withheld_secret_file" => "This file looked like it holds a secret, so all of it was left out".into(),
        "link_outside_home" => "This is a link to something outside your home folder, and links are not followed there".into(),
        _ => "This is a link to a private file, so it is not followed".into(), // link_to_private
    }
}

/// The first `READ_CAP` bytes of a regular file (never through a link) as whole lines.
fn lines(p: &Path) -> Vec<String> {
    let Ok(m) = fs::symlink_metadata(p) else { return vec![] };
    if !m.file_type().is_file() {
        return vec![];
    }
    let Ok(f) = fs::File::open(p) else { return vec![] };
    let mut buf = Vec::new();
    if f.take(READ_CAP).read_to_end(&mut buf).is_err() {
        return vec![];
    }
    let cut = m.len() > READ_CAP;
    let text = String::from_utf8_lossy(&buf);
    let mut out: Vec<String> = text.lines().map(String::from).collect();
    if cut {
        out.pop(); // the last line may be cut in half
    }
    out
}

fn from_list(dir: &Path, (name, allowed): (&str, &[&'static str]), out: &mut Vec<Item>) {
    for l in lines(&dir.join(name)) {
        let Ok(v) = serde_json::from_str::<Value>(&l) else { continue };
        let Some(why) = v["why"].as_str().and_then(|w| allowed.iter().find(|a| **a == w)).copied() else { continue };
        let path = match &v["path"] {
            Value::Null => None,
            Value::String(s) => shown(s),
            _ => continue,
        };
        let at = v["since"].as_i64().filter(|t| *t > 0).unwrap_or(0);
        let since = (at > 0).then(|| chrono::DateTime::from_timestamp(at, 0)).flatten().map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Secs, true));
        let detail = detail(why, path.as_deref(), &v);
        out.push(Item { path, why, detail, since, can_include: matches!(why, "too_large" | "unreadable"), at });
    }
}

/// Everything left out, newest first (then by reason and path); one item per reason and path.
pub fn items(dir: &Path) -> Vec<Item> {
    let mut all = Vec::new();
    from_list(dir, SKIPPED, &mut all);
    from_list(dir, WITHHELD, &mut all);
    let mut seen = BTreeSet::new();
    all.retain(|i| i.path.is_none() || seen.insert((i.why, i.path.clone())));
    let rank = |w: &str| WHYS.iter().position(|x| *x == w).unwrap_or(WHYS.len());
    all.sort_by(|a, b| b.at.cmp(&a.at).then(rank(a.why).cmp(&rank(b.why))).then_with(|| a.path.cmp(&b.path)));
    all
}

/// How many per reason (every reason present, 0 when none).
pub fn counts(items: &[Item]) -> BTreeMap<&'static str, usize> {
    let mut c: BTreeMap<&'static str, usize> = WHYS.iter().map(|w| (*w, 0)).collect();
    for i in items {
        *c.entry(i.why).or_default() += 1;
    }
    c
}

/// For `backup-state`: `{"count":N,"by_why":{...}}`.
pub fn summary_json(dir: &Path) -> Value {
    let all = items(dir);
    json!({"count": all.len(), "by_why": counts(&all)})
}

/// JSON lines: at most `limit` items (clamped to 1..=1000, and to 512 KB), then the summary of everything.
pub fn render(all: &[Item], limit: usize) -> String {
    let mut out = String::new();
    for i in all.iter().take(limit.clamp(1, MAX_LIMIT)) {
        let Ok(line) = serde_json::to_string(i) else { continue };
        if out.len() + line.len() + 1 > OUTPUT_CAP {
            break;
        }
        out.push_str(&line);
        out.push('\n');
    }
    out.push_str(&json!({"event": "summary", "counts": counts(all)}).to_string());
    out.push('\n');
    out
}

pub fn cmd(share: Option<std::path::PathBuf>, limit: usize) -> anyhow::Result<()> {
    let dir = crate::backup::dir_of(share)?;
    print!("{}", render(&items(&dir), limit));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fresh() -> PathBuf {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let d = std::env::temp_dir().join(format!("ls-leftout-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::SeqCst)));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn with(skipped: &str, withheld: &str) -> PathBuf {
        let d = fresh();
        fs::write(d.join("skipped.jsonl"), skipped).unwrap();
        fs::write(d.join("withheld.jsonl"), withheld).unwrap();
        d
    }

    #[test]
    fn every_reason_reads_with_a_plain_sentence() {
        let d = with(
            concat!(
                r#"{"why":"too_large","path":".config/big/huge.bin","since":1800000000,"bytes":125829120,"limit":52428800}"#, "\n",
                r#"{"why":"unreadable","path":".config/many/f3","since":1800000000}"#, "\n",
                r#"{"why":"private_by_rule","path":".ssh/config","since":1800000000}"#, "\n",
                r#"{"why":"private_by_rule","path":".config/gtk-3.0/bookmarks","since":1800000000}"#, "\n",
                r#"{"why":"link_outside_home","path":".outside","since":1800000000}"#, "\n",
                r#"{"why":"link_to_private","path":".mylink","since":1800000000}"#, "\n",
            ),
            concat!(
                r#"{"why":"withheld_secret_lines","path":".bashrc","since":1800000000,"count":2}"#, "\n",
                r#"{"why":"withheld_secret_file","path":".config/odd/tok.conf","since":1800000000}"#, "\n",
            ),
        );
        let all = items(&d);
        assert_eq!(all.len(), 8);
        let get = |p: &str| all.iter().find(|i| i.path.as_deref() == Some(p)).unwrap().clone();
        assert_eq!(get(".config/big/huge.bin").detail, "120 MB is over the 50 MB limit");
        assert!(get(".config/big/huge.bin").can_include && get(".config/many/f3").can_include);
        assert_eq!(get(".bashrc").detail, "2 lines that looked like secrets were left out; the rest of the file is backed up");
        assert!(get(".ssh/config").detail.contains("SSH keys"));
        assert!(get(".config/gtk-3.0/bookmarks").detail.starts_with("Never backed up"));
        for p in [".ssh/config", ".bashrc", ".config/odd/tok.conf", ".outside", ".mylink"] {
            assert!(!get(p).can_include, "{p}: a private or secret kind can never be included");
        }
        assert_eq!(get(".bashrc").since.as_deref(), Some("2027-01-15T08:00:00Z"));
        let c = counts(&all);
        assert_eq!((c["private_by_rule"], c["too_large"], c["withheld_secret_file"]), (2, 1, 1));
    }

    #[test]
    fn private_by_rule_is_never_includable() {
        let d = with(r#"{"why":"private_by_rule","path":".aws/credentials","since":1}"#, "");
        assert!(!items(&d)[0].can_include);
    }

    #[test]
    fn malformed_lines_unknown_reasons_and_misplaced_reasons_are_skipped() {
        let d = with(
            concat!(
                "not json\n",
                r#"{"why":"made_up","path":"a","since":1}"#, "\n",
                r#"{"why":"withheld_secret_file","path":"a","since":1}"#, "\n", // a withheld reason in the skipped list
                r#"{"why":"too_large","path":7,"since":1}"#, "\n",
                r#"{"path":"a"}"#, "\n",
                r#"{"why":"unreadable","path":"ok/file","since":"soon"}"#, "\n",
            ),
            r#"{"why":"too_large","path":"b","since":1}"#,
        );
        let all = items(&d);
        assert_eq!(all.len(), 1, "{all:?}");
        assert_eq!((all[0].path.as_deref(), all[0].since.as_deref()), (Some("ok/file"), None));
        assert!(items(&fresh()).is_empty(), "no lists: nothing, never an error");
    }

    #[test]
    fn unsafe_paths_are_null_never_cleaned_or_cut() {
        let long = "a/".repeat(130) + "b";
        let lines: String = ["/etc/passwd", "~/x", "../x", "a/../../x", "a\u{202E}gpj.exe", "a\nb", "a\\b", long.as_str(), "", "a//b"]
            .iter()
            .map(|p| json!({"why": "unreadable", "path": p, "since": 1}).to_string() + "\n")
            .collect();
        let d = with(&lines, "");
        let all = items(&d);
        assert_eq!(all.len(), 10, "every line is still listed (and counted)");
        assert!(all.iter().all(|i| i.path.is_none()), "{all:?}");
    }

    #[test]
    fn duplicates_are_one_item_and_newest_comes_first() {
        let d = with(
            concat!(
                r#"{"why":"unreadable","path":"x","since":10}"#, "\n",
                r#"{"why":"unreadable","path":"x","since":10}"#, "\n",
                r#"{"why":"too_large","path":"y","since":20,"bytes":1,"limit":1}"#, "\n",
            ),
            "",
        );
        let all = items(&d);
        assert_eq!(all.iter().map(|i| i.path.as_deref().unwrap()).collect::<Vec<_>>(), ["y", "x"]);
    }

    #[test]
    fn output_is_capped_and_always_ends_with_the_summary() {
        let lines: String = (0..3000).map(|n| json!({"why": "unreadable", "path": format!("d/{n:0>200}"), "since": 1}).to_string() + "\n").collect();
        let d = with(&lines, "");
        let all = items(&d);
        assert_eq!(all.len(), 3000);
        let out = render(&all, 5000);
        assert!(out.len() <= OUTPUT_CAP + 1024, "{}", out.len());
        let n = out.lines().count() - 1;
        assert!(n <= MAX_LIMIT, "{n}");
        let last: Value = serde_json::from_str(out.lines().last().unwrap()).unwrap();
        assert_eq!((last["event"].as_str(), last["counts"]["unreadable"].as_u64()), (Some("summary"), Some(3000)));
        assert_eq!(render(&all, 0).lines().count(), 2, "a limit of 0 still shows one");
        assert_eq!(render(&all, 3).lines().count(), 4);
    }

    #[test]
    fn a_list_that_is_a_link_or_huge_is_not_followed_or_read_whole() {
        let d = fresh();
        let elsewhere = d.join("elsewhere.jsonl");
        fs::write(&elsewhere, r#"{"why":"unreadable","path":"x","since":1}"#).unwrap();
        std::os::unix::fs::symlink(&elsewhere, d.join("skipped.jsonl")).unwrap();
        assert!(items(&d).is_empty(), "a list that is a link is never read");
        let mut big = String::new();
        let mut n = 0;
        while big.len() as u64 <= READ_CAP + 4096 {
            big.push_str(&json!({"why": "unreadable", "path": format!("f{n}"), "since": 1}).to_string());
            big.push('\n');
            n += 1;
        }
        fs::remove_file(d.join("skipped.jsonl")).unwrap();
        fs::write(d.join("skipped.jsonl"), &big).unwrap();
        let all = items(&d);
        assert!(all.len() < n && !all.is_empty());
    }

    #[test]
    fn the_summary_for_backup_state_counts_by_reason() {
        let d = with(r#"{"why":"unreadable","path":"x","since":1}"#, r#"{"why":"withheld_secret_file","path":"y","since":1}"#);
        let v = summary_json(&d);
        assert_eq!(v["count"], 2);
        assert_eq!(v["by_why"]["withheld_secret_file"], 1);
        assert_eq!(v["by_why"]["link_to_private"], 0);
    }
}
