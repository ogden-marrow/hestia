//! `lifestream feed` end to end, on a synthetic journal and watcher history in a scratch HOME.
//! Every test runs with HOME and XDG_DATA_HOME inside the system temp folder; the real home is never read or written.

use chrono::{Duration, SecondsFormat, Utc};
use lifestream::{By, Record};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

struct Scratch {
    home: PathBuf,
    stream: PathBuf,
    history: PathBuf,
}

fn scratch(tag: &str) -> Scratch {
    let home = std::env::temp_dir().join(format!("ls-feed-{tag}-{}", std::process::id()));
    assert!(home.starts_with(std::env::temp_dir()), "the test home must be under the temp folder");
    let _ = fs::remove_dir_all(&home);
    let stream = home.join("stream");
    let history = home.join(".local/share/hestia/watch-history.jsonl");
    fs::create_dir_all(&stream).unwrap();
    fs::create_dir_all(history.parent().unwrap()).unwrap();
    Scratch { home, stream, history }
}

fn ago(secs: i64) -> String {
    (Utc::now() - Duration::seconds(secs)).to_rfc3339_opts(SecondsFormat::Nanos, true)
}

fn epoch_ago(secs: i64) -> i64 {
    (Utc::now() - Duration::seconds(secs)).timestamp()
}

fn write_lines(p: &Path, lines: &[Value]) {
    let body: String = lines.iter().map(|l| format!("{l}\n")).collect();
    fs::write(p, body).unwrap();
}

fn run(b: &Scratch, args: &[&str]) -> (bool, Vec<Value>, String) {
    assert!(b.home.starts_with(std::env::temp_dir()));
    let out = Command::new(env!("CARGO_BIN_EXE_lifestream"))
        .arg("feed")
        .args(args)
        .arg("--out")
        .arg(&b.stream)
        .env("HOME", &b.home)
        .env("XDG_DATA_HOME", b.home.join(".local/share"))
        .output()
        .unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    let items = text.lines().map(|l| serde_json::from_str(l).expect("every output line is JSON")).collect();
    (out.status.success(), items, text)
}

/// A journal and history holding every kind this reader emits and every actor.
fn full(b: &Scratch) {
    let one_batch = ago(900);
    write_lines(
        &b.stream.join("moves.jsonl"),
        &[
            json!({"batch": one_batch, "id": "x1", "from": "~/Downloads/a.pdf", "to": "~/Documents/Finance/a.pdf", "actor": "hestia_rule", "reason": "rule: pdf downloads"}),
            json!({"batch": one_batch, "id": "x2", "from": "~/Downloads/b.pdf", "to": "~/Documents/Finance/b.pdf", "actor": "hestia_rule", "reason": "rule: pdf downloads"}),
            json!({"batch": ago(800), "id": "x3", "from": "~/Downloads/c.pdf", "to": "~/Documents/Taxes/c.pdf", "actor": "model", "reason": "both models agreed: invoice"}),
            json!({"batch": ago(700), "id": "x4", "from": "~/Downloads/d.pdf", "to": "~/Documents/Taxes/d.pdf", "actor": "you", "reason": "you answered: Taxes"}),
            json!({"batch": ago(600), "kind": "mkdir", "to": "~/Documents/Receipts", "actor": "you", "reason": "you made a new folder"}),
            json!({"batch": ago(500), "kind": "dir", "from": "~/Documents/Old", "to": "~/Documents/Older", "actor": "you", "reason": "you changed your folders"}),
            json!({"batch": ago(400), "kind": "dir", "from": "~/Documents/Older", "to": "~/Archive/Older", "actor": "you", "reason": "you changed your folders"}),
        ],
    );
    write_lines(&b.stream.join("undone.jsonl"), &[json!({"at": ago(300), "files": 2, "folders": 0, "removed": 0, "paths": ["~/Downloads/a.pdf", "~/Downloads/b.pdf"]})]);
    write_lines(
        &b.history,
        &[
            json!({"at": epoch_ago(200), "result": "ok: 12 commit(s), pushed", "commits": 12}),
            json!({"at": epoch_ago(150), "result": "FAILED: offline: push could not reach GitHub, will retry", "commits": 0}),
        ],
    );
}

#[test]
fn every_kind_and_actor_newest_first_with_the_contract_fields() {
    let b = scratch("all");
    full(&b);
    let (ok, items, _) = run(&b, &[]);
    assert!(ok);
    let kinds: Vec<&str> = items.iter().map(|i| i["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["backup_failed", "backup_saved", "undone", "moved", "renamed", "folder_created", "filed", "filed", "filed"]);
    let actors: std::collections::BTreeSet<&str> = items.iter().map(|i| i["actor"].as_str().unwrap()).collect();
    assert_eq!(actors, ["hestia_rule", "model", "watcher", "you"].into_iter().collect());
    let ats: Vec<&str> = items.iter().map(|i| i["at"].as_str().unwrap()).collect();
    assert!(ats.windows(2).all(|w| w[0] >= w[1]), "newest first: {ats:?}");
    for i in &items {
        for k in ["id", "at", "kind", "actor", "reason", "summary", "count", "paths", "undoable", "batch"] {
            assert!(i.get(k).is_some(), "{k} missing in {i}");
        }
        assert!(chrono::DateTime::parse_from_rfc3339(i["at"].as_str().unwrap()).is_ok());
        assert!(!i["summary"].as_str().unwrap().is_empty() && !i["reason"].as_str().unwrap().is_empty());
        assert!(!i["summary"].as_str().unwrap().contains("sha256"), "no hashes in a summary");
    }
    let rule = items.iter().find(|i| i["actor"] == "hestia_rule").unwrap();
    assert_eq!((rule["count"].as_u64(), rule["reason"].as_str(), rule["undoable"].as_bool()), (Some(2), Some("rule: pdf downloads"), Some(true)));
    assert_eq!(rule["summary"], "Filed 2 files into Documents/Finance");
    assert_eq!(rule["paths"], json!(["Documents/Finance/a.pdf", "Documents/Finance/b.pdf"]));
    assert!(rule["batch"].as_str().is_some(), "a filed item carries the batch id undo takes");
    let model = items.iter().find(|i| i["actor"] == "model").unwrap();
    assert_eq!((model["reason"].as_str(), model["summary"].as_str()), (Some("both models agreed: invoice"), Some("Filed c.pdf into Documents/Taxes")));
    let by_kind = |k: &str| items.iter().find(|i| i["kind"] == k).unwrap();
    assert_eq!(by_kind("renamed")["summary"], "Renamed the folder Old to Older");
    assert_eq!(by_kind("moved")["summary"], "Moved the folder Older into Archive");
    assert_eq!(by_kind("folder_created")["summary"], "Made a new folder Documents/Receipts");
    let undone = by_kind("undone");
    assert_eq!((undone["summary"].as_str(), undone["undoable"].as_bool(), undone["batch"].is_null()), (Some("Put back 2 files"), Some(false), true));
    let saved = by_kind("backup_saved");
    assert_eq!((saved["summary"].as_str(), saved["count"].as_u64(), saved["batch"].is_null()), (Some("Backup saved 12 changes"), Some(12), true));
    let failed = by_kind("backup_failed");
    assert!(failed["summary"].as_str().unwrap().contains("offline"), "{failed}");
    assert_eq!(failed["undoable"], false);
    fs::remove_dir_all(&b.home).unwrap();
}

#[test]
fn since_and_limit_narrow_the_feed() {
    let b = scratch("since");
    full(&b);
    // 650 s ago keeps the two newest journal batches (400 s, 500 s, 600 s) and everything after them
    let (ok, items, _) = run(&b, &["--since", "650s"]);
    assert!(ok);
    assert_eq!(items.len(), 6, "{items:?}");
    let (_, items, _) = run(&b, &["--since", "1h"]);
    assert_eq!(items.len(), 9);
    let (_, items, _) = run(&b, &["--since", &(Utc::now() - Duration::seconds(450)).to_rfc3339_opts(SecondsFormat::Secs, true)]);
    assert_eq!(items.len(), 4, "a timestamp works as well: {items:?}");
    let (_, items, _) = run(&b, &["--limit", "3"]);
    assert_eq!(items.iter().map(|i| i["kind"].as_str().unwrap()).collect::<Vec<_>>(), ["backup_failed", "backup_saved", "undone"], "the newest three");
    let (ok, items, _) = run(&b, &["--since", "yesterday-ish"]);
    assert!(!ok && items.is_empty(), "a time it cannot read is an error, not an empty feed");
    fs::remove_dir_all(&b.home).unwrap();
}

#[test]
fn malformed_and_tampered_lines_are_skipped_not_fatal() {
    let b = scratch("bad");
    let good = json!({"batch": ago(60), "id": "g", "from": "~/Downloads/g.pdf", "to": "~/Documents/g.pdf", "actor": "you", "reason": "you answered: Taxes"});
    let junk = [
        "not json at all".to_string(),
        "{\"batch\": ".to_string(),
        "[1,2,3]".to_string(),
        json!({"batch": "yesterday", "id": "x", "from": "~/a", "to": "~/b"}).to_string(),
        json!({"batch": ago(50), "id": "x", "from": "~/a"}).to_string(),
        json!({"batch": ago(50), "id": "x", "from": "~/a", "to": "~/b", "kind": "teleport"}).to_string(),
        json!({"batch": 7, "to": "~/b"}).to_string(),
        "\u{0}\u{1}\u{2}".to_string(),
        good.to_string(),
    ];
    fs::write(b.stream.join("moves.jsonl"), junk.join("\n") + "\n").unwrap();
    fs::write(b.stream.join("undone.jsonl"), "garbage\n{\"at\":\"never\",\"files\":3}\n{\"at\":\"2026-01-01T00:00:00Z\",\"files\":0}\n").unwrap();
    fs::write(&b.history, "garbage\n{\"at\":\"soon\",\"result\":\"ok\"}\n{\"at\":1}\n{\"result\":\"ok\"}\n").unwrap();
    let (ok, items, _) = run(&b, &[]);
    assert!(ok, "bad lines never fail the command");
    assert_eq!(items.len(), 1, "{items:?}");
    assert_eq!(items[0]["paths"], json!(["Documents/g.pdf"]));
    let (ok, items, _) = run(&Scratch { stream: b.home.join("no-such-stream"), ..b }, &[]);
    assert!(ok && items.is_empty(), "no journal at all is an empty feed");
}

#[test]
fn a_line_pointing_outside_home_is_never_listed_with_its_path() {
    let b = scratch("out");
    let bad = [
        ("~/Downloads/a", "/etc/passwd"),
        ("/etc/shadow", "~/Documents/a"),
        ("~/Downloads/a", "~/../../etc/passwd"),
        ("~/Downloads/a", "~/Documents/../../x"),
        ("~/Downloads/a", "~/.ssh/authorized_keys"),
        ("~/.gnupg/x", "~/Documents/a"),
        ("~/Downloads/a", "Documents/no-tilde"),
        ("~/Downloads/a", "~/"),
    ];
    let mut lines: Vec<Value> = bad.iter().map(|(f, t)| json!({"batch": ago(60), "id": "x", "from": f, "to": t, "actor": "you", "reason": "r"})).collect();
    lines.push(json!({"batch": ago(60), "kind": "mkdir", "to": "/root/.ssh", "actor": "you", "reason": "r"}));
    lines.push(json!({"batch": ago(60), "kind": "dir", "from": "~/Real", "to": "/etc", "actor": "you", "reason": "r"}));
    write_lines(&b.stream.join("moves.jsonl"), &lines);
    write_lines(&b.stream.join("undone.jsonl"), &[json!({"at": ago(10), "files": 1, "paths": ["/etc/passwd", "~/.ssh/id_rsa", "~/Documents/ok.pdf"]})]);
    let (ok, items, text) = run(&b, &[]);
    assert!(ok);
    for leak in ["/etc", "passwd", "shadow", ".ssh", ".gnupg", "id_rsa", "no-tilde", ".."] {
        assert!(!text.contains(leak), "{leak} leaked: {text}");
    }
    assert_eq!(items.len(), 1, "only the undo survived, and with its one good path: {items:?}");
    assert_eq!(items[0]["paths"], json!(["Documents/ok.pdf"]));
    fs::remove_dir_all(&b.home).unwrap();
}

#[test]
fn output_is_capped_in_items_paths_and_bytes() {
    let b = scratch("cap");
    let long = "n".repeat(100);
    let mut lines = Vec::new();
    for batch in 0..1500 {
        let at = (Utc::now() - Duration::seconds(10_000 - batch)).to_rfc3339_opts(SecondsFormat::Nanos, true);
        for k in 0..8 {
            lines.push(json!({"batch": at, "id": "x", "from": format!("~/Downloads/{k}"), "to": format!("~/Documents/{long}{k}/{long}{k}"), "actor": "you", "reason": "you answered: x"}));
        }
    }
    write_lines(&b.stream.join("moves.jsonl"), &lines);
    let (ok, items, text) = run(&b, &["--limit", "999999"]);
    assert!(ok);
    assert!(items.len() <= 1000, "never more than 1000 items, got {}", items.len());
    assert!(text.len() <= 512 * 1024, "output is {} bytes", text.len());
    assert!(items.iter().all(|i| i["paths"].as_array().unwrap().len() <= 5 && i["count"] == 8), "paths are capped, the count is not");
    let (_, items, _) = run(&b, &[]);
    assert_eq!(items.len(), 100, "the default is 100");
    let (_, items, _) = run(&b, &["--limit", "0"]);
    assert_eq!(items.len(), 1, "a limit of 0 still shows the newest");
    fs::remove_dir_all(&b.home).unwrap();
}

#[test]
fn lines_from_before_actors_were_recorded_are_read_or_called_unknown() {
    let b = scratch("old");
    let rec = |id: &str, tier: &str| Record {
        id: id.into(), first_seen: "n".into(), mtime: "2025-09-15T00:00:00Z".into(), kind: "document/invoice".into(), size: 1,
        sources: vec![format!("~/Documents/{id}.pdf")], tags: vec![], summary: None,
        by: By { tier: tier.into(), name: "pdf-docs".into(), confidence: 1.0 },
        status: "filed".into(), store: None, future_at: None, was: vec![],
    };
    lifestream::write_all(&b.stream, &[rec("r1", "rule"), rec("h1", "human")]).unwrap();
    write_lines(
        &b.stream.join("moves.jsonl"),
        &[
            json!({"batch": ago(300), "id": "r1", "from": "~/Downloads/r1.pdf", "to": "~/Documents/r1.pdf"}),
            json!({"batch": ago(200), "id": "h1", "from": "~/Downloads/h1.pdf", "to": "~/Documents/h1.pdf"}),
            json!({"batch": ago(100), "id": "gone", "from": "~/Downloads/z.pdf", "to": "~/Documents/z.pdf"}),
            json!({"batch": ago(50), "kind": "mkdir", "to": "~/Documents/New"}),
        ],
    );
    let (ok, items, _) = run(&b, &[]);
    assert!(ok);
    let got: Vec<(&str, &str)> = items.iter().map(|i| (i["actor"].as_str().unwrap(), i["reason"].as_str().unwrap())).collect();
    assert_eq!(got, [("you", "you made a new folder"), ("unknown", "recorded before Hestia kept track of who decided"), ("you", "you answered: invoice"), ("hestia_rule", "rule: pdf docs")]);
    fs::remove_dir_all(&b.home).unwrap();
}

#[test]
fn watcher_history_failures_collapse_and_quiet_runs_are_not_events() {
    let b = scratch("watch");
    let fail = "FAILED: offline: push could not reach GitHub, will retry";
    write_lines(
        &b.history,
        &[
            json!({"at": epoch_ago(1000), "result": "ok: 0 commit(s), up to date", "commits": 0}),
            json!({"at": epoch_ago(900), "result": "busy: waiting for a quieter moment", "commits": 0}),
            json!({"at": epoch_ago(800), "result": fail, "commits": 0}),
            json!({"at": epoch_ago(700), "result": "busy: waiting for a quieter moment", "commits": 0}),
            json!({"at": epoch_ago(600), "result": fail, "commits": 0}),
            json!({"at": epoch_ago(500), "result": fail, "commits": 0}),
            json!({"at": epoch_ago(400), "result": "ok: 1 commit(s), pushed", "commits": 1}),
            json!({"at": epoch_ago(300), "result": "FAILED: capture failed: disk is full", "commits": 0}),
        ],
    );
    let (_, items, _) = run(&b, &[]);
    let got: Vec<(&str, u64)> = items.iter().map(|i| (i["kind"].as_str().unwrap(), i["count"].as_u64().unwrap())).collect();
    assert_eq!(got, [("backup_failed", 1), ("backup_saved", 1), ("backup_failed", 3)], "{items:?}");
    assert_eq!(items[1]["summary"], "Backup saved 1 change");
    fs::remove_dir_all(&b.home).unwrap();
}

#[test]
fn the_feed_changes_nothing_on_disk() {
    let b = scratch("ro");
    full(&b);
    fs::write(b.home.join("keep.txt"), "mine").unwrap();
    let snap = |b: &Scratch| -> Vec<(PathBuf, Vec<u8>)> {
        let mut v: Vec<_> = ["moves.jsonl", "undone.jsonl"].iter().map(|n| b.stream.join(n)).chain([b.history.clone(), b.home.join("keep.txt")]).map(|p| (p.clone(), fs::read(p).unwrap())).collect();
        v.sort();
        v
    };
    let before = snap(&b);
    let n = || fs::read_dir(&b.stream).unwrap().count();
    let files = n();
    run(&b, &[]);
    run(&b, &["--since", "1h", "--limit", "2"]);
    assert_eq!(before, snap(&b));
    assert_eq!(files, n(), "no new file appears");
    fs::remove_dir_all(&b.home).unwrap();
}
