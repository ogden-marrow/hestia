//! The feed: one read-only, time-ordered list of what happened and who decided it. It is the one reader the
//! Stream screen uses. Sources: the move journal (`<stream>/moves.jsonl`, with `actor` and `reason` on each line),
//! the log of undos (`<stream>/undone.jsonl`) and the watcher's run history (`<share>/hestia/watch-history.jsonl`).
//! Every source is a file on disk, so every line is untrusted: a line that does not parse, or names a path that is not a
//! plain visible path inside the home folder, is skipped, never fatal and never printed. Nothing is moved, changed or
//! deleted, and no file contents are read, only relative paths the journal already holds.
//!
//! Not emitted yet, because nothing records them: `question_asked`, `question_answered` (an answer is stored on the
//! record, not logged) and `scan` (a scan leaves no log). The kinds are reserved in the contract.

use crate::{tree, By};
use anyhow::{bail, Context, Result};
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Largest slice of each source file that is read (its newest end).
const READ_CAP: u64 = 4 * 1024 * 1024;
/// Paths listed per item (the item's `count` still says how many there were).
const PATHS_CAP: usize = 5;
pub const DEFAULT_LIMIT: usize = 100;
pub const MAX_LIMIT: usize = 1000;
/// The whole output never exceeds this many bytes.
const OUTPUT_CAP: usize = 512 * 1024;
const TEXT_CAP: usize = 160;
const PATH_LEN_CAP: usize = 240;

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct Item {
    /// stable per event; not a hash
    pub id: String,
    /// RFC3339, UTC
    pub at: String,
    /// filed | moved | renamed | folder_created | undone | backup_saved | backup_failed
    /// (reserved: question_asked | question_answered | scan)
    pub kind: &'static str,
    /// hestia_rule | model | you | watcher | unknown
    pub actor: &'static str,
    pub reason: String,
    pub summary: String,
    pub count: usize,
    /// relative to the home folder, at most 5
    pub paths: Vec<String>,
    pub undoable: bool,
    /// the id `lifestream file --undo --batch ID` takes; null when nothing moved
    pub batch: Option<String>,
    #[serde(skip)]
    when: DateTime<Utc>,
}

pub struct Opts {
    pub since: Option<DateTime<Utc>>,
    pub limit: usize,
}

/// `90s`, `30m`, `2h`, `7d`, `1w`, or an RFC3339 timestamp.
pub fn parse_since(s: &str, now: DateTime<Utc>) -> Result<DateTime<Utc>> {
    let s = s.trim();
    if let Ok(t) = DateTime::parse_from_rfc3339(s) {
        return Ok(t.with_timezone(&Utc));
    }
    let bad = || format!("not a time: {s} (use 30m, 2h, 7d or a timestamp like 2026-11-18T09:00:00Z)");
    let (n, unit) = s.split_at(s.len().saturating_sub(1));
    let n: i64 = n.parse().ok().filter(|n| (0..=100_000).contains(n)).with_context(bad)?;
    let d = match unit {
        "s" => Duration::seconds(n),
        "m" => Duration::minutes(n),
        "h" => Duration::hours(n),
        "d" => Duration::days(n),
        "w" => Duration::weeks(n),
        _ => bail!(bad()),
    };
    Ok(now - d)
}

/// Who decided a filing, and why in one plain sentence, from the record that was filed.
pub fn actor_reason(by: &By, kind: &str) -> (&'static str, String) {
    let label = kind.rsplit('/').next().unwrap_or(kind);
    match by.tier.as_str() {
        "human" => ("you", format!("you answered: {label}")),
        "rule" => ("hestia_rule", format!("rule: {}", by.name.replace(['-', '_'], " "))),
        "model" => ("model", format!("both models agreed: {label}")),
        _ => ("unknown", String::new()),
    }
}

fn actor_name(s: &str) -> &'static str {
    match s {
        "hestia_rule" => "hestia_rule",
        "model" => "model",
        "you" => "you",
        "watcher" => "watcher",
        _ => "unknown",
    }
}

/// Printable text only, no direction-changing characters, bounded.
fn clean(s: &str) -> String {
    plain(s).take(TEXT_CAP).collect()
}

fn plain(s: &str) -> impl Iterator<Item = char> + '_ {
    s.chars().filter(|c| !c.is_control() && !matches!(c, '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'))
}

/// A path as listed: left out (not cut short) if it has odd characters or is very long.
fn path_ok(p: &str) -> Option<String> {
    (p.chars().count() <= PATH_LEN_CAP && plain(p).count() == p.chars().count()).then(|| p.to_string())
}

/// The newest `READ_CAP` bytes of a file as lines (a first line cut in half is dropped). Missing file: no lines.
fn tail_lines(p: &Path) -> Vec<String> {
    let Ok(mut f) = fs::File::open(p) else { return vec![] };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let cut = len > READ_CAP;
    if cut && f.seek(SeekFrom::Start(len - READ_CAP)).is_err() {
        return vec![];
    }
    let mut buf = Vec::new();
    if f.take(READ_CAP).read_to_end(&mut buf).is_err() {
        return vec![];
    }
    let text = String::from_utf8_lossy(&buf);
    text.lines().skip(usize::from(cut)).map(String::from).collect()
}

fn rfc(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn parse_time(v: &Value) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(v.as_str()?).ok().map(|t| t.with_timezone(&Utc))
}

fn name_of(rel: &str) -> &str {
    rel.rsplit('/').next().unwrap_or(rel)
}

fn parent_of(rel: &str) -> &str {
    rel.rsplit_once('/').map_or("", |(p, _)| p)
}

fn plural(n: usize, one: &str, many: &str) -> String {
    if n == 1 { format!("1 {one}") } else { format!("{n} {many}") }
}

fn nice(folder: &str) -> &str {
    if folder.is_empty() { "your home folder" } else { folder }
}

/// One journal line that passed every check. Paths are relative to home.
struct Line {
    batch: String,
    when: DateTime<Utc>,
    kind: &'static str,
    actor: &'static str,
    reason: String,
    from: String,
    to: String,
}

fn journal_items(home: &Path, stream: &Path, out: &mut Vec<Item>) {
    let raw: Vec<Value> = tail_lines(&stream.join("moves.jsonl")).iter().filter_map(|l| serde_json::from_str(l).ok()).collect();
    // lines written before actors were recorded: a file's actor is read from the record that was filed
    let legacy = raw.iter().any(|v| v["actor"].is_null() && !matches!(v["kind"].as_str(), Some("dir" | "mkdir")));
    let recs = if legacy { crate::load_all(stream).unwrap_or_default() } else { Vec::new() };
    let mut lines = Vec::new();
    for v in &raw {
        let (Some(batch), Some(when)) = (v["batch"].as_str(), parse_time(&v["batch"])) else { continue };
        let jkind = v["kind"].as_str().unwrap_or("file");
        let Some(to_s) = v["to"].as_str() else { continue };
        // the same test undo uses: a plain, visible path inside the home folder, or the line is not listed at all
        if tree::under_home(home, to_s).is_none() {
            continue;
        }
        let from_s = v["from"].as_str().unwrap_or("");
        if jkind != "mkdir" && tree::under_home(home, from_s).is_none() {
            continue;
        }
        let to = to_s[2..].to_string();
        let from = if jkind == "mkdir" { String::new() } else { from_s[2..].to_string() };
        let kind = match jkind {
            "file" => "filed",
            "mkdir" => "folder_created",
            "dir" if parent_of(&from) == parent_of(&to) => "renamed",
            "dir" => "moved",
            _ => continue,
        };
        let (actor, reason) = match v["actor"].as_str() {
            Some(a) => (actor_name(a), clean(v["reason"].as_str().unwrap_or(""))),
            None if jkind == "mkdir" => ("you", "you made a new folder".to_string()),
            None if jkind == "dir" => ("you", "you changed your folders".to_string()),
            None => match v["id"].as_str().and_then(|id| recs.iter().find(|r| r.id == id)) {
                Some(r) => actor_reason(&r.by, &r.kind),
                None => ("unknown", String::new()),
            },
        };
        let reason = if actor == "unknown" && reason.is_empty() { "recorded before Hestia kept track of who decided".to_string() } else { reason };
        lines.push(Line { batch: batch.to_string(), when, kind, actor, reason, from, to });
    }
    // one item per batch, kind, actor and reason: a batch can hold several decisions
    let mut groups: BTreeMap<(String, &str, &str, String), Vec<&Line>> = BTreeMap::new();
    for l in &lines {
        groups.entry((l.batch.clone(), l.kind, l.actor, l.reason.clone())).or_default().push(l);
    }
    let mut ordinal: BTreeMap<String, usize> = BTreeMap::new();
    for ((batch, kind, actor, reason), ls) in groups {
        let n = ordinal.entry(batch.clone()).or_default();
        let id = format!("{batch}:{n}");
        *n += 1;
        let count = ls.len();
        let first = ls[0];
        let parents: BTreeSet<&str> = ls.iter().map(|l| parent_of(&l.to)).collect();
        let summary = match kind {
            "filed" if count == 1 => format!("Filed {} into {}", name_of(&first.to), nice(parent_of(&first.to))),
            "filed" if parents.len() == 1 => format!("Filed {} into {}", plural(count, "file", "files"), nice(parent_of(&first.to))),
            "filed" => format!("Filed {} into {}", plural(count, "file", "files"), plural(parents.len(), "folder", "folders")),
            "renamed" => format!("Renamed the folder {} to {}", name_of(&first.from), name_of(&first.to)),
            "moved" => format!("Moved the folder {} into {}", name_of(&first.from), nice(parent_of(&first.to))),
            _ => format!("Made a new folder {}", nice(&first.to)),
        };
        out.push(Item {
            id,
            at: rfc(first.when),
            kind,
            actor,
            reason,
            summary: clean(&summary),
            count,
            paths: ls.iter().filter_map(|l| path_ok(&l.to)).take(PATHS_CAP).collect(),
            undoable: true,
            batch: Some(batch),
            when: first.when,
        });
    }
}

/// `<stream>/undone.jsonl`: one line per undo that put something back, written by `tree::undo_batch`.
fn undone_items(home: &Path, stream: &Path, out: &mut Vec<Item>) {
    for l in tail_lines(&stream.join("undone.jsonl")) {
        let Ok(v) = serde_json::from_str::<Value>(&l) else { continue };
        let Some(when) = parse_time(&v["at"]) else { continue };
        let num = |k: &str| v[k].as_u64().unwrap_or(0).min(1_000_000) as usize;
        let (files, folders, removed) = (num("files"), num("folders"), num("removed"));
        let count = files + folders + removed;
        if count == 0 {
            continue;
        }
        let paths: Vec<String> = v["paths"]
            .as_array()
            .map(|a| a.iter().filter_map(|p| p.as_str()).filter(|p| tree::under_home(home, p).is_some()).filter_map(|p| path_ok(&p[2..])).take(PATHS_CAP).collect())
            .unwrap_or_default();
        let mut parts = Vec::new();
        if files > 0 {
            parts.push(format!("Put back {}", plural(files, "file", "files")));
        }
        if folders > 0 {
            parts.push(format!("put back {}", plural(folders, "folder", "folders")));
        }
        if removed > 0 {
            parts.push(format!("took away {}", plural(removed, "new folder", "new folders")));
        }
        let mut summary = parts.join(" and ");
        if files == 0 {
            summary = summary[..1].to_uppercase() + &summary[1..];
        }
        out.push(Item {
            id: format!("undone:{}", rfc(when)),
            at: rfc(when),
            kind: "undone",
            actor: "you",
            reason: "you pressed Undo".into(),
            summary,
            count,
            paths,
            undoable: false,
            batch: None,
            when,
        });
    }
}

/// `<share>/hestia/watch-history.jsonl`: `{"at":epoch,"result":"ok: ... | FAILED: ...","commits":N}`, one per watcher run.
/// Runs that changed nothing, and the routine "waiting for a quieter moment" runs, are not events. A failure that repeats
/// run after run with the same words is one item (at its first run, with how many runs it covers).
fn watch_items(home: &Path, history: &Path, out: &mut Vec<Item>) {
    let mut runs: Vec<(i64, String, u64)> = tail_lines(history)
        .iter()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter_map(|v| Some((v["at"].as_i64()?, v["result"].as_str()?.to_string(), v["commits"].as_u64().unwrap_or(0))))
        .collect();
    runs.sort_by_key(|r| r.0);
    let home_s = home.to_string_lossy();
    let mut last_failed: Option<usize> = None; // index in `out` of the failure the previous runs began
    let mut last_text = String::new();
    for (at, result, commits) in runs {
        let Some(when) = DateTime::from_timestamp(at, 0) else { continue };
        if let Some(text) = result.strip_prefix("FAILED") {
            let text = clean(&text.trim_start_matches(':').trim().replace(home_s.as_ref(), "~"));
            if let (Some(i), true) = (last_failed, text == last_text) {
                out[i].count += 1;
                continue;
            }
            last_failed = Some(out.len());
            last_text = text.clone();
            out.push(Item {
                id: format!("backup:{at}"),
                at: rfc(when),
                kind: "backup_failed",
                actor: "watcher",
                reason: "the backup ran by itself and could not finish".into(),
                summary: clean(&format!("Backup could not finish: {text}")),
                count: 1,
                paths: vec![],
                undoable: false,
                batch: None,
                when,
            });
            continue;
        }
        if result.starts_with("busy") || result.starts_with("battery") || result.starts_with("git is busy") {
            continue; // routine waiting does not end a run of failures
        }
        last_failed = None;
        if result.starts_with("ok") && commits > 0 {
            let n = commits.min(1_000_000) as usize;
            out.push(Item {
                id: format!("backup:{at}"),
                at: rfc(when),
                kind: "backup_saved",
                actor: "watcher",
                reason: "the backup ran by itself when the computer was quiet".into(),
                summary: format!("Backup saved {}", plural(n, "change", "changes")),
                count: n,
                paths: vec![],
                undoable: false,
                batch: None,
                when,
            });
        }
    }
}

/// The feed, newest first.
pub fn feed(home: &Path, stream: &Path, history: &Path, opts: &Opts) -> Vec<Item> {
    let mut items = Vec::new();
    journal_items(home, stream, &mut items);
    undone_items(home, stream, &mut items);
    watch_items(home, history, &mut items);
    if let Some(since) = opts.since {
        items.retain(|i| i.when >= since);
    }
    items.sort_by(|a, b| b.when.cmp(&a.when).then_with(|| b.id.cmp(&a.id)));
    items.truncate(opts.limit.clamp(1, MAX_LIMIT));
    items
}

/// JSON lines, stopping before the output would pass `OUTPUT_CAP` bytes.
pub fn render(items: &[Item]) -> String {
    let mut out = String::new();
    for i in items {
        let Ok(line) = serde_json::to_string(i) else { continue };
        if out.len() + line.len() + 1 > OUTPUT_CAP {
            break;
        }
        out.push_str(&line);
        out.push('\n');
    }
    out
}

/// Where the watcher keeps its run history.
pub fn history_path() -> Option<PathBuf> {
    Some(crate::share_dir()?.join("hestia/watch-history.jsonl"))
}

pub fn cmd_feed(since: Option<String>, limit: usize, stream: &Path) -> Result<()> {
    let home = crate::home()?;
    let since = since.map(|s| parse_since(&s, Utc::now())).transpose()?;
    let history = history_path().unwrap_or_default();
    print!("{}", render(&feed(&home, stream, &history, &Opts { since, limit })));
    Ok(())
}
