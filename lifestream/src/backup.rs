//! Is the backup working, and does anyone need to be told? The one place that policy lives (`lifestream backup-state`).
//! Inputs, all untrusted files: `<share>/hestia/watch.json` and `watch-history.jsonl` (written by the watcher) and
//! `<share>/hestia/notified.json` (the episode a notification was already sent for). Output: one JSON line with a
//! state, a plain sentence and a yes/no for "send a desktop notification now". No file contents and no paths are ever
//! printed: the sentence is chosen from a fixed list by the kind of failure, never copied from the watcher's text.
//! Also read, offline and cheap: the home repository's git config and commit times (found through the settings file's
//! `home=`, like the rest of the engine) and `<share>/hestia/private-check.json`, the watcher's last answer to "is the push
//! target a private repository of yours". A backup only on the same disk is not a backup: no verified private remote is
//! `local_only`, and commits that have waited over a day to be pushed are `failing`.

use anyhow::{Context, Result};
use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// No successful run for this long, while the timer should have fired many times: the backup is stale.
pub const STALE_AFTER: i64 = 36 * 3600;
/// A problem must last this long before anyone is told (not on the first blip).
pub const NOTIFY_AFTER: i64 = 6 * 3600;
/// A new home repository gets this long after its first commit to be signed in, and a commit gets this long to be pushed.
pub const GRACE: i64 = 24 * 3600;
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
    pub rehearsal: Rehearsal,
}

/// The last restore rehearsal (`<share>/hestia/rehearsal.json`, written by `rehearse.sh`). Only the time and the verdict are
/// read; the plain sentences in `problems` are never copied.
#[derive(Debug, PartialEq, Clone)]
pub struct Rehearsal {
    pub at: Option<i64>,
    /// `passed`, `failed` or `never` (no readable result yet)
    pub result: &'static str,
    pub age_days: Option<i64>,
}

/// A rehearsal older than this is worth a second line when the backup already needs you.
pub const REHEARSAL_OLD_DAYS: i64 = 14;

impl Rehearsal {
    fn never() -> Self {
        Rehearsal { at: None, result: "never", age_days: None }
    }
    /// A failed or overdue rehearsal: the one thing to add as a second line (only ever to a reason that already exists).
    fn second_line(&self) -> Option<&'static str> {
        match (self.result, self.age_days) {
            ("failed", _) => Some("Your last restore check failed: your backup may not restore. Open Hestia to see what to do."),
            (_, Some(d)) if d > REHEARSAL_OLD_DAYS => Some("Your backup has not been checked by a restore for more than two weeks."),
            _ => None,
        }
    }
}

fn rehearsal(dir: &Path, now: i64) -> Rehearsal {
    let Some(v) = read_tail(&dir.join("rehearsal.json")).and_then(|t| serde_json::from_str::<Value>(t.trim()).ok()) else { return Rehearsal::never() };
    let (Some(at), Some(result)) = (v["at"].as_i64(), v["result"].as_str()) else { return Rehearsal::never() };
    let result = match result {
        "passed" => "passed",
        "failed" => "failed",
        _ => return Rehearsal::never(),
    };
    Rehearsal { at: Some(at), result, age_days: Some(((now - at).max(0)) / 86400) }
}

impl State {
    fn quiet(state: &'static str, last_ok: Option<i64>) -> Self {
        State { state, since: None, reason: None, failing_runs: 0, last_ok, notify: false, rehearsal: Rehearsal::never() }
    }
    /// Does this state need a person (and so can it be an "episode")?
    pub fn needs_you(&self) -> bool {
        matches!(self.state, "failing" | "stale" | "local_only")
    }
    pub fn to_json(&self) -> String {
        let t = |s: Option<i64>| s.and_then(|s| DateTime::from_timestamp(s, 0)).map(|d| d.to_rfc3339_opts(SecondsFormat::Secs, true));
        json!({"state": self.state, "since": t(self.since), "reason": self.reason, "failing_runs": self.failing_runs, "last_ok": t(self.last_ok), "notify": self.notify,
              "rehearsal": {"at": t(self.rehearsal.at), "result": self.rehearsal.result, "age_days": self.rehearsal.age_days}}).to_string()
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
/// The restore rehearsal rides along: it never changes the state or `notify`; a failed or overdue one only adds a second line
/// to a reason that already exists (a healthy backup has no reason).
pub fn compute(dir: &Path, now: i64, notifications: bool) -> State {
    compute_in(dir, repo_dir().as_deref(), now, notifications)
}

/// `compute` for a given home repository (`None`: no opinion about it, only the watcher's files count).
pub fn compute_in(dir: &Path, repo: Option<&Path>, now: i64, notifications: bool) -> State {
    let mut st = policy(dir, now, notifications);
    if !st.needs_you() {
        let local = repo.map_or(Local::NoOpinion, |r| local_view(r, dir, now));
        st = overlay(st, local, dir, notifications);
    }
    st.rehearsal = rehearsal(dir, now);
    if let (Some(r), Some(line)) = (st.reason.as_mut(), st.rehearsal.second_line()) {
        r.push('\n');
        r.push_str(line);
    }
    st
}

/// What the home repository itself says, read offline.
#[derive(Debug, PartialEq)]
enum Local {
    /// no home repository, or nothing committed yet: nothing to say
    NoOpinion,
    Fine,
    /// no verified private push target, past the sign-in grace; `since` = first commit + grace
    OnlyHere { since: i64 },
    /// a verified target, but the oldest unpushed commit is over a day old; `since` = its time + grace
    Unpushed { since: i64 },
    /// git cannot be read (broken repository, odd config)
    Garbage,
}

/// The home repository: `home=` in the settings file, else `~/Home` (what `watch.sh` and `home-repo.sh` decide).
fn repo_dir() -> Option<PathBuf> {
    crate::settings_home().or_else(|| Some(PathBuf::from(std::env::var_os("HOME")?).join("Home")))
}

fn git(repo: &Path, args: &[&str]) -> Option<String> {
    let o = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).into_owned())
}

/// The commit times (epoch seconds) git prints one per line; None if any line is not a number.
fn times(out: &str) -> Option<Vec<i64>> {
    out.lines().filter(|l| !l.trim().is_empty()).map(|l| l.trim().parse::<i64>().ok()).collect()
}

/// `owner/name` of a GitHub URL, the same way `watch.sh` takes it. Anything else (a token in the URL, another host) is None.
fn slug_of(url: &str) -> Option<String> {
    let u = url.trim();
    let rest = u.strip_prefix("https://github.com/").or_else(|| u.strip_prefix("git@github.com:"))?;
    let rest = rest.strip_suffix(".git").unwrap_or(rest);
    let ok = rest.split('/').count() == 2 && rest.split('/').all(|p| !p.is_empty()) && rest.chars().all(|c| c.is_ascii_alphanumeric() || "-_./".contains(c));
    ok.then(|| rest.to_string())
}

/// Did the watcher last find `slug` to be a private repository of the person's? (`private-check.json`)
fn verified_private(dir: &Path, slug: &str) -> bool {
    let Some(v) = read_tail(&dir.join("private-check.json")).and_then(|t| serde_json::from_str::<Value>(t.trim()).ok()) else { return false };
    v["private"].as_bool() == Some(true) && v["slug"].as_str().is_some_and(|s| s.eq_ignore_ascii_case(slug))
}

fn local_view(repo: &Path, dir: &Path, now: i64) -> Local {
    // the same gate as the watcher: nothing to judge until Hestia has made the home repository
    if !repo.join("tree.toml").is_file() || !repo.join(".git").exists() {
        return Local::NoOpinion;
    }
    if git(repo, &["rev-parse", "--git-dir"]).is_none() {
        return Local::Garbage;
    }
    if git(repo, &["rev-parse", "-q", "--verify", "HEAD"]).is_none() {
        return Local::NoOpinion; // nothing committed yet
    }
    let Some(first) = git(repo, &["log", "--max-parents=0", "--format=%ct", "HEAD"]).and_then(|o| times(&o)).and_then(|t| t.into_iter().min()) else { return Local::Garbage };
    let Some(remotes) = git(repo, &["remote"]) else { return Local::Garbage };
    let verified = if remotes.lines().any(|l| l.trim() == "origin") {
        let Some(url) = git(repo, &["remote", "get-url", "--push", "origin"]) else { return Local::Garbage };
        slug_of(&url).is_some_and(|s| verified_private(dir, &s))
    } else {
        false
    };
    if !verified {
        return if now - first > GRACE { Local::OnlyHere { since: first + GRACE } } else { Local::Fine };
    }
    let range = if git(repo, &["rev-parse", "-q", "--verify", "origin/main"]).is_some() { "origin/main..HEAD" } else { "HEAD" };
    let Some(unpushed) = git(repo, &["log", "--format=%ct", range]).and_then(|o| times(&o)) else { return Local::Garbage };
    match unpushed.into_iter().min() {
        Some(oldest) if now - oldest > GRACE => Local::Unpushed { since: oldest + GRACE },
        _ => Local::Fine,
    }
}

/// Fold what the repository says into the watcher's verdict (which is not failing or stale: those win, their sentence is the
/// more specific). The sentences are fixed; nothing from git is copied.
fn overlay(st: State, local: Local, dir: &Path, notifications: bool) -> State {
    let (state, since, sentence) = match local {
        Local::NoOpinion | Local::Fine => return st,
        Local::Garbage => return State::quiet("unknown", st.last_ok),
        Local::OnlyHere { since } => ("local_only", since, "Your backup is only on this computer. Sign in to keep a private copy online."),
        Local::Unpushed { since } => ("failing", since, "Your newest changes have not reached your private copy for over a day."),
    };
    let notify = notifications && notified_episode(dir) != Some(since);
    State { state, since: Some(since), reason: Some(sentence.to_string()), failing_runs: 0, last_ok: st.last_ok, notify, rehearsal: Rehearsal::never() }
}

fn policy(dir: &Path, now: i64, notifications: bool) -> State {
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
    State { state, since: Some(since), reason: Some(reason), failing_runs: runs_n, last_ok, notify, rehearsal: Rehearsal::never() }
}

/// `<share>/hestia`: where the watcher keeps its files.
pub fn dir_of(share: Option<PathBuf>) -> Result<PathBuf> {
    Ok(share.or_else(crate::share_dir).context("no data folder (is HOME set?)")?.join("hestia"))
}

/// For the tray: the plain sentence when the backup needs a person (failing, stale, or local_only after its grace), else None.
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
        ("failing" | "stale" | "local_only", Some(since)) => {
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

    /// The watcher-only tests: no home repository, so nothing here can read the real one.
    fn compute(d: &Path, now: i64, notifications: bool) -> State {
        compute_in(d, None, now, notifications)
    }

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

    fn with_rehearsal(d: &Path, at: i64, result: &str) {
        fs::write(d.join("rehearsal.json"), json!({"at": at, "result": result, "checked": 9, "problems": ["/home/me/.ssh/id_rsa ghp_abcdefghijklmnopqrstuvwxyz0123456789"], "repo_commit": "abc1234"}).to_string()).unwrap();
    }

    #[test]
    fn no_rehearsal_yet_is_never_and_adds_nothing() {
        let d = dir_with(&[(NOW - 9 * H, "FAILED: offline: x")]);
        let s = compute(&d, NOW, true);
        assert_eq!(s.rehearsal, Rehearsal::never());
        assert!(!s.reason.clone().unwrap().contains('\n'));
        assert!(s.to_json().contains(r#""rehearsal":{"age_days":null,"at":null,"result":"never"}"#), "{}", s.to_json());
    }

    #[test]
    fn a_passed_rehearsal_is_reported_with_its_age() {
        let d = dir_with(&[(NOW - H, "ok: 1 commit(s), pushed")]);
        with_rehearsal(&d, NOW - 3 * 86400 - 5, "passed");
        let s = compute(&d, NOW, true);
        assert_eq!((s.state, s.rehearsal.result, s.rehearsal.age_days), ("ok", "passed", Some(3)));
        assert!(s.reason.is_none());
        assert!(s.to_json().contains(r#""age_days":3"#));
    }

    #[test]
    fn a_failed_or_overdue_rehearsal_is_only_a_second_line_and_never_makes_ok_failing() {
        let ok = dir_with(&[(NOW - H, "ok: 1 commit(s), pushed")]);
        with_rehearsal(&ok, NOW - H, "failed");
        let s = compute(&ok, NOW, true);
        assert_eq!((s.state, s.notify, s.reason, s.rehearsal.result), ("ok", false, None, "failed"));
        let old = dir_with(&[(NOW - H, "ok: 1 commit(s), pushed")]);
        with_rehearsal(&old, NOW - 15 * 86400, "passed");
        let s = compute(&old, NOW, true);
        assert_eq!((s.state, s.reason), ("ok", None));
        assert_eq!(s.rehearsal.age_days, Some(15));
        // a backup that already needs you gets the second line, after the first
        let bad = dir_with(&[(NOW - 9 * H, "FAILED: offline: x")]);
        with_rehearsal(&bad, NOW - H, "failed");
        let s = compute(&bad, NOW, true);
        let r = s.reason.clone().unwrap();
        let lines: Vec<_> = r.lines().collect();
        assert_eq!(lines.len(), 2, "{r}");
        assert!(lines[0].contains("internet") && lines[1].contains("restore check failed"), "{r}");
        assert!(s.notify, "the notification policy is unchanged");
        // a fresh pass adds nothing; an old pass says it has been a while
        with_rehearsal(&bad, NOW - H, "passed");
        assert_eq!(compute(&bad, NOW, true).reason.unwrap().lines().count(), 1);
        with_rehearsal(&bad, NOW - 20 * 86400, "passed");
        assert!(compute(&bad, NOW, true).reason.unwrap().contains("two weeks"));
    }

    #[test]
    fn a_garbage_rehearsal_file_is_never_and_its_words_are_never_copied() {
        let d = dir_with(&[(NOW - 9 * H, "FAILED: offline: x")]);
        for junk in ["{{{{", r#"{"at":"x","result":"passed"}"#, r#"{"at":1,"result":"bogus"}"#] {
            fs::write(d.join("rehearsal.json"), junk).unwrap();
            assert_eq!(compute(&d, NOW, true).rehearsal, Rehearsal::never(), "{junk}");
        }
        with_rehearsal(&d, NOW - H, "failed");
        let j = compute(&d, NOW, true).to_json();
        assert!(!j.contains("ghp_") && !j.contains(".ssh") && !j.contains("abc1234"), "{j}");
    }

    #[test]
    fn the_sentence_never_copies_the_watchers_words() {
        let d = dir_with(&[(NOW - 9 * H, "FAILED: git commit failed: /home/me/secret ghp_abcdefghijklmnopqrstuvwxyz0123456789")]);
        let r = compute(&d, NOW, true).reason.unwrap();
        assert!(!r.contains('/') && !r.contains("ghp_") && !r.contains("secret ghp"), "{r}");
    }

    // ---- #256: a backup only on this disk is not a backup. Every repository below lives in a temp folder; git is
    // told to ignore the real user and system config, and nothing here reads HOME.

    const LOCAL_ONLY: &str = "Your backup is only on this computer. Sign in to keep a private copy online.";
    const UNPUSHED: &str = "Your newest changes have not reached your private copy for over a day.";

    fn git_in(r: &Path, at: i64, args: &[&str]) {
        assert!(r.starts_with(std::env::temp_dir()), "the test repository must be under the temp dir: {r:?}");
        let date = format!("@{at} +0000");
        let o = Command::new("git")
            .arg("-C")
            .arg(r)
            .args(["-c", "user.name=t", "-c", "user.email=t@t.t", "-c", "commit.gpgsign=false", "-c", "init.defaultBranch=main"])
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_AUTHOR_DATE", &date)
            .env("GIT_COMMITTER_DATE", &date)
            .output()
            .unwrap();
        assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
    }

    fn commit(r: &Path, at: i64) {
        let n = fs::read_dir(r).unwrap().count();
        fs::write(r.join(format!("f{n}.txt")), format!("{at}")).unwrap();
        git_in(r, at, &["add", "-A"]);
        git_in(r, at, &["commit", "-q", "-m", "machine: change"]);
    }

    /// (hestia data dir, home repository whose first commit is `first_h` hours before NOW, with a tree.toml)
    fn home_repo(first_h: i64) -> (PathBuf, PathBuf) {
        let d = fresh();
        let r = d.join("repo");
        fs::create_dir_all(&r).unwrap();
        git_in(&r, NOW, &["init", "-q"]);
        fs::write(r.join("tree.toml"), "").unwrap();
        git_in(&r, NOW, &["add", "-A"]);
        git_in(&r, NOW - first_h * H, &["commit", "-q", "-m", "init"]);
        // the watcher has been running fine (tests that need otherwise overwrite this)
        fs::write(d.join("watch-history.jsonl"), format!("{}\n", json!({"at": NOW - H, "result": OK_RUN, "commits": 1}))).unwrap();
        (d, r)
    }

    fn with_remote(r: &Path, url: &str) {
        git_in(r, NOW, &["remote", "add", "origin", url]);
    }

    fn verified(d: &Path, slug: &str, private: bool) {
        fs::write(d.join("private-check.json"), json!({"at": NOW, "slug": slug, "private": private}).to_string()).unwrap();
    }

    fn pushed(r: &Path) {
        git_in(r, NOW, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    }

    fn judge(d: &Path, r: &Path, now: i64) -> State {
        compute_in(d, Some(r), now, true)
    }

    const OK_RUN: &str = "ok: 1 commit(s), pushed";

    #[test]
    fn a_fresh_repository_with_no_remote_is_fine_inside_the_grace_and_local_only_after() {
        let (d, r) = home_repo(2);
        fs::write(d.join("watch-history.jsonl"), format!("{}\n", json!({"at": NOW - H, "result": "ok: 0 commit(s) (no remote yet)", "commits": 0}))).unwrap();
        let s = judge(&d, &r, NOW);
        assert_eq!((s.state, s.notify, s.reason), ("ok", false, None), "2 hours in: signing in is not expected yet");
        let (d, r) = home_repo(23);
        let s = judge(&d, &r, NOW);
        assert_eq!((s.state, s.notify), ("ok", false), "just inside the grace");
        let (d, r) = home_repo(25);
        let s = judge(&d, &r, NOW);
        assert_eq!((s.state, s.reason.as_deref(), s.notify, s.since), ("local_only", Some(LOCAL_ONLY), true, Some(NOW - 25 * H + 24 * H)));
        assert!(s.needs_you(), "the tray treats it as needs-you");
        assert_eq!(s.failing_runs, 0);
        assert!(s.to_json().contains(r#""state":"local_only""#));
    }

    #[test]
    fn local_only_notifies_once_per_episode_and_respects_notifications_off() {
        let (d, r) = home_repo(30);
        assert!(!compute_in(&d, Some(&r), NOW, false).notify, "notifications=off");
        let s = judge(&d, &r, NOW);
        assert!(s.notify);
        mark(&d, &s).unwrap();
        assert_eq!(notified_episode(&d), s.since);
        let again = judge(&d, &r, NOW + 5 * H);
        assert_eq!((again.state, again.notify), ("local_only", false), "told once for this episode");
        // signing in (a verified remote that is pushed) closes it
        with_remote(&r, "https://github.com/ada/home.git");
        verified(&d, "ada/home", true);
        pushed(&r);
        let ok = judge(&d, &r, NOW + 6 * H);
        assert_eq!(ok.state, "ok");
        assert!(mark(&d, &ok).unwrap(), "the told episode was closed");
        assert!(notified_episode(&d).is_none());
    }

    #[test]
    fn a_remote_nobody_verified_is_still_local_only() {
        let url = "https://github.com/ada/home.git";
        // never checked
        let (d, r) = home_repo(30);
        with_remote(&r, url);
        pushed(&r);
        assert_eq!(judge(&d, &r, NOW).state, "local_only");
        // checked: not private
        verified(&d, "ada/home", false);
        assert_eq!(judge(&d, &r, NOW).state, "local_only");
        // verified, but for another repository (the remote was swapped)
        verified(&d, "ada/other", true);
        assert_eq!(judge(&d, &r, NOW).state, "local_only");
        // not GitHub at all, and GitHub URLs the watcher would not read as owner/name
        for u in ["https://example.com/ada/home.git", "ssh://github.com/ada/home.git", "https://github.com/ada/home/extra.git", "/tmp/somewhere/bare.git"] {
            git_in(&r, NOW, &["remote", "set-url", "origin", u]);
            verified(&d, "ada/home", true);
            let s = judge(&d, &r, NOW);
            assert_eq!(s.state, "local_only", "{u}");
            assert!(!s.to_json().contains("github") && !s.to_json().contains("example.com"), "the URL is never printed");
        }
        // garbage in the record is not a verification
        fs::write(d.join("private-check.json"), "{{{{").unwrap();
        git_in(&r, NOW, &["remote", "set-url", "origin", url]);
        assert_eq!(judge(&d, &r, NOW).state, "local_only");
        // the remote is removed after being verified
        verified(&d, "ada/home", true);
        assert_eq!(judge(&d, &r, NOW).state, "ok");
        git_in(&r, NOW, &["remote", "remove", "origin"]);
        assert_eq!(judge(&d, &r, NOW).state, "local_only");
    }

    #[test]
    fn a_verified_remote_with_everything_pushed_is_ok_and_ssh_urls_count() {
        for url in ["https://github.com/ada/home.git", "git@github.com:Ada/Home.git", "https://github.com/ada/home"] {
            let (d, r) = home_repo(100);
            with_remote(&r, url);
            verified(&d, "ada/home", true);
            pushed(&r);
            let s = judge(&d, &r, NOW);
            assert_eq!((s.state, s.notify, s.reason), ("ok", false, None), "{url}");
        }
    }

    #[test]
    fn commits_unpushed_for_over_a_day_are_failing_and_younger_ones_are_fine() {
        let (d, r) = home_repo(100);
        with_remote(&r, "https://github.com/ada/home.git");
        verified(&d, "ada/home", true);
        pushed(&r);
        commit(&r, NOW - 23 * H);
        assert_eq!(judge(&d, &r, NOW).state, "ok", "23 hours is inside the day");
        let (d, r) = home_repo(100);
        with_remote(&r, "https://github.com/ada/home.git");
        verified(&d, "ada/home", true);
        pushed(&r);
        commit(&r, NOW - 25 * H);
        commit(&r, NOW - H);
        let s = judge(&d, &r, NOW);
        assert_eq!((s.state, s.reason.as_deref(), s.notify, s.since), ("failing", Some(UNPUSHED), true, Some(NOW - 25 * H + 24 * H)));
        assert!(s.needs_you());
        // told once
        mark(&d, &s).unwrap();
        assert!(!judge(&d, &r, NOW + H).notify);
        // pushed: clears
        pushed(&r);
        assert_eq!(judge(&d, &r, NOW + 2 * H).state, "ok");
        // a remote that was never fetched or pushed (no origin/main yet): everything is unpushed
        let (d, r) = home_repo(30);
        with_remote(&r, "https://github.com/ada/home.git");
        verified(&d, "ada/home", true);
        assert_eq!(judge(&d, &r, NOW).state, "failing");
    }

    #[test]
    fn failing_and_stale_from_the_watcher_win_over_the_repository() {
        let f = "FAILED: offline: push could not reach GitHub, will retry";
        // local_only conditions too, but the watcher's own sentence is the more specific one
        let (d, r) = home_repo(40);
        fs::write(d.join("watch-history.jsonl"), format!("{}\n", json!({"at": NOW - 8 * H, "result": f, "commits": 0}))).unwrap();
        let s = judge(&d, &r, NOW);
        assert_eq!(s.state, "failing");
        assert!(s.reason.unwrap().contains("internet"));
        // stale likewise
        let (d, r) = home_repo(100);
        fs::write(d.join("watch-history.jsonl"), format!("{}\n", json!({"at": NOW - 40 * H, "result": OK_RUN, "commits": 0}))).unwrap();
        assert_eq!(judge(&d, &r, NOW).state, "stale");
        // a healthy watcher does not hide a local-only backup
        let (d, r) = home_repo(40);
        fs::write(d.join("watch-history.jsonl"), format!("{}\n", json!({"at": NOW - H, "result": "ok: 0 commit(s) (no remote yet)", "commits": 0}))).unwrap();
        let s = judge(&d, &r, NOW);
        assert_eq!((s.state, s.last_ok), ("local_only", Some(NOW - H)));
    }

    #[test]
    fn the_rehearsal_second_line_rides_along_on_local_only() {
        let (d, r) = home_repo(40);
        with_rehearsal(&d, NOW - H, "failed");
        let s = judge(&d, &r, NOW);
        let reason = s.reason.unwrap();
        assert!(reason.starts_with(LOCAL_ONLY) && reason.lines().count() == 2, "{reason}");
    }

    #[test]
    fn garbage_git_state_is_unknown_never_a_panic_and_never_a_notification() {
        // a .git that is not a repository
        let d = fresh();
        let r = d.join("repo");
        fs::create_dir_all(r.join(".git")).unwrap();
        fs::write(r.join("tree.toml"), "").unwrap();
        fs::write(r.join(".git/HEAD"), "\u{0}\u{1}garbage").unwrap();
        fs::write(d.join("watch-history.jsonl"), format!("{}\n", json!({"at": NOW - H, "result": OK_RUN, "commits": 1}))).unwrap();
        let s = judge(&d, &r, NOW);
        assert_eq!((s.state, s.notify, s.reason), ("unknown", false, None));
        // .git is a file with junk
        let r2 = d.join("repo2");
        fs::create_dir_all(&r2).unwrap();
        fs::write(r2.join("tree.toml"), "").unwrap();
        fs::write(r2.join(".git"), "gitdir: /does/not/exist").unwrap();
        let s = judge(&d, &r2, NOW);
        assert_eq!((s.state, s.notify), ("unknown", false));
        // a good repository with a broken config
        let (d, r) = home_repo(40);
        fs::write(r.join(".git/config"), "[remote \"origin\"\n\u{0}garbage = =").unwrap();
        let s = judge(&d, &r, NOW);
        assert_eq!((s.state, s.notify), ("unknown", false));
        // a missing folder, a folder without tree.toml and a repository with no commit say nothing
        let d = fresh();
        assert_eq!(compute_in(&d, Some(&d.join("nope")), NOW, true).state, "unknown");
        let empty = d.join("empty");
        fs::create_dir_all(&empty).unwrap();
        git_in(&empty, NOW, &["init", "-q"]);
        fs::write(empty.join("tree.toml"), "").unwrap();
        let s = compute_in(&d, Some(&empty), NOW, true);
        assert_eq!((s.state, s.notify), ("unknown", false), "nothing committed yet");
    }

    #[test]
    fn the_repository_is_found_through_the_settings_file_like_everywhere_else() {
        let home = fresh();
        let text = "home=~/Documents/MyHome\nnotifications=off\n";
        assert_eq!(crate::parse_settings_home(text, &home), Some(home.join("Documents/MyHome")));
    }
}
