//! Updating Hestia from where it was installed (#265). The app's Settings > About calls these; nothing here runs by itself.
//!
//! Layout made by `install.sh`: every build is its own folder, `<prefix>/lib/hestia/builds/<date>-<build id>/` (bin/ and libexec/),
//! `current` and `previous` are links to two of them, and the installed programs (`<prefix>/bin/*`) and scripts
//! (`<prefix>/libexec/hestia`) are links through `current`, so the systemd units never name a build. Switching is one atomic rename
//! of a link; exactly one older build is kept, for rollback. `<share>/hestia/install.json` says what is installed and where it came from.
//!
//! - `check`: one JSON line, is there a newer Hestia? For a checkout install, a `git fetch` of the public repository (https, no
//!   credentials, 20 s), never touching the checkout's working tree.
//! - `apply`: JSON-lines progress; refuses while a backup or restore check runs or when the checkout has changes of its own;
//!   fast-forwards the checkout, then `install.sh --stage`, `--smoke`, `--activate` (the one installer does the work). The running
//!   build stays installed unless every step passed.
//! - `rollback`: back to the kept previous build.
//! - `switch` (used by `install.sh --activate`): the one place that moves the links, prunes and writes install.json.
use crate::share_dir;
use anyhow::{bail, Context, Result};
use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The only repository Hestia updates from: the public one, over https.
pub const PUBLIC_ORIGINS: &[&str] = &["https://github.com/ogden-marrow/hestia.git", "https://github.com/ogden-marrow/hestia"];
/// Tests point the checkout at a local bare repository; this replaces the public address. Used only by tests/update.sh.
const ORIGIN_ENV: &str = "HESTIA_UPDATE_ORIGIN";
const FETCH_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_CHANGES: usize = 10;
const PROGRAMS: [&str; 3] = ["hestia", "hestia-tray", "lifestream"];

const NOT_INSTALLED: &str = "Hestia was not installed with install.sh, so it does not know where updates come from.";
const OFFLINE: &str = "Could not reach the Hestia repository to look for updates. Check the internet connection and try again later.";
const NOT_PUBLIC: &str = "The folder Hestia was installed from does not point at the public Hestia repository, so Hestia will not update from it.";
const GONE: &str = "The folder Hestia was installed from is missing or is not a git checkout any more.";
const RELEASE: &str = "This copy of Hestia came from a download. Updating it from here comes with signed releases; until then, install the new release by hand.";

/// What `install.sh` recorded about the installed build (`<share>/hestia/install.json`).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Install {
    pub build_id: String,
    /// `checkout` (built from a git checkout) or `release` (a downloaded tarball)
    pub source: String,
    pub source_dir: PathBuf,
    #[serde(default)]
    pub installed_at: String,
    #[serde(default)]
    pub prefix: PathBuf,
}

pub fn install_json() -> Option<PathBuf> {
    share_dir().map(|s| s.join("hestia/install.json"))
}

pub fn read_install() -> Option<Install> {
    serde_json::from_str(&fs::read_to_string(install_json()?).ok()?).ok()
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

// ---------------------------------------------------------------- git, quietly and never asking for anything

fn git_cmd(dir: &Path) -> Command {
    let mut c = Command::new("git");
    c.arg("-C").arg(dir).env("GIT_TERMINAL_PROMPT", "0").env_remove("GIT_ASKPASS").env_remove("SSH_ASKPASS").stdin(Stdio::null());
    c
}

/// stdout of a git command that must work, trimmed; Err carries git's first complaint (for the log, never shown as is).
fn git(dir: &Path, args: &[&str]) -> std::result::Result<String, String> {
    match git_cmd(dir).args(args).output() {
        Ok(o) if o.status.success() => Ok(String::from_utf8_lossy(&o.stdout).trim().to_string()),
        Ok(o) => Err(String::from_utf8_lossy(&o.stderr).lines().next().unwrap_or("git failed").to_string()),
        Err(e) => Err(e.to_string()),
    }
}

/// Runs a command, killing it (and what it started) after `limit`. Ok(true) if it exited 0.
fn run_with_timeout(mut c: Command, limit: Duration) -> std::io::Result<bool> {
    use std::os::unix::process::CommandExt;
    let mut child = c.stdout(Stdio::null()).stderr(Stdio::null()).process_group(0).spawn()?;
    let start = Instant::now();
    loop {
        if let Some(s) = child.try_wait()? {
            return Ok(s.success());
        }
        if start.elapsed() > limit {
            let _ = Command::new("kill").arg("-KILL").arg(format!("-{}", child.id())).status();
            let _ = child.kill();
            let _ = child.wait();
            return Ok(false);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn allowed_origins() -> Vec<String> {
    match std::env::var(ORIGIN_ENV) {
        Ok(v) if !v.is_empty() => vec![v],
        _ => PUBLIC_ORIGINS.iter().map(|s| s.to_string()).collect(),
    }
}

/// The checkout must be a git work tree whose origin (as git will really fetch it, after insteadOf) is the public repository.
fn verify_source(dir: &Path) -> std::result::Result<(), &'static str> {
    if !dir.is_dir() || git(dir, &["rev-parse", "--is-inside-work-tree"]).as_deref() != Ok("true") {
        return Err(GONE);
    }
    let url = git(dir, &["remote", "get-url", "origin"]).map_err(|_| NOT_PUBLIC)?;
    if allowed_origins().contains(&url) {
        Ok(())
    } else {
        Err(NOT_PUBLIC)
    }
}

/// `git fetch origin main` of the public repository: https only (a local path for tests), no credential helper, no prompt, no
/// redirects, no tags, at most 20 seconds. Writes only inside .git (origin/main), never the working tree.
fn fetch(dir: &Path) -> std::result::Result<(), &'static str> {
    let mut c = git_cmd(dir);
    c.args([
        "-c", "credential.helper=", "-c", "core.askPass=", "-c", "http.followRedirects=false",
        "-c", "protocol.allow=never", "-c", "protocol.https.allow=always", "-c", "protocol.file.allow=always",
        "fetch", "--quiet", "--no-tags", "--no-recurse-submodules", "origin", "+refs/heads/main:refs/remotes/origin/main",
    ]);
    match run_with_timeout(c, FETCH_TIMEOUT) {
        Ok(true) => Ok(()),
        _ => Err(OFFLINE),
    }
}

fn short(dir: &Path, rev: &str) -> Option<String> {
    git(dir, &["rev-parse", "--short=8", "--verify", "--quiet", &format!("{rev}^{{commit}}")]).ok()
}

/// The commit part of a build id (`abc12345+local` was built from abc12345 with changes on top).
fn commit_of(build_id: &str) -> &str {
    build_id.split('+').next().unwrap_or(build_id)
}

/// Is there something on origin/main that the installed build does not have?
fn is_newer(dir: &Path, installed: &str) -> bool {
    // origin/main already inside the installed build (the same commit, or local commits on top): nothing newer
    !git_cmd(dir).args(["merge-base", "--is-ancestor", "origin/main", commit_of(installed)]).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
}

/// Up to ten plain one-line entries added to CHANGELOG.md between two commits. Empty if either is unknown here.
fn changes(dir: &Path, from: &str, to: &str) -> Vec<String> {
    let Ok(diff) = git(dir, &["diff", "--no-color", "--no-ext-diff", "-U0", commit_of(from), to, "--", "CHANGELOG.md"]) else { return vec![] };
    changelog_entries(&diff)
}

/// The top-level list entries a diff of CHANGELOG.md adds, made plain (see `plain_entry`).
pub(crate) fn changelog_entries(diff: &str) -> Vec<String> {
    diff.lines().filter_map(|l| l.strip_prefix("+- ")).map(plain_entry).filter(|e| !e.is_empty()).take(MAX_CHANGES).collect()
}

/// One changelog entry as one plain line: no markdown, no issue numbers or commit hashes, the first sentence, at most 160 characters.
pub(crate) fn plain_entry(s: &str) -> String {
    let s = s.replace("**", "").replace('`', "");
    let refs = regex::Regex::new(r"\s*\(#\d+[^)]*\)").expect("valid");
    let s = refs.replace_all(&s, "");
    let words: Vec<&str> = s
        .split_whitespace()
        .filter(|w| {
            let t = w.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '#');
            let hash = (7..=40).contains(&t.len()) && t.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()) && t.chars().any(|c| c.is_ascii_digit());
            let issue = t.len() > 1 && t.starts_with('#') && t[1..].chars().all(|c| c.is_ascii_digit());
            !hash && !issue
        })
        .collect();
    let s = words.join(" ");
    let s = match s.find(". ") {
        Some(i) => &s[..=i],
        None => s.as_str(),
    };
    let s = s.trim().trim_end_matches(':').trim();
    if s.chars().count() > 160 {
        format!("{}…", s.chars().take(159).collect::<String>().trim_end())
    } else {
        s.to_string()
    }
}

// ---------------------------------------------------------------- check

/// `lifestream update check`: one JSON object (see lifestream/README.md for the contract). Never fails: problems are `error`.
pub fn check(source: Option<PathBuf>) -> Value {
    let inst = read_install();
    let installed = inst.as_ref().map(|i| i.build_id.clone());
    let kind = if source.is_some() { "checkout".to_string() } else { inst.as_ref().map(|i| i.source.clone()).unwrap_or_else(|| "checkout".into()) };
    let out = |available: Option<String>, newer: bool, changes: Vec<String>, error: Option<&str>| {
        json!({"installed": installed, "available": available, "newer": newer, "changes": changes, "source": kind, "checked_at": now(), "error": error})
    };
    if kind == "release" {
        // TODO(#244): signed releases. Ask the release feed for the newest signed release, verify its signature, and report it here.
        return out(release_available(), false, vec![], Some(RELEASE));
    }
    let Some(dir) = source.or_else(|| inst.as_ref().map(|i| i.source_dir.clone())) else { return out(None, false, vec![], Some(NOT_INSTALLED)) };
    if let Err(e) = verify_source(&dir).and_then(|_| fetch(&dir)) {
        return out(None, false, vec![], Some(e));
    }
    let Some(available) = short(&dir, "origin/main") else { return out(None, false, vec![], Some(OFFLINE)) };
    let newer = installed.as_deref().is_some_and(|i| is_newer(&dir, i));
    let ch = match (&installed, newer) {
        (Some(i), true) => changes(&dir, i, "origin/main"),
        _ => vec![],
    };
    out(Some(available), newer, ch, None)
}

/// The hook for #244: the newest signed release, once there are signed releases. None until then.
fn release_available() -> Option<String> {
    None
}

// ---------------------------------------------------------------- locks

/// The same one-at-a-time locks the watcher (watch.sh) and the restore check (rehearse.sh) take with flock(1). Held until dropped.
fn take_locks() -> std::result::Result<Vec<File>, &'static str> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").filter(|d| !d.is_empty()).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp"));
    let mut held = vec![];
    for (name, busy) in [
        ("hestia-update.lock", "Hestia is already being updated."),
        ("hestia-watch.lock", "A backup is running right now, so Hestia was not changed. Try again in a few minutes."),
        ("hestia-rehearse.lock", "A restore check is running right now, so Hestia was not changed. Try again in a few minutes."),
    ] {
        let f = OpenOptions::new().create(true).truncate(false).write(true).open(dir.join(name)).map_err(|_| "Hestia could not check whether a backup is running, so it was not changed.")?;
        match f.try_lock() {
            Ok(()) => held.push(f),
            Err(TryLockError::WouldBlock) => return Err(busy),
            Err(TryLockError::Error(_)) => return Err("Hestia could not check whether a backup is running, so it was not changed."),
        }
    }
    Ok(held)
}

// ---------------------------------------------------------------- apply and rollback

fn emit(out: &mut impl Write, v: Value) {
    let _ = writeln!(out, "{v}");
    let _ = out.flush();
}

fn step(out: &mut impl Write, step: &str, pct: u32) {
    emit(out, json!({"event": "step", "step": step, "pct": pct}));
}

fn failed(out: &mut impl Write, message: &str) -> bool {
    emit(out, json!({"event": "failed", "message": message}));
    false
}

fn builds_dir(prefix: &Path) -> PathBuf {
    prefix.join("lib/hestia/builds")
}

/// A build folder this install made: a direct child of `<prefix>/lib/hestia/builds` with programs in it.
fn own_build(prefix: &Path, dir: &Path) -> bool {
    dir.parent() == Some(builds_dir(prefix).as_path()) && dir.file_name().is_some_and(|n| !n.to_string_lossy().starts_with('.')) && dir.join("bin/lifestream").is_file()
}

fn build_info(dir: &Path) -> Result<Install> {
    Ok(serde_json::from_str(&fs::read_to_string(dir.join("build-info.json")).context("the build has no build-info.json")?)?)
}

fn log_path() -> Option<PathBuf> {
    share_dir().map(|s| s.join("hestia/update.log"))
}

/// Runs `bash <source>/install.sh <args>` with the installed prefix. Its stderr (cargo's output) goes to update.log; each
/// "Compiling" line moves the build step's percentage. Returns its stdout when it exits 0.
fn install_sh(out: &mut impl Write, src: &Path, prefix: &Path, args: &[&str], progress: Option<(u32, u32, usize)>) -> Option<String> {
    let log = log_path().and_then(|p| {
        let _ = fs::create_dir_all(p.parent()?);
        OpenOptions::new().create(true).append(true).open(p).ok()
    });
    let mut child = Command::new("bash")
        .arg(src.join("install.sh"))
        .args(args)
        .env("PREFIX", prefix)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stdout.read_to_string(&mut s);
        s
    });
    let (mut seen, mut last) = (0usize, 0u32);
    for line in BufReader::new(child.stderr.take()?).lines().map_while(|l| l.ok()) {
        if let Some(mut f) = log.as_ref() {
            let _ = writeln!(f, "{line}");
        }
        if let (Some((lo, hi, total)), true) = (progress, line.trim_start().starts_with("Compiling ")) {
            seen += 1;
            let pct = lo + ((hi - lo) as usize * seen.min(total) / total.max(1)) as u32;
            if pct > last {
                last = pct;
                step(out, "build", pct);
            }
        }
    }
    let ok = child.wait().is_ok_and(|s| s.success());
    let stdout = reader.join().unwrap_or_default();
    ok.then_some(stdout)
}

/// `lifestream update apply`: JSON lines on `out`; true when the new build is installed.
pub fn apply(out: &mut impl Write) -> bool {
    let Some(inst) = read_install() else { return failed(out, NOT_INSTALLED) };
    if inst.source != "checkout" {
        // TODO(#244): download, verify and stage a signed release here, then the same smoke and switch.
        return failed(out, RELEASE);
    }
    let _locks = match take_locks() {
        Ok(l) => l,
        Err(m) => return failed(out, m),
    };
    let (src, prefix) = (inst.source_dir.clone(), inst.prefix.clone());
    if let Err(m) = verify_source(&src) {
        return failed(out, m);
    }
    if git(&src, &["symbolic-ref", "--short", "-q", "HEAD"]).as_deref() != Ok("main") {
        return failed(out, "The folder Hestia was installed from is not on the main branch, so it was not updated.");
    }
    match git(&src, &["status", "--porcelain", "--untracked-files=no"]) {
        Ok(s) if s.is_empty() => {}
        _ => return failed(out, "The folder Hestia was installed from has changes of its own, so it was not updated. Nothing was changed."),
    }
    step(out, "fetch", 5);
    if let Err(m) = fetch(&src) {
        return failed(out, m);
    }
    // fast-forward only: never a reset, never a merge commit, never a force
    if git(&src, &["merge", "--ff-only", "--quiet", "refs/remotes/origin/main"]).is_err() {
        return failed(out, "The folder Hestia was installed from has commits of its own that are not on the public repository, so it was not updated.");
    }
    if short(&src, "HEAD").as_deref() == Some(inst.build_id.as_str()) {
        return failed(out, "Hestia is already up to date; there was nothing to install.");
    }
    step(out, "build", 15);
    let total = fs::read_to_string(src.join("Cargo.lock")).map(|s| s.matches("[[package]]").count()).unwrap_or(400);
    let Some(staged) = install_sh(out, &src, &prefix, &["--stage"], Some((15, 75, total))) else {
        return failed(out, "The new version did not build, so Hestia kept the version you have. The details are in update.log.");
    };
    let dir = PathBuf::from(staged.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").trim());
    if !own_build(&prefix, &dir) {
        return failed(out, "The new version did not build, so Hestia kept the version you have. The details are in update.log.");
    }
    step(out, "test", 80);
    let d = dir.to_string_lossy().to_string();
    if install_sh(out, &src, &prefix, &["--smoke", &d], None).is_none() {
        discard(&prefix, &dir);
        return failed(out, "The new version did not start properly, so Hestia kept the version you have.");
    }
    step(out, "switch", 90);
    if install_sh(out, &src, &prefix, &["--activate", &d], None).is_none() || current(&prefix).as_deref() != Some(dir.as_path()) {
        return failed(out, "The new version could not be switched on, so Hestia kept the version you have. The details are in update.log.");
    }
    let to = build_info(&dir).map(|b| b.build_id).unwrap_or_default();
    emit(out, json!({"event": "updated", "from": inst.build_id, "to": to}));
    true
}

/// `lifestream update rollback`: back to the kept previous build (the build it replaces becomes the kept one).
pub fn rollback(out: &mut impl Write) -> bool {
    let Some(inst) = read_install() else { return failed(out, NOT_INSTALLED) };
    let _locks = match take_locks() {
        Ok(l) => l,
        Err(m) => return failed(out, m),
    };
    let root = inst.prefix.join("lib/hestia");
    let Some(prev) = fs::read_link(root.join("previous")).ok().map(|p| root.join(p)).filter(|p| own_build(&inst.prefix, p)) else {
        return failed(out, "There is no earlier version to go back to.");
    };
    step(out, "switch", 50);
    if let Err(e) = switch(&inst.prefix, &prev) {
        if let Some(mut f) = log_path().and_then(|p| OpenOptions::new().create(true).append(true).open(p).ok()) {
            let _ = writeln!(f, "rollback: {e:#}");
        }
        return failed(out, "Hestia could not go back to the earlier version. The details are in update.log.");
    }
    let to = build_info(&prev).map(|b| b.build_id).unwrap_or_default();
    emit(out, json!({"event": "rolled_back", "from": inst.build_id, "to": to}));
    true
}

/// The build `current` points at.
fn current(prefix: &Path) -> Option<PathBuf> {
    let root = prefix.join("lib/hestia");
    fs::read_link(root.join("current")).ok().map(|p| root.join(p))
}

/// Removes a staged build that never became current (it failed its check). Only ever inside our own builds folder.
fn discard(prefix: &Path, dir: &Path) {
    let root = prefix.join("lib/hestia");
    let kept = ["current", "previous"].map(|l| fs::read_link(root.join(l)).ok().map(|p| root.join(p)));
    if dir.parent() == Some(builds_dir(prefix).as_path()) && !kept.iter().any(|k| k.as_deref() == Some(dir)) {
        let _ = fs::remove_dir_all(dir);
    }
}

/// Points `link` at `target` in one atomic step (a new link renamed over the old one).
fn replace_link(link: &Path, target: &Path) -> Result<()> {
    let tmp = link.with_file_name(format!(".{}.new", link.file_name().map(|n| n.to_string_lossy()).unwrap_or_default()));
    let _ = fs::remove_file(&tmp);
    std::os::unix::fs::symlink(target, &tmp).with_context(|| format!("could not make the link {}", tmp.display()))?;
    fs::rename(&tmp, link).with_context(|| format!("could not switch {}", link.display()))?;
    Ok(())
}

/// Makes `dir` the installed build: `current` -> dir and `previous` -> what was current (a rollback swaps the two), the stable
/// program and script links, every other build folder removed, install.json written, then the user units refreshed by the
/// build's own setup-user.sh (a timer restarts only when its unit file changed). The one place links move.
pub fn switch(prefix: &Path, dir: &Path) -> Result<()> {
    let dir = if dir.is_absolute() { dir.to_path_buf() } else { std::env::current_dir()?.join(dir) };
    if !own_build(prefix, &dir) {
        bail!("{} is not a build folder under {}", dir.display(), builds_dir(prefix).display());
    }
    let info = build_info(&dir)?;
    let root = prefix.join("lib/hestia");
    let name = dir.file_name().context("no name")?;
    let new_rel = PathBuf::from("builds").join(name);
    let old = fs::read_link(root.join("current")).ok();
    if old.as_deref() != Some(new_rel.as_path()) {
        replace_link(&root.join("current"), &new_rel)?;
        match old.filter(|o| root.join(o).is_dir()) {
            Some(o) => replace_link(&root.join("previous"), &o)?,
            None => {
                let _ = fs::remove_file(root.join("previous"));
            }
        }
    }
    fs::create_dir_all(prefix.join("bin"))?;
    for p in PROGRAMS {
        replace_link(&prefix.join("bin").join(p), &Path::new("../lib/hestia/current/bin").join(p))?;
    }
    let libexec = prefix.join("libexec/hestia");
    fs::create_dir_all(prefix.join("libexec"))?;
    if fs::symlink_metadata(&libexec).is_ok_and(|m| m.is_dir()) {
        fs::remove_dir_all(&libexec)?; // Hestia's own scripts, copied by an install from before builds had folders
    }
    replace_link(&libexec, Path::new("../lib/hestia/current/libexec"))?;
    // exactly one older build is kept
    let kept: Vec<PathBuf> = ["current", "previous"].iter().filter_map(|l| fs::read_link(root.join(l)).ok()).collect();
    for e in fs::read_dir(builds_dir(prefix))?.flatten() {
        if !kept.contains(&PathBuf::from("builds").join(e.file_name())) {
            let p = e.path();
            if fs::symlink_metadata(&p).is_ok_and(|m| m.is_dir()) { fs::remove_dir_all(&p)? } else { fs::remove_file(&p)? }
        }
    }
    let rec = Install { installed_at: now(), prefix: prefix.to_path_buf(), ..info };
    let path = install_json().context("no data folder (HOME is not set)")?;
    fs::create_dir_all(path.parent().context("no parent")?)?;
    let tmp = path.with_extension("json.new");
    fs::write(&tmp, format!("{}\n", serde_json::to_string(&rec)?))?;
    fs::rename(&tmp, &path)?;
    let ok = Command::new("bash").arg(libexec.join("setup-user.sh")).stdin(Stdio::null()).stdout(Stdio::null()).status().is_ok_and(|s| s.success());
    if !ok {
        bail!("setup-user.sh failed");
    }
    // the tray keeps running the program it started with: start the new one (only if it was running)
    let _ = Command::new("systemctl").args(["--user", "try-restart", "hestia-tray.service"]).stdout(Stdio::null()).stderr(Stdio::null()).status();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changelog_entries_are_plain_lines() {
        let diff = "\
diff --git a/CHANGELOG.md b/CHANGELOG.md
+++ b/CHANGELOG.md
@@ -7,0 +8,3 @@
+- **A weekly restore rehearsal** (#226): a backup nobody has restored is a hope. Once a week Hestia copies things.
+  a continuation line that is not an entry
+- **Fixed in `watch.sh`** after 1a2b3c4d5e and #12: the watcher now waits. More words.
-- **An old entry**: removed lines are not changes.
";
        assert_eq!(changelog_entries(diff), vec!["A weekly restore rehearsal: a backup nobody has restored is a hope.", "Fixed in watch.sh after and the watcher now waits."]);
    }

    #[test]
    fn a_long_entry_is_cut_and_at_most_ten_are_kept() {
        let long = format!("- {}", "word ".repeat(80));
        assert!(plain_entry(&long).chars().count() <= 160);
        assert!(plain_entry(&long).ends_with('…'));
        let diff: String = (0..15).map(|i| format!("+- entry {i}\n")).collect();
        assert_eq!(changelog_entries(&diff).len(), 10);
    }

    #[test]
    fn words_that_only_look_like_hashes_stay() {
        assert_eq!(plain_entry("**Faded** colours were effaced in 2026"), "Faded colours were effaced in 2026");
    }

    #[test]
    fn build_ids_name_their_commit() {
        assert_eq!(commit_of("abc12345+local"), "abc12345");
        assert_eq!(commit_of("abc12345"), "abc12345");
    }
}
