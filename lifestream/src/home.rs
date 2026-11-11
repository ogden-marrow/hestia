//! The home repository: the private git repo that holds everything of yours (documents, machines,
//! applications, the folder tree) with Hestia added as a submodule so the tool is tracked by the thing
//! it manages. `create` makes one from nothing; it never touches an existing folder that has other content.

use crate::{domains, machine, tree};
use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Command;

const README: &str = "# My home repository\n\n\
Everything of mine, managed by [Hestia](hestia/): documents, pictures, videos, music, applications, games,\n\
software engineering and data, plus one folder per computer I own.\n\n\
```\n\
domains/        what each part of my computer is for (edit in Hestia, Settings, Domains)\n\
machines/       one fingerprint per computer\n\
applications/   what runs where and how it is set up, by operating system\n\
manifest/       what must never be read, and what is backed up where\n\
tree.toml       where each kind of file is filed\n\
hestia/         the Hestia app itself, as a submodule\n\
```\n\n\
This repository is private. Secrets never go in it.\n";

const MANIFEST: &str = "# What Hestia must never read, and what is backed up where. Paths are relative to $HOME.\n\
# tier \"store\": irreplaceable, goes to the encrypted store. tier \"skip\": regenerable, never backed up.\n\n\
[[rule]]\ntier = \"skip\"\npath = \".cache\"\nnote = \"Caches come back by themselves.\"\n";

const GITIGNORE: &str = "# Secrets: never commit.\n.env\n.env.*\n*.key\n*.pem\n*.p12\nid_*\n*_ed25519\n*_rsa\n*.kdbx\ntarget/\n";

/// Does this folder already look like a home repository?
pub fn is_home(dir: &Path) -> bool {
    dir.join(".git").exists() && (dir.join("tree.toml").is_file() || dir.join("machines").is_dir())
}

fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let o = Command::new("git").arg("-C").arg(dir).args(args).output().context("git is not installed")?;
    if !o.status.success() {
        bail!("git {}: {}", args.join(" "), String::from_utf8_lossy(&o.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// Create the home repository in `dir` (made if missing, must be empty if it exists). Returns what was done.
pub fn create(dir: &Path, hestia_url: Option<&str>, today: &str) -> Result<Vec<String>> {
    if is_home(dir) {
        bail!("{} is already a home repository", dir.display());
    }
    if dir.exists() && std::fs::read_dir(dir)?.next().is_some() {
        bail!("{} is not empty: pick a new folder so nothing of yours is mixed in", dir.display());
    }
    let mut done = Vec::new();
    std::fs::create_dir_all(dir)?;
    git(dir, &["init", "-q", "-b", "main"])?;
    done.push("created the git repository".to_string());

    std::fs::write(dir.join("README.md"), README)?;
    std::fs::write(dir.join(".gitignore"), GITIGNORE)?;
    std::fs::create_dir_all(dir.join("manifest"))?;
    std::fs::write(dir.join("manifest/rules.toml"), MANIFEST)?;
    std::fs::create_dir_all(dir.join("domains"))?;
    std::fs::write(dir.join("domains/README.md"), "Your own domain files go here (`<id>.toml`). They replace or add to the nine stock domains.\n")?;

    let all = domains::defaults();
    let starter = tree::Tree {
        inbox: vec!["Downloads".into()],
        folders: all.iter().filter(|d| !d.folder.is_empty()).map(|d| tree::Folder { path: d.folder.clone(), kind: d.kinds.clone(), ..Default::default() }).collect(),
    };
    starter.save(&dir.join("tree.toml"))?;
    done.push(format!("wrote tree.toml with {} domain folders", starter.folders.len()));

    let m = machine::probe(today);
    machine::write(dir, &m)?;
    std::fs::create_dir_all(dir.join(&m.profile))?;
    std::fs::write(dir.join(&m.profile).join("README.md"), format!("How {} is set up: package lists, settings and dotfiles are captured here.\n", m.name))?;
    done.push(format!("fingerprinted this computer as {}", m.name));

    git(dir, &["add", "-A"])?;
    git(dir, &["commit", "-q", "-m", "Start the home repository"]).context("could not make the first commit: is your git name and email set?")?;
    done.push("made the first commit".to_string());

    if let Some(url) = hestia_url.filter(|u| !u.trim().is_empty()) {
        match git(dir, &["submodule", "add", url.trim(), "hestia"]).and_then(|_| git(dir, &["commit", "-q", "-m", "Track Hestia as a submodule"])) {
            Ok(_) => done.push("added Hestia as a submodule, so the tool is tracked too".to_string()),
            Err(e) => done.push(format!("could not add Hestia as a submodule ({e:#}); you can add it later")),
        }
    } else {
        done.push("Hestia's public repository is not set, so no submodule yet".to_string());
    }
    Ok(done)
}

/// The address of `origin`, if the repository has one.
pub fn remote(dir: &Path) -> Option<String> {
    git(dir, &["remote", "get-url", "origin"]).ok().filter(|u| !u.is_empty())
}

/// The GitHub account the `gh` tool is signed in to, if any.
pub fn github_account() -> Option<String> {
    let o = Command::new("gh").args(["api", "user", "--jq", ".login"]).output().ok()?;
    let login = String::from_utf8_lossy(&o.stdout).trim().to_string();
    (o.status.success() && !login.is_empty()).then_some(login)
}

fn name_ok(s: &str) -> bool {
    !s.is_empty() && s.len() <= 100 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

/// Where the repository will live on GitHub: the address git will use as `origin`.
pub fn remote_url(account: &str, repo: &str) -> String {
    format!("https://github.com/{account}/{repo}.git")
}

/// Create a PRIVATE repository `account/repo` on GitHub, make it `origin` and push the first commit.
/// Only called when the user confirms; fails if the repository already exists or `gh` is not signed in.
pub fn publish(dir: &Path, account: &str, repo: &str) -> Result<String> {
    if !name_ok(account) || !name_ok(repo) {
        bail!("the GitHub account and repository names may only use letters, digits, - _ and .");
    }
    if !is_home(dir) {
        bail!("{} is not a home repository yet", dir.display());
    }
    // Everything in the history is about to become reachable on GitHub: look first.
    let found = crate::secrets::scan_git(dir, crate::secrets::Scope::AllHistory)?;
    if !found.is_empty() {
        let list: Vec<String> = found.iter().take(5).map(|(f, x)| format!("{f} ({})", x.rule)).collect();
        bail!("not published: {} possible secret(s) in the history: {}. Nothing was sent.", found.len(), list.join(", "));
    }
    let full = format!("{account}/{repo}");
    let o = Command::new("gh").args(["repo", "create", &full, "--private", "--source"]).arg(dir).args(["--remote", "origin", "--push"]).output().context("the `gh` tool is not installed")?;
    if !o.status.success() {
        bail!("{}", String::from_utf8_lossy(&o.stderr).trim());
    }
    Ok(remote_url(account, repo))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("hestia-home-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn publish_refuses_a_history_with_a_secret_before_calling_anything() {
        let d = temp("leak");
        unsafe { std::env::set_var("GIT_AUTHOR_NAME", "t"); std::env::set_var("GIT_AUTHOR_EMAIL", "t@t.t"); std::env::set_var("GIT_COMMITTER_NAME", "t"); std::env::set_var("GIT_COMMITTER_EMAIL", "t@t.t"); }
        create(&d, None, "2026-01-01").unwrap();
        std::fs::write(d.join("notes.txt"), format!("GITHUB_TOKEN=gho_{}\n", "aB1cD2eF3g".repeat(4))).unwrap();
        git(&d, &["add", "-A"]).unwrap();
        git(&d, &["commit", "-q", "-m", "oops"]).unwrap();
        let e = publish(&d, "someone", "home").unwrap_err().to_string();
        assert!(e.contains("possible secret") && e.contains("notes.txt"), "{e}");
        assert!(!e.contains("gho_"), "the message must not repeat the secret");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn creates_a_valid_repo_with_domains_machine_and_a_first_commit() {
        let d = temp("new");
        // a throwaway identity so the test does not depend on the machine's git config
        unsafe { std::env::set_var("GIT_AUTHOR_NAME", "t"); std::env::set_var("GIT_AUTHOR_EMAIL", "t@t.t"); std::env::set_var("GIT_COMMITTER_NAME", "t"); std::env::set_var("GIT_COMMITTER_EMAIL", "t@t.t"); }
        let steps = create(&d, None, "2026-01-01").unwrap();
        assert!(is_home(&d));
        assert!(steps.iter().any(|s| s.contains("first commit")));
        let t = tree::Tree::load(&d.join("tree.toml")).unwrap();
        assert!(t.folders.iter().any(|f| f.path == "Pictures" && f.kind.iter().any(|k| k == "image/raw")));
        assert_eq!(machine::list(&d).len(), 1);
        crate::sensitive_paths(&d.join("manifest/rules.toml"), Path::new("/home/x")).expect("the starter manifest parses");
        assert!(git(&d, &["log", "--oneline"]).unwrap().contains("Start the home repository"));
        assert!(create(&d, None, "2026-01-01").unwrap_err().to_string().contains("already"));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn the_remote_address_and_names_are_checked() {
        assert_eq!(remote_url("me", "home"), "https://github.com/me/home.git");
        assert!(name_ok("my-home_1.x") && !name_ok("bad name") && !name_ok("a/b") && !name_ok(""));
        assert!(publish(Path::new("/nonexistent"), "me", "bad name").is_err());
    }

    #[test]
    fn refuses_a_folder_that_already_has_things_in_it() {
        let d = temp("full");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("mine.txt"), "x").unwrap();
        assert!(create(&d, None, "2026-01-01").unwrap_err().to_string().contains("not empty"));
        std::fs::remove_dir_all(&d).unwrap();
    }
}
