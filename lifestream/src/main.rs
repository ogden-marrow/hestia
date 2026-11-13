//! lifestream command line and interactive screen.

use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use lifestream::*;
use std::path::PathBuf;

#[derive(Parser)]
#[command(version, about = "lifestream: read-only ingest, search and briefing (the app is Hestia)")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Scan folders and update the stream. Read-only with respect to the folders.
    Scan {
        /// Folders to scan
        #[arg(required = true)]
        roots: Vec<PathBuf>,
        /// Where the stream is written (default: ~/.local/share/lifestream/stream)
        #[arg(long)]
        out: Option<PathBuf>,
        /// Rules file (default: built-in rules.toml)
        #[arg(long)]
        rules: Option<PathBuf>,
        /// home-repo manifest/rules.toml listing sensitive paths (default: search upward from cwd)
        #[arg(long)]
        manifest: Option<PathBuf>,
        /// Compute and print everything but write nothing
        #[arg(long)]
        dry_run: bool,
    },
    /// Print statistics for an existing stream
    Report {
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Search the stream (newest first). Words match path, kind and tags.
    Find {
        text: Vec<String>,
        #[arg(long)]
        tag: Vec<String>,
        #[arg(long)]
        kind: Option<String>,
        #[arg(long)]
        year: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long, default_value_t = 30)]
        limit: usize,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Saved views (substreams) defined in your home repo, views/*.toml
    Views {
        #[command(subcommand)]
        cmd: ViewsCmd,
    },
    /// Embed record names/tags with a LOCAL model so `ask` can search by meaning
    Embed {
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long, default_value = lifestream::semantic::DEFAULT_HOST)]
        host: String,
        #[arg(long, default_value = lifestream::semantic::DEFAULT_MODEL)]
        model: String,
        /// also embed derived files such as thumbnails
        #[arg(long)]
        all: bool,
    },
    /// Find by meaning, e.g. `ask dark souls save games`
    Ask {
        text: Vec<String>,
        #[arg(long, default_value_t = 15)]
        limit: usize,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long, default_value = lifestream::semantic::DEFAULT_HOST)]
        host: String,
        #[arg(long, default_value = lifestream::semantic::DEFAULT_MODEL)]
        model: String,
    },
    /// Ask both local models what each unclassified folder is (cached; run after a scan)
    Suggest {
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long, default_value = semantic::DEFAULT_HOST)]
        host: String,
    },
    /// Show the directory tree (tree.toml); --apply creates its folders
    Tree {
        #[arg(long)]
        tree: Option<PathBuf>,
        #[arg(long)]
        apply: bool,
        /// drag and drop from the shell: move FROM (with its children) to TO, on disk with --apply
        #[arg(long, num_args = 2, value_names = ["FROM", "TO"])]
        r#move: Vec<String>,
    },
    /// File loose files from the inbox folders into the tree. Dry run unless --apply
    File {
        #[arg(long)]
        tree: Option<PathBuf>,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        manifest: Option<PathBuf>,
        #[arg(long)]
        apply: bool,
        /// put the last batch of moves back where it came from (never overwrites); prints one JSON line
        #[arg(long, conflicts_with = "apply")]
        undo: bool,
    },
    /// Rename or create one folder, journaled so `file --undo` can take it back. One JSON line; exit 1 with a "failed" event if refused.
    Folder {
        #[command(subcommand)]
        cmd: FolderCmd,
        #[arg(long, global = true)]
        out: Option<PathBuf>,
        #[arg(long, global = true)]
        manifest: Option<PathBuf>,
    },
    /// Fingerprint this computer into machines/<name>/machine.toml (--write), or just print it
    Machine {
        /// the home repository (default: search upward for tree.toml)
        #[arg(long)]
        repo: Option<PathBuf>,
        #[arg(long)]
        write: bool,
    },
    /// Which of these home-relative paths must never be captured? Prints the refused ones; exit 1 if any.
    DenyPath {
        paths: Vec<String>,
    },
    /// Find, adopt or create your home repository on GitHub (needs `auth login` first). One JSON event per line.
    Home {
        #[command(subcommand)]
        what: HomeCmd,
    },
    /// Sign in with GitHub. There is no Hestia account: GitHub only stores your private home repository.
    /// Every command prints one JSON event per line; none ever contains the token.
    Auth {
        #[command(subcommand)]
        what: AuthCmd,
    },
    /// Look for secrets before anything leaves this computer. Exit 1 if any are found; never prints the secret.
    SecretScan {
        /// the repository to check
        #[arg(long)]
        repo: Option<PathBuf>,
        /// check what is staged to be committed
        #[arg(long)]
        staged: bool,
        /// check every commit in a range, like origin/main..HEAD, or `all` for the whole history
        #[arg(long)]
        range: Option<String>,
        /// print `path`, line and rule each ended by a NUL byte, so any file name is safe to read back
        #[arg(short = 'z', long)]
        nul: bool,
        /// or check these files (a folder means every file inside it, except .git)
        paths: Vec<PathBuf>,
    },
    /// The morning briefing: what happened overnight and what needs me
    Brief {
        #[arg(long, default_value_t = 24)]
        since_hours: i64,
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum FolderCmd {
    /// Rename FROM to TO (both relative to your home folder, e.g. Documents/Old Archive/Old); same disk only, never merges
    Move { from: String, to: String },
    /// Create one new empty folder PATH (relative to your home folder); its parent must exist
    New { path: String },
}

#[derive(Subcommand)]
enum HomeCmd {
    /// Look through your own private repositories for a home repository
    Discover {
        #[arg(long)]
        json: bool,
    },
    /// Download one of your private home repositories and make it this computer's home
    Adopt {
        /// owner/name
        repo: String,
        /// where to put it (default ~/Home); must be empty or missing
        #[arg(long)]
        into: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Is this one of your own PRIVATE repositories? (the watcher asks before every push)
    CheckPrivate {
        /// owner/name
        repo: String,
        #[arg(long)]
        json: bool,
    },
    /// Make a new private home repository on GitHub, set it up here, and send the first copy
    Create {
        /// the repository name (default: home)
        #[arg(long, default_value = "home")]
        name: String,
        /// where to put it on this computer (default ~/Home); must be empty or missing
        #[arg(long)]
        into: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum AuthCmd {
    /// Show a code, wait for you to approve in your browser, keep the token in the system keyring
    Login {
        #[arg(long)]
        json: bool,
    },
    /// Who is signed in
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Forget the token on this computer
    Logout {
        #[arg(long)]
        json: bool,
    },
    /// Answers git's password question from the keyring, for github.com over https only (used as a git credential helper)
    GitCredential { op: String },
}

#[derive(Subcommand)]
enum ViewsCmd {
    /// List views and how many files each matches
    List {
        #[arg(long)]
        dir: Option<PathBuf>,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Render every view as a folder of symlinks (never moves or copies files)
    Render {
        #[arg(long)]
        dir: Option<PathBuf>,
        /// where to put the view folders (default ~/.local/share/lifestream/views)
        #[arg(long)]
        into: Option<PathBuf>,
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    let cmd = Cli::parse().cmd;
    match cmd {
        Cmd::Tree { tree, apply, r#move } => lifestream::tree::cmd_tree(tree, apply, r#move),
        Cmd::File { tree, out, manifest, apply, undo } => lifestream::tree::cmd_file(tree, &stream_dir(out)?, manifest, apply, undo),
        Cmd::Folder { cmd, out, manifest } => {
            let stream = stream_dir(out)?;
            match cmd {
                FolderCmd::Move { from, to } => lifestream::tree::cmd_folder_move(&from, &to, &stream, manifest),
                FolderCmd::New { path } => lifestream::tree::cmd_folder_new(&path, &stream),
            }
        }
        Cmd::Machine { repo, write } => {
            let m = lifestream::machine::probe(&chrono_today());
            if write {
                let repo = repo.or_else(|| lifestream::query::find_up("tree.toml").and_then(|p| p.parent().map(|x| x.to_path_buf()))).ok_or_else(|| anyhow::anyhow!("no home repository found: pass --repo"))?;
                println!("wrote {}", lifestream::machine::write(&repo, &m)?.display());
            } else {
                print!("{}", toml::to_string_pretty(&m)?);
            }
            Ok(())
        }
        Cmd::DenyPath { paths } => {
            let refused: Vec<&String> = paths.iter().filter(|p| lifestream::deny::capture_refused(p)).collect();
            refused.iter().for_each(|p| println!("{p}"));
            if !refused.is_empty() {
                std::process::exit(1);
            }
            Ok(())
        }
        Cmd::Home { what } => {
            use lifestream::{auth, auth::Store, ghrepo};
            let mut out = std::io::stdout();
            let ep = match auth::Endpoints::from_env() {
                Ok(e) => e,
                Err(e) => {
                    auth::failed(&mut out, "other", Some(("message", &e.to_string())));
                    std::process::exit(1);
                }
            };
            let token = match auth::Keyring.get() {
                Ok(Some(t)) => t,
                Ok(None) => {
                    auth::failed(&mut out, "other", Some(("message", "you are not signed in to GitHub")));
                    std::process::exit(1);
                }
                Err(auth::StoreErr::NoKeyring) => {
                    auth::failed(&mut out, "no-keyring", None);
                    std::process::exit(1);
                }
                Err(auth::StoreErr::Other(m)) => {
                    auth::failed(&mut out, "other", Some(("message", &format!("the keyring said: {m}"))));
                    std::process::exit(1);
                }
            };
            let ok = match what {
                HomeCmd::Discover { .. } => ghrepo::discover(&mut out, &token, &ep),
                HomeCmd::CheckPrivate { repo, .. } => ghrepo::check_private(&mut out, &token, &ep, &repo),
                HomeCmd::Create { name, into, .. } => {
                    let dest = into.unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("Home"));
                    let settings = ghrepo::default_settings().unwrap_or_else(|| PathBuf::from("settings"));
                    let exe = std::env::current_exe()?;
                    ghrepo::create(&mut out, &ghrepo::Ctx::new(&token, &ep, "https://github.com", &settings, &exe, &chrono_today()), &name, &dest)
                }
                HomeCmd::Adopt { repo, into, .. } => {
                    let dest = into.unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("Home"));
                    let settings = ghrepo::default_settings().unwrap_or_else(|| PathBuf::from("settings"));
                    let exe = std::env::current_exe()?;
                    ghrepo::adopt(&mut out, &ghrepo::Ctx::new(&token, &ep, "https://github.com", &settings, &exe, &chrono_today()), &repo, &dest)
                }
            };
            if !ok {
                std::process::exit(1);
            }
            Ok(())
        }
        Cmd::Auth { what } => {
            use lifestream::auth;
            let mut out = std::io::stdout();
            let ep = match auth::Endpoints::from_env() {
                Ok(e) => e,
                Err(e) => {
                    auth::failed(&mut out, "other", Some(("message", &e.to_string())));
                    std::process::exit(1);
                }
            };
            let ok = match what {
                AuthCmd::Login { .. } => auth::login(&mut out, &auth::Keyring, &ep, auth::CLIENT_ID),
                AuthCmd::Status { .. } => auth::status(&mut out, &auth::Keyring, &ep),
                AuthCmd::Logout { .. } => auth::logout(&mut out, &auth::Keyring),
                AuthCmd::GitCredential { op } => {
                    let mut input = String::new();
                    std::io::Read::read_to_string(&mut std::io::stdin(), &mut input)?;
                    auth::git_credential(&op, &input, &auth::Keyring, &mut out).is_ok()
                }
            };
            if !ok {
                std::process::exit(1);
            }
            Ok(())
        }
        Cmd::SecretScan { repo, staged, range, nul, paths } => {
            use std::io::Write;
            use std::os::unix::ffi::OsStrExt;
            let (mut found, mut broke) = (0, false);
            let mut say = |name: &[u8], line: usize, rule: &str| {
                let mut o = std::io::stdout().lock();
                if nul { let _ = o.write_all(&[name, b"\0", line.to_string().as_bytes(), b"\0", rule.as_bytes(), b"\0"].concat()); }
                else { let _ = writeln!(o, "{}:{line}: {rule}", String::from_utf8_lossy(name)); }
                found += 1;
            };
            if !paths.is_empty() {
                let mut files = Vec::new();
                for p in &paths { if std::fs::metadata(p).is_ok_and(|m| m.is_file()) { files.push(p.clone()); } else if let Err(e) = lifestream::secrets::files_under(p, &mut files) { eprintln!("could not read {}: {e}", p.display()); broke = true; } }
                for p in &files {
                    match std::fs::File::open(p).and_then(|f| lifestream::secrets::scan_reader(std::io::BufReader::new(f))) {
                        Ok(fs) => for f in fs { say(p.as_os_str().as_bytes(), f.line, f.rule) },
                        Err(e) => { eprintln!("could not read {}: {e}", p.display()); broke = true; }
                    }
                }
            } else {
                let repo = repo.unwrap_or_else(|| PathBuf::from("."));
                let scope = match (staged, range.as_deref()) {
                    (true, _) => lifestream::secrets::Scope::Staged,
                    (_, Some("all")) => lifestream::secrets::Scope::AllHistory,
                    (_, Some(r)) => lifestream::secrets::Scope::Range(r),
                    _ => anyhow::bail!("say what to check: --staged, --range <a..b|all>, or file paths"),
                };
                match lifestream::secrets::scan_git(&repo, scope) {
                    Ok(fs) => for (file, f) in fs { say(file.as_bytes(), f.line, f.rule) },
                    Err(e) => { eprintln!("could not scan: {e}"); broke = true; }
                }
            }
            // exit 1 means "found something"; exit 2 means "could not look": a broken scan is never reported as a finding
            if found > 0 { eprintln!("{found} possible secret(s) found: nothing was sent anywhere"); std::process::exit(1); }
            if broke { std::process::exit(2); }
            if !nul { println!("no secrets found"); }
            Ok(())
        }
        Cmd::Suggest { out, host } => {
            let stream = stream_dir(out)?;
            let dir = stream.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| stream.clone());
            let tree_path = lifestream::query::find_up("tree.toml");
            let tree = tree_path.as_ref().and_then(|p| lifestream::tree::Tree::load(p).ok());
            let domains = lifestream::domains::load(tree_path.as_ref().and_then(|p| p.parent()).map(|r| r.join("domains")).as_deref())?;
            lifestream::bundle::run(&load_all(&stream)?, &dir, &host, &lifestream::bundle::taxonomy(tree.as_ref(), &domains))
        }
        Cmd::Find { text, tag, kind, year, status, limit, out } => {
            let recs = load_all(&stream_dir(out)?)?;
            let q = lifestream::query::Query {
                text: if text.is_empty() { None } else { Some(text.join(" ")) },
                tags_all: tag,
                kind_prefix: kind,
                year,
                status,
                ..Default::default()
            };
            lifestream::query::find(&recs, &q, limit);
            Ok(())
        }
        Cmd::Embed { out, host, model, all } => {
            let stream = stream_dir(out)?;
            let recs = load_all(&stream)?;
            let dir = stream.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| stream.clone());
            lifestream::semantic::embed(&recs, &dir, &host, &model, all)
        }
        Cmd::Ask { text, limit, out, host, model } => {
            if text.is_empty() {
                bail!("ask what? e.g. `lifestream ask dark souls save games`");
            }
            let stream = stream_dir(out)?;
            let recs = load_all(&stream)?;
            let dir = stream.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| stream.clone());
            lifestream::semantic::ask(&recs, &dir, &host, &model, &text.join(" "), limit)
        }
        Cmd::Views { cmd } => match cmd {
            ViewsCmd::List { dir, out } => {
                let recs = load_all(&stream_dir(out)?)?;
                for v in lifestream::query::load_views(&views_dir(dir)?)? {
                    let n = recs.iter().filter(|r| v.matches(r)).count();
                    println!("{:20} {:6}  {}", v.name.as_deref().unwrap_or(""), n, v.description.as_deref().unwrap_or(""));
                }
                Ok(())
            }
            ViewsCmd::Render { dir, into, out } => {
                let home = home()?;
                let recs = load_all(&stream_dir(out)?)?;
                let into = into.unwrap_or_else(|| home.join(".local/share/lifestream/views"));
                println!("rendering views into {}", into.display());
                lifestream::query::render(&lifestream::query::load_views(&views_dir(dir)?)?, &recs, &into, &home)
            }
        },
        Cmd::Brief { since_hours, out } => {
            print!("{}", lifestream::query::brief(&load_all(&stream_dir(out)?)?, since_hours));
            Ok(())
        }
        Cmd::Scan { roots, out, rules, manifest, dry_run } => cmd_scan(roots, out, rules, manifest, dry_run),
        Cmd::Report { out } => {
            let out = match out {
                Some(o) => o,
                None => home()?.join(".local/share/lifestream/stream"),
            };
            print_stats(&load_all(&out)?);
            Ok(())
        }
    }
}

fn chrono_today() -> String {
    String::from_utf8_lossy(&std::process::Command::new("date").arg("+%F").output().map(|o| o.stdout).unwrap_or_default()).trim().to_string()
}
