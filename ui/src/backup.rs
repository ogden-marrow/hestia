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
    if st.needs_you() {
        let why = st.reason.clone().unwrap_or_else(|| "Your backup needs a look.".into());
        let fix = if why.contains("signed out") { Fix::SignIn } else { Fix::BackUpNow };
        return Verdict::NeedsYou { why, fix };
    }
    match (st.state, st.last_ok) {
        ("ok", Some(t)) => Verdict::Safe { when: ago(now - t) },
        _ => Verdict::Unknown,
    }
}

/// Where the watcher's run history lives, as a change marker: it changes whenever a run is recorded.
pub fn stamp() -> Option<String> {
    let p = lifestream::feed::history_path()?;
    let m = std::fs::metadata(p).ok()?;
    Some(format!("{}-{:?}", m.len(), m.modified().ok()))
}

pub fn current(repo_dir: &Path) -> Verdict {
    if !lifestream::home::is_home(repo_dir) {
        return Verdict::Unknown;
    }
    let Ok(dir) = lifestream::backup::dir_of(None) else { return Verdict::Unknown };
    let now = chrono::Utc::now().timestamp();
    from_engine(&lifestream::backup::compute(&dir, now, false), now)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine(state: &'static str, reason: Option<&str>, last_ok: Option<i64>) -> Engine {
        Engine { state, since: None, reason: reason.map(String::from), failing_runs: 0, last_ok, notify: false }
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
