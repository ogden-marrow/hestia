//! The answers log (`<stream>/answers.jsonl`): questions asked and answered, and how the feed shows them.
//! Every test works in a scratch home under the system temp folder; the real home is never read or written.

use chrono::{Duration, SecondsFormat, Utc};
use lifestream::feed::{feed, Item, Opts};
use lifestream::tree::{Folder, Tree};
use lifestream::{answers, bundle, load_all, teach, teach_and_file, tree, write_all, By, FileInto, Record};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

fn scratch(tag: &str) -> (PathBuf, PathBuf) {
    let home = std::env::temp_dir().join(format!("ls-answers-{tag}-{}", std::process::id()));
    assert!(home.starts_with(std::env::temp_dir()), "the test home must be under the temp folder");
    let _ = fs::remove_dir_all(&home);
    fs::create_dir_all(home.join("Downloads")).unwrap();
    let home = fs::canonicalize(home).unwrap();
    let stream = home.join("stream");
    (home.clone(), stream)
}

fn pending(home: &Path, rel: &str) -> Record {
    let p = home.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(&p, rel.as_bytes()).unwrap();
    Record {
        id: format!("id-{rel}"), first_seen: "n".into(), mtime: "2024-03-01T00:00:00Z".into(), kind: "unknown/pdf".into(), size: rel.len() as u64,
        sources: vec![format!("~/{rel}")], tags: vec![], summary: None,
        by: By { tier: "rule".into(), name: "none".into(), confidence: 0.0 }, status: "pending".into(), store: None, future_at: None, was: vec![],
    }
}

fn log(stream: &Path) -> Vec<Value> {
    fs::read_to_string(stream.join(answers::FILE)).unwrap_or_default().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
}

fn items(home: &Path, stream: &Path) -> Vec<Item> {
    feed(home, stream, &home.join("no-history.jsonl"), &Opts { since: None, limit: 1000 })
}

fn of<'a>(items: &'a [Item], kind: &str) -> Vec<&'a Item> {
    items.iter().filter(|i| i.kind == kind).collect()
}

fn taxes_tree() -> Tree {
    Tree { inbox: vec!["Downloads".into()], folders: vec![Folder { path: "Documents/Taxes".into(), kind: vec!["finance".into()], by_year: true, ..Default::default() }] }
}

#[test]
fn asking_writes_one_asked_line_per_new_folder_and_the_feed_groups_them() {
    let (h, stream) = scratch("ask");
    let recs = vec![pending(&h, "Downloads/A/x/1.pdf"), pending(&h, "Downloads/B/y/2.pdf"), pending(&h, "Downloads/C/3.pdf"), pending(&h, "Music/z/4.mp3")];
    write_all(&stream, &recs).unwrap();
    // no model is listening on this port: the questions are still asked, the suggestions just stay empty
    bundle::run(&recs, &stream, &h, "http://127.0.0.1:9", &bundle::Taxonomy::default()).unwrap();
    let lines = log(&stream);
    assert_eq!(lines.len(), 4, "{lines:?}");
    assert!(lines.iter().all(|l| l["kind"] == "asked" && l["actor"] == "model" && l["batch"].is_null() && l["choice"].is_null()));
    let folders: Vec<&str> = lines.iter().map(|l| l["folder"].as_str().unwrap()).collect();
    assert!(folders.contains(&"Downloads/A/x") && folders.contains(&"Music/z"), "{folders:?}");
    assert!(!fs::read_to_string(stream.join(answers::FILE)).unwrap().contains(".pdf"), "no file names, only folders");
    bundle::run(&recs, &stream, &h, "http://127.0.0.1:9", &bundle::Taxonomy::default()).unwrap();
    assert_eq!(log(&stream).len(), 4, "a folder already asked about is not asked again");

    let feed = items(&h, &stream);
    let asked = of(&feed, "question_asked");
    let downloads = asked.iter().find(|i| i.summary == "Hestia has 3 questions about Downloads").expect("grouped by top folder");
    assert_eq!((downloads.actor, downloads.count, downloads.undoable, downloads.batch.as_deref()), ("model", 3, false, None));
    assert_eq!(downloads.paths, ["Downloads/A/x", "Downloads/B/y", "Downloads/C"]);
    assert!(asked.iter().any(|i| i.summary == "Hestia has a question about Music/z" && i.count == 1), "{asked:?}");
    fs::remove_dir_all(h).unwrap();
}

#[test]
fn answering_and_filing_logs_the_batch_and_its_undo_restores_the_files() {
    let (h, stream) = scratch("file");
    let recs = vec![pending(&h, "Downloads/2024.pdf"), pending(&h, "Downloads/receipt.pdf")];
    write_all(&stream, &recs).unwrap();
    let ids: Vec<String> = recs.iter().map(|r| r.id.clone()).collect();
    let into = FileInto { home: &h, tree: &taxes_tree(), excluded: &[] };
    let done = teach_and_file(&stream, &ids, "finance/taxes", &[], &into).unwrap();
    assert_eq!((done.moved, done.failed.len()), (2, 0));
    assert!(h.join("Documents/Taxes/2024/2024.pdf").is_file() && !h.join("Downloads/2024.pdf").exists());
    let batch = done.batch.clone().expect("files moved, so there is a batch");
    let lines = log(&stream);
    assert_eq!(lines.len(), 1, "one folder answered, one line: {lines:?}");
    assert_eq!(lines[0]["kind"], "answered");
    assert_eq!(lines[0]["actor"], "you");
    assert_eq!(lines[0]["folder"], "Downloads");
    assert_eq!(lines[0]["choice"], "finance/taxes");
    assert_eq!(lines[0]["batch"].as_str(), Some(batch.as_str()));
    assert_eq!(lines[0]["id"].as_str(), Some(answers::question_id("~/Downloads").as_str()), "the answer names the question it answers");

    let feed = items(&h, &stream);
    let ans = of(&feed, "question_answered");
    assert_eq!(ans.len(), 1, "{feed:?}");
    assert_eq!((ans[0].actor, ans[0].summary.as_str(), ans[0].reason.as_str()), ("you", "You answered: Taxes for Downloads", "you answered: taxes"));
    assert_eq!((ans[0].undoable, ans[0].batch.as_deref(), &ans[0].paths[..]), (true, Some(batch.as_str()), &["Downloads".to_string()][..]));
    assert!(of(&feed, "filed").iter().all(|i| i.batch.as_deref() == Some(batch.as_str()) && i.actor == "you"), "the files show as filed by you, same batch");

    let r = tree::undo_batch(&stream, &h, ans[0].batch.as_deref()).unwrap();
    assert_eq!(r.restored, 2);
    assert!(h.join("Downloads/2024.pdf").is_file() && h.join("Downloads/receipt.pdf").is_file(), "undo put the files back");
    let feed = items(&h, &stream);
    let ans = of(&feed, "question_answered");
    assert_eq!((ans.len(), ans[0].undoable, ans[0].batch.is_none()), (1, false, true), "an undone answer stays in the Stream, without Undo");
    fs::remove_dir_all(h).unwrap();
}

#[test]
fn an_answer_that_moved_nothing_is_shown_without_undo() {
    let (h, stream) = scratch("nomove");
    let recs = vec![pending(&h, "Downloads/2024/a.pdf"), pending(&h, "Pictures/b.png")];
    write_all(&stream, &recs).unwrap();
    let back = teach(&stream, &[recs[0].id.clone(), recs[1].id.clone()], "Finance/Taxes", &[]).unwrap();
    assert!(back.iter().all(|r| r.by.tier == "human" && r.kind == "finance/taxes"));
    let lines = log(&stream);
    assert_eq!(lines.len(), 2, "one line per folder: {lines:?}");
    assert!(lines.iter().all(|l| l["batch"].is_null() && l["choice"] == "finance/taxes"));
    let feed = items(&h, &stream);
    let ans = of(&feed, "question_answered");
    assert_eq!(ans.len(), 2);
    assert!(ans.iter().all(|i| !i.undoable && i.batch.is_none() && i.actor == "you"));
    assert!(ans.iter().any(|i| i.summary == "You answered: Taxes for Downloads/2024"), "{ans:?}");
    assert!(teach(&stream, &["no-such".into()], "a/b", &[]).is_err(), "no record, no answer");
    assert_eq!(log(&stream).len(), 2, "and nothing logged for it");
    assert_eq!(load_all(&stream).unwrap().len(), 2);
    fs::remove_dir_all(h).unwrap();
}

fn at(secs: i64) -> String {
    (Utc::now() - Duration::seconds(secs)).to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn write(stream: &Path, lines: &[String]) {
    fs::create_dir_all(stream).unwrap();
    fs::write(stream.join(answers::FILE), lines.join("\n") + "\n").unwrap();
}

#[test]
fn malformed_answer_lines_are_skipped_not_fatal() {
    let (h, stream) = scratch("bad");
    let good = json!({"at": at(10), "kind": "answered", "id": "q-1", "actor": "you", "folder": "Downloads", "choice": "finance/taxes", "batch": null});
    let junk = [
        "not json".to_string(),
        "{\"at\":".to_string(),
        "[1,2]".to_string(),
        json!({"at": "yesterday", "kind": "asked", "id": "q", "actor": "model", "folder": "A"}).to_string(),
        json!({"at": at(5), "kind": "teleported", "id": "q", "actor": "you", "folder": "A", "choice": "a"}).to_string(),
        json!({"at": at(5), "kind": "answered", "id": "q", "actor": "admin", "folder": "A", "choice": "a"}).to_string(),
        json!({"at": at(5), "kind": "answered", "id": "q", "actor": "you", "folder": "A"}).to_string(),
        json!({"at": at(5), "kind": "answered", "id": "q", "actor": "you", "folder": "A", "choice": "rm -rf /; echo hi"}).to_string(),
        json!({"at": at(5), "kind": "answered", "id": "q/../x", "actor": "you", "folder": "A", "choice": "a"}).to_string(),
        json!({"at": at(5), "kind": "answered", "id": "q", "actor": "you", "choice": "a"}).to_string(),
        json!({"at": at(5), "kind": "answered", "id": "q", "actor": "you", "folder": 7, "choice": "a"}).to_string(),
        "\u{0}\u{1}".to_string(),
        good.to_string(),
    ];
    write(&stream, &junk);
    let feed = items(&h, &stream);
    assert_eq!(feed.len(), 1, "{feed:?}");
    assert_eq!(feed[0].summary, "You answered: Taxes for Downloads");
    assert!(items(&h, &h.join("no-such-stream")).is_empty(), "no log at all is an empty feed");
    fs::remove_dir_all(h).unwrap();
}

#[test]
fn answers_outside_home_are_never_shown() {
    let (h, stream) = scratch("out");
    let bad = ["/etc", "/etc/passwd", "../../etc", "Downloads/../../etc", ".ssh", "Documents/.gnupg", "~/Documents", "Docs//x", "a\nb", "Docs/\u{202E}gpj.exe"];
    let mut lines: Vec<String> = Vec::new();
    for f in bad {
        lines.push(json!({"at": at(10), "kind": "asked", "id": "q-1", "actor": "model", "folder": f}).to_string());
        lines.push(json!({"at": at(9), "kind": "answered", "id": "q-1", "actor": "you", "folder": f, "choice": "a/b"}).to_string());
    }
    lines.push(json!({"at": at(8), "kind": "answered", "id": "q-2", "actor": "you", "folder": "Documents/ok", "choice": "a/b"}).to_string());
    write(&stream, &lines);
    let feed = items(&h, &stream);
    let text = lifestream::feed::render(&feed);
    for leak in ["/etc", "passwd", ".ssh", ".gnupg", "..", "~", "\u{202E}", "//"] {
        assert!(!text.contains(leak), "{leak} leaked: {text}");
    }
    assert_eq!(feed.len(), 1, "only the answer about a plain folder in the home: {feed:?}");
    assert_eq!(feed[0].paths, ["Documents/ok"]);
    fs::remove_dir_all(h).unwrap();
}

#[test]
fn a_batch_the_journal_does_not_hold_is_not_offered_as_undo() {
    let (h, stream) = scratch("forged");
    write(&stream, &[json!({"at": at(10), "kind": "answered", "id": "q-1", "actor": "you", "folder": "Downloads", "choice": "a/b", "batch": "2026-01-01T00:00:00.000000000Z"}).to_string()]);
    let feed = items(&h, &stream);
    assert_eq!((feed.len(), feed[0].undoable, feed[0].batch.is_none()), (1, false, true), "{feed:?}");
    fs::remove_dir_all(h).unwrap();
}

#[test]
fn many_questions_are_capped_in_paths_items_and_bytes() {
    let (h, stream) = scratch("cap");
    let when = at(1);
    let long = "n".repeat(100);
    let mut lines: Vec<String> = (0..3000).map(|i| json!({"at": when, "kind": "asked", "id": format!("q-{i}"), "actor": "model", "folder": format!("Downloads/{long}{i}")}).to_string()).collect();
    lines.extend((0..3000).map(|i| json!({"at": at(100 + i % 40), "kind": "answered", "id": format!("q-{i}"), "actor": "you", "folder": format!("Downloads/{long}{i}"), "choice": "a/b"}).to_string()));
    write(&stream, &lines);
    let all = feed(&h, &stream, &h.join("none"), &Opts { since: None, limit: 999_999 });
    assert!(all.len() <= 1000, "never more than 1000 items, got {}", all.len());
    let asked = of(&all, "question_asked");
    assert_eq!(asked.len(), 1, "one run, one top folder, one item");
    assert_eq!((asked[0].count, asked[0].paths.len()), (3000, 5), "the count is whole, the paths are capped");
    assert_eq!(asked[0].summary, "Hestia has 3000 questions about Downloads");
    assert!(lifestream::feed::render(&all).len() <= 512 * 1024);
    fs::remove_dir_all(h).unwrap();
}
