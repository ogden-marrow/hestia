//! "Is everything backed up?" as one calm answer. Read from the home repository itself: when the
//! newest commit was made and whether it has reached the private remote. Never writes.

use std::path::Path;
use std::process::Command;

/// What git says, before it is turned into words.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Facts {
    pub is_home: bool,
    /// Unix seconds of the newest commit.
    pub last: Option<i64>,
    pub has_remote: bool,
    /// Commits not yet on the remote.
    pub ahead: usize,
}

/// The answer: safe, or what to do about it. `Unknown` means there is no home repository to ask about.
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

const STALE_DAYS: i64 = 3;

fn git(dir: &Path, args: &[&str]) -> String {
    Command::new("git").arg("-C").arg(dir).args(args).output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default()
}

pub fn read(dir: &Path) -> Facts {
    if !lifestream::home::is_home(dir) {
        return Facts::default();
    }
    let last = git(dir, &["log", "-1", "--format=%ct"]).trim().parse().ok();
    let has_remote = !git(dir, &["remote"]).trim().is_empty();
    let ahead = git(dir, &["rev-list", "--count", "@{u}..HEAD"]).trim().parse().unwrap_or(0);
    Facts { is_home: true, last, has_remote, ahead }
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

pub fn verdict(f: &Facts, now: i64) -> Verdict {
    if !f.is_home {
        return Verdict::Unknown;
    }
    let Some(last) = f.last else {
        return Verdict::NeedsYou { why: "Nothing has been backed up yet.".into(), fix: Fix::BackUpNow };
    };
    if !f.has_remote {
        return Verdict::NeedsYou { why: "Your backup is only on this computer. Sign in to keep a private copy on GitHub.".into(), fix: Fix::SignIn };
    }
    let age = now - last;
    if age > STALE_DAYS * 86_400 {
        return Verdict::NeedsYou { why: format!("The last backup was {}.", ago(age)), fix: Fix::BackUpNow };
    }
    if f.ahead > 0 && age > 86_400 {
        return Verdict::NeedsYou { why: "The newest changes have not reached GitHub yet.".into(), fix: Fix::BackUpNow };
    }
    Verdict::Safe { when: ago(age) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts() -> Facts {
        Facts { is_home: true, last: Some(1_000_000), has_remote: true, ahead: 0 }
    }

    #[test]
    fn says_nothing_without_a_home_repository() {
        assert_eq!(verdict(&Facts::default(), 5), Verdict::Unknown);
    }

    #[test]
    fn recent_and_pushed_is_all_safe() {
        assert_eq!(verdict(&facts(), 1_000_000 + 720), Verdict::Safe { when: "12 minutes ago".into() });
    }

    #[test]
    fn a_fresh_unpushed_commit_is_not_alarming() {
        let f = Facts { ahead: 2, ..facts() };
        assert!(matches!(verdict(&f, 1_000_000 + 600), Verdict::Safe { .. }));
    }

    #[test]
    fn each_problem_names_one_plain_fix() {
        let day = 86_400;
        assert!(matches!(verdict(&Facts { last: None, ..facts() }, 0), Verdict::NeedsYou { fix: Fix::BackUpNow, .. }));
        assert!(matches!(verdict(&Facts { has_remote: false, ..facts() }, 1_000_100), Verdict::NeedsYou { fix: Fix::SignIn, .. }));
        assert!(matches!(verdict(&Facts { ahead: 1, ..facts() }, 1_000_000 + day + 1), Verdict::NeedsYou { fix: Fix::BackUpNow, .. }));
        assert!(matches!(verdict(&facts(), 1_000_000 + 4 * day), Verdict::NeedsYou { fix: Fix::BackUpNow, .. }));
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
