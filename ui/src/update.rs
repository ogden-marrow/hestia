//! Updating Hestia from Settings > About. The engine does the work (`lifestream update check|apply|rollback`:
//! it refuses during a backup, keeps the previous build and switches only after a smoke test); this shows what
//! it says, runs it as a job with its progress, and offers Restart when the new build is in place.

use super::{style, State};
use crate::design::Design;
use rune::tok::type_;
use rune::{col, row, text, El};
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

/// What `lifestream update check` said.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Check {
    pub installed: String,
    pub available: Option<String>,
    pub newer: bool,
    pub changes: Vec<String>,
    pub error: Option<String>,
}

pub fn parse_check(line: &str) -> Option<Check> {
    let v: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).map(String::from);
    Some(Check {
        installed: s("installed").unwrap_or_default(),
        available: s("available"),
        newer: v.get("newer").and_then(|x| x.as_bool()).unwrap_or(false),
        changes: v.get("changes").and_then(|x| x.as_array()).map(|a| a.iter().filter_map(|c| c.as_str().map(String::from)).collect()).unwrap_or_default(),
        error: s("error").filter(|e| !e.is_empty()),
    })
}

/// One line of `lifestream update apply`: a step with its percentage, or the end.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    Progress { step: String, pct: u8 },
    Updated { version: String },
    Failed { why: String },
}

pub fn parse_step(line: &str) -> Option<Step> {
    let v: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).map(String::from);
    match s("event").as_deref() {
        Some("updated") | Some("rolled_back") => Some(Step::Updated { version: s("to").unwrap_or_default() }),
        Some("failed") => Some(Step::Failed { why: s("message").or_else(|| s("why")).unwrap_or_else(|| "The update did not finish. Nothing was changed.".into()) }),
        Some("step") | None => Some(Step::Progress { step: s("step")?, pct: v.get("pct").and_then(|x| x.as_u64()).unwrap_or(0).min(100) as u8 }),
        Some(_) => None,
    }
}

/// The step names as words.
pub fn step_words(step: &str) -> &'static str {
    match step {
        "fetch" => "Downloading",
        "build" => "Building",
        "test" => "Checking the new build",
        "switch" => "Switching over",
        _ => "Working",
    }
}

/// Where an update stands, shared with the thread that runs it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    pub check: Option<Check>,
    pub checking: bool,
    pub step: Option<(String, u8)>,
    pub done: Option<Result<String, String>>,
}

fn engine(args: &[&str]) -> std::io::Result<std::process::Child> {
    Command::new(crate::engine::bin()).args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn()
}

impl State {
    /// A quiet look at the public repository: is there a newer Hestia?
    pub(crate) fn update_check(&mut self) {
        let (st, wake) = (self.update.clone(), self.jobs.waker());
        st.lock().unwrap().checking = true;
        std::thread::spawn(move || {
            let check = engine(&["update", "check"]).ok().and_then(|c| c.wait_with_output().ok())
                .and_then(|o| String::from_utf8_lossy(&o.stdout).lines().find_map(parse_check))
                .unwrap_or(Check { error: Some("Couldn't check for updates right now.".into()), ..Default::default() });
            { let mut s = st.lock().unwrap(); s.check = Some(check); s.checking = false; }
            if let Some(w) = wake { w(); }
        });
    }

    /// Install the newer build as a background job, showing each step; the engine refuses during a backup.
    pub(crate) fn update_apply(&mut self) {
        self.update_run("apply");
    }

    /// Go back to the build before the last update (a second rollback goes forward again).
    pub(crate) fn update_rollback(&mut self) {
        self.update_run("rollback");
    }

    fn update_run(&mut self, what: &'static str) {
        let (st, wake) = (self.update.clone(), self.jobs.waker());
        { let mut s = st.lock().unwrap(); s.step = Some((if what == "apply" { "fetch" } else { "switch" }.into(), 0)); s.done = None; }
        std::thread::spawn(move || {
            let mut finished = None;
            if let Ok(mut child) = engine(&["update", what]) {
                if let Some(out) = child.stdout.take() {
                    for line in BufReader::new(out).lines().map_while(Result::ok) {
                        match parse_step(&line) {
                            Some(Step::Progress { step, pct }) => st.lock().unwrap().step = Some((step, pct)),
                            Some(Step::Updated { version }) => finished = Some(Ok(version)),
                            Some(Step::Failed { why }) => finished = Some(Err(why)),
                            None => {}
                        }
                        if let Some(w) = &wake { w(); }
                    }
                }
                let _ = child.wait();
            }
            let mut s = st.lock().unwrap();
            s.step = None;
            s.done = Some(finished.unwrap_or_else(|| Err("The update stopped before it finished. Nothing was changed.".into())));
            drop(s);
            if let Some(w) = wake { w(); }
        });
    }
}

/// Start the new build and close this one.
fn restart() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = Command::new(exe).spawn();
    }
    std::process::exit(0);
}

/// The Updates row for Settings > About: one status line, the changes when there are some, and one action.
pub fn section(s: &State, d: &Design) -> El {
    let t = d.theme();
    let u = s.update.lock().unwrap().clone();
    let (line, action): (String, Option<El>) = if let Some(done) = &u.done {
        match done {
            Ok(v) => (format!("Hestia build {v} is in place. Restart to use it."), Some(crate::questions::button(d, "upd-restart", "Restart Hestia".into(), true, |_: &mut State| restart()))),
            Err(why) => (why.clone(), Some(crate::questions::button(d, "upd-retry", "Try again".into(), false, |s: &mut State| s.update_apply()))),
        }
    } else if let Some((step, pct)) = &u.step {
        (format!("{}… {pct}%", step_words(step)), None)
    } else if u.checking {
        ("Checking for updates…".into(), None)
    } else {
        match &u.check {
            Some(c) if c.error.as_deref().is_some_and(|e| e.contains("install.sh")) => ("Run the installer once to enable updates from here.".into(), None),
            Some(c) if c.error.is_some() => (c.error.clone().unwrap_or_default(), Some(crate::questions::button(d, "upd-check", "Check again".into(), false, |s: &mut State| s.update_check()))),
            Some(c) if c.newer => (format!("Hestia {} is available.", c.available.clone().unwrap_or_default()), Some(crate::questions::button(d, "upd-apply", "Update Hestia".into(), true, |s: &mut State| s.update_apply()))),
            Some(_) => ("Hestia is up to date.".into(), Some(row().gap(d.gap())
                .child(crate::questions::button(d, "upd-back", "Go back a version".into(), false, |s: &mut State| s.update_rollback()))
                .child(crate::questions::button(d, "upd-check", "Check again".into(), false, |s: &mut State| s.update_check())))),
            None => ("Hestia checks for a newer version when you ask.".into(), Some(crate::questions::button(d, "upd-check", "Check for updates".into(), false, |s: &mut State| s.update_check()))),
        }
    };
    let mut c = col().w_full().gap(d.gap()).py(d.gap() * 1.5)
        .child(row().w_full().gap(d.pad()).center_y()
            .child(col().grow(1.0).shrink().min_w(0.0).gap(2.0)
                .child(text("Updates".to_string()).font(style(d, type_::BODY)).color(t.text))
                .child(text(line).font(style(d, type_::LABEL)).color(t.text_secondary).wrap_anywhere().shrink()))
            .children(action));
    if let Some(ch) = u.check.as_ref().filter(|c| c.newer && u.done.is_none()) {
        for l in ch.changes.iter().take(5) {
            c = c.child(text(format!("•  {l}")).font(style(d, type_::LABEL)).color(t.text_secondary).wrap_anywhere().shrink());
        }
    }
    c
}

/// Shared status for a new window.
pub fn new_status() -> Arc<Mutex<Status>> {
    Arc::new(Mutex::new(Status::default()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_check_reads_newer_and_its_changes() {
        let c = parse_check(r#"{"installed":"0.1.0+abc","available":"0.2.0","newer":true,"changes":["Search opens apps on Enter","Calmer dark mode"],"source":"github","checked_at":"2026-11-25T10:00:00Z","error":null}"#).unwrap();
        assert!(c.newer && c.error.is_none());
        assert_eq!((c.available.as_deref(), c.changes.len()), (Some("0.2.0"), 2));
        let e = parse_check(r#"{"installed":"0.1.0","newer":false,"changes":[],"error":"offline"}"#).unwrap();
        assert_eq!(e.error.as_deref(), Some("offline"));
    }

    #[test]
    fn apply_lines_become_steps_and_an_end() {
        assert_eq!(parse_step(r#"{"event":"step","pct":40,"step":"build"}"#), Some(Step::Progress { step: "build".into(), pct: 40 }));
        assert_eq!(parse_step(r#"{"event":"updated","from":"9641b2ee","to":"a1b2c3d4"}"#), Some(Step::Updated { version: "a1b2c3d4".into() }));
        assert_eq!(parse_step(r#"{"event":"rolled_back","from":"a1b2c3d4","to":"9641b2ee"}"#), Some(Step::Updated { version: "9641b2ee".into() }));
        assert!(matches!(parse_step(r#"{"event":"failed","message":"A backup is running; try again in a minute."}"#), Some(Step::Failed { .. })));
        assert_eq!(parse_step("not json"), None);
        assert_eq!(step_words("test"), "Checking the new build");
    }
}
