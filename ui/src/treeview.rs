//! Folders: a real tree of the home folder. Expand and collapse, select, breadcrumbs, rename and move a
//! file, with Undo. Every move goes through the engine's mover (`lifestream::tree::apply`), which
//! never overwrites, refuses links and writes the journal that Undo reads. Nothing here deletes.

use super::{style, State};
use crate::design::Surface;
use crate::files::{self, home, tilde};
use rune::tok::{self, type_};
use rune::{col, row, text, El, ScrollSpec};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Row {
    pub path: PathBuf,
    pub name: String,
    pub depth: usize,
    pub is_dir: bool,
    pub open: bool,
    pub size: u64,
}

/// The visible rows: the root's children, and the children of every folder in `open`, depth first.
pub fn rows(root: &Path, open: &HashSet<PathBuf>, kinds: &HashMap<String, String>, hidden: bool) -> Vec<Row> {
    fn walk(dir: &Path, depth: usize, open: &HashSet<PathBuf>, kinds: &HashMap<String, String>, hidden: bool, out: &mut Vec<Row>) {
        for e in files::list(dir, kinds, hidden) {
            let is_open = e.is_dir && open.contains(&e.path);
            out.push(Row { path: e.path.clone(), name: e.name, depth, is_dir: e.is_dir, open: is_open, size: e.size });
            if is_open && depth < 12 {
                walk(&e.path, depth + 1, open, kinds, hidden, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, 0, open, kinds, hidden, &mut out);
    out
}

/// "~/Documents/Taxes" -> [("~", home), ("Documents", ~/Documents), ("Taxes", ~/Documents/Taxes)].
pub fn crumbs(path: &Path, home: &Path) -> Vec<(String, PathBuf)> {
    let mut out = vec![("Home".to_string(), home.to_path_buf())];
    if let Ok(rel) = path.strip_prefix(home) {
        let mut acc = home.to_path_buf();
        for c in rel.components() {
            acc.push(c);
            out.push((c.as_os_str().to_string_lossy().into_owned(), acc.clone()));
        }
    }
    out
}

/// Why a move must not happen, in words; `None` when it may go ahead. The engine's mover adds its own
/// refusals (overwrite, links); these are the ones about where things may go.
pub fn refuse(from: &Path, to: &Path, home: &Path) -> Option<String> {
    let rel = |p: &Path| p.strip_prefix(home).ok().map(Path::to_path_buf);
    let (Some(rf), Some(rt)) = (rel(from), rel(to)) else { return Some("Files can only be moved inside your home folder.".into()) };
    let hidden = |p: &Path| p.components().any(|c| c.as_os_str().to_string_lossy().starts_with('.'));
    if hidden(&rf) || hidden(&rt) {
        return Some("Hidden files and folders are left alone.".into());
    }
    if lifestream::deny::capture_refused(&rf.to_string_lossy()) || lifestream::deny::capture_refused(&rt.to_string_lossy()) {
        return Some("That place holds something private, so Hestia does not move things in or out of it.".into());
    }
    if from.is_dir() {
        return Some("Moving whole folders is not available from here yet.".into());
    }
    if to.exists() {
        return Some(format!("{} already exists there. Nothing was overwritten.", to.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
    }
    None
}

/// A name for a rename: one path component, not hidden, not empty.
pub fn valid_name(name: &str) -> Result<String, String> {
    let n = name.trim();
    if n.is_empty() {
        return Err("Type a new name.".into());
    }
    if n.contains('/') || n == "." || n == ".." {
        return Err("A name can't contain a slash.".into());
    }
    if n.starts_with('.') {
        return Err("A name can't start with a dot.".into());
    }
    Ok(n.to_string())
}

impl State {
    pub(crate) fn reload_tree(&mut self) {
        self.tree_rows = rows(&home(), &self.tree_open, &self.kinds, self.settings.hidden);
    }

    pub(crate) fn tree_toggle(&mut self, p: &Path) {
        if !self.tree_open.remove(p) {
            self.tree_open.insert(p.to_path_buf());
        }
        self.reload_tree();
    }

    pub(crate) fn tree_select(&mut self, p: PathBuf) {
        self.tree_rename.text = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        self.tree_sel = Some(p);
        self.msg.clear();
    }

    /// Move one file through the engine's mover, so it is journaled and Undo can put it back.
    fn tree_move(&mut self, from: &Path, to: &Path) {
        if let Some(why) = refuse(from, to, &home()) {
            self.msg = why;
            return;
        }
        let Some(stream) = self.stream.clone() else { self.msg = "There is no stream to record the move in.".into(); return };
        let id = self.recs.iter().find(|r| r.sources.iter().any(|x| *x == tilde(from))).map(|r| r.id.clone()).unwrap_or_default();
        let size = std::fs::metadata(from).map(|m| m.len()).unwrap_or(0);
        let plan = lifestream::tree::Plan { moves: vec![lifestream::tree::Move { id, from: from.to_path_buf(), to: to.to_path_buf(), size }], skipped: vec![] };
        match lifestream::tree::apply(&plan, &stream, &home()) {
            Ok((1, _)) => {
                self.recs = lifestream::load_all(&stream).unwrap_or_default();
                self.refresh_derived();
                self.tree_sel = Some(to.to_path_buf());
                self.tree_undoable = true;
                self.msg = format!("Moved {} to {}.", from.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), tilde(to.parent().unwrap_or(to)));
                self.reload_tree();
            }
            Ok((_, failed)) => self.msg = failed.first().cloned().unwrap_or_else(|| "Nothing was moved.".into()),
            Err(e) => self.msg = format!("Couldn't move it: {e}"),
        }
    }

    pub(crate) fn tree_rename(&mut self) {
        let Some(from) = self.tree_sel.clone() else { return };
        match valid_name(&self.tree_rename.text) {
            Ok(n) => {
                let to = from.with_file_name(n);
                self.tree_move(&from, &to);
            }
            Err(why) => self.msg = why,
        }
    }

    /// The folder was clicked while a file is waiting to be moved: put it inside.
    pub(crate) fn tree_drop(&mut self, dir: &Path) {
        let Some(from) = self.tree_moving.take() else { return };
        let to = dir.join(from.file_name().unwrap_or_default());
        self.tree_move(&from, &to);
    }
}

fn chevron(d: &crate::design::Design, r: &Row) -> El {
    let t = d.theme();
    let size = d.size(16.0);
    if r.is_dir {
        let p = r.path.clone();
        text(if r.open { "▾" } else { "▸" }.to_string()).font(style(d, type_::LABEL)).color(t.text_secondary).w(size).key(format!("tw-{}", r.path.display()))
            .on_press(move |s: &mut State| s.tree_toggle(&p))
    } else {
        col().size(size, size)
    }
}

pub fn panel(s: &State) -> El {
    let d = s.design;
    let t = d.theme();
    let sel = s.tree_sel.clone();

    let mut crumb_row = row().w_full().gap(d.gap()).center_y().flex_wrap();
    let at = sel.clone().unwrap_or_else(home);
    for (n, (label, p)) in crumbs(sel.as_deref().and_then(|x| x.parent()).unwrap_or(&at), &home()).into_iter().enumerate() {
        if n > 0 {
            crumb_row = crumb_row.child(text("›".to_string()).font(style(&d, type_::LABEL)).color(t.text_secondary));
        }
        crumb_row = crumb_row.child(text(label).font(style(&d, type_::LABEL)).color(t.text_secondary).key(format!("crumb-{}", p.display()))
            .on_press(move |s: &mut State| { s.tree_open.insert(p.clone()); s.reload_tree(); }));
    }

    let moving = s.tree_moving.clone();
    let banner = moving.as_ref().map(|m| row().w_full().px(d.item_px()).py(d.gap()).gap(d.gap()).center_y().tile(&d)
        .child(text(format!("Click the folder to put “{}” in.", m.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())).font(style(&d, type_::LABEL)).color(t.text).grow(1.0).shrink().min_w(0.0).wrap_anywhere())
        .child(crate::questions::button(&d, "tv-cancel", "Cancel".into(), false, |s: &mut State| s.tree_moving = None)));

    let mut list = col().w_full().gap(1.0);
    for r in &s.tree_rows {
        let on = sel.as_deref() == Some(r.path.as_path());
        let target = r.is_dir && moving.is_some();
        let p = r.path.clone();
        let icon = rune::icon(if r.is_dir { &crate::icons::FOLDER } else { &crate::icons::DOC }, d.size(16.0)).color(if r.is_dir { d.accent_color() } else { t.text_secondary });
        list = list.child(
            row().key(format!("tr-{}", r.path.display())).w_full().h(d.row_h()).pl(d.item_px() + r.depth as f32 * d.size(18.0)).pr(d.item_px()).gap(d.gap()).center_y().radius(d.r(tok::radius::SM))
                .fill(if on { d.select() } else { rune::Color::TRANSPARENT })
                .border(1.0, if target { d.accent_color().alpha(0.45) } else { rune::Color::TRANSPARENT })
                .on_press(move |s: &mut State| { if s.tree_moving.is_some() { let d = if p.is_dir() { p.clone() } else { p.parent().map(Path::to_path_buf).unwrap_or_default() }; s.tree_drop(&d) } else { s.tree_select(p.clone()) } })
                .child(chevron(&d, r))
                .child(icon)
                .child(text(r.name.clone()).font(style(&d, type_::BODY)).color(t.text).grow(1.0).shrink().min_w(0.0).truncate())
                .child(text(if r.is_dir { String::new() } else { lifestream::human(r.size) }).font(style(&d, type_::LABEL)).color(t.text_secondary)),
        );
    }
    if s.tree_rows.is_empty() {
        list = list.child(text("Nothing to show in this folder.".to_string()).font(style(&d, type_::LABEL)).color(t.text_secondary).px(d.item_px()).py(d.pad()));
    }

    let mut center = col().grow(1.0).shrink().min_w(0.0).padding(d.pad()).gap(d.gap()).panel(&d)
        .child(col().w_full().gap(2.0)
            .child(text("Folders").font(style(&d, type_::TITLE)).color(t.text))
            .child(text("Your home folder. Rename or move a file; Undo puts the last move back. Nothing is ever deleted.".to_string()).font(style(&d, type_::LABEL)).color(t.text_secondary).wrap_anywhere().shrink()))
        .child(crumb_row);
    if let Some(b) = banner {
        center = center.child(b);
    }
    let center = center.child(col().w_full().scroll(ScrollSpec { stick_bottom: false }).shrink().min_h(0.0).grow(1.0).child(list));

    let mut detail = col().w(d.size(300.0)).shrink().min_w(220.0).padding(d.pad()).gap(d.pad()).panel(&d);
    match sel.as_ref().and_then(|p| s.tree_rows.iter().find(|r| &r.path == p)) {
        Some(r) => {
            let planned = s.tplan.as_ref().map(|pl| pl.moves.iter().filter(|m| m.from.starts_with(&r.path)).count()).unwrap_or(0);
            detail = detail
                .child(text(r.name.clone()).font(style(&d, type_::TITLE)).color(t.text).wrap_anywhere().shrink())
                .child(text(tilde(&r.path)).font(style(&d, type_::MONO_CAPTION)).color(t.text_secondary).wrap_anywhere().shrink())
                .child(text(if r.is_dir { "Folder".to_string() } else { format!("File · {}", lifestream::human(r.size)) }).font(style(&d, type_::LABEL)).color(t.text_secondary));
            if r.is_dir {
                if planned > 0 {
                    detail = detail.child(text(format!("{} would be filed from here tonight.", crate::today::count(planned, "file", "files"))).font(style(&d, type_::LABEL)).color(t.text).wrap_anywhere().shrink())
                        .child(row().child(crate::questions::button(&d, "tv-review", "Review in Files".into(), false, |s: &mut State| { s.pane = super::Pane::Files; s.open_files() })));
                } else {
                    detail = detail.child(text("Nothing waiting to be filed from here.".to_string()).font(style(&d, type_::LABEL)).color(t.text_secondary));
                }
            } else {
                detail = detail
                    .child(super::field(&d, "tv-name", &s.tree_rename, "New name", |s: &mut State| &mut s.tree_rename).on_submit(|s: &mut State, _: &mut rune::Tx| s.tree_rename()))
                    .child(row().w_full().gap(d.gap()).flex_wrap()
                        .child(crate::questions::button(&d, "tv-rename", "Rename".into(), true, |s: &mut State| s.tree_rename()))
                        .child(crate::questions::button(&d, "tv-move", "Move to…".into(), false, { let p = r.path.clone(); move |s: &mut State| { s.tree_moving = Some(p.clone()); s.msg.clear() } })));
            }
        }
        None => detail = detail.child(text("Select a file or folder.".to_string()).font(style(&d, type_::LABEL)).color(t.text_secondary)),
    }
    if s.tree_undoable {
        detail = detail.child(row().child(crate::questions::button(&d, "tv-undo", "Undo last move".into(), false, |s: &mut State| { s.tree_undoable = false; s.undo_filing() })));
    }
    vec![center, detail].into_iter().fold(row().grow(1.0).min_w(0.0).min_h(0.0), |r, c| r.child(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("hestia-tree-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn rows_follow_what_is_expanded() {
        let root = scratch("rows");
        std::fs::create_dir_all(root.join("a/b")).unwrap();
        std::fs::write(root.join("a/one.txt"), "x").unwrap();
        std::fs::write(root.join("top.txt"), "x").unwrap();
        let none = HashMap::new();
        let closed: Vec<_> = rows(&root, &HashSet::new(), &none, false).into_iter().map(|r| r.name).collect();
        assert_eq!(closed, ["a", "top.txt"]);
        let open: HashSet<PathBuf> = [root.join("a")].into();
        let r = rows(&root, &open, &none, false);
        assert_eq!(r.iter().map(|x| (x.name.as_str(), x.depth)).collect::<Vec<_>>(), [("a", 0), ("b", 1), ("one.txt", 1), ("top.txt", 0)]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn crumbs_walk_down_from_home() {
        let h = Path::new("/home/u");
        let c = crumbs(Path::new("/home/u/Documents/Taxes"), h);
        assert_eq!(c.iter().map(|x| x.0.as_str()).collect::<Vec<_>>(), ["Home", "Documents", "Taxes"]);
        assert_eq!(c[2].1, Path::new("/home/u/Documents/Taxes"));
    }

    #[test]
    fn moves_are_refused_outside_home_into_hidden_or_private_places_and_over_existing_files() {
        let home = scratch("refuse");
        std::fs::write(home.join("a.txt"), "x").unwrap();
        std::fs::write(home.join("b.txt"), "x").unwrap();
        std::fs::create_dir_all(home.join("dir")).unwrap();
        assert!(refuse(&home.join("a.txt"), Path::new("/tmp/x/a.txt"), &home).is_some());
        assert!(refuse(&home.join("a.txt"), &home.join(".cache/a.txt"), &home).is_some());
        assert!(refuse(&home.join("a.txt"), &home.join(".ssh/a.txt"), &home).is_some());
        assert!(refuse(&home.join("a.txt"), &home.join("b.txt"), &home).is_some());
        assert!(refuse(&home.join("dir"), &home.join("dir2"), &home).is_some());
        assert!(refuse(&home.join("a.txt"), &home.join("dir/a.txt"), &home).is_none());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn names_for_a_rename_are_one_plain_component() {
        assert!(valid_name("  ").is_err() && valid_name("a/b").is_err() && valid_name("..").is_err() && valid_name(".hid").is_err());
        assert_eq!(valid_name(" new name.txt ").unwrap(), "new name.txt");
    }

    #[test]
    fn a_move_goes_through_the_engine_and_undo_puts_it_back() {
        let home = scratch("engine");
        let stream = home.join(".stream");
        std::fs::write(home.join("a.txt"), "hello").unwrap();
        std::fs::create_dir_all(home.join("dir")).unwrap();
        let plan = lifestream::tree::Plan { moves: vec![lifestream::tree::Move { id: String::new(), from: home.join("a.txt"), to: home.join("dir/a.txt"), size: 5 }], skipped: vec![] };
        let (n, failed) = lifestream::tree::apply(&plan, &stream, &home).unwrap();
        assert_eq!((n, failed.len()), (1, 0));
        assert!(!home.join("a.txt").exists() && home.join("dir/a.txt").exists());
        let r = lifestream::tree::undo_last(&stream, &home).unwrap();
        assert_eq!(r.restored, 1);
        assert!(home.join("a.txt").exists() && !home.join("dir/a.txt").exists());
        let _ = std::fs::remove_dir_all(&home);
    }
}
