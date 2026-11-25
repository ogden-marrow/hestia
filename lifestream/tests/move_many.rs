//! `lifestream folder move-many` end to end: one JSON line, one batch in the feed, one undo. Every test runs with HOME inside
//! the system temp folder; the real home is never read or written.

use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn scratch(tag: &str) -> PathBuf {
    let home = std::env::temp_dir().join(format!("ls-move-many-{tag}-{}", std::process::id()));
    assert!(home.starts_with(std::env::temp_dir()), "the test home must be under the temp folder");
    let _ = fs::remove_dir_all(&home);
    for (p, body) in [("Downloads/a.pdf", "a"), ("Pictures/b.png", "b"), ("Documents/Saab/c.bin", "c"), ("Secret/key.txt", "k")] {
        fs::create_dir_all(home.join(p).parent().unwrap()).unwrap();
        fs::write(home.join(p), body).unwrap();
    }
    fs::create_dir_all(home.join("Archive")).unwrap();
    fs::create_dir_all(home.join("manifest")).unwrap();
    fs::write(home.join("manifest/rules.toml"), "[[rule]]\npath = \"Secret\"\nsensitive = true\n").unwrap();
    fs::canonicalize(home).unwrap()
}

fn run(home: &Path, args: &[&str]) -> (bool, Vec<Value>) {
    assert!(home.starts_with(fs::canonicalize(std::env::temp_dir()).unwrap()));
    let out = Command::new(env!("CARGO_BIN_EXE_lifestream"))
        .args(args)
        .arg("--out")
        .arg(home.join("stream"))
        .current_dir(home)
        .env("HOME", home)
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .output()
        .unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    (out.status.success(), text.lines().map(|l| serde_json::from_str(l).expect("every output line is JSON")).collect())
}

#[test]
fn move_many_is_one_batch_in_the_feed_and_one_undo() {
    let h = scratch("ok");
    let (ok, out) = run(&h, &["folder", "move-many", "--to", "Archive", "Downloads/a.pdf", "Documents/Saab/", "Pictures/b.png"]);
    assert!(ok, "{out:?}");
    let ev = &out[0];
    assert_eq!((ev["event"].as_str(), ev["count"].as_u64()), (Some("moved"), Some(3)), "{ev}");
    assert_eq!(ev["items"][1], serde_json::json!({"from": "Documents/Saab", "to": "Archive/Saab"}));
    let batch = ev["batch"].as_str().unwrap().to_string();
    assert!(h.join("Archive/a.pdf").is_file() && h.join("Archive/Saab/c.bin").is_file() && h.join("Archive/b.png").is_file());

    let (_, feed) = run(&h, &["feed"]);
    assert_eq!(feed.len(), 1, "{feed:?}");
    assert_eq!((feed[0]["summary"].as_str(), feed[0]["batch"].as_str()), (Some("Moved 3 items into Archive"), Some(batch.as_str())));

    let (ok, undo) = run(&h, &["file", "--undo", "--batch", &batch]);
    assert!(ok);
    assert_eq!(undo[0]["restored"].as_u64(), Some(3), "{undo:?}");
    assert!(h.join("Downloads/a.pdf").is_file() && h.join("Documents/Saab/c.bin").is_file() && h.join("Pictures/b.png").is_file());
    assert_eq!(fs::read_dir(h.join("Archive")).unwrap().count(), 0);
    fs::remove_dir_all(h).unwrap();
}

#[test]
fn move_many_refuses_the_whole_group_with_one_failed_line() {
    let h = scratch("refused");
    let (ok, out) = run(&h, &["folder", "move-many", "--to", "Archive", "Downloads/a.pdf", "Secret", "Pictures/b.png"]);
    assert!(!ok);
    assert_eq!((out[0]["event"].as_str(), out[0]["kind"].as_str()), (Some("failed"), Some("refused")), "{out:?}");
    let msg = out[0]["message"].as_str().unwrap();
    assert!(msg.starts_with("Nothing was moved") && msg.contains("Secret"), "{msg}");
    assert!(h.join("Downloads/a.pdf").is_file() && h.join("Pictures/b.png").is_file() && h.join("Secret/key.txt").is_file());
    assert!(!h.join("stream/moves.jsonl").exists());
    fs::remove_dir_all(h).unwrap();
}
