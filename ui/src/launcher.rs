//! The launcher: one search box for Hestia's screens, installed apps, clipboard history and files.
//! No plugins. Apps come from the system's desktop entries; the clipboard from Hestia's own history.
//! The same results serve Search inside the app and `hestia --serve`, the engine behind the Shell's Alt+Space bar.

use super::{style, Pane, State};
use crate::clipboard;
use crate::design::Surface;
use lifestream::human;
use lifestream::query::Query;
use rune::tok::{self, type_};
use rune::{col, row, text, El, KeyEvent, Key, ScrollSpec, TextStyle, Tx};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct App {
    pub id: String,
    pub name: String,
    pub generic: String,
    pub keywords: String,
    pub path: PathBuf,
}

pub fn dirs() -> Vec<PathBuf> {
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    vec![
        home.join(".local/share/applications"),
        home.join(".local/share/flatpak/exports/share/applications"),
        "/var/lib/flatpak/exports/share/applications".into(),
        "/var/lib/snapd/desktop/applications".into(),
        "/usr/local/share/applications".into(),
        "/usr/share/applications".into(),
    ]
}

/// The visible apps, one per desktop id; the user's own entries win over the system's.
pub fn load_apps() -> Vec<App> {
    let mut seen: HashMap<String, App> = HashMap::new();
    for d in dirs() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let path = e.path();
            if path.extension().and_then(|x| x.to_str()) != Some("desktop") {
                continue;
            }
            let id = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if seen.contains_key(&id) {
                continue;
            }
            if let Some(app) = parse_entry(&id, &path, &std::fs::read_to_string(&path).unwrap_or_default()) {
                seen.insert(id, app);
            }
        }
    }
    let mut v: Vec<App> = seen.into_values().collect();
    v.sort_by_key(|a| a.name.to_lowercase());
    v
}

pub fn parse_entry(id: &str, path: &std::path::Path, text: &str) -> Option<App> {
    let (mut name, mut generic, mut keywords) = (String::new(), String::new(), String::new());
    let (mut hidden, mut is_app, mut in_main) = (false, false, false);
    for l in text.lines() {
        let l = l.trim();
        if l.starts_with('[') {
            in_main = l == "[Desktop Entry]";
            continue;
        }
        if !in_main {
            continue;
        }
        let Some((k, v)) = l.split_once('=') else { continue };
        match k {
            "Name" => name = v.to_string(),
            "GenericName" => generic = v.to_string(),
            "Keywords" => keywords = v.to_string(),
            "NoDisplay" | "Hidden" => hidden |= v.eq_ignore_ascii_case("true"),
            "Type" => is_app = v == "Application",
            _ => {}
        }
    }
    (is_app && !hidden && !name.is_empty()).then(|| App { id: id.to_string(), name, generic, keywords, path: path.to_path_buf() })
}

#[cfg(test)]
thread_local!(static TEST_LAUNCHES: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) });

fn launches_file() -> PathBuf {
    #[cfg(test)]
    if let Some(p) = TEST_LAUNCHES.with(|t| t.borrow().clone()) {
        return p;
    }
    PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/share/hestia/launches.tsv")
}

/// How often each app was launched from here: id -> count.
pub fn load_launches() -> HashMap<String, u32> {
    std::fs::read_to_string(launches_file()).unwrap_or_default().lines()
        .filter_map(|l| { let (id, n) = l.split_once('\t')?; Some((id.to_string(), n.parse().ok()?)) }).collect()
}

/// Open an app. A launcher that fails silently is worse than none, so a failure comes back as
/// words a person can read, and a failed launch is not counted.
pub fn launch(app: &App, counts: &mut HashMap<String, u32>) -> Result<(), String> {
    run_gio(&app.path).map_err(|why| format!("Couldn't open {}: {why}", app.name))?;
    *counts.entry(app.id.clone()).or_default() += 1;
    if let Some(d) = launches_file().parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let body: String = counts.iter().map(|(k, v)| format!("{k}\t{v}\n")).collect();
    let _ = std::fs::write(launches_file(), body);
    Ok(())
}

/// `gio launch <entry>`; returns once the desktop has taken the request.
fn run_gio(path: &std::path::Path) -> Result<(), String> {
    let out = std::process::Command::new("gio").arg("launch").arg(path)
        .stdin(std::process::Stdio::null()).output().map_err(|e| format!("gio is not available ({e})"))?;
    if out.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&out.stderr);
    let first = err.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("the desktop refused");
    Err(first.trim_start_matches("gio: ").to_string())
}

/// How well a query matches a name: prefix beats word-prefix beats anywhere.
pub fn score(q: &str, hay: &str) -> u32 {
    let h = hay.to_lowercase();
    if h == q { 100 } else if h.starts_with(q) { 80 } else if h.split([' ', '-', '_', '.']).any(|w| w.starts_with(q)) { 60 } else if h.contains(q) { 30 } else { 0 }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Screen(&'static str),
    App(usize),
    Clip(usize),
    File(usize),
    Meaning,
}

#[derive(Clone, Debug)]
pub struct Hit {
    pub top: bool,
    pub group: &'static str,
    pub kind: Kind,
    pub title: String,
    pub sub: String,
}

pub const SCREENS: [(&str, &str); 9] = [
    ("Today", "today"), ("Time", "time"), ("Files", "files"), ("History", "history"), ("Machine", "machine"),
    ("Questions", "questions"), ("System", "system"), ("Settings", "settings"), ("Search", "search"),
];

pub fn pane_of(key: &str) -> Option<Pane> {
    Some(match key {
        "today" => Pane::Today, "time" => Pane::Time, "files" => Pane::Files, "history" => Pane::History, "machine" => Pane::Machine,
        "questions" => Pane::Questions, "system" => Pane::System, "settings" => Pane::Settings, "search" => Pane::Ask,
        _ => return None,
    })
}

/// Everything that matches what was typed, in the order it is shown.
pub fn results(s: &State) -> Vec<Hit> {
    let q = s.ask.text.trim().to_lowercase();
    let mut out: Vec<Hit> = Vec::new();
    if q.is_empty() {
        return out; // like Spotlight: just the bar until you type
    }
    // Hestia's own screens
    for (name, key) in SCREENS {
        if score(&q, name) > 0 {
            out.push(Hit { top: false, group: "Hestia", kind: Kind::Screen(key), title: name.to_string(), sub: "Hestia".into() });
        }
    }
    // apps: best match first, then the ones you launch most
    let mut apps: Vec<(u32, usize)> = s.apps.iter().enumerate().filter_map(|(i, a)| {
        let sc = score(&q, &a.name).max(score(&q, &a.generic) / 2).max(score(&q, &a.keywords) / 3);
        let used = s.launches.get(&a.id).copied().unwrap_or(0);
        (sc > 0).then_some((sc + used.min(20), i))
    }).collect();
    apps.sort_by_key(|x| std::cmp::Reverse(x.0));
    for (_, i) in apps.into_iter().take(6) {
        let a = &s.apps[i];
        out.push(Hit { top: false, group: "Apps", kind: Kind::App(i), title: a.name.clone(), sub: a.generic.clone() });
    }
    // clipboard
    // typing "clipboard" lists your latest copies; otherwise only copies that contain what you typed
    let browse = score(&q, "clipboard") > 0;
    for (i, c) in s.clips.iter().enumerate().filter(|(_, c)| browse || c.text.to_lowercase().contains(&q)).take(if browse { 8 } else { 4 }) {
        out.push(Hit { top: false, group: "Clipboard", kind: Kind::Clip(i), title: clipboard::preview(c), sub: String::new() });
    }
    {
        // files by words, instantly
        let words = Query { text: Some(q.clone()), ..Default::default() };
        let mut files: Vec<usize> = (0..s.recs.len()).filter(|&i| s.recs[i].status != "gone" && words.matches(&s.recs[i])).collect();
        files.sort_by(|&a, &b| s.recs[b].mtime.cmp(&s.recs[a].mtime));
        for i in files.into_iter().take(5) {
            let r = &s.recs[i];
            let src = r.sources.first().or(r.was.first()).map(|x| x.as_str()).unwrap_or("");
            out.push(Hit { top: false, group: "Files", kind: Kind::File(i), title: src.rsplit('/').next().unwrap_or(src).to_string(), sub: format!("{} · {}", r.kind, human(r.size)) });
        }
        // files by meaning: already searched, or one Enter away
        if s.asked == s.ask.text.trim() && !s.ask_hits.is_empty() {
            for &i in s.ask_hits.iter().take(8) {
                let r = &s.recs[i];
                let src = r.sources.first().or(r.was.first()).map(|x| x.as_str()).unwrap_or("");
                out.push(Hit { top: false, group: "Closest in meaning", kind: Kind::File(i), title: src.rsplit('/').next().unwrap_or(src).to_string(), sub: format!("{} · {}", r.kind, human(r.size)) });
            }
        } else {
            out.push(Hit { top: false, group: "Files", kind: Kind::Meaning, title: format!("Search files by meaning: “{}”", s.ask.text.trim()), sub: "uses the local model".into() });
        }
    }
    // apps first, as a launcher should: what you type is most often an app
    let order = |g: &str| match g { "Apps" => 0, "Hestia" => 1, "Clipboard" => 2, "Files" => 3, _ => 4 };
    out.sort_by_key(|h| order(h.group));
    if let Some(first) = out.first_mut() {
        first.top = true; // shown under "Best match"; the rest keep their own headings
    }
    out
}

pub fn heading_of(h: &Hit) -> &'static str {
    if h.top { "Best match" } else { match h.group { "Apps" => "Applications", "Files" => "Documents", "Clipboard" => "Recently copied", g => g } }
}

impl State {
    /// Do what the selected result says. True when it was handled for the headless engine (the Shell bar
    /// closes itself); in the app the result shows in place.
    pub(crate) fn activate(&mut self, hit: &Hit) {
        self.activate_inner(hit);
    }
    fn activate_inner(&mut self, hit: &Hit) -> bool {
        match &hit.kind {
            Kind::Screen(key) => {
                if self.headless {
                    let _ = std::process::Command::new(std::env::current_exe().unwrap_or_else(|_| "hestia".into())).arg(format!("--pane={key}")).spawn();
                    return true;
                }
                if let Some(p) = pane_of(key) {
                    self.pane = p;
                    match p {
                        Pane::Files => self.open_files(),
                        Pane::History => self.reload_repo(),
                        Pane::Machine => self.reload_machine(),
                        _ => {}
                    }
                }
                false
            }
            Kind::App(i) => {
                let Some(a) = self.apps.get(*i).cloned() else {
                    self.msg = "That app is gone. Search again.".into();
                    return false;
                };
                match launch(&a, &mut self.launches) {
                    Ok(()) => true,
                    Err(why) => {
                        self.msg = why;
                        false
                    }
                }
            }
            Kind::Clip(i) => {
                if let Some(c) = self.clips.get(*i) {
                    self.copy_text = Some(c.text.clone());
                    self.msg = "copied, ready to paste".into();
                }
                self.headless
            }
            Kind::File(i) => {
                if let Some(src) = self.recs.get(*i).and_then(|r| r.sources.first().or(r.was.first())) {
                    super::open_folder(src);
                }
                self.headless
            }
            Kind::Meaning => {
                self.do_ask();
                self.asked = self.ask.text.trim().to_string();
                false
            }
        }
    }
}

/// `hestia --serve`: the launcher's engine without a window. The GNOME Shell extension draws the
/// bar and keeps this process running; it sends one JSON line per keystroke and gets the hits back.
///   {"q":"fire"}  ->  {"hits":[{"i":0,"top":true,"group":"Applications","title":"Firefox","sub":"Web Browser","kind":"app","path":"/usr/share/applications/firefox.desktop"}]}
///   {"run":0}     ->  {"done":true}  or  {"copy":"text"} for a clipboard item (the Shell owns the clipboard)
pub fn serve() {
    let s = State { headless: true, ..State::default() };
    serve_with(s, std::io::stdin().lock(), std::io::stdout().lock());
}

fn serve_with(mut s: State, input: impl std::io::BufRead, mut out: impl std::io::Write) {
    let mut hits: Vec<Hit> = Vec::new();
    for line in input.lines().map_while(Result::ok) {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else { continue };
        let reply = if let Some(q) = v["q"].as_str() {
            s.ask.text = q.to_string();
            s.clips = clipboard::load();
            hits = results(&s).into_iter().filter(|h| h.kind != Kind::Meaning).take(10).collect();
            let list: Vec<serde_json::Value> = hits.iter().enumerate().map(|(i, h)| {
                let (kind, path) = match &h.kind {
                    Kind::App(n) => ("app", s.apps.get(*n).map(|a| a.path.to_string_lossy().into_owned()).unwrap_or_default()),
                    Kind::Clip(_) => ("clip", String::new()),
                    Kind::File(n) => ("file", s.recs.get(*n).and_then(|r| r.sources.first().or(r.was.first())).cloned().unwrap_or_default()),
                    _ => ("screen", String::new()),
                };
                serde_json::json!({"i": i, "top": h.top, "group": h.group, "title": h.title, "sub": h.sub, "kind": kind, "path": path})
            }).collect();
            serde_json::json!({"hits": list})
        } else if let Some(i) = v["run"].as_u64() {
            match hits.get(i as usize).cloned() {
                Some(Hit { kind: Kind::Clip(n), .. }) => serde_json::json!({"copy": s.clips.get(n).map(|c| c.text.clone()).unwrap_or_default()}),
                Some(h) => {
                    s.msg.clear();
                    if s.activate_inner(&h) { serde_json::json!({"done": true}) } else { serde_json::json!({"done": false, "error": s.msg}) }
                }
                None => serde_json::json!({"done": false, "error": "Nothing to open. Search again."}),
            }
        } else {
            continue;
        };
        let _ = writeln!(out, "{reply}");
        let _ = out.flush();
    }
}

/// The picture beside a result: an app's own icon, or one of Hestia's drawn ones.
fn icon_for(s: &State, h: &Hit, on: bool) -> El {
    let d = s.design;
    let t = d.theme();
    let tint = if on { d.on_accent() } else { t.text_secondary };
    match &h.kind {
        Kind::App(i) => match s.apps.get(*i) {
            Some(a) => col().size(30.0, 30.0).pixels(s.icons.app_icon(&a.path, 30.0).pixels()),
            None => col().size(30.0, 30.0),
        },
        Kind::Screen(key) => {
            let ic = match *key {
                "time" => &crate::icons::TIME, "files" => &crate::icons::FOLDER, "history" => &crate::icons::HISTORY,
                "machine" => &crate::icons::LAPTOP, "questions" => &crate::icons::QUESTIONS, "folders" => &crate::icons::TREE,
                "system" => &crate::icons::SYSTEM, "settings" => &crate::icons::SETTINGS, "search" => &crate::icons::ASK,
                _ => &crate::icons::TODAY,
            };
            col().size(30.0, 30.0).center().child(rune::icon(ic, 22.0).color(tint))
        }
        Kind::Meaning => col().size(30.0, 30.0).center().child(rune::icon(&crate::icons::ASK, 22.0).color(tint)),
        Kind::File(_) | Kind::Clip(_) => col().size(30.0, 30.0).center().child(rune::icon(&crate::icons::DOC, 22.0).color(tint)),
    }
}

/// The launcher, drawn like Spotlight: a wide bar with a magnifier, and results only once you type.
pub fn panel(s: &State) -> El {
    let d = s.design;
    let t = d.theme();
    let acc = d.accent_color();
    let big = TextStyle { size: d.size(22.0), ..type_::BODY };
    let hits = results(s);
    let n = hits.len();
    let field = rune::text_input(&s.ask, d.font(big)).key("ask-big").single_line().grow(1.0).h(64.0).py(19.0).color(t.text)
        .placeholder("Search your files, apps and folders", t.text_secondary)
        .on_edit(|s: &mut State| &mut s.ask)
        .on_input_change(|s: &mut State, _: &mut Tx| {
            s.asel = 0;
            s.asked.clear();
            s.ask_hits.clear();
        })
        .on_input_key(move |s: &mut State, ev: &KeyEvent, _: &mut Tx| {
            let count = results(s).len();
            match ev.key {
                Key::Down => s.asel = (s.asel + 1).min(count.saturating_sub(1)),
                Key::Up => s.asel = s.asel.saturating_sub(1),
                Key::Escape => s.pane = Pane::Today,
                _ => return false,
            }
            true
        })
        .on_submit(|s: &mut State, _: &mut Tx| {
            let all = results(s);
            // the highlighted row is clamped when drawn, so Enter opens what is lit
            if let Some(h) = all.get(s.asel.min(all.len().saturating_sub(1))).cloned() {
                s.msg.clear();
                s.activate(&h);
            }
        });
    let bar = row().w_full().px(20.0).gap(14.0).center_y()
        .child(rune::icon(&crate::icons::ASK, 24.0).color(t.text_secondary))
        .child(field);

    let mut list = col().w_full().px(8.0).gap(2.0);
    let mut last = "";
    for (i, h) in hits.iter().take(10).enumerate() {
        let heading = heading_of(h);
        if heading != last || i == 0 {
            list = list.child(text(heading.to_string()).font(style(&d, type_::SECTION)).color(t.text_secondary).px(12.0).pt(10.0).pb(4.0));
            last = heading;
        }
        let on = i == s.asel.min(n.saturating_sub(1));
        let hit = h.clone();
        list = list.child(
            row().key(format!("hit-{i}")).w_full().h(44.0).px(12.0).gap(14.0).center_y().radius(d.r(tok::radius::MD))
                .fill(if on { acc } else { rune::Color::TRANSPARENT })
                .on_press(move |s: &mut State| {
                    s.asel = i;
                    s.activate(&hit);
                })
                .child(icon_for(s, h, on))
                .child(text(h.title.clone()).font(TextStyle { size: d.size(15.0), ..style(&d, type_::BODY) }).color(if on { d.on_accent() } else { t.text }).grow(1.0).shrink().min_w(0.0).truncate())
                .child(text(h.sub.clone()).font(style(&d, type_::LABEL)).color(if on { d.on_accent() } else { t.text_secondary }).truncate()),
        );
    }
    let mut c = col().w_full().grow(1.0).child(bar);
    if n > 0 {
        c = c.child(col().w_full().h(1.0).fill(t.card_border))
            .child(col().w_full().scroll(ScrollSpec { stick_bottom: false }).shrink().min_h(0.0).grow(1.0).child(list));
    } else {
        let try_it = |key: &str, word: &'static str| crate::questions::button(&d, key, word.into(), false, move |s: &mut State| { s.ask.text = word.into(); s.asel = 0 });
        c = c.child(crate::questions::empty(&d, &crate::icons::ASK, "Search this computer",
            "Apps, files, things you copied, and Hestia itself. Nothing leaves this computer.",
            vec![try_it("try-invoice", "invoice"), try_it("try-photos", "photos"), try_it("try-clipboard", "clipboard")]));
    }
    if !s.msg.is_empty() {
        c = c.child(text(s.msg.clone()).font(style(&d, type_::LABEL)).color(acc).wrap_anywhere().shrink().px(20.0).py(d.gap()));
    }
    c.panel(&d)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Drive `{"q":..}` then `{"run":0}` through the real `serve` loop with a fake desktop entry
    /// whose `Exec` touches a marker file, and check the app was started.
    #[test]
    fn serve_runs_the_selected_app_and_reports_failure() {
        let dir = std::env::temp_dir().join(format!("hestia-serve-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        TEST_LAUNCHES.with(|t| *t.borrow_mut() = Some(dir.join("launches.tsv")));
        let marker = dir.join("ran");
        let entry = dir.join("zzfake.desktop");
        let text = format!("[Desktop Entry]\nType=Application\nName=Zzfake Thing\nExec=touch {}\n", marker.display());
        std::fs::write(&entry, &text).unwrap();
        let state = || {
            let mut s = State { headless: true, apps: vec![parse_entry("zzfake.desktop", &entry, &text).unwrap()], ..State::default() };
            s.clips.clear();
            s
        };
        let mut out = Vec::new();
        serve_with(state(), "{\"q\":\"zzfake\"}\n{\"run\":0}\n".as_bytes(), &mut out);
        let out = String::from_utf8(out).unwrap();
        assert!(out.contains("Zzfake Thing") && out.contains("\"done\":true"), "{out}");
        for _ in 0..50 {
            if marker.exists() { break; }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        assert!(marker.exists(), "the selected app was not started");

        // Enter with nothing to open says so instead of staying silent
        let mut out = Vec::new();
        serve_with(state(), "{\"run\":0}\n".as_bytes(), &mut out);
        let out = String::from_utf8(out).unwrap();
        assert!(out.contains("\"done\":false") && out.contains("error"), "{out}");

        // a launch the desktop refuses is reported, not swallowed, and not counted
        let mut counts = HashMap::new();
        let gone = App { id: "gone.desktop".into(), name: "Gone".into(), generic: String::new(), keywords: String::new(), path: dir.join("missing.desktop") };
        let err = launch(&gone, &mut counts).unwrap_err();
        assert!(err.starts_with("Couldn't open Gone"), "{err}");
        assert!(counts.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn nothing_starts_the_removed_launcher_window() {
        let entry = include_str!("../../packaging/desktop/app.hestia.Hestia.desktop");
        assert!(!entry.contains("--launch"), "the desktop entry must not start the removed launcher window");
        assert!(pane_of("search").is_some(), "the entry's Search action opens the in-app Search screen");
    }

    #[test]
    fn matches_rank_prefix_over_word_over_anywhere() {
        assert!(score("fire", "Firefox") > score("fire", "Backfire Tool"));
        assert!(score("tool", "Backfire Tool") > score("ire", "Backfire Tool"));
        assert_eq!(score("zzz", "Firefox"), 0);
    }

    #[test]
    fn desktop_entries_hide_nodisplay_and_non_apps() {
        let ok = "[Desktop Entry]\nType=Application\nName=Calc\nGenericName=Calculator\nKeywords=math;\n[Desktop Action X]\nName=Other\n";
        let a = parse_entry("calc.desktop", std::path::Path::new("/x/calc.desktop"), ok).unwrap();
        assert_eq!((a.name.as_str(), a.generic.as_str()), ("Calc", "Calculator"));
        assert!(parse_entry("h.desktop", std::path::Path::new("/x"), "[Desktop Entry]\nType=Application\nName=H\nNoDisplay=true\n").is_none());
        assert!(parse_entry("l.desktop", std::path::Path::new("/x"), "[Desktop Entry]\nType=Link\nName=L\n").is_none());
    }
}
