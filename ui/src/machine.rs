//! The machine: your whole system's configuration as Hestia captured it (dotfiles, config, packages,
//! GNOME settings), and how it differs from the machine right now. The captured copy lives in
//! `applications/linux/ubuntu/26.10/` of the repo, so every change is also a commit you can open in History.

use super::{style, State};
use crate::design::Surface;
use crate::repo::{DiffLine, Kind};
use rune::tok::{self, type_};
use rune::{col, row, text, El, ScrollSpec, Tx};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Cond {
    Same,
    Changed,
    /// captured, but not on this machine now
    Missing,
    /// on this machine, not captured yet
    New,
}

pub struct Item {
    pub name: String,
    pub cond: Cond,
    /// the capture-paths line (config and dotfiles only), so it can be removed
    pub path: Option<String>,
    pub diff: Vec<DiffLine>,
}

pub struct Cat {
    pub name: &'static str,
    pub items: Vec<Item>,
}

impl Cat {
    pub fn drift(&self) -> usize {
        self.items.iter().filter(|i| i.cond != Cond::Same).count()
    }
}

pub fn dir(repo: &Path) -> PathBuf {
    repo.join("applications/linux/ubuntu/26.10")
}

pub fn read_paths(dir: &Path) -> Vec<String> {
    std::fs::read_to_string(dir.join("capture-paths.txt")).unwrap_or_default().lines()
        .map(|l| l.split('#').next().unwrap_or("").trim().to_string()).filter(|l| !l.is_empty()).collect()
}

/// Where capture.sh stores a path: ~/.config/X in config/X, other files in dotfiles/.
pub fn dest_of(rel: &str) -> String {
    match rel.strip_prefix(".config/") {
        Some(r) => format!("config/{r}"),
        None => format!("dotfiles/{rel}"),
    }
}

/// Paths that must never be captured, however they are added (capture.sh refuses the same ones).
pub fn secret_path(rel: &str) -> bool {
    let lower = rel.to_lowercase();
    rel.contains("..")
        || [".ssh", ".gnupg", ".config/gh", ".config/claude", ".config/helium", ".config/goa-1.0", ".config/evolution"].iter().any(|p| lower.starts_with(p))
        || ["password", "secret", "token", ".pem", ".key", "id_rsa", "id_ed25519"].iter().any(|p| lower.contains(p))
        || lifestream::looks_secret(rel.rsplit('/').next().unwrap_or(rel))
}

/// "~/.vimrc", ".vimrc" or "/home/you/.vimrc" -> ".vimrc", if it is something we may capture.
pub fn normalize(input: &str, home: &Path) -> Result<String, String> {
    let t = input.trim().trim_end_matches('/');
    let rel = t.strip_prefix("~/").map(String::from)
        .or_else(|| Path::new(t).strip_prefix(home).ok().map(|p| p.to_string_lossy().into_owned()))
        .unwrap_or_else(|| t.trim_start_matches('/').to_string());
    if rel.is_empty() {
        return Err("type a path under your home folder, like ~/.vimrc".into());
    }
    if secret_path(&rel) {
        return Err(format!("~/{rel} looks like it holds secrets, so it is never captured"));
    }
    if !home.join(&rel).exists() {
        return Err(format!("~/{rel} does not exist"));
    }
    Ok(rel)
}

pub fn add_path(dir: &Path, rel: &str) -> std::io::Result<()> {
    let mut text = std::fs::read_to_string(dir.join("capture-paths.txt")).unwrap_or_default();
    if !text.ends_with('\n') && !text.is_empty() {
        text.push('\n');
    }
    text.push_str(&format!("~/{rel}\n"));
    std::fs::write(dir.join("capture-paths.txt"), text)
}

pub fn remove_path(dir: &Path, line: &str) -> std::io::Result<()> {
    let text = std::fs::read_to_string(dir.join("capture-paths.txt"))?;
    let kept: Vec<&str> = text.lines().filter(|l| l.split('#').next().unwrap_or("").trim() != line).collect();
    std::fs::write(dir.join("capture-paths.txt"), kept.join("\n") + "\n")
}

fn out(cmd: &str, args: &[&str]) -> String {
    Command::new(cmd).args(args).output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default()
}

fn lines(s: &str) -> BTreeSet<String> {
    s.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect()
}

/// `diff -u` text as numbered-free lines (file headers become hunk lines), capped.
pub fn parse_unified(text: &str, cap: usize) -> Vec<DiffLine> {
    text.lines().take(cap).map(|l| {
        let (kind, body) = if l.starts_with("+++") || l.starts_with("---") || l.starts_with("@@") || l.starts_with("diff ") || l.starts_with("Only in") {
            (Kind::Hunk, l)
        } else if let Some(r) = l.strip_prefix('+') {
            (Kind::Add, r)
        } else if let Some(r) = l.strip_prefix('-') {
            (Kind::Del, r)
        } else {
            (Kind::Ctx, l.strip_prefix(' ').unwrap_or(l))
        };
        DiffLine { kind, old: None, new: None, text: body.to_string() }
    }).collect()
}

fn compare_set(name: &'static str, captured: BTreeSet<String>, live: BTreeSet<String>, show_new: bool) -> Cat {
    let mut items: Vec<Item> = Vec::new();
    for n in live.difference(&captured).filter(|_| show_new) {
        items.push(Item { name: n.clone(), cond: Cond::New, path: None, diff: vec![] });
    }
    for n in captured.difference(&live) {
        items.push(Item { name: n.clone(), cond: Cond::Missing, path: None, diff: vec![] });
    }
    for n in captured.intersection(&live) {
        items.push(Item { name: n.clone(), cond: Cond::Same, path: None, diff: vec![] });
    }
    Cat { name, items }
}

/// Compare the captured copy with the live machine. Read-only.
pub fn scan(dir: &Path, home: &Path) -> Vec<Cat> {
    let mut files = Cat { name: "Config and dotfiles", items: vec![] };
    for p in read_paths(dir) {
        let Some(rel) = p.strip_prefix("~/") else { continue };
        let (captured, live) = (dir.join(dest_of(rel)), home.join(rel));
        let (cond, diff) = if !live.exists() {
            (Cond::Missing, vec![])
        } else if !captured.exists() {
            (Cond::New, vec![])
        } else {
            let d = out("diff", &["-ru", &captured.to_string_lossy(), &live.to_string_lossy()]);
            if d.is_empty() { (Cond::Same, vec![]) } else { (Cond::Changed, parse_unified(&d, 400)) }
        };
        files.items.push(Item { name: p.clone(), cond, path: Some(p), diff });
    }
    let read = |f: &str| std::fs::read_to_string(dir.join(f)).unwrap_or_default();
    let first = |s: &str| lines(&s.lines().map(|l| l.split_whitespace().next().unwrap_or("").to_string()).collect::<Vec<_>>().join("\n"));
    let packages = compare_set("Packages (apt)", lines(&read("apt-manual.txt")), lines(&out("apt-mark", &["showmanual"])), true);
    let snaps = compare_set("Snaps", first(&read("snap.txt")), first(&out("snap", &["list"]).lines().skip(1).collect::<Vec<_>>().join("\n")), false);
    let flatpaks = compare_set("Flatpaks", first(&read("flatpak.txt")), lines(&out("flatpak", &["list", "--app", "--columns=application"])), true);
    let live_dconf = out("dconf", &["dump", "/"]);
    let captured_dconf = read("dconf.ini");
    let gnome_diff = if live_dconf == captured_dconf { String::new() } else {
        let tmp = std::env::temp_dir().join(format!("hestia-dconf-{}", std::process::id()));
        let _ = std::fs::write(&tmp, &live_dconf);
        let d = out("diff", &["-u", &dir.join("dconf.ini").to_string_lossy(), &tmp.to_string_lossy()]);
        let _ = std::fs::remove_file(&tmp);
        d
    };
    let gnome = Cat { name: "GNOME settings", items: vec![Item {
        name: "Desktop settings (dconf)".into(),
        cond: if gnome_diff.is_empty() { Cond::Same } else { Cond::Changed },
        path: None, diff: parse_unified(&gnome_diff, 400),
    }] };
    let mut cats = vec![files, packages, snaps, flatpaks, gnome];
    for c in &mut cats {
        c.items.sort_by_key(|i| (i.cond == Cond::Same, i.name.clone()));
    }
    cats
}

fn say(c: Cond) -> &'static str {
    match c {
        Cond::Same => "matches the capture",
        Cond::Changed => "changed since captured",
        Cond::Missing => "captured, not on this machine now",
        Cond::New => "on this machine, not captured yet",
    }
}

impl State {
    pub(crate) fn reload_machine(&mut self) {
        self.mach = scan(&dir(&self.repo_dir), &super::files::home());
        self.mcat = self.mcat.min(self.mach.len().saturating_sub(1));
        self.mit = self.mit.min(self.mach.get(self.mcat).map(|c| c.items.len().saturating_sub(1)).unwrap_or(0));
    }
    /// "Capture now" is the same thing the watcher does, started by hand: capture, then commit each kind of change.
    fn capture_now(&mut self) {
        let script = std::env::current_exe().ok().and_then(|e| e.parent().and_then(|p| p.parent()).map(|p| p.join("libexec/hestia/watch.sh")));
        let Some(script) = script.filter(|p| p.exists()) else {
            self.msg = "the capture script is not installed: run Hestia's install.sh again".into();
            return;
        };
        let repo = self.repo_dir.clone();
        self.msg = "capturing this machine…".into();
        self.jobs.start("capture", Some(|s: &mut State| s.reload_machine()),
            move || super::jobs::run(Command::new("bash").arg(&script).arg(&repo).env("HESTIA_NOW", "1"), "the capture").map(|_| "captured. History shows what changed".to_string()));
    }
    fn add_capture(&mut self) {
        let d = dir(&self.repo_dir);
        match normalize(&self.f_cap.text, &super::files::home()) {
            Ok(rel) if read_paths(&d).contains(&format!("~/{rel}")) => self.msg = format!("~/{rel} is already captured"),
            Ok(rel) => match add_path(&d, &rel) {
                Ok(()) => {
                    self.f_cap.clear();
                    self.msg = format!("~/{rel} added. Run Capture now to copy it.");
                    self.reload_machine();
                }
                Err(e) => self.msg = format!("could not save: {e}"),
            },
            Err(e) => self.msg = e,
        }
    }
    fn remove_capture(&mut self, line: String) {
        let _ = remove_path(&dir(&self.repo_dir), &line);
        self.msg = format!("{line} will no longer be captured");
        self.reload_machine();
    }
}

pub fn panels(s: &State) -> Vec<El> {
    let d = s.design;
    let t = d.theme();
    let acc = d.accent_color();
    let scroll = || ScrollSpec { stick_bottom: false };

    let mut cats = col().w_full().gap(2.0);
    for (i, c) in s.mach.iter().enumerate() {
        let on = i == s.mcat;
        let drift = c.drift();
        cats = cats.child(
            row().key(format!("mcat-{i}")).h(d.row_h()).px(d.item_px()).gap(d.gap()).center_y().radius(d.r(tok::radius::SM))
                .fill(if on { acc } else { rune::Color::TRANSPARENT })
                .on_press(move |s: &mut State| { s.mcat = i; s.mit = 0 })
                .child(text(c.name.to_string()).font(style(&d, type_::LABEL)).color(if on { d.on_accent() } else { t.text }).grow(1.0))
                .child(text(if drift > 0 { format!("{drift} differ") } else { "ok".into() }).font(style(&d, type_::LABEL)).color(if on { d.on_accent() } else if drift > 0 { acc } else { t.text_secondary })),
        );
    }
    let left = col().w(240.0).padding(d.gap()).gap(d.gap()).side(&d)
        .child(text("Machine").font(style(&d, type_::TITLE)).color(t.text).px(d.item_px()).py(d.gap() * 0.5))
        .child(cats);

    let cat = s.mach.get(s.mcat);
    let mut list = col().w_full().gap(1.0);
    if let Some(c) = cat {
        for (i, it) in c.items.iter().enumerate() {
            let on = i == s.mit;
            list = list.child(
                row().key(format!("mitem-{i}")).h(d.row_h()).px(d.item_px()).gap(d.pad()).center_y().radius(d.r(tok::radius::SM))
                    .fill(if on { d.select() } else { rune::Color::TRANSPARENT })
                    .on_press(move |s: &mut State| { s.mit = i; s.msg.clear() })
                    .child(text(it.name.clone()).font(style(&d, type_::BODY)).color(t.text).grow(1.0).shrink().min_w(0.0).truncate())
                    .child(text(match it.cond { Cond::Same => "", Cond::Changed => "changed", Cond::Missing => "missing", Cond::New => "new" }.to_string())
                        .font(style(&d, type_::LABEL)).color(if it.cond == Cond::Same { t.text_secondary } else { acc })),
            );
        }
    }
    let btn = |key: &str, label: &str, primary: bool| {
        row().key(key.to_string()).h(d.row_h()).px(d.pad()).center_y().radius(d.r(tok::radius::SM))
            .fill(if primary { acc } else { rune::Color::TRANSPARENT }).border(1.0, if primary { acc } else { t.card_border })
            .child(text(label.to_string()).font(style(&d, type_::LABEL)).color(if primary { d.on_accent() } else { t.text }))
    };
    let middle = col().grow(1.0).shrink().min_w(270.0).padding(d.gap()).gap(d.gap()).panel(&d)
        .child(text(cat.map(|c| c.name).unwrap_or("")).font(style(&d, type_::TITLE)).color(t.text).px(d.item_px()).wrap_anywhere().shrink())
        .child(row().w_full().gap(d.gap()).center_y().px(d.item_px())
            .child(btn("m-refresh", "Refresh", false).on_press(|s: &mut State| s.reload_machine()))
            .child(super::questions::action(&d, "m-capture", "Capture now", "Capturing…", true, s.jobs.running("capture"), |s: &mut State| s.capture_now())))
        .child(text(if s.msg.is_empty() { "Compared with the copy saved in your repository. Capture now updates that copy; History shows what changed.".to_string() } else { s.msg.clone() })
            .font(style(&d, type_::LABEL)).color(t.text_secondary).px(d.item_px()).wrap_anywhere().shrink())
        .child(col().w_full().scroll(scroll()).shrink().min_h(0.0).grow(1.0).child(list));

    let mut detail = col().w(280.0).shrink().min_w(200.0).padding(d.pad()).gap(d.pad()).panel(&d);
    let mut body = col().w_full().shrink().min_w(0.0).gap(d.pad());
    if let Some(it) = cat.and_then(|c| c.items.get(s.mit)) {
        body = body.child(col().w_full().gap(4.0)
            .child(text(it.name.clone()).font(style(&d, type_::TITLE)).color(t.text).wrap_anywhere().shrink())
            .child(text(say(it.cond)).font(style(&d, type_::LABEL)).color(if it.cond == Cond::Same { t.text_secondary } else { acc })));
        if !it.diff.is_empty() {
            let (add_bg, del_bg) = (rune::oklcha(0.72, 0.15, 150.0, 0.17), rune::oklcha(0.62, 0.2, 25.0, 0.17));
            let mut lines = col().w_full();
            for l in &it.diff {
                let (bg, fg) = match l.kind { Kind::Add => (add_bg, t.text), Kind::Del => (del_bg, t.text), Kind::Hunk => (t.control_track, acc), Kind::Ctx => (rune::Color::TRANSPARENT, t.text_secondary) };
                lines = lines.child(row().w_full().px(d.gap()).fill(bg)
                    .child(text(l.text.clone()).font(style(&d, type_::MONO_CAPTION)).color(fg).grow(1.0).shrink().min_w(0.0).wrap_anywhere()));
            }
            body = body.child(col().w_full().gap(d.gap()).child(text("WHAT CHANGED (captured → now)").font(style(&d, type_::SECTION)).color(t.text_secondary)).child(lines));
        }
        if let Some(p) = it.path.clone() {
            body = body.child(btn("m-remove", "Stop capturing this", false).on_press(move |s: &mut State| s.remove_capture(p.clone())));
        }
    }
    if s.mcat == 0 {
        body = body.child(col().w_full().gap(d.gap())
            .child(text("CAPTURE ANOTHER FILE OR FOLDER").font(style(&d, type_::SECTION)).color(t.text_secondary).wrap_anywhere().shrink())
            .child(super::field(&d, "f-cap", &s.f_cap, "like ~/.vimrc or ~/.config/kitty ↵", |s: &mut State| &mut s.f_cap).on_submit(|s: &mut State, _: &mut Tx| s.add_capture()))
            .child(text("Anything that looks like it holds secrets (keys, tokens, passwords, browser and mail data) is refused.").font(style(&d, type_::LABEL)).color(t.text_secondary).wrap_anywhere().shrink()));
    }
    detail = detail.child(col().w_full().scroll(scroll()).shrink().min_h(0.0).grow(1.0).child(body));
    vec![left, middle, detail]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_are_refused_and_paths_normalised() {
        let home = Path::new("/home/u");
        for bad in [".ssh/id_rsa", ".gnupg", ".config/gh/hosts.yml", "notes/my password.txt", ".config/Claude/x", "a/../b"] {
            assert!(secret_path(bad), "{bad}");
        }
        assert!(!secret_path(".vimrc") && !secret_path(".config/kitty"));
        assert_eq!(dest_of(".config/htop"), "config/htop");
        assert_eq!(dest_of(".bashrc"), "dotfiles/.bashrc");
        assert!(normalize("~/.ssh", home).is_err());
        assert!(normalize("", home).is_err());
    }

    #[test]
    fn unified_diffs_are_coloured_by_kind() {
        let d = parse_unified("--- a\n+++ b\n@@ -1 +1 @@\n-old\n+new\n same\n", 50);
        let kinds: Vec<Kind> = d.iter().map(|l| l.kind).collect();
        assert_eq!(kinds, vec![Kind::Hunk, Kind::Hunk, Kind::Hunk, Kind::Del, Kind::Add, Kind::Ctx]);
    }

    #[test]
    fn sets_compare_into_new_missing_and_same() {
        let a: BTreeSet<String> = ["x", "y"].map(String::from).into();
        let b: BTreeSet<String> = ["y", "z"].map(String::from).into();
        let c = compare_set("t", a, b, true);
        let conds: Vec<(String, Cond)> = c.items.iter().map(|i| (i.name.clone(), i.cond)).collect();
        assert_eq!(conds, vec![("z".into(), Cond::New), ("x".into(), Cond::Missing), ("y".into(), Cond::Same)]);
    }
}
