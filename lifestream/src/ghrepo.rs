//! Find, adopt and (next) create the person's home repository on GitHub, using the token that `auth` keeps in the
//! system keyring. Same output contract as `auth` (one JSON event per line, never a token; see issue #110).
//!
//! Rules enforced here:
//!  * Only a repository the person OWNS and that is PRIVATE can be a home repository. A public one is refused.
//!  * What is found on GitHub is untrusted: nothing in it runs. (The bootstrap script holds back executable config.)
//!  * Adopting never overwrites: it clones into a temporary sibling folder and moves it into place only if the
//!    destination is free, and it never replaces a different home that is already set.
//!  * git gets the token from the keyring through the credential helper, so it never appears on a command line,
//!    in the environment or in a file.

use crate::auth::{classify, emit, failed, whoami, Endpoints};
use anyhow::{bail, Result};
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

const TOPIC: &str = "hestia-home";
const NAMES: [&str; 3] = ["home", "hestia-home", "Home"];

/// `owner/name`: letters, digits, - _ . only (what GitHub allows), exactly one slash.
pub fn valid_full_name(s: &str) -> bool {
    let ok = |p: &str| !p.is_empty() && p.len() <= 100 && p.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')) && p != "." && p != "..";
    s.split_once('/').is_some_and(|(o, n)| ok(o) && ok(n) && !n.contains('/'))
}

fn get(ep: &Endpoints, token: &str, path: &str) -> Result<Value, ureq::Error> {
    let r = crate::auth::agent()
        .get(&format!("{}{path}", ep.api))
        .set("Authorization", &format!("Bearer {token}"))
        .set("Accept", "application/vnd.github+json")
        .call()?;
    Ok(r.into_json().unwrap_or(Value::Null))
}

fn fail_http(out: &mut dyn Write, e: ureq::Error) {
    let (kind, extra) = classify(e);
    failed(out, kind, extra.as_ref().map(|(k, v)| (*k, v.as_str())));
}

fn other(out: &mut dyn Write, msg: &str) -> bool {
    failed(out, "other", Some(("message", msg)));
    false
}

/// The signed-in person, or a failure event.
fn who(out: &mut dyn Write, ep: &Endpoints, token: &str) -> Option<String> {
    match whoami(ep, token) {
        Ok(Some(u)) => Some(u),
        Ok(None) => {
            other(out, "you are not signed in to GitHub");
            None
        }
        Err(e) => {
            fail_http(out, e);
            None
        }
    }
}

/// Look through the person's own private repositories for a home repository.
pub fn discover(out: &mut dyn Write, token: &str, ep: &Endpoints) -> bool {
    let Some(user) = who(out, ep, token) else { return false };
    let list = match get(ep, token, "/user/repos?affiliation=owner&visibility=private&sort=pushed&per_page=100") {
        Ok(v) => v,
        Err(e) => {
            fail_http(out, e);
            return false;
        }
    };
    let mut found = Vec::new();
    let mut checked = 0;
    for r in list.as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
        let (Some(name), Some(owner)) = (r["name"].as_str(), r["owner"]["login"].as_str()) else { continue };
        if r["private"].as_bool() != Some(true) || !owner.eq_ignore_ascii_case(&user) {
            continue;
        }
        let full = format!("{owner}/{name}");
        let tagged = r["topics"].as_array().is_some_and(|t| t.iter().any(|x| x.as_str() == Some(TOPIC)));
        let named = NAMES.contains(&name);
        let is_home = tagged || (named && checked < 10 && {
            checked += 1;
            get(ep, token, &format!("/repos/{full}/contents/tree.toml")).is_ok()
        });
        if is_home {
            found.push(json!({"name": full, "updated": r["pushed_at"].as_str().unwrap_or("")}));
        }
    }
    if found.is_empty() {
        emit(out, json!({"event": "none"}));
    } else {
        emit(out, json!({"event": "found", "repos": found}));
    }
    true
}

/// What the settings file says: `key=value` lines under ~/.config/hestia/settings.
fn settings_path() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var("HOME").ok()?).join(".config/hestia/settings"))
}

/// Set `home=<dir>` once. Refuses to replace a different home that is already set.
pub fn set_home_setting(file: &Path, dir: &Path) -> Result<()> {
    let want = dir.to_string_lossy().to_string();
    let text = std::fs::read_to_string(file).unwrap_or_default();
    let mut lines: Vec<String> = text.lines().map(String::from).collect();
    match lines.iter().position(|l| l.starts_with("home=")) {
        Some(i) => {
            let cur = lines[i].trim_start_matches("home=").trim();
            if !cur.is_empty() && cur != want {
                bail!("a different home repository is already set ({cur}); switch it in Settings first");
            }
            lines[i] = format!("home={want}");
        }
        None => lines.push(format!("home={want}")),
    }
    if let Some(d) = file.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(file, lines.join("\n") + "\n")?;
    Ok(())
}

/// The credential helper git should use for this repository: ours, which answers from the keyring.
fn helper_for(exe: &Path) -> Result<String> {
    let p = exe.to_string_lossy();
    if p.contains('\'') {
        bail!("the Hestia program path has a quote in it; cannot hand it to git safely");
    }
    Ok(format!("!'{p}' auth git-credential"))
}

fn git_ok(args: &[&str], helper: Option<&str>) -> Result<()> {
    let mut c = Command::new("git");
    if let Some(h) = helper {
        c.args(["-c", "credential.helper=", "-c"]).arg(format!("credential.helper={h}"));
    }
    let o = c.args(args).env("GIT_TERMINAL_PROMPT", "0").output()?;
    if !o.status.success() {
        bail!("git {}: {}", args.first().unwrap_or(&""), String::from_utf8_lossy(&o.stderr).trim());
    }
    Ok(())
}

/// Clone the person's own private home repository into `dest` and make it this computer's home.
/// `git_base` is where git fetches from (https://github.com in production; a local folder in tests).
pub fn adopt(out: &mut dyn Write, token: &str, ep: &Endpoints, git_base: &str, full: &str, dest: &Path, settings: &Path, exe: &Path, today: &str) -> bool {
    if !valid_full_name(full) {
        return other(out, "that is not a repository name (use owner/name)");
    }
    let Some(user) = who(out, ep, token) else { return false };
    let (owner, name) = full.split_once('/').unwrap();
    if !owner.eq_ignore_ascii_case(&user) {
        return other(out, "a home repository must be one of your own");
    }
    match get(ep, token, &format!("/repos/{full}")) {
        Ok(v) if v["private"].as_bool() == Some(true) => {}
        Ok(_) => return other(out, "that repository is public; a home repository must be private"),
        Err(ureq::Error::Status(404, _)) => return other(out, "that repository was not found"),
        Err(e) => {
            fail_http(out, e);
            return false;
        }
    }
    if get(ep, token, &format!("/repos/{full}/contents/tree.toml")).is_err() {
        return other(out, "that repository is not a Hestia home repository (no tree.toml)");
    }
    let helper = match helper_for(exe) {
        Ok(h) => h,
        Err(e) => return other(out, &e.to_string()),
    };
    let url = format!("{git_base}/{full}.git");
    let already = crate::home::is_home(dest) && crate::home::remote(dest).is_some_and(|r| r.trim_end_matches(".git").ends_with(&format!("/{full}")));
    if !already {
        if dest.exists() && std::fs::read_dir(dest).map(|mut d| d.next().is_some()).unwrap_or(true) {
            return other(out, &format!("{} already has files in it; choose an empty place", dest.display()));
        }
        let Some(parent) = dest.parent() else { return other(out, "no place to put it") };
        if std::fs::create_dir_all(parent).is_err() {
            return other(out, "could not create the folder for it");
        }
        // clone beside the destination and move it in only if the destination is still free
        let part = parent.join(format!(".{name}.hestia-part"));
        let _ = std::fs::remove_dir_all(&part); // our own leftover from an earlier attempt, never user data
        if let Err(e) = git_ok(&["clone", "--quiet", &url, &part.to_string_lossy()], Some(&helper)) {
            let _ = std::fs::remove_dir_all(&part);
            let m = e.to_string();
            if m.contains("Could not resolve") || m.contains("unable to access") {
                failed(out, "offline", None);
                return false;
            }
            return other(out, &format!("could not download it: {}", m.replace(token, "***")));
        }
        if !part.join("tree.toml").is_file() {
            let _ = std::fs::remove_dir_all(&part);
            return other(out, "that repository is not a Hestia home repository (no tree.toml)");
        }
        if dest.exists() {
            let _ = std::fs::remove_dir(dest); // an empty folder we were told is free
        }
        if std::fs::rename(&part, dest).is_err() {
            let _ = std::fs::remove_dir_all(&part);
            return other(out, "could not move it into place");
        }
    }
    // pushes from this repository use the keyring too
    let _ = git_ok(&["-C", &dest.to_string_lossy(), "config", "credential.https://github.com.helper", &helper], None);
    if let Err(e) = crate::machine::write(dest, &crate::machine::probe(today)) {
        return other(out, &format!("could not register this computer: {e}"));
    }
    if let Err(e) = set_home_setting(settings, dest) {
        return other(out, &e.to_string());
    }
    emit(out, json!({"event": "cloned", "path": dest.to_string_lossy(), "repo": full}));
    true
}

/// Where the settings file lives for the CLI.
pub fn default_settings() -> Option<PathBuf> {
    settings_path()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    fn tok() -> String {
        format!("{}_{}", "gho", "FAKE0123".repeat(5))
    }

    /// A GitHub that answers GET paths from a table (path -> (status, body)).
    fn fake(routes: Vec<(String, u16, String)>) -> (Endpoints, Arc<Mutex<Vec<String>>>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://127.0.0.1:{}", l.local_addr().unwrap().port());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen2 = seen.clone();
        std::thread::spawn(move || {
            for s in l.incoming() {
                let Ok(mut s) = s else { break };
                let mut data = Vec::new();
                let mut chunk = [0u8; 4096];
                loop {
                    let n = s.read(&mut chunk).unwrap_or(0);
                    if n == 0 { break }
                    data.extend_from_slice(&chunk[..n]);
                    if String::from_utf8_lossy(&data).contains("\r\n\r\n") { break }
                }
                let req = String::from_utf8_lossy(&data).to_string();
                let path = req.split_whitespace().nth(1).unwrap_or("").to_string();
                seen2.lock().unwrap().push(req);
                let (code, body) = routes.iter().find(|(p, _, _)| *p == path).map(|(_, c, b)| (*c, b.clone())).unwrap_or((404, "{}".into()));
                let _ = write!(s, "HTTP/1.1 {code} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            }
        });
        (Endpoints { web: base.clone(), api: base }, seen)
    }

    const LIST: &str = "/user/repos?affiliation=owner&visibility=private&sort=pushed&per_page=100";
    fn run_discover(routes: Vec<(String, u16, String)>) -> (bool, String) {
        let (ep, _) = fake(routes);
        let mut out = Vec::new();
        let ok = discover(&mut out, &tok(), &ep);
        (ok, String::from_utf8(out).unwrap())
    }
    fn me() -> (String, u16, String) {
        ("/user".into(), 200, r#"{"login":"ada"}"#.into())
    }
    fn repo(name: &str, private: bool, owner: &str, topics: &str, pushed: &str) -> String {
        format!(r#"{{"name":"{name}","private":{private},"owner":{{"login":"{owner}"}},"topics":{topics},"pushed_at":"{pushed}"}}"#)
    }

    #[test]
    fn finds_by_topic_and_by_name_with_tree_toml_newest_first_and_ignores_the_rest() {
        let list = format!("[{},{},{},{},{}]",
            repo("my-setup", true, "ada", r#"["hestia-home"]"#, "2026-11-10T12:00:00Z"),
            repo("home", true, "ada", "[]", "2026-11-09T12:00:00Z"),
            repo("hestia-home", true, "ada", "[]", "2026-11-08T12:00:00Z"),     // named right but no tree.toml
            repo("holiday-photos", true, "ada", "[]", "2026-11-07T12:00:00Z"),  // not a home
            repo("home", true, "someone-else", "[]", "2026-11-06T12:00:00Z"));  // not mine
        let (ok, out) = run_discover(vec![me(), (LIST.into(), 200, list), ("/repos/ada/home/contents/tree.toml".into(), 200, "{}".into())]);
        assert!(ok, "{out}");
        assert!(out.contains(r#""event":"found""#) && out.contains("ada/my-setup") && out.contains("ada/home"), "{out}");
        assert!(out.find("ada/my-setup").unwrap() < out.find("ada/home").unwrap(), "newest first: {out}");
        assert!(!out.contains("hestia-home\"") && !out.contains("holiday") && !out.contains("someone-else"), "{out}");
        assert!(!out.contains(&tok()) && !out.contains("gho_"));
    }

    #[test]
    fn none_when_nothing_qualifies_and_public_repos_never_count() {
        let list = format!("[{},{}]", repo("home", false, "ada", r#"["hestia-home"]"#, "2026-11-10T12:00:00Z"), repo("stuff", true, "ada", "[]", "2026-11-09T12:00:00Z"));
        let (ok, out) = run_discover(vec![me(), (LIST.into(), 200, list)]);
        assert!(ok && out.trim() == r#"{"event":"none"}"#, "{out}");
    }

    #[test]
    fn signed_out_and_offline_are_reported() {
        let (ok, out) = run_discover(vec![("/user".into(), 401, "{}".into())]);
        assert!(!ok && out.contains("not signed in"), "{out}");
        let dead = Endpoints { web: "http://127.0.0.1:9".into(), api: "http://127.0.0.1:9".into() };
        let mut o = Vec::new();
        assert!(!discover(&mut o, &tok(), &dead));
        assert!(String::from_utf8(o).unwrap().contains(r#""kind":"offline""#));
    }

    #[test]
    fn names_are_validated() {
        for ok in ["ada/home", "a-b/c_d.e"] { assert!(valid_full_name(ok), "{ok}"); }
        for bad in ["", "home", "ada/", "/home", "ada/home/x", "ada/../x", "a b/c", "ada/ho;me", "ada/.."] { assert!(!valid_full_name(bad), "{bad}"); }
    }

    #[test]
    fn the_home_setting_is_written_once_and_never_replaced_by_a_different_one() {
        let d = std::env::temp_dir().join(format!("ls-ghrepo-set-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let f = d.join("settings");
        set_home_setting(&f, Path::new("/home/x/Home")).unwrap();
        std::fs::write(&f, "scheme=dark\nhome=/home/x/Home\nwatch=on\n").unwrap();
        set_home_setting(&f, Path::new("/home/x/Home")).unwrap(); // same: fine
        assert!(set_home_setting(&f, Path::new("/home/x/Other")).unwrap_err().to_string().contains("already set"));
        let t = std::fs::read_to_string(&f).unwrap();
        assert!(t.contains("scheme=dark") && t.contains("watch=on") && t.matches("home=").count() == 1, "{t}");
        std::fs::remove_dir_all(&d).unwrap();
    }

    fn git(dir: &Path, a: &[&str]) {
        let o = Command::new("git").arg("-C").arg(dir).args(a).env("GIT_AUTHOR_NAME", "t").env("GIT_AUTHOR_EMAIL", "t@t.t").env("GIT_COMMITTER_NAME", "t").env("GIT_COMMITTER_EMAIL", "t@t.t").output().unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    }

    /// A pretend GitHub's git side: <base>/ada/home.git (a bare repo holding a home repository).
    fn git_side(tag: &str, with_tree: bool) -> (PathBuf, String) {
        let root = std::env::temp_dir().join(format!("ls-ghrepo-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let work = root.join("work");
        std::fs::create_dir_all(&work).unwrap();
        git(&work, &["init", "-q", "-b", "main"]);
        if with_tree { std::fs::write(work.join("tree.toml"), "").unwrap(); } else { std::fs::write(work.join("readme"), "x").unwrap(); }
        git(&work, &["add", "-A"]);
        git(&work, &["commit", "-q", "-m", "init"]);
        let bare = root.join("git/ada/home.git");
        std::fs::create_dir_all(bare.parent().unwrap()).unwrap();
        git(&root, &["clone", "-q", "--bare", work.to_str().unwrap(), bare.to_str().unwrap()]);
        (root.clone(), format!("file://{}", root.join("git").display()))
    }

    fn adopt_routes(private: bool, tree: bool) -> Vec<(String, u16, String)> {
        vec![me(), ("/repos/ada/home".into(), 200, format!(r#"{{"private":{private}}}"#)), ("/repos/ada/home/contents/tree.toml".into(), if tree { 200 } else { 404 }, "{}".into())]
    }

    #[test]
    fn adopt_clones_registers_this_computer_sets_home_once_and_never_prints_the_token() {
        let (root, base) = git_side("ok", true);
        let (ep, _) = fake(adopt_routes(true, true));
        let dest = root.join("Home");
        let settings = root.join("cfg/settings");
        let mut out = Vec::new();
        let ok = adopt(&mut out, &tok(), &ep, &base, "ada/home", &dest, &settings, Path::new("/usr/bin/true"), "2026-11-11");
        let o = String::from_utf8(out).unwrap();
        assert!(ok, "{o}");
        assert!(o.contains(r#""event":"cloned""#) && o.contains("ada/home") && !o.contains("gho_"), "{o}");
        assert!(dest.join("tree.toml").is_file() && crate::home::is_home(&dest));
        assert_eq!(crate::machine::list(&dest).len(), 1, "this computer is registered");
        assert!(std::fs::read_to_string(&settings).unwrap().contains(&format!("home={}", dest.display())));
        // pushes from here use the keyring helper
        let c = Command::new("git").arg("-C").arg(&dest).args(["config", "--get", "credential.https://github.com.helper"]).output().unwrap();
        assert!(String::from_utf8_lossy(&c.stdout).contains("auth git-credential"));
        // doing it again is fine and changes nothing
        let mut out2 = Vec::new();
        assert!(adopt(&mut out2, &tok(), &ep, &base, "ada/home", &dest, &settings, Path::new("/usr/bin/true"), "2026-11-11"), "{}", String::from_utf8_lossy(&out2));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn adopt_refuses_public_foreign_missing_nonhome_and_busy_destinations() {
        let (root, base) = git_side("no", true);
        let settings = root.join("cfg/settings");
        let run = |routes: Vec<(String, u16, String)>, full: &str, dest: &Path| {
            let (ep, _) = fake(routes);
            let mut out = Vec::new();
            let ok = adopt(&mut out, &tok(), &ep, &base, full, dest, &settings, Path::new("/usr/bin/true"), "2026-11-11");
            (ok, String::from_utf8(out).unwrap())
        };
        let dest = root.join("Home");
        let (ok, o) = run(adopt_routes(false, true), "ada/home", &dest);
        assert!(!ok && o.contains("public") && !dest.exists(), "{o}");
        let (ok, o) = run(adopt_routes(true, true), "someone/home", &dest);
        assert!(!ok && o.contains("one of your own") && !dest.exists(), "{o}");
        let (ok, o) = run(adopt_routes(true, false), "ada/home", &dest);
        assert!(!ok && o.contains("tree.toml") && !dest.exists(), "{o}");
        let (ok, o) = run(adopt_routes(true, true), "not a name", &dest);
        assert!(!ok && o.contains("owner/name"), "{o}");
        // a folder that already has files is never touched
        std::fs::create_dir_all(&dest).unwrap();
        std::fs::write(dest.join("mine.txt"), "keep me").unwrap();
        let (ok, o) = run(adopt_routes(true, true), "ada/home", &dest);
        assert!(!ok && o.contains("already has files"), "{o}");
        assert_eq!(std::fs::read_to_string(dest.join("mine.txt")).unwrap(), "keep me");
        // a clone that turns out not to be a home repository is not left behind
        let (root2, base2) = git_side("nothome", false);
        let (ep, _) = fake(adopt_routes(true, true));
        let dest2 = root2.join("Home");
        let mut out = Vec::new();
        assert!(!adopt(&mut out, &tok(), &ep, &base2, "ada/home", &dest2, &root2.join("s"), Path::new("/usr/bin/true"), "2026-11-11"));
        assert!(!dest2.exists() && !root2.join(".home.hestia-part").exists());
        std::fs::remove_dir_all(&root).unwrap();
        std::fs::remove_dir_all(&root2).unwrap();
    }
}
