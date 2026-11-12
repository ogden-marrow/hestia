//! Talks to the engine (`lifestream auth|home ... --json`) for the sign-in flow (#110).
//!
//! Contract v1: one JSON object per line on stdout; exit 0 means the last line is a success event,
//! non-zero means it is `{"event":"failed",...}`. No line ever contains a token. Cancel is SIGTERM.
//! Lines become `account::Event`s on a background thread; the UI drains them in `poll`.

use crate::account::{Event, Fail, Repo};
use crate::State;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

/// Where the engine is. `HESTIA_LIFESTREAM` overrides (tests and development).
pub fn bin() -> PathBuf {
    if let Some(p) = std::env::var_os("HESTIA_LIFESTREAM") {
        return PathBuf::from(p);
    }
    let home = std::env::var("HOME").unwrap_or_default();
    let beside = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("lifestream")));
    beside.into_iter().chain([".local/bin", ".cargo/bin"].map(|d| PathBuf::from(&home).join(d).join("lifestream")))
        .find(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from("lifestream"))
}

/// "2026-11-10T12:00:00Z" becomes "3 days ago". Anything unreadable is shown as it came.
pub fn ago(iso: &str, now: chrono::DateTime<chrono::Utc>) -> String {
    let Ok(t) = chrono::DateTime::parse_from_rfc3339(iso) else { return iso.to_string() };
    let days = (now - t.with_timezone(&chrono::Utc)).num_days();
    match days {
        d if d < 1 => "today".into(),
        1 => "yesterday".into(),
        d if d < 30 => format!("{d} days ago"),
        d if d < 60 => "a month ago".into(),
        d if d < 365 => format!("{} months ago", d / 30),
        _ => "over a year ago".into(),
    }
}

/// One line of the contract as an event, or None for a line that carries nothing for the flow.
pub fn parse_line(line: &str, now: chrono::DateTime<chrono::Utc>) -> Option<Event> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    let str_of = |k: &str| v[k].as_str().map(str::to_string);
    Some(match v["event"].as_str()? {
        "code" => Event::Code(str_of("user_code")?),
        "approved" => Event::Approved(str_of("user")?),
        "status" => match (v["signed_in"].as_bool()?, str_of("user")) {
            (true, Some(user)) => Event::Known(user),
            _ => return None,
        },
        "signed_out" => Event::SignedOut,
        "found" => Event::Found(v["repos"].as_array()?.iter().filter_map(|r| {
            Some(Repo { name: r["name"].as_str()?.to_string(), updated: ago(r["updated"].as_str().unwrap_or(""), now) })
        }).collect()),
        "none" => Event::NoneFound,
        "created" => Event::Created(str_of("repo")?),
        "cloned" => Event::Cloned(str_of("repo")?),
        "failed" => Event::Failed(match v["kind"].as_str()? {
            "offline" => Fail::Offline,
            "denied" => Fail::Denied,
            "expired" => Fail::Expired,
            "sso" => Fail::Sso(str_of("org").unwrap_or_default()),
            "exists" => Fail::Exists(str_of("repo").unwrap_or_default()),
            "no-keyring" => Fail::NoKeyring,
            _ => Fail::Other(str_of("message").unwrap_or_else(|| "That did not work.".into())),
        }),
        _ => return None,
    })
}

fn is_final(e: &Event) -> bool {
    !matches!(e, Event::Code(_))
}

/// Start the engine. A program that was written a moment ago can briefly be "text file busy" (ETXTBSY) when
/// another thread forks while the file is still open for writing; that clears within milliseconds, so try again.
fn spawn_patiently(bin: &Path, args: &[&str]) -> std::io::Result<std::process::Child> {
    const TEXT_FILE_BUSY: i32 = 26;
    let mut tries = 0;
    loop {
        match Command::new(bin).args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn() {
            Err(e) if e.raw_os_error() == Some(TEXT_FILE_BUSY) && tries < 50 => {
                tries += 1;
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            other => return other,
        }
    }
}

/// Run one command and hand each event to `sink` as it arrives. `pid` holds the running process so
/// the UI can cancel it; if it was cancelled, no failure is reported for the exit.
pub fn stream(bin: &Path, args: &[&str], pid: &Mutex<Option<u32>>, mut sink: impl FnMut(Event)) {
    let spawned = spawn_patiently(bin, args);
    let mut child = match spawned {
        Ok(c) => c,
        Err(_) => return sink(Event::Failed(Fail::Other("Hestia's helper program isn't installed, so it can't sign in yet.".into()))),
    };
    *pid.lock().unwrap() = Some(child.id());
    let mut ended_with_result = false;
    if let Some(out) = child.stdout.take() {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            if let Some(e) = parse_line(&line, chrono::Utc::now()) {
                ended_with_result |= is_final(&e);
                sink(e);
            }
        }
    }
    let _ = child.wait();
    let cancelled = pid.lock().unwrap().take().is_none();
    if !ended_with_result && !cancelled {
        sink(Event::Failed(Fail::Other("The sign-in helper stopped before it finished. Try again.".into())));
    }
}

/// Start a command in the background; its events arrive through `poll`.
pub fn run(s: &State, args: &[&str]) {
    let (inbox, pid, wake) = (s.auth_inbox.clone(), s.auth_pid.clone(), s.jobs.waker());
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    std::thread::spawn(move || {
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        stream(&bin(), &refs, &pid, |e| {
            inbox.lock().unwrap().push(e);
            if let Some(w) = &wake { w(); }
        });
    });
}

/// Cancel whatever is running (SIGTERM; the engine leaves nothing half-written).
pub fn cancel(s: &State) {
    if let Some(p) = s.auth_pid.lock().unwrap().take() {
        let _ = Command::new("kill").args(["-TERM", &p.to_string()]).status();
    }
}

/// What `lifestream file --undo` said: how many files went back, and which ones could not (path, plain reason).
#[derive(Debug, Default, PartialEq)]
pub struct Undone {
    pub restored: usize,
    pub skipped: Vec<(String, String)>,
}

pub fn parse_undo(line: &str) -> Option<Undone> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    (v["event"].as_str()? == "undone").then(|| Undone {
        restored: v["restored"].as_u64().unwrap_or(0) as usize,
        skipped: v["skipped"].as_array().map(|a| a.iter().filter_map(|x| Some((x["path"].as_str()?.to_string(), x["why"].as_str()?.to_string()))).collect()).unwrap_or_default(),
    })
}

/// Put the last batch of filed files back (blocking: run it as a job).
pub fn undo(stream: &Path) -> Result<Undone, String> {
    let out = Command::new(bin()).args(["file", "--undo", "--out"]).arg(stream).stdin(Stdio::null()).stderr(Stdio::null()).output()
        .map_err(|_| "Hestia's helper program isn't installed, so it can't put files back yet.".to_string())?;
    String::from_utf8_lossy(&out.stdout).lines().rev().find_map(parse_undo)
        .ok_or_else(|| "Putting the files back did not finish. Nothing was changed that Hestia could not undo; try again.".to_string())
}

/// Take everything the engine has said since the last look.
pub fn drain(inbox: &Arc<Mutex<Vec<Event>>>) -> Vec<Event> {
    std::mem::take(&mut *inbox.lock().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::parse_from_rfc3339("2026-11-12T12:00:00Z").unwrap().with_timezone(&chrono::Utc)
    }

    #[test]
    fn every_line_of_the_contract_maps_to_one_event() {
        let p = |l: &str| parse_line(l, now());
        assert_eq!(p(r#"{"event":"code","user_code":"ABCD-1234","verification_uri":"https://github.com/login/device","expires_in":900,"scope":"repo"}"#), Some(Event::Code("ABCD-1234".into())));
        assert_eq!(p(r#"{"event":"approved","user":"ada"}"#), Some(Event::Approved("ada".into())));
        assert_eq!(p(r#"{"event":"status","signed_in":true,"user":"ada"}"#), Some(Event::Known("ada".into())));
        assert_eq!(p(r#"{"event":"status","signed_in":false}"#), None);
        assert_eq!(p(r#"{"event":"signed_out"}"#), Some(Event::SignedOut));
        assert_eq!(p(r#"{"event":"none"}"#), Some(Event::NoneFound));
        assert_eq!(p(r#"{"event":"created","repo":"ada/home","path":"/x"}"#), Some(Event::Created("ada/home".into())));
        assert_eq!(p(r#"{"event":"cloned","path":"/x","repo":"ada/home"}"#), Some(Event::Cloned("ada/home".into())));
        assert_eq!(p(r#"{"event":"found","repos":[{"name":"ada/home","updated":"2026-11-10T12:00:00Z"}]}"#),
            Some(Event::Found(vec![Repo { name: "ada/home".into(), updated: "2 days ago".into() }])));
        for (line, want) in [
            (r#"{"event":"failed","kind":"offline"}"#, Fail::Offline),
            (r#"{"event":"failed","kind":"denied"}"#, Fail::Denied),
            (r#"{"event":"failed","kind":"expired"}"#, Fail::Expired),
            (r#"{"event":"failed","kind":"sso","org":"lab"}"#, Fail::Sso("lab".into())),
            (r#"{"event":"failed","kind":"exists","repo":"ada/home"}"#, Fail::Exists("ada/home".into())),
            (r#"{"event":"failed","kind":"no-keyring"}"#, Fail::NoKeyring),
            (r#"{"event":"failed","kind":"other","message":"plain words"}"#, Fail::Other("plain words".into())),
            (r#"{"event":"failed","kind":"something-new"}"#, Fail::Other("That did not work.".into())),
        ] {
            assert_eq!(p(line), Some(Event::Failed(want)), "{line}");
        }
        assert_eq!(p("not json"), None);
        assert_eq!(p(r#"{"event":"unknown"}"#), None);
    }

    #[test]
    fn the_undo_line_is_read() {
        assert_eq!(parse_undo(r#"{"event":"undone","restored":2,"skipped":[{"path":"~/Pictures/a.png","why":"something is already at the old place"}]}"#),
            Some(Undone { restored: 2, skipped: vec![("~/Pictures/a.png".into(), "something is already at the old place".into())] }));
        assert_eq!(parse_undo(r#"{"event":"undone","restored":0,"skipped":[]}"#), Some(Undone::default()));
        assert_eq!(parse_undo("garbage"), None);
        assert_eq!(parse_undo(r#"{"event":"other"}"#), None);
    }

    #[test]
    fn updated_dates_are_human() {
        assert_eq!(ago("2026-11-12T08:00:00Z", now()), "today");
        assert_eq!(ago("2026-11-11T08:00:00Z", now()), "yesterday");
        assert_eq!(ago("2026-09-01T00:00:00Z", now()), "2 months ago");
        assert_eq!(ago("2024-01-01T00:00:00Z", now()), "over a year ago");
        assert_eq!(ago("garbled", now()), "garbled");
    }

    /// A fake engine printing the contract's lines, driven through the real streaming code.
    fn fake(script: &str) -> (PathBuf, PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("hestia-engine-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        let bin = dir.join("lifestream");
        std::fs::write(&bin, format!("#!/bin/sh\n{script}\n")).unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        (dir, bin)
    }

    fn collect(bin: &Path) -> Vec<Event> {
        let mut got = Vec::new();
        stream(bin, &["auth", "login", "--json"], &Mutex::new(None), |e| got.push(e));
        got
    }

    #[test]
    fn a_login_streams_its_code_then_its_approval() {
        let (dir, bin) = fake(r#"echo '{"event":"code","user_code":"ABCD-1234"}'; sleep 0.1; echo '{"event":"approved","user":"ada"}'"#);
        assert_eq!(collect(&bin), vec![Event::Code("ABCD-1234".into()), Event::Approved("ada".into())]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_helper_that_dies_silently_becomes_a_failure_the_person_can_read() {
        let (dir, bin) = fake("exit 3");
        let got = collect(&bin);
        assert!(matches!(got.as_slice(), [Event::Failed(Fail::Other(m))] if m.contains("stopped")), "{got:?}");
        let _ = std::fs::remove_dir_all(dir);
        let got = collect(Path::new("/nonexistent/lifestream"));
        assert!(matches!(got.as_slice(), [Event::Failed(Fail::Other(m))] if m.contains("isn't installed")), "{got:?}");
    }

    /// Regression for #123: many fakes written and run at once must never fail to start.
    #[test]
    fn a_fake_engine_starts_reliably_even_when_many_start_at_once() {
        let threads: Vec<_> = (0..8).map(|_| std::thread::spawn(|| {
            for _ in 0..25 {
                let (dir, bin) = fake(r#"echo '{"event":"failed","kind":"denied"}'; exit 1"#);
                assert_eq!(collect(&bin), vec![Event::Failed(Fail::Denied)]);
                let _ = std::fs::remove_dir_all(dir);
            }
        })).collect();
        for t in threads {
            t.join().expect("a fake engine failed to start");
        }
    }

    #[test]
    fn a_reported_failure_is_not_doubled() {
        let (dir, bin) = fake(r#"echo '{"event":"failed","kind":"denied"}'; exit 1"#);
        assert_eq!(collect(&bin), vec![Event::Failed(Fail::Denied)]);
        let _ = std::fs::remove_dir_all(dir);
    }
}
