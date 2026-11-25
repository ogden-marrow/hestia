//! "Is everything backed up?" as one calm answer. The engine owns the verdict (`lifestream::backup`, the same
//! one the tray and `lifestream backup-state` use); this only turns it into what the screens show. Never writes.

use lifestream::backup::State as Engine;
use std::path::Path;

/// The answer: safe, or what to do about it. `Unknown` means there is no home repository, or no run yet.
#[derive(Clone, Debug, PartialEq)]
pub enum Verdict {
    Unknown,
    Safe { when: String },
    NeedsYou { why: String, fix: Fix },
}

/// The one plain action offered, each to the screen that already owns it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fix {
    BackUpNow,
    SignIn,
}

/// "just now", "12 minutes ago", "3 hours ago", "yesterday", "5 days ago".
pub fn ago(secs: i64) -> String {
    let n = |v: i64, unit: &str| format!("{v} {unit}{} ago", if v == 1 { "" } else { "s" });
    match secs.max(0) {
        s if s < 60 => "just now".into(),
        s if s < 3600 => n(s / 60, "minute"),
        s if s < 86_400 => n(s / 3600, "hour"),
        s if s < 2 * 86_400 => "yesterday".into(),
        s => n(s / 86_400, "day"),
    }
}

/// The engine's state as the UI's verdict. A signed-out backup is fixed by signing in; anything else by backing up.
pub fn from_engine(st: &Engine, now: i64) -> Verdict {
    let why = || st.reason.clone().unwrap_or_else(|| "Your backup needs a look.".into());
    match (st.state, st.last_ok) {
        // a backup only on this computer is fixed by signing in, so a private copy can be kept online
        ("local_only", _) => Verdict::NeedsYou { why: why(), fix: Fix::SignIn },
        ("failing" | "stale", _) => {
            let why = why();
            let fix = if why.contains("signed out") { Fix::SignIn } else { Fix::BackUpNow };
            Verdict::NeedsYou { why, fix }
        }
        ("ok", Some(t)) => Verdict::Safe { when: ago(now - t) },
        // "unknown", and any state this build does not know yet, says nothing rather than guess
        _ => Verdict::Unknown,
    }
}

/// The restore rehearsal as one calm line, and whether it is a problem: "Last restore check: passed 3 days ago",
/// or "... failed", or "... overdue" past two weeks. Nothing at all when it has never run.
pub fn restore_line(st: &Engine) -> Option<(String, bool)> {
    let r = &st.rehearsal;
    let days = r.age_days?;
    let when = if days <= 0 { "today".to_string() } else if days == 1 { "yesterday".to_string() } else { format!("{days} days ago") };
    match r.result {
        "passed" if days > lifestream::backup::REHEARSAL_OLD_DAYS => Some((format!("Last restore check: passed {when}. It is overdue."), true)),
        "passed" => Some((format!("Last restore check: passed {when}."), false)),
        "failed" => Some((format!("Last restore check: failed {when}."), true)),
        _ => None,
    }
}

/// One thing the backup left out, as the engine says it (`lifestream::leftout`).
#[derive(Clone, Debug, PartialEq)]
pub struct Left {
    pub path: Option<String>,
    pub why: String,
    pub detail: String,
    pub can_include: bool,
}

/// What is left out, newest first, read-only.
pub fn left_out() -> Vec<Left> {
    let Ok(dir) = lifestream::backup::dir_of(None) else { return vec![] };
    lifestream::leftout::items(&dir).into_iter().take(500)
        .map(|i| Left { path: i.path, why: i.why.to_string(), detail: i.detail, can_include: i.can_include }).collect()
}

/// The groups, in the order a person reads them; a reason this build does not know goes last under its own words.
pub const GROUPS: [(&str, &[&str]); 5] = [
    ("Too large", &["too_large"]),
    ("Could not be read", &["unreadable"]),
    ("Private by your rules", &["private_by_rule"]),
    ("Withheld for a possible secret", &["withheld_secret_lines", "withheld_secret_file"]),
    ("Links", &["link_outside_home", "link_to_private"]),
];

/// What a person can do about it, only where the engine says it could be included.
pub fn fix_for(l: &Left) -> Option<&'static str> {
    if !l.can_include {
        return None;
    }
    match l.why.as_str() {
        "too_large" => Some("Make it smaller, or keep it somewhere that is not backed up; it is included again next time if it fits."),
        "unreadable" => Some("Make it readable for your user; it is included on the next backup."),
        _ => Some("Fix what the line above says; it is included on the next backup."),
    }
}

/// Where the watcher's run history lives, as a change marker: it changes whenever a run is recorded.
pub fn stamp() -> Option<String> {
    let p = lifestream::feed::history_path()?;
    let m = std::fs::metadata(p).ok()?;
    Some(format!("{}-{:?}", m.len(), m.modified().ok()))
}

/// The verdict and the restore-check line, from one read of the engine.
pub fn current(repo_dir: &Path) -> (Verdict, Option<(String, bool)>) {
    if !lifestream::home::is_home(repo_dir) {
        return (Verdict::Unknown, None);
    }
    let Ok(dir) = lifestream::backup::dir_of(None) else { return (Verdict::Unknown, None) };
    let now = chrono::Utc::now().timestamp();
    let st = lifestream::backup::compute(&dir, now, false);
    (from_engine(&st, now), restore_line(&st))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine(state: &'static str, reason: Option<&str>, last_ok: Option<i64>) -> Engine {
        Engine { state, since: None, reason: reason.map(String::from), failing_runs: 0, last_ok, notify: false, rehearsal: lifestream::backup::Rehearsal { at: None, result: "never", age_days: None } }
    }

    #[test]
    fn ok_is_safe_with_how_long_ago() {
        assert_eq!(from_engine(&engine("ok", None, Some(1_000_000)), 1_000_720), Verdict::Safe { when: "12 minutes ago".into() });
    }

    #[test]
    fn failing_and_stale_need_you_with_the_engines_own_sentence() {
        for state in ["failing", "stale"] {
            match from_engine(&engine(state, Some("Your backup can't reach GitHub."), Some(1)), 5) {
                Verdict::NeedsYou { why, fix } => assert_eq!((why.as_str(), fix), ("Your backup can't reach GitHub.", Fix::BackUpNow)),
                other => panic!("{other:?}"),
            }
        }
    }

    #[test]
    fn signed_out_is_fixed_by_signing_in() {
        let st = engine("failing", Some("Hestia is signed out of GitHub, so your backup can't be sent."), None);
        assert!(matches!(from_engine(&st, 5), Verdict::NeedsYou { fix: Fix::SignIn, .. }));
    }

    #[test]
    fn the_restore_check_is_calm_and_silent_when_never_run() {
        use lifestream::backup::Rehearsal;
        let with = |result: &'static str, age: Option<i64>| { let mut s = engine("ok", None, Some(1)); s.rehearsal = Rehearsal { at: None, result, age_days: age }; s };
        assert_eq!(restore_line(&with("passed", Some(3))), Some(("Last restore check: passed 3 days ago.".into(), false)));
        assert_eq!(restore_line(&with("passed", Some(20))).map(|x| x.1), Some(true));
        assert_eq!(restore_line(&with("failed", Some(1))), Some(("Last restore check: failed yesterday.".into(), true)));
        assert_eq!(restore_line(&with("never", None)), None);
    }

    #[test]
    fn a_backup_only_on_this_computer_is_fixed_by_signing_in() {
        let st = engine("local_only", Some("Your backup is only on this computer. Sign in to keep a private copy online."), Some(1));
        assert!(matches!(from_engine(&st, 5), Verdict::NeedsYou { fix: Fix::SignIn, .. }));
        assert_eq!(from_engine(&engine("someday_new", Some("x"), Some(1)), 5), Verdict::Unknown);
    }

    #[test]
    fn only_includable_things_get_fix_guidance() {
        let l = |why: &str, can: bool| Left { path: None, why: why.into(), detail: String::new(), can_include: can };
        assert!(fix_for(&l("too_large", true)).is_some());
        assert!(fix_for(&l("private_by_rule", false)).is_none());
        assert!(fix_for(&l("withheld_secret_file", false)).is_none());
        let grouped: Vec<&str> = GROUPS.iter().flat_map(|g| g.1.iter().copied()).collect();
        for w in ["too_large", "unreadable", "private_by_rule", "withheld_secret_lines", "withheld_secret_file", "link_outside_home", "link_to_private"] {
            assert!(grouped.contains(&w), "{w}");
        }
    }

    #[test]
    fn nothing_known_says_nothing() {
        assert_eq!(from_engine(&engine("unknown", None, None), 5), Verdict::Unknown);
    }

    #[test]
    fn ago_reads_like_a_person() {
        assert_eq!(ago(5), "just now");
        assert_eq!(ago(60), "1 minute ago");
        assert_eq!(ago(7200), "2 hours ago");
        assert_eq!(ago(90_000), "yesterday");
        assert_eq!(ago(5 * 86_400), "5 days ago");
    }
}
