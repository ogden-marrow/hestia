//! Is the backup working, and does anyone need to be told? The one place that policy lives (`lifestream backup-state`).
//! Inputs, all untrusted files: `<share>/hestia/watch.json` and `watch-history.jsonl` (written by the watcher) and
//! `<share>/hestia/notified.json` (the episode a notification was already sent for). Output: one JSON line with a
//! state, a plain sentence and a yes/no for "send a desktop notification now". No file contents and no paths are ever
//! printed: the sentence is chosen from a fixed list by the kind of failure, never copied from the watcher's text.

use anyhow::{Context, Result};
use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

/// No successful run for this long, while the timer should have fired many times: the backup is stale.
pub const STALE_AFTER: i64 = 36 * 3600;
/// A problem must last this long before anyone is told (not on the first blip).
pub const NOTIFY_AFTER: i64 = 6 * 3600;
/// Only the newest end of the history is read.
const READ_CAP: u64 = 1024 * 1024;

#[derive(Debug, PartialEq)]
pub struct State {
    pub state: &'static str,
    pub since: Option<i64>,
    pub reason: Option<String>,
    pub failing_runs: usize,
    pub last_ok: Option<i64>,
    pub notify: bool,
}

impl State {
    fn quiet(state: &'static str, last_ok: Option<i64>) -> Self {
        State { state, since: None, reason: None, failing_runs: 0, last_ok, notify: false }
    }
    /// Does this state need a person (and so can it be an "episode")?
    pub fn needs_you(&self) -> bool {
        matches!(self.state, "failing" | "stale")
    }
    pub fn to_json(&self) -> String {
        let t = |s: Option<i64>| s.and_then(|s| DateTime::from_timestamp(s, 0)).map(|d| d.to_rfc3339_opts(SecondsFormat::Secs, true));
        json!({"state": self.state, "since": t(self.since), "reason": self.reason, "failing_runs": self.failing_runs, "last_ok": t(self.last_ok), "notify": self.notify}).to_string()
    }
}

#[derive(PartialEq, Clone, Copy)]
enum Kind {
    Ok,
    Failed,
    Routine, // waiting for a quiet moment, battery, git busy: neither good nor bad
}

fn read_tail(p: &Path) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = fs::File::open(p).ok()?;
    let len = f.metadata().ok()?.len();
    if len > READ_CAP {
        f.seek(SeekFrom::Start(len - READ_CAP)).ok()?;
    }
    let mut buf = Vec::new();
    f.take(READ_CAP).read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

fn run_of(v: &Value) -> Option<(i64, Kind, String)> {
    let at = v["at"].as_i64()?;
    let result = v["result"].as_str()?;
    let kind = if result.starts_with("FAILED") {
        Kind::Failed
    } else if result.starts_with("ok") {
        Kind::Ok
    } else {
        Kind::Routine
    };
    Some((at, kind, result.to_string()))
}

/// Every run we can read, oldest first. A line that does not parse is skipped.
fn runs(dir: &Path) -> Vec<(i64, Kind, String)> {
    let mut out: Vec<_> = read_tail(&dir.join("watch-history.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter_map(|v| run_of(&v))
        .collect();
    // the last run's own file covers a history that is missing or behind
    if let Some(last) = read_tail(&dir.join("watch.json")).and_then(|t| serde_json::from_str::<Value>(t.trim()).ok()).and_then(|v| run_of(&v)) {
        if out.iter().all(|r| r.0 < last.0) {
            out.push(last);
        }
    }
    out.sort_by_key(|r| r.0);
    out
}

fn notified_episode(dir: &Path) -> Option<i64> {
    serde_json::from_str::<Value>(&read_tail(&dir.join("notified.json"))?).ok()?["episode"].as_i64()
}

fn hours(secs: i64) -> String {
    let h = (secs / 3600).max(1);
    if h < 48 { format!("about {h} hour{}", if h == 1 { "" } else { "s" }) } else { format!("about {} days", h / 24) }
}

/// A fixed sentence for the kind of failure. Nothing from the watcher's own words is copied (they can hold paths).
fn failure_sentence(result: &str) -> &'static str {
    let r = result.to_lowercase();
    if r.contains("offline") || r.contains("timed out") {
        "Your backup can't reach GitHub. Check your internet connection."
    } else if r.contains("signed out") {
        "Hestia is signed out of GitHub, so your backup can't be sent. Open Hestia to sign in again."
    } else if r.contains("repository is gone") {
        "GitHub says your backup repository no longer exists. Open Hestia to set it up again."
    } else if r.contains("is public") {
        "Your backup repository is public, so nothing is sent to it. Open Hestia to fix this."
    } else if r.contains("sso") {
        "Your organisation needs you to approve Hestia on GitHub."
    } else if r.contains("secret") {
        "Hestia held your backup back because a file may hold a secret. Open Hestia to see what to do."
    } else {
        "Your backup keeps failing. Open Hestia to see why."
    }
}

/// The policy. `dir` is `<share>/hestia`; `now` is epoch seconds; `notifications` is the setting.
pub fn compute(dir: &Path, now: i64, notifications: bool) -> State {
    let all = runs(dir);
    let real: Vec<_> = all.iter().filter(|r| r.1 != Kind::Routine).collect();
    let last_ok = real.iter().rev().find(|r| r.1 == Kind::Ok).map(|r| r.0);
    let Some(latest) = real.last() else { return State::quiet("unknown", None) };
    let notified = notified_episode(dir);
    let (state, since, runs_n, sentence) = if latest.1 == Kind::Failed {
        let n = real.iter().rev().take_while(|r| r.1 == Kind::Failed).count();
        let mut since = real[real.len() - n].0;
        // the history keeps 200 runs: a failure that outlasts it must not look like a new episode
        if n == real.len() {
            if let Some(e) = notified.filter(|e| *e <= since) {
                since = e;
            }
        }
        ("failing", since, n, failure_sentence(&latest.2).to_string())
    } else {
        match last_ok {
            Some(ok) if now - ok > STALE_AFTER => ("stale", ok + STALE_AFTER, 0, "No backup has finished lately. Is the computer off, or has Hestia's timer stopped? Open Hestia to check.".to_string()),
            Some(_) => return State::quiet("ok", last_ok),
            None => return State::quiet("unknown", None),
        }
    };
    let age = (now - since).max(0);
    let reason = if state == "failing" { format!("{} It has not worked for {}.", sentence, hours(age)) } else { format!("{} Nothing has finished for {}.", sentence, hours(now - last_ok.unwrap_or(since))) };
    let notify = notifications && age >= NOTIFY_AFTER && notified != Some(since);
    State { state, since: Some(since), reason: Some(reason), failing_runs: runs_n, last_ok, notify }
}

/// `<share>/hestia`: where the watcher keeps its files.
pub fn dir_of(share: Option<PathBuf>) -> Result<PathBuf> {
    Ok(share.or_else(crate::share_dir).context("no data folder (is HOME set?)")?.join("hestia"))
}

/// For the tray: the plain sentence when the backup needs a person (failing or stale), else None.
pub fn needs_you() -> Option<String> {
    let st = compute(&dir_of(None).ok()?, Utc::now().timestamp(), true);
    if st.needs_you() { st.reason } else { None }
}

/// Remember that this episode was told (a problem), or forget it (the backup works again). Unknown changes nothing.
/// True when it just closed an episode that had been told.
pub fn mark(dir: &Path, st: &State) -> Result<bool> {
    let p = dir.join("notified.json");
    match (st.state, st.since) {
        ("ok", _) => return Ok(fs::remove_file(&p).is_ok()),
        ("failing" | "stale", Some(since)) => {
            fs::create_dir_all(dir)?;
            let tmp = dir.join("notified.json.tmp");
            fs::write(&tmp, json!({"episode": since}).to_string() + "\n")?;
            fs::rename(&tmp, &p)?;
        }
        _ => {}
    }
    Ok(false)
}

/// `--mark-notified` exits with this when it just closed an episode that had been told and notifications are on:
/// the one moment to say "working again".
pub const EXIT_RECOVERED: i32 = 10;

pub fn cmd(share: Option<PathBuf>, mark_notified: bool) -> Result<()> {
    let dir = dir_of(share)?;
    let on = crate::setting("notifications").as_deref() != Some("off");
    let st = compute(&dir, Utc::now().timestamp(), on);
    let recovered = mark_notified && mark(&dir, &st)?;
    println!("{}", st.to_json());
    if recovered && on {
        std::process::exit(EXIT_RECOVERED);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: i64 = 3600;
    const NOW: i64 = 2_000_000_000;

    /// a fresh folder under the temp dir, one per call
    fn fresh() -> PathBuf {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let d = std::env::temp_dir().join(format!("ls-backup-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::SeqCst)));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn dir_with(lines: &[(i64, &str)]) -> PathBuf {
        let d = fresh();
        let hist: String = lines.iter().map(|(a, r)| format!("{}\n", json!({"at": a, "result": r, "commits": 0}))).collect();
        fs::write(d.join("watch-history.jsonl"), hist).unwrap();
        d
    }

    #[test]
    fn ok_never_notifies() {
        let d = dir_with(&[(NOW - H, "ok: 1 commit(s), pushed")]);
        let s = compute(&d, NOW, true);
        assert_eq!((s.state, s.notify, s.reason), ("ok", false, None));
        assert_eq!(s.last_ok, Some(NOW - H));
    }

    #[test]
    fn failing_waits_six_hours_then_tells_once() {
        let f = "FAILED: offline: push could not reach GitHub, will retry";
        let d = dir_with(&[(NOW - 10 * H, "ok: 0 commit(s), up to date"), (NOW - 5 * H, f), (NOW - 4 * H, f), (NOW, f)]);
        let s = compute(&d, NOW, true);
        assert_eq!((s.state, s.failing_runs, s.since, s.notify), ("failing", 3, Some(NOW - 5 * H), false));
        let s = compute(&d, NOW + 2 * H, true);
        assert!(s.notify, "7 hours in");
        assert!(s.reason.as_deref().unwrap().contains("internet"));
        mark(&d, &s).unwrap();
        assert!(!compute(&d, NOW + 3 * H, true).notify, "already told for this episode");
        assert!(!compute(&d, NOW + 2 * H, false).notify, "notifications are off");
        // ok again clears; a new failure is a new episode
        assert!(mark(&d, &State::quiet("ok", None)).unwrap(), "a told episode was closed");
        assert!(!mark(&d, &State::quiet("ok", None)).unwrap(), "nothing left to close");
        assert!(notified_episode(&d).is_none());
    }

    #[test]
    fn a_failure_that_outlasts_the_history_is_one_episode() {
        let f = "FAILED: x";
        let d = dir_with(&[(NOW, f)]);
        fs::write(d.join("notified.json"), r#"{"episode":1999900000}"#).unwrap();
        let s = compute(&d, NOW + 10 * H, true);
        assert_eq!((s.since, s.notify), (Some(1_999_900_000), false));
    }

    #[test]
    fn stale_when_nothing_finished_for_a_day_and_a_half() {
        let d = dir_with(&[(NOW - 40 * H, "ok: 0 commit(s), up to date"), (NOW - 39 * H, "busy: waiting for a quieter moment")]);
        let s = compute(&d, NOW, true);
        assert_eq!((s.state, s.since, s.notify), ("stale", Some(NOW - 4 * H), false));
        assert!(compute(&d, NOW + 2 * H, true).notify);
    }

    #[test]
    fn garbage_or_missing_files_are_unknown_and_quiet() {
        let d = fresh();
        assert_eq!(compute(&d, NOW, true).state, "unknown");
        fs::write(d.join("watch-history.jsonl"), "not json\n{\"at\":\"x\"}\n\u{0}\u{1}").unwrap();
        fs::write(d.join("watch.json"), "{{{{").unwrap();
        let s = compute(&d, NOW, true);
        assert_eq!((s.state, s.notify), ("unknown", false));
    }

    #[test]
    fn the_sentence_never_copies_the_watchers_words() {
        let d = dir_with(&[(NOW - 9 * H, "FAILED: git commit failed: /home/me/secret ghp_abcdefghijklmnopqrstuvwxyz0123456789")]);
        let r = compute(&d, NOW, true).reason.unwrap();
        assert!(!r.contains('/') && !r.contains("ghp_") && !r.contains("secret ghp"), "{r}");
    }
}
