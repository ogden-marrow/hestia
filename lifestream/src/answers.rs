//! The answers log: `<stream>/answers.jsonl`, append-only, one JSON object per line. It is how the Stream learns that
//! Hestia asked about a folder and how the question was answered. Two writers, each the one place that does the thing:
//! `bundle::run` (the models look at a new folder: a question is asked) and `crate::answer` behind `teach` (an answer is
//! recorded). The feed (`feed.rs`) is the one reader. Lines:
//!
//! ```text
//! {"at":RFC3339,"kind":"asked","id":"q-…","actor":"model","folder":"Downloads/App","choice":null,"batch":null}
//! {"at":RFC3339,"kind":"answered","id":"q-…","actor":"you","folder":"Downloads/2024","choice":"finance/taxes","batch":"<journal batch>"|null}
//! ```
//!
//! `folder` is relative to the home folder ("" is the home folder itself) and only ever a plain, visible path
//! (`tree::rel_ok`); a folder that is not is never written. `choice` is the kind the person picked, in the characters
//! `teach` accepts. `batch` is the journal batch (`moves.jsonl`) when the answer moved files, so the feed can offer Undo.
//! No file names, contents or secrets: only the folder the question is about and the label.

use crate::tree;
use anyhow::Result;
use chrono::{SecondsFormat, Utc};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::path::Path;

pub const FILE: &str = "answers.jsonl";

/// The id of the question about a folder (`~/Downloads/App`). Stable, so an answer carries the id of the question it answers.
pub fn question_id(root: &str) -> String {
    format!("q-{}", &format!("{:x}", Sha256::digest(root.as_bytes()))[..16])
}

/// `~/Downloads/2024` -> `Downloads/2024`, `~` -> `` (the home folder itself); `None` for anything that is not a plain,
/// visible path inside the home folder.
pub(crate) fn folder_of(root: &str) -> Option<String> {
    if root == "~" {
        return Some(String::new());
    }
    let rel = root.strip_prefix("~/")?;
    tree::rel_ok(rel).then(|| rel.to_string())
}

fn append(stream: &Path, lines: &[Value]) -> Result<()> {
    if lines.is_empty() {
        return Ok(());
    }
    let body: String = lines.iter().map(|l| format!("{l}\n")).collect();
    fs::create_dir_all(stream)?;
    fs::OpenOptions::new().create(true).append(true).open(stream.join(FILE))?.write_all(body.as_bytes())?;
    Ok(())
}

fn lines(roots: &[String], kind: &str, actor: &str, choice: Option<&str>, batch: Option<&str>) -> Vec<Value> {
    let at = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
    let mut seen = std::collections::BTreeSet::new();
    roots
        .iter()
        .filter(|r| seen.insert(r.as_str()))
        .filter_map(|r| Some((question_id(r), folder_of(r)?)))
        .map(|(id, folder)| json!({"at": at, "kind": kind, "id": id, "actor": actor, "folder": folder, "choice": choice, "batch": batch}))
        .collect()
}

/// Questions about these folders (`~/…`, as `bundle` names them) were just asked; one `at` for the whole set.
pub(crate) fn asked(stream: &Path, roots: &[String]) -> Result<()> {
    append(stream, &lines(roots, "asked", "model", None, None))
}

/// The questions about these folders were answered with `choice`, by `actor` (`you` or `model`); `batch` when files moved.
pub(crate) fn answered(stream: &Path, roots: &[String], choice: &str, actor: &str, batch: Option<&str>) -> Result<()> {
    append(stream, &lines(roots, "answered", actor, Some(choice), batch))
}
