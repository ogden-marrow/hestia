//! Hestia, drawn with Rune. `hestia` opens the window;
//! `hestia --snap out.png --scene dark-pink --size 1280x760` renders headless.

mod account;
mod backup;
mod clipboard;
mod engine;
mod design;
mod contents;
mod domains_ui;
mod files;
mod fixture;
mod folders;
mod history;
mod icons;
mod jobs;
mod launcher;
mod machine;
mod prefs;
mod questions;
mod repo;
mod schedule;
mod settings;
mod sidebar;
mod today;
mod timeview;
mod setup;
mod welcome;

use design::{Design, Surface};
use settings::{Scheme, Settings};
use lifestream::bundle::{self, Bundle, Cached};
use repo::{Change, Commit, DiffLine};
use lifestream::semantic;
use lifestream::query::Query;
use lifestream::{load_all, stream_dir, Record};
use rune::tok::{self, type_, TextStyle};
use rune::{AppSpec, Color, Cx, El, Key, KeyEvent, ScrollSpec, TextEdit, Tx, col, el, oklch, row, text, text_input};
use lifestream::system::{self, Status};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Pane {
    Today,
    Questions,
    System,
    Ask,
    History,
    Files,
    Time,
    Machine,
    Settings,
    Welcome,
}

/// What the background thread hands to the screen.
struct Fresh {
    status: Option<(Status, (bool, Color))>, // gathered every other tick
    repo: String,
    dir: u64,
}

struct State {
    design: Design,
    recs: Vec<Record>,
    total: usize, // live files in the stream
    auth: Option<account::Auth>, // the sign-in flow (#110); None until the person reaches it
    auth_inbox: Arc<Mutex<Vec<account::Event>>>, // lines from the engine, drained in poll
    account_user: Option<String>, // who is signed in, as the engine last said
    flow_return: Option<Pane>, // where the sign-in flow goes back to when opened from Settings
    auth_pid: Arc<Mutex<Option<u32>>>, // the running engine command, so it can be cancelled
    dom_sel: usize,
    dom_edit: lifestream::domains::Domain,
    d_name: TextEdit,
    d_desc: TextEdit,
    d_kinds: TextEdit,
    d_folder: TextEdit,
    d_new: TextEdit,
    domains: Vec<lifestream::domains::Domain>,
    stream: Option<PathBuf>, // None in tests: nothing is written
    ask: TextEdit,
    teach: TextEdit,
    ask_hits: Vec<usize>, // record indexes: word matches and meaning matches together
    asel: usize,
    msg: String,
    jobs: jobs::Jobs,
    pane: Pane,
    status: Status,
    fresh: Arc<Mutex<Option<Fresh>>>, // written by the background thread, taken by poll
    repo_dir: PathBuf,
    repo_status: String,
    repo_stamp: String,
    backup: backup::Verdict,
    backup_stamp: String,
    watch_seen: Option<String>,
    commits: Vec<Commit>,
    csel: usize,
    changes: Vec<Change>,
    fsel: usize,
    diff: Vec<DiffLine>,
    cwd: PathBuf,
    entries: Vec<files::Entry>,
    esel: usize,
    dir_stamp: u64,
    kinds: HashMap<String, String>,
    watch: Arc<Mutex<PathBuf>>, // the folder the background thread watches
    days: Vec<timeview::Day>,
    cum: Vec<usize>,
    dsel: usize,
    compare: bool,
    overnight: (usize, usize, usize),
    asked: String,
    apps: Vec<launcher::App>,
    icons: rune::ImageCache, // app icons for the launcher
    clips: Vec<clipboard::Entry>,
    launches: HashMap<String, u32>,
    copy_text: Option<String>, // set by a click, written to the clipboard by poll
    wstep: usize, // welcome tour step
    headless: bool, // the `hestia --serve` engine behind the Shell launcher: no window; screen hits start the app
    bundles: Vec<Bundle>,
    suggestions: HashMap<String, Cached>,
    qsel: usize,
    settings: Settings,
    desktop: (bool, Color),
    stab: usize,
    ext_kinds: HashMap<String, (String, usize)>,
    skipped: Vec<String>,
    tree: lifestream::tree::Tree,
    tree_path: PathBuf,
    tsel: usize,
    tplan: Option<lifestream::tree::Plan>,
    confirm_move: bool,
    last_filed: usize, // files moved by the latest 'File them now', so Undo is offered right after
    undo_lines: Vec<String>, // what could not be put back, one calm line each
    undo_inbox: Arc<Mutex<Vec<String>>>,
    f_path: TextEdit,
    f_kind: TextEdit,
    f_tag: TextEdit,
    f_name: TextEdit,
    f_year: bool,
    f_new: TextEdit,
    f_inbox: TextEdit,
    f_root: TextEdit,
    f_cap: TextEdit,
    f_filekind: TextEdit,
    moving: Option<usize>,
    pending_move: Option<(String, String)>,
    mach: Vec<machine::Cat>,
    mcat: usize,
    mit: usize,
}

impl Default for State {
    fn default() -> Self {
        let stream = stream_dir(None).ok();
        let recs = stream.as_deref().and_then(|d| load_all(d).ok()).unwrap_or_default();
        let mut s = State::with(recs, stream);
        s.settings = Settings::load();
        s.desktop = desktop();
        s.restyle();
        if !s.settings.welcomed { s.pane = Pane::Welcome; }
        let args = rune::extra_args();
        if let Some(key) = args.iter().find_map(|a| a.strip_prefix("--pane="))
            && let Some(p) = launcher::pane_of(key) {
                s.pane = p;
            }
        if args.iter().any(|a| a == "--questions") {
            s.pane = Pane::Questions;
        }
        s
    }
}

/// Where your home repository lives: the `home` setting (the one place; the watcher and restore script read the same
/// line), else `~/Home` (the app is launched from the app grid, so cwd means nothing).
fn home_repo() -> PathBuf {
    if !cfg!(test) {
        let chosen = Settings::load().home;
        if !chosen.is_empty() {
            return files::home().join(chosen);
        }
    }
    let guess = files::home().join("Home");
    if guess.join("manifest/rules.toml").exists() { guess } else { std::env::current_dir().unwrap_or(guess) }
}

/// The desktop's light/dark setting and accent colour (GNOME settings, read-only).
fn desktop() -> (bool, Color) {
    let read = |key: &str| -> String {
        if cfg!(test) {
            return String::new();
        }
        std::process::Command::new("gsettings").args(["get", "org.gnome.desktop.interface", key]).output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().trim_matches('\'').to_string()).unwrap_or_default()
    };
    let dark = read("color-scheme") != "prefer-light" && read("color-scheme") != "default";
    let accent = match read("accent-color").as_str() {
        "blue" => oklch(0.68, 0.16, 255.0),
        "teal" => oklch(0.72, 0.11, 190.0),
        "green" => oklch(0.72, 0.16, 150.0),
        "yellow" => oklch(0.82, 0.15, 95.0),
        "orange" => oklch(0.74, 0.16, 55.0),
        "red" => oklch(0.66, 0.2, 25.0),
        "purple" => oklch(0.66, 0.17, 305.0),
        "slate" => oklch(0.66, 0.03, 250.0),
        _ => oklch(0.7, 0.19, 5.0), // pink, and the fallback
    };
    (dark, accent)
}

pub fn greeting() -> &'static str {
    use chrono::Timelike;
    match chrono::Local::now().hour() {
        0..=4 => "Good night",
        5..=11 => "Good morning",
        12..=17 => "Good afternoon",
        _ => "Good evening",
    }
}

fn open_folder(src: &str) {
    let home = std::env::var("HOME").unwrap_or_default();
    let p = match src.strip_prefix("~/") {
        Some(rest) => PathBuf::from(&home).join(rest),
        None => PathBuf::from(src),
    };
    let dir = if p.is_dir() { p } else { p.parent().map(|d| d.to_path_buf()).unwrap_or(p) };
    if dir.exists() {
        let _ = std::process::Command::new("xdg-open").arg(&dir)
            .stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn();
    }
}

impl State {
    fn with(recs: Vec<Record>, stream: Option<PathBuf>) -> State {
        let stream_path = stream.clone().unwrap_or_default();
        let dir = stream.as_deref().and_then(|d| d.parent()).map(|p| p.to_path_buf()).unwrap_or_default();
        let suggestions = bundle::load_cache(&dir);
        let mut s = State {
            design: Design::default(), recs, total: 0, auth: None, auth_inbox: Default::default(), auth_pid: Default::default(), flow_return: None, account_user: None, dom_sel: 0, dom_edit: Default::default(), d_name: TextEdit::new(""), d_desc: TextEdit::new(""), d_kinds: TextEdit::new(""), d_folder: TextEdit::new(""), d_new: TextEdit::new(""), domains: lifestream::domains::load(Some(&home_repo().join("domains"))).unwrap_or_else(|_| lifestream::domains::defaults()), stream,
            ask: TextEdit::new(""), teach: TextEdit::new(""), ask_hits: vec![], asel: 0, msg: String::new(), jobs: jobs::Jobs::default(),
            pane: Pane::Today, status: Status::default(), fresh: Arc::new(Mutex::new(None)), asked: String::new(), apps: launcher::load_apps(), icons: rune::ImageCache::new(Arc::new(|| {})), clips: clipboard::load(), launches: launcher::load_launches(), copy_text: None, wstep: 0, headless: false, repo_dir: home_repo(), repo_status: String::new(), repo_stamp: String::new(), backup: backup::Verdict::Unknown, backup_stamp: String::new(), watch_seen: None, commits: vec![], csel: 0, changes: vec![], fsel: 0, diff: vec![], cwd: files::home(), entries: vec![], esel: 0, dir_stamp: 0, kinds: HashMap::new(), watch: Arc::new(Mutex::new(files::home())), days: vec![], cum: vec![], dsel: 0, compare: false, overnight: (0, 0, 0), bundles: vec![], suggestions, qsel: 0, settings: Settings::default(), desktop: (true, oklch(0.7, 0.19, 5.0)), stab: 0, ext_kinds: HashMap::new(), skipped: vec![],
            tree: lifestream::tree::Tree::default(), tree_path: PathBuf::new(), tsel: 0, tplan: None, confirm_move: false, last_filed: 0, undo_lines: vec![], undo_inbox: Default::default(),
            f_path: TextEdit::new(""), f_kind: TextEdit::new(""), f_tag: TextEdit::new(""), f_name: TextEdit::new(""), f_year: false, f_new: TextEdit::new(""), f_inbox: TextEdit::new(""), f_root: TextEdit::new(""), f_cap: TextEdit::new(""), f_filekind: TextEdit::new(""), moving: None, pending_move: None, mach: vec![], mcat: 0, mit: 0,
        };
        let _ = stream_path;
        s.tree_path = home_repo().join("tree.toml");
        s.tree = lifestream::tree::Tree::load(&s.tree_path).unwrap_or_default();
        s.select_folder(0);
        s.refresh_derived();
        s.commits = repo::log(&s.repo_dir, 1); // just the newest, for the Today tile; History loads the rest
        s.reload_backup();
        s.reload_dir();
        s
    }
    /// Everything computed from the records. Run when the records change, not on every keystroke.
    fn refresh_derived(&mut self) {
        self.kinds = files::kinds(&self.recs);
        let mut by_ext: HashMap<String, HashMap<String, usize>> = HashMap::new();
        for r in self.recs.iter().filter(|r| r.status == "filed" && !r.kind.starts_with("unknown/") && !r.tags.iter().any(|t| t == "derived")) {
            if let Some((_, e)) = r.sources.first().and_then(|p| p.rsplit('/').next()).and_then(|n| n.rsplit_once('.')) {
                *by_ext.entry(format!(".{}", e.to_lowercase())).or_default().entry(r.kind.clone()).or_default() += 1;
            }
        }
        self.ext_kinds = by_ext.into_iter()
            .filter_map(|(e, k)| k.into_iter().max_by_key(|x| x.1).filter(|x| x.1 >= 3).map(|best| (e, best)))
            .collect();
        let live = |r: &Record| r.status != "gone";
        self.total = self.recs.iter().filter(|r| live(r)).count();
        self.bundles = bundle::bundles(&self.recs);
        self.order_bundles();
        self.overnight = today::overnight(&self.recs);
        self.days = timeview::days(&self.recs);
        self.cum = timeview::cumulative(&self.days);
    }
    /// Skipped folders go to the back of the queue.
    fn order_bundles(&mut self) {
        let skipped = self.skipped.clone();
        self.bundles.sort_by_key(|b| skipped.contains(&b.root));
        self.qsel = self.qsel.min(self.bundles.len().saturating_sub(1));
    }
    fn restyle(&mut self) {
        self.design = Design::build(&self.settings, self.desktop);
    }
    fn change_settings(&mut self, f: impl FnOnce(&mut Settings)) {
        f(&mut self.settings);
        self.settings.save();
        self.restyle();
        self.reload_dir(); // "show hidden files" and the like take effect at once
    }
    /// A setting that changes the system's schedule: save it, then apply it.
    fn change_schedule(&mut self, f: impl FnOnce(&mut Settings)) {
        self.change_settings(f);
        self.msg = schedule::apply(&self.settings);
    }
    /// Answer the selected folder (and, if enabled, its similar neighbours): every file in them
    /// gets this kind, as a human decision.
    fn answer(&mut self, kind: &str, tags: &[String]) {
        let Some(b) = self.bundles.get(self.qsel) else { return };
        let Some(stream) = self.stream.clone() else { self.msg = "no stream to write to".into(); return };
        let mut ids = b.ids.clone();
        let mut folders = 1;
        if self.settings.siblings {
            for j in questions::siblings(&self.bundles, self.qsel) {
                ids.extend(self.bundles[j].ids.iter().cloned());
                folders += 1;
            }
        }
        match lifestream::teach(&stream, &ids, kind, tags) {
            Ok(recs) => {
                self.recs = recs;
                self.refresh_derived();
                self.teach.clear();
                self.msg = format!("{} files in {} folder{} → {}", ids.len(), folders, if folders == 1 { "" } else { "s" }, kind.to_lowercase());
            }
            Err(e) => self.msg = format!("not saved: {e}"),
        }
    }
    /// Ask: files whose names, kinds or tags match your words, and files closest in meaning, together.
    /// Files that match both come first.
    fn do_ask(&mut self) {
        let q = self.ask.text.trim().to_string();
        self.asel = 0;
        if q.is_empty() {
            self.ask_hits.clear();
            self.msg.clear();
            return;
        }
        let words = Query { text: Some(q.clone()), ..Default::default() };
        let mut lexical: Vec<usize> = (0..self.recs.len()).filter(|&i| self.recs[i].status != "gone" && words.matches(&self.recs[i])).collect();
        lexical.sort_by(|&a, &b| self.recs[b].mtime.cmp(&self.recs[a].mtime));
        let dir = self.stream.as_deref().and_then(|d| d.parent()).map(|p| p.to_path_buf()).unwrap_or_default();
        let index: HashMap<&str, usize> = self.recs.iter().enumerate().map(|(i, r)| (r.id.as_str(), i)).collect();
        let (semantic, note): (Vec<usize>, String) = match lifestream::semantic::search(&self.recs, &dir, lifestream::semantic::DEFAULT_HOST, lifestream::semantic::DEFAULT_MODEL, &q) {
            Ok(h) => (h.into_iter().take(60).filter_map(|(_, id)| index.get(id.as_str()).copied()).collect(), format!("words and meaning together for “{q}”")),
            Err(_) => (vec![], format!("word matches for “{q}” (the local model is off, so meaning search is unavailable)")),
        };
        let both: Vec<usize> = semantic.iter().copied().filter(|i| lexical.contains(i)).collect();
        let mut hits = both.clone();
        hits.extend(lexical.iter().copied().filter(|i| !both.contains(i)).take(100));
        let extra: Vec<usize> = semantic.iter().copied().filter(|i| !hits.contains(i)).collect();
        hits.extend(extra);
        hits.truncate(200);
        self.ask_hits = hits;
        self.msg = note;
    }
    /// Move through time: +1 = an earlier day, -1 = a later one.
    fn step_time(&mut self, back: i32) {
        let last = self.days.len().saturating_sub(1) as i32;
        self.dsel = (self.dsel as i32 + back).clamp(0, last) as usize;
    }
    fn reload_backup(&mut self) {
        self.backup_stamp = repo::stamp(&self.repo_dir);
        self.backup = backup::verdict(&backup::read(&self.repo_dir), chrono::Utc::now().timestamp());
    }
    fn reload_repo(&mut self) {
        self.repo_status = repo::status_line(&self.repo_dir);
        self.repo_stamp = repo::stamp(&self.repo_dir);
        let mut commits = repo::log(&self.repo_dir, 150);
        if repo::dirty(&self.repo_dir) {
            commits.insert(0, Commit { hash: repo::WORKTREE.into(), short: "now".into(), subject: "Uncommitted changes".into(), when: String::new(), author: String::new() });
        }
        self.commits = commits;
        self.select_commit(self.csel.min(self.commits.len().saturating_sub(1)));
    }
    fn select_commit(&mut self, i: usize) {
        self.csel = i;
        self.fsel = 0;
        self.changes = self.commits.get(i).map(|c| repo::changes(&self.repo_dir, &c.hash)).unwrap_or_default();
        self.load_diff();
    }
    fn load_diff(&mut self) {
        self.diff = match (self.commits.get(self.csel), self.changes.get(self.fsel)) {
            (Some(c), Some(ch)) => repo::diff(&self.repo_dir, &c.hash, &ch.path, 800),
            _ => vec![],
        };
    }
    fn reload_dir(&mut self) {
        self.entries = files::list(&self.cwd, &self.kinds, self.settings.hidden);
        self.esel = self.esel.min(self.entries.len().saturating_sub(1));
        self.dir_stamp = files::fingerprint(&self.cwd);
        *self.watch.lock().unwrap() = self.cwd.clone();
    }
    fn go(&mut self, p: PathBuf) {
        self.cwd = p;
        self.esel = 0;
        self.reload_dir();
    }
    /// Open the Files screen: the plan for tonight, and the folder you were last in.
    /// The engine made or cloned the home repository and wrote the home setting: start using it.
    fn adopted(&mut self) {
        self.settings.home = Settings::load().home;
        self.repo_dir = home_repo();
        self.tree_path = self.repo_dir.join("tree.toml");
        self.tree = lifestream::tree::Tree::load(&self.tree_path).unwrap_or_default();
        self.reload_domains();
        self.msg = "your home repository is ready".into();
    }
    fn turn_on_search(&mut self) {
        self.msg = "turning on Hestia Search…".into();
        self.jobs.start("search-on", None, || Ok(setup::enable_extension()));
    }
    fn start_scan(&mut self) {
        self.msg = "scanning your folders: this can take a few minutes".into();
        self.jobs.start("scan", Some(|s: &mut State| { if let Some(p) = &s.stream { s.recs = lifestream::load_all(p).unwrap_or_default(); s.refresh_derived(); s.total = s.recs.len(); } }),
            || jobs::run(std::process::Command::new("systemctl").args(["--user", "start", "lifestream-nightly.service"]), "the scan").map(|_| "the scan finished".to_string()));
    }
    fn open_files(&mut self) {
        self.refresh_plan();
        self.reload_dir();
    }
    /// Go to a real folder; if the tree knows it, select it so its rule shows beside its contents.
    fn go_folder(&mut self, p: PathBuf) {
        let rel = p.strip_prefix(files::home()).ok().map(|r| r.to_string_lossy().into_owned());
        self.go(p);
        if let Some(i) = rel.and_then(|r| self.tree.folders.iter().position(|f| f.path == r)) {
            self.select_folder(i);
        }
    }
    fn new_note(&mut self) {
        match files::new_note(&self.cwd) {
            Ok(p) => {
                self.msg = format!("created {}", p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
                self.reload_dir();
                open_folder(&p.to_string_lossy());
            }
            Err(e) => self.msg = format!("could not create a note: {e}"),
        }
    }
}

fn style(d: &Design, base: TextStyle) -> TextStyle {
    d.font(base)
}


fn view(s: &State, _: &mut Cx<State>) -> El {
    let d = s.design;
    let t = d.theme();
    let acc = d.accent_color();
    let pad = d.pad();

    // ---- header: the morning briefing
    let needs = s.bundles.len(); // one unit everywhere: folders waiting (#30)
    let title = match s.pane {
        Pane::Today => "Today", Pane::Questions => "Questions", Pane::System => "System",
        Pane::Ask => "Search", Pane::History => "History", Pane::Files => "Files", Pane::Time => "Time",
        Pane::Machine => "Machine", Pane::Settings => "Settings", Pane::Welcome => "Welcome",
    };
    let (status_text, status_color) = match (&s.backup, needs) {
        (_, n) if n > 0 => (crate::today::count(n, "question", "questions"), acc),
        (backup::Verdict::Safe { when }, _) => (format!("Backed up {when}"), t.text_secondary),
        (backup::Verdict::NeedsYou { .. }, _) => ("Backup needs you".to_string(), d.danger()),
        _ => ("All clear".to_string(), t.text_secondary),
    };
    let header = row().w_full().h(d.size(28.0)).px(pad).center_y().fill(t.card)
        .border_sides([0.0, 0.0, 1.0, 0.0], t.card_border)
        .child(row().w(200.0).gap(d.gap()).center_y()
            .child(rune::icon(&icons::FLAME, d.size(16.0)).color(acc))
            .child(text("Hestia").font(style(&d, type_::LABEL)).color(t.text_secondary)))
        .child(text(title).font(style(&d, type_::TITLE)).color(t.text).grow(1.0))
        .child(
            text(status_text).key("need-you").font(style(&d, type_::LABEL)).color(status_color)
                .on_press(move |s: &mut State| s.pane = if needs > 0 { Pane::Questions } else { Pane::Today }),
        );

    // the info line under every window, as in the classic Finder: "26 items, 4.2 GB available"
    let info = match s.pane {
        Pane::Today => if s.bundles.is_empty() { format!("{} sorted · nothing waiting for you", today::count(s.total, "file", "files")) } else { format!("{} sorted · {} waiting for you", today::count(s.total, "file", "files"), today::count(s.bundles.len(), "folder", "folders")) },
        Pane::Questions => if s.bundles.is_empty() { "No questions waiting".to_string() } else { format!("{} · {}", today::count(s.bundles.len(), "folder", "folders"), today::count(s.bundles.iter().map(|b| b.ids.len()).sum::<usize>(), "file", "files")) },
        Pane::Files => format!("{} · {} · {}", today::count(s.tree.folders.len(), "folder", "folders"), today::count(s.tree.inbox.len(), "watched folder", "watched folders"), files::tilde(&s.cwd)),
        Pane::History => format!("{} saved", today::count(s.commits.len(), "change", "changes")),
        Pane::Time => today::count(s.days.len(), "day with files", "days with files"),
        Pane::Machine => format!("{} since your last backup", today::count(s.mach.iter().map(|c| c.drift()).sum::<usize>(), "change", "changes")),
        Pane::System => format!("models {}", if s.status.ollama_up { "ready" } else { "off" }),
        Pane::Ask => "apps, files, clipboard and Hestia, on this computer".into(),
        Pane::Settings => "changes apply right away".into(),
        Pane::Welcome => "a short tour".into(),
    };
    let footer = row().w_full().h(d.size(24.0)).px(pad).center_y().fill(t.card)
        .border_sides([1.0, 0.0, 0.0, 0.0], t.card_border)
        .child(if s.msg.is_empty() && !s.jobs.any() {
            text(info).font(style(&d, type_::LABEL)).color(t.text_secondary).grow(1.0).shrink().min_w(0.0).truncate()
        } else {
            // one status strip on every screen: what is happening, or what just happened (press to dismiss)
            text(format!("{}{}", if s.jobs.any() { "●  " } else { "" }, s.msg)).key("status").font(style(&d, type_::LABEL)).color(acc).grow(1.0).shrink().min_w(0.0).truncate()
                .on_press(|s: &mut State| if !s.jobs.any() { s.msg.clear() })
        });

    let views = sidebar::panel(s);

    if s.pane == Pane::Welcome {
        return welcome::screen(s);
    }
    let panes: Vec<El> = match s.pane {
        Pane::Questions => questions::panels(s),
        Pane::System => vec![system_panel(s)],
        Pane::Today => vec![today::panel(s)],
        Pane::Settings => prefs::panels(s),
        Pane::Machine => machine::panels(s),
        Pane::Ask => vec![launcher::panel(s)],
        Pane::History => history::panels(s),
        Pane::Files => folders::panels(s),
        Pane::Time => vec![timeview::panel(s)],
        Pane::Welcome => vec![],
    };
    let mut body = row().w_full().grow(1.0).gap(0.0).min_h(0.0).child(views);
    for p in panes {
        body = body.child(p);
    }
    col().w_full().h_full().padding(0.0).gap(0.0).backdrop(&d)
        .child(header)
        .child(body)
        .child(footer)
}

/// Everything that is running: timers and services, models, GPU, pipeline progress.
fn system_panel(s: &State) -> El {
    let d = s.design;
    let t = d.theme();
    let acc = d.accent_color();
    let st = &s.status;
    let bad = d.danger();
    let card = |title: &str| col().w_full().padding(d.pad()).gap(d.gap()).tile(&d)
        .child(text(title.to_string()).font(style(&d, type_::SECTION)).color(t.text_secondary));
    let cell = |txt: String, w: f32, c: rune::Color| text(txt).font(style(&d, type_::LABEL)).color(c).w(d.size(w));

    let scan_now = row().w_full().gap(d.gap()).center_y().tile(&d).padding(d.pad())
        .child(col().grow(1.0).gap(2.0)
            .child(text("Overnight scan").font(style(&d, type_::BODY)).color(t.text))
            .child(text("Reads your folders and updates the stream. The schedule is in Settings.").font(style(&d, type_::LABEL)).color(t.text_secondary)))
        .child(questions::action(&d, "scan-now", "Scan now", "Scanning…", true, s.jobs.running("scan"), |s: &mut State| s.start_scan()));
    let mut timers = card("Background jobs");
    if st.units.is_empty() {
        timers = timers.child(text("No background jobs found. Turn them on in Settings, General.").font(style(&d, type_::LABEL)).color(t.text_secondary).wrap_anywhere().shrink());
    } else {
        timers = timers.child(
            row().gap(d.pad()).child(cell("".into(), 14.0, t.text_secondary)).child(cell("Name".into(), 230.0, t.text_secondary))
                .child(cell("State".into(), 130.0, t.text_secondary)).child(cell("Next run".into(), 150.0, t.text_secondary)).child(cell("Last run".into(), 150.0, t.text_secondary)),
        );
    }
    for u in &st.units {
        let ok = (u.active.starts_with("active") || u.active == "idle") && (u.result.is_empty() || u.result == "success");
        timers = timers.child(
            row().h(d.row_h()).gap(d.pad()).center_y()
                .child(el().size(8.0, 8.0).radius(4.0).fill(if ok { acc } else { bad }))
                .child(text(u.what.clone()).font(style(&d, type_::BODY)).color(t.text).w(d.size(230.0)))
                .child(cell(u.active.clone(), 130.0, if ok { t.text_secondary } else { bad }))
                .child(cell(u.next.clone(), 150.0, t.text_secondary))
                .child(cell(u.last.clone(), 150.0, t.text_secondary)),
        );
    }

    let role = |m: &str| -> &'static str {
        if m.starts_with(bundle::MODELS[0].split(':').next().unwrap_or("")) && m.contains("instruct") { "folder suggestions (A)" }
        else if m.starts_with("qwen3.5") { "folder suggestions (B)" }
        else if m.starts_with(semantic::DEFAULT_MODEL) { "search by meaning" }
        else { "installed" }
    };
    let mut models = card("Models");
    if st.installed.is_empty() {
        models = models.child(text(if st.ollama_up { "No models installed yet." } else { "Models are off. Start Ollama to get folder suggestions and search by meaning." }).font(style(&d, type_::LABEL)).color(t.text_secondary).wrap_anywhere().shrink());
    }
    for (name, gb) in &st.installed {
        let live = st.loaded.iter().find(|l| &l.name == name);
        models = models.child(
            row().h(d.row_h()).gap(d.pad()).center_y()
                .child(el().size(8.0, 8.0).radius(4.0).fill(if live.is_some() { acc } else { t.control_off }))
                .child(text(name.clone()).font(style(&d, type_::BODY)).color(t.text).w(d.size(280.0)))
                .child(cell(role(name).to_string(), 170.0, t.text_secondary))
                .child(cell(format!("{gb:.1} GB"), 70.0, t.text_secondary))
                .child(cell(match live { Some(l) => format!("in GPU memory {} MB · until {}", l.vram_mb, l.expires), None => "not loaded".into() }, 260.0, if live.is_some() { acc } else { t.text_secondary })),
        );
    }

    let mut hw = card("Graphics");
    if let Some(g) = &st.gpu {
        {
            let frac = g.used_mb as f32 / g.total_mb.max(1) as f32;
            hw = hw
                .child(text(format!("memory {} / {} MB · busy {}% · {}°C", g.used_mb, g.total_mb, g.util, g.temp)).font(style(&d, type_::BODY)).color(t.text))
                .child(el().key("gpu-track").w_full().h(8.0).radius(4.0).fill(t.control_off)
                    .child(el().key("gpu-bar").w(560.0 * frac).h(8.0).radius(4.0).fill(if frac > 0.9 { bad } else { acc })));
        }
    }

    let pipe = card("Progress")
        .child(kv(&d, "stream last updated", &st.stream_age))
        .child(kv(&d, "files waiting for you", &st.pending.to_string()))
        .child(kv(&d, "folders suggested by the models", &format!("{} of {}", st.suggested, st.folders)));

    let inner = col().w_full().gap(d.pad()).child(scan_now).child(timers).child(models).child(row().w_full().gap(d.pad()).child(pipe.grow(1.0)).child(if st.gpu.is_some() { hw.grow(1.0) } else { col().size(0.0, 0.0) }));
    col().grow(1.0).shrink().min_w(0.0).padding(d.pad()).gap(d.pad()).panel(&d)
        .child(col().w_full().scroll(ScrollSpec { stick_bottom: false }).shrink().min_h(0.0).grow(1.0).child(inner))
}

/// Arrow keys move through time on the time view.
fn on_key(s: &mut State, ev: &KeyEvent, _tx: &mut Tx) -> bool {
    if s.pane != Pane::Time {
        return false;
    }
    match ev.key {
        Key::Up | Key::Left => s.step_time(1),
        Key::Down | Key::Right => s.step_time(-1),
        _ => return false,
    }
    true
}

fn setup(s: &mut State, wake: rune::Waker) {
    // Windowed runs only (headless snapshots gather once, at startup).
    if std::env::args().any(|a| matches!(a.as_str(), "--snap" | "--trace" | "--tree" | "--frames" | "--script" | "--list-scenes")) { return; }
    s.icons = rune::ImageCache::new(wake.clone());
    s.jobs.set_waker(wake.clone());
    let (slot, stream, repo_dir, watch) = (s.fresh.clone(), s.stream.clone().unwrap_or_default(), s.repo_dir.clone(), s.watch.clone());
    std::thread::spawn(move || {
        let mut tick = 0u64;
        loop {
            let status = tick.is_multiple_of(2).then(|| (system::gather(&load_all(&stream).unwrap_or_default(), &stream), desktop()));
            let cwd = watch.lock().unwrap().clone();
            *slot.lock().unwrap() = Some(Fresh { status, repo: repo::stamp(&repo_dir), dir: files::fingerprint(&cwd) });
            wake();
            tick += 1;
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
    });
}

fn poll(s: &mut State, _t: f64, tx: &mut Tx) -> bool {
    if let Some(text) = s.copy_text.take() {
        tx.copy_rich(rune::ClipboardContent::new(text));
    }
    let heard = engine::drain(&s.auth_inbox);
    let mut any = !heard.is_empty();
    for e in heard {
        account::apply(s, e);
    }
    let finished = s.jobs.take_finished();
    any |= !finished.is_empty();
    for j in finished {
        match j.result {
            Ok(m) => { s.msg = m; if let Some(f) = j.then { f(s); } }
            Err(e) => s.msg = e,
        }
    }
    let Some(f) = s.fresh.lock().unwrap().take() else { return any };
    if s.pane == Pane::Ask {
        s.clips = clipboard::load();
    }
    let mut changed = false;
    if let Some((st, (dark, accent))) = f.status {
        s.status = st;
        let before = (s.desktop.0, s.desktop.1.r.to_bits());
        s.desktop = (dark, accent);
        if before != (dark, accent.r.to_bits()) {
            s.restyle();
            changed = true;
        }
        changed |= s.pane == Pane::System;
    }
    let watch = backup::watch_json();
    if f.repo != s.backup_stamp || watch != s.watch_seen {
        s.watch_seen = watch;
        s.reload_backup();
        changed = true;
    }
    if f.repo != s.repo_stamp && s.pane == Pane::History {
        s.reload_repo();
        changed = true;
    }
    if f.dir != s.dir_stamp && s.pane == Pane::Files {
        s.reload_dir();
        changed = true;
    }
    changed || any
}

fn field(d: &Design, key: &str, edit: &TextEdit, hint: &str, get: fn(&mut State) -> &mut TextEdit) -> El {
    let t = d.theme();
    text_input(edit, style(d, type_::BODY)).key(key).single_line().w_full().h(d.row_h()).px(d.item_px()).py((d.row_h() - d.size(13.0) * 1.25) / 2.0).radius(d.r(tok::radius::SM))
        .fill(t.control_track).color(t.text).placeholder(hint, t.text_secondary)
        .on_edit(get)
}

fn kv(d: &Design, k: &str, v: &str) -> El {
    let t = d.theme();
    col().gap(2.0)
        .child(text(k.to_string()).font(style(d, type_::SECTION)).color(t.text_secondary))
        .child(text(v.to_string()).font(style(d, type_::BODY)).color(t.text).wrap_anywhere().shrink())
}

/// A headless scene: a look and a pane, with whatever data that pane shows.
/// `HESTIA_LOOK=dark,orange,1.25,airy` styles a snapshot without touching saved settings, so one
/// scene name covers the whole look matrix (`ui/scripts/snap-matrix.sh`).
fn apply_look(settings: &mut Settings, look: &str) {
    use settings::Accent as A;
    for w in look.split(',').map(str::trim) {
        match w {
            "auto" => settings.scheme = Scheme::Auto,
            "light" => settings.scheme = Scheme::Light,
            "dark" => settings.scheme = Scheme::Dark,
            "compact" => settings.density = design::Density::Compact,
            "cozy" => settings.density = design::Density::Cozy,
            "airy" => settings.density = design::Density::Airy,
            "blue" => settings.accent = A::Blue,
            "teal" => settings.accent = A::Teal,
            "green" => settings.accent = A::Green,
            "orange" => settings.accent = A::Orange,
            "purple" => settings.accent = A::Purple,
            "pink" => settings.accent = A::Pink,
            "graphite" => settings.accent = A::Graphite,
            n => if let Ok(t) = n.parse::<f32>() { settings.text = t.clamp(0.8, 1.4) },
        }
    }
}

fn scene(s: &mut State, pane: Pane) {
    // HESTIA_FIXTURE=1: generic data, folders waiting. HESTIA_FIXTURE=calm: nothing waiting.
    let fixture = std::env::var("HESTIA_FIXTURE").ok();
    if let Some(f) = &fixture {
        fixture::apply(s, f != "calm");
        s.backup_stamp = repo::stamp(&s.repo_dir);
        s.backup = match std::env::var("HESTIA_BACKUP").as_deref() {
            Ok("failed") => backup::Verdict::NeedsYou { why: "The last backup failed: git commit: no identity.".into(), fix: backup::Fix::BackUpNow },
            Ok("needs") => backup::Verdict::NeedsYou { why: "Your backup is only on this computer. Sign in to keep a private copy on GitHub.".into(), fix: backup::Fix::SignIn },
            _ => backup::Verdict::Safe { when: "12 minutes ago".into() },
        };
    }
    if let Ok(look) = std::env::var("HESTIA_LOOK") {
        apply_look(&mut s.settings, &look);
    }
    // HESTIA_AUTH=waiting shows one state of the sign-in flow (see account::ALL).
    if let Some(a) = std::env::var("HESTIA_AUTH").ok().and_then(|n| account::canned(&n)) {
        s.auth = Some(a);
        s.wstep = 2;
        s.restyle();
        s.pane = Pane::Welcome;
        return;
    }
    s.restyle();
    s.pane = pane;
    if fixture.is_some() {
        return; // the panes that read this machine stay empty rather than show someone's files
    }
    match pane {
        Pane::System | Pane::Settings => s.status = system::gather(&s.recs, s.stream.as_deref().unwrap_or(std::path::Path::new(""))),
        Pane::History => s.reload_repo(),
        Pane::Files => s.open_files(),
        Pane::Machine => s.reload_machine(),
        Pane::Ask => { s.ask.text = "fi".into(); for a in s.apps.iter().take(40) { let _ = s.icons.app_icon(&a.path, 30.0).wait(); } }
        _ => {}
    }
}

fn main() {
    if std::env::args().any(|a| a == "--serve") {
        launcher::serve();
        return;
    }
    // The window's identity: GNOME matches it to hestia.desktop for the taskbar name and icon.
    rune::set_app_id("app.hestia.Hestia");
    let spec = AppSpec::<State> {
        scenes: &[
            ("today", |s| scene(s, Pane::Today)),
            ("questions", |s| scene(s, Pane::Questions)),
            ("time", |s| scene(s, Pane::Time)),
            ("files", |s| scene(s, Pane::Files)),
            ("history", |s| scene(s, Pane::History)),
            ("system", |s| scene(s, Pane::System)),
            ("ask", |s| scene(s, Pane::Ask)),
            ("machine", |s| { scene(s, Pane::Machine); s.reload_machine() }),
            ("welcome", |s| scene(s, Pane::Welcome)),
            ("welcome-key", |s| { s.wstep = 3; scene(s, Pane::Welcome) }),
            ("settings", |s| scene(s, Pane::Settings)),
            ("settings-scanning", |s| { s.stab = 2; scene(s, Pane::Settings) }),
            ("settings-general", |s| { s.stab = 1; scene(s, Pane::Settings) }),
            ("settings-account", |s| { s.stab = 7; s.account_user = std::env::var("HESTIA_ACCOUNT_USER").ok(); scene(s, Pane::Settings) }),
            ("settings-domains", |s| { s.stab = 6; s.select_domain(0); scene(s, Pane::Settings) }),
            ("time-compare", |s| { s.dsel = 40; s.compare = true; scene(s, Pane::Time) }),
        ],
        setup: Some(setup),
        poll: Some(poll),
        on_key: Some(on_key),
        min_size: Some((960.0, 620.0)),
        ..AppSpec::new("Hestia", (1280.0, 760.0), State::default, view)
    };
    rune::run(spec);
}

#[cfg(test)]
mod tests {
    use super::*;
    use lifestream::By;
    use rune::{Env, Runtime};

    fn rec(id: &str, src: &str) -> Record {
        Record {
            id: id.into(), first_seen: "n".into(), mtime: "2025-05-01T00:00:00Z".into(), kind: "unknown/dat".into(), size: 10,
            sources: vec![src.into()], tags: vec![], summary: None,
            by: By { tier: "rule".into(), name: "none".into(), confidence: 0.0 }, status: "pending".into(), store: None, future_at: None, was: vec![],
        }
    }

    #[test]
    fn answering_a_folder_files_every_file_in_it_and_empties_the_queue() {
        let d = std::env::temp_dir().join(format!("ls-ui-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let stream = d.join("stream");
        let recs = vec![rec("a", "~/Downloads/App/x/1.dat"), rec("b", "~/Downloads/App/x/2.dat"), rec("c", "~/Other/y/3.dat")];
        lifestream::write_all(&stream, &recs).unwrap();
        let mut s = State::with(recs, Some(stream.clone()));
        assert_eq!(s.bundles.len(), 2);
        s.pane = Pane::Questions;
        s.answer("software/app-bundle", &[]); // the biggest folder is selected first
        assert_eq!(s.bundles.len(), 1, "the answered folder leaves the queue");
        let back = load_all(&stream).unwrap();
        let human_filed = back.iter().filter(|r| r.by.tier == "human" && r.kind == "software/app-bundle").count();
        assert_eq!(human_filed, 2, "both files in the folder are filed");
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn the_questions_row_opens_the_queue() {
        let recs = vec![rec("a", "~/Downloads/App/x/1.dat")];
        let mut rt = Runtime::new(State::with(recs, None), view, (1280.0, 760.0), Env { system_dark: true, scale: 1.0 });
        rt.frame(0.0);
        let (x, y) = rt.find("view-q").unwrap();
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(0.1);
        assert_eq!(rt.state.pane, Pane::Questions);
        assert!(rt.find("bundle-0").is_some(), "the folder shows as a question");
    }
}


#[cfg(test)]
mod look_tests {
    use super::*;
    use rune::{Env, Runtime};

    #[test]
    fn choosing_an_accent_in_settings_restyles_the_whole_app() {
        let mut rt = Runtime::new(State::with(vec![], None), view, (1280.0, 760.0), Env { system_dark: true, scale: 1.0 });
        rt.frame(0.0);
        let before = rt.state.design.accent.r.to_bits();
        let click = |rt: &mut Runtime<State>, key: &str| {
            let (x, y) = rt.find(key).unwrap_or_else(|| panic!("{key} is on screen"));
            rt.pointer_down(x, y);
            rt.pointer_up(x, y);
            rt.frame(0.1);
        };
        click(&mut rt, "nav-settings");
        assert_eq!(rt.state.pane, Pane::Settings);
        click(&mut rt, "accent-orange");
        assert_ne!(rt.state.design.accent.r.to_bits(), before, "choosing an accent restyles the app");
    }
}

/// Developer vocabulary the owner must never read on a primary screen (#90). Checks the exact
/// strings this sweep replaced, so they cannot creep back; Advanced/Details may still use them.
#[cfg(test)]
mod calm_words {
    #[test]
    fn replaced_strings_do_not_return() {
        let banned = [
            "\"git\".into()", "main…origin/main", "Remove from tree", "(regex)", "\"Top Hit\"", "Config and dotfiles",
            "Packages (apt)", "\"Flatpaks\"", "\"Snaps\"", "differences from the capture", "need you\"", "inboxes · ", "Keep the capture current",
        ];
        for (file, src) in [
            ("main.rs", include_str!("main.rs")), ("history.rs", include_str!("history.rs")), ("machine.rs", include_str!("machine.rs")),
            ("folders.rs", include_str!("folders.rs")), ("sidebar.rs", include_str!("sidebar.rs")), ("prefs.rs", include_str!("prefs.rs")),
            ("launcher.rs", include_str!("launcher.rs")), ("questions.rs", include_str!("questions.rs")),
        ] {
            let code = src.split("mod calm_words").next().unwrap_or(src);
            for b in banned {
                assert!(!code.contains(b), "{file} brings back {b:?}");
            }
        }
    }
}

#[cfg(test)]
mod today_tests {
    use super::*;
    use rune::{Env, Runtime};

    #[test]
    fn it_opens_on_today_and_the_sidebar_leads_somewhere() {
        let mut rt = Runtime::new(State::with(vec![], None), view, (1280.0, 800.0), Env { system_dark: true, scale: 1.0 });
        rt.frame(0.0);
        assert_eq!(rt.state.pane, Pane::Today);
        let (x, y) = rt.find("view-time").expect("the Time item is in the sidebar");
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(0.1);
        assert_eq!(rt.state.pane, Pane::Time);
    }
}

#[cfg(test)]
mod perf {
    use super::*;

    /// Run with: cargo test --release perf -- --ignored --nocapture
    #[test]
    #[ignore]
    fn real_data_timings() {
        let t = std::time::Instant::now();
        let mut s = State::default();
        eprintln!("State::default            {:>6.1} ms ({} records)", t.elapsed().as_secs_f64() * 1e3, s.recs.len());
        let t = std::time::Instant::now();
        for _ in 0..5 { s.order_bundles(); }
        eprintln!("recompute (x1)            {:>6.1} ms", t.elapsed().as_secs_f64() * 1e3 / 5.0);
        let t = std::time::Instant::now();
        s.reload_repo();
        eprintln!("reload_repo (git)         {:>6.1} ms", t.elapsed().as_secs_f64() * 1e3);
        let t = std::time::Instant::now();
        s.reload_dir();
        eprintln!("reload_dir                {:>6.1} ms", t.elapsed().as_secs_f64() * 1e3);
        let t = std::time::Instant::now();
        let _ = system::gather(&s.recs, s.stream.as_deref().unwrap());
        eprintln!("system::gather            {:>6.1} ms", t.elapsed().as_secs_f64() * 1e3);
        let t = std::time::Instant::now();
        let _ = bundle::bundles(&s.recs);
        eprintln!("bundles                   {:>6.1} ms", t.elapsed().as_secs_f64() * 1e3);
        let t = std::time::Instant::now();
        let _ = timeview::days(&s.recs);
        eprintln!("timeview::days            {:>6.1} ms", t.elapsed().as_secs_f64() * 1e3);
    }
}
