//! Files: where your files live, grouped by domain (documents, pictures, ...), as a tree you can see, edit and
//! reorganise, with each folder's contents beside its rule. One screen for folders and files. Suggested
//! folders (from the two models) appear dashed in the tree; the right side says what each folder
//! accepts and what would land in it tonight. Nothing moves on disk without a confirmed click.

use super::{field, icons, style, State};
use crate::design::{Design, Surface};
use crate::questions::{button, chip, heading};
use lifestream::human;
use lifestream::tree::{self, Folder};
use rune::tok::{self, type_};
use rune::{col, row, text, El, ScrollSpec, TextEdit, TextStyle, Tx};
use std::collections::BTreeMap;

fn csv(v: &[String]) -> String {
    v.join(", ")
}

fn split(t: &str) -> Vec<String> {
    t.split([',', ' ']).map(str::trim).filter(|x| !x.is_empty()).map(String::from).collect()
}

fn summary(f: &Folder) -> String {
    let mut v = Vec::new();
    if !f.kind.is_empty() {
        v.push(f.kind.iter().map(|k| crate::kinds::label(k)).collect::<Vec<_>>().join(", "));
    }
    if !f.tag.is_empty() {
        v.push(format!("tags {}", f.tag.join(" ")));
    }
    if let Some(n) = &f.name {
        v.push(format!("name /{n}/"));
    }
    if f.by_year {
        v.push("by year".into());
    }
    if v.is_empty() { "structure only".into() } else { v.join("  ·  ") }
}

/// "software/app-bundle" -> "Software/App-Bundle"
pub fn path_for(kind: &str) -> String {
    kind.split('/').map(|p| p.split('-').map(|w| { let mut c = w.chars(); c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default() }).collect::<Vec<_>>().join("-")).collect::<Vec<_>>().join("/")
}

pub struct Suggestion {
    pub kind: String,
    pub path: String,
    pub folders: usize,
}

fn accepts(tree: &tree::Tree, kind: &str) -> bool {
    tree.folders.iter().any(|f| f.kind.iter().any(|k| kind == k || kind.starts_with(&format!("{k}/"))))
}

/// Kinds both models agree on that no folder in the tree accepts yet, and that were not dismissed.
pub fn suggestions(s: &State) -> Vec<Suggestion> {
    let mut by: BTreeMap<String, usize> = BTreeMap::new();
    for c in s.suggestions.values().filter(|c| c.agree()) {
        let kind = c.suggestions[0].kind.clone();
        if kind != "unsure" && !accepts(&s.tree, &kind) && !s.settings.dismissed.contains(&kind) {
            *by.entry(kind).or_default() += 1;
        }
    }
    let mut out: Vec<Suggestion> = by.into_iter().map(|(kind, folders)| Suggestion { path: path_for(&kind), kind, folders }).collect();
    out.sort_by_key(|x| std::cmp::Reverse(x.folders));
    out
}

/// The tree edit behind Accept: a folder that already exists learns the kind; otherwise it is added.
/// Returns the folder's index and whether it already existed.
fn accept_into(tree: &mut tree::Tree, sg: &Suggestion) -> Result<(usize, bool), String> {
    if let Some(i) = tree.folders.iter().position(|f| f.path == sg.path) {
        if !tree.folders[i].kind.contains(&sg.kind) {
            tree.folders[i].kind.push(sg.kind.clone());
        }
        return Ok((i, true));
    }
    tree.add_folder(Folder { path: sg.path.clone(), kind: vec![sg.kind.clone()], ..Default::default() }).map_err(|e| format!("{e:#}"))?;
    Ok((tree.folders.len() - 1, false))
}

impl State {
    pub(crate) fn select_folder(&mut self, i: usize) {
        self.tsel = i;
        self.msg.clear();
        if let Some(f) = self.tree.folders.get(i).cloned() {
            self.go(super::files::home().join(&f.path));
            self.f_path = TextEdit::new(f.path.rsplit('/').next().unwrap_or(&f.path));
            self.f_kind = TextEdit::new(csv(&f.kind));
            self.f_tag = TextEdit::new(csv(&f.tag));
            self.f_name = TextEdit::new(f.name.as_deref().unwrap_or(""));
            self.f_year = f.by_year;
        }
    }
    pub(crate) fn refresh_plan(&mut self) {
        self.tplan = tree::preview(&self.recs, &self.tree, &self.repo_dir).ok();
        self.confirm_move = false;
    }
    fn save_tree(&mut self) -> bool {
        match self.tree.save(&self.tree_path) {
            Ok(()) => {
                self.refresh_plan();
                true
            }
            Err(e) => {
                self.msg = format!("not saved: {e:#}");
                false
            }
        }
    }
    fn save_folder(&mut self) {
        let Some(f) = self.tree.folders.get(self.tsel).cloned() else { return };
        let name = self.f_path.text.trim().trim_matches('/').to_string();
        let parent = f.path.rsplit_once('/').map(|x| x.0.to_string());
        let new_path = match (&parent, name.is_empty()) {
            (_, true) => f.path.clone(),
            (Some(p), false) => format!("{p}/{name}"),
            (None, false) => name.clone(),
        };
        // the rules first, under the current path
        {
            let g = &mut self.tree.folders[self.tsel];
            g.kind = split(&self.f_kind.text);
            g.tag = split(&self.f_tag.text);
            g.name = Some(self.f_name.text.trim().to_string()).filter(|n| !n.is_empty());
            g.by_year = self.f_year;
        }
        if new_path != f.path {
            let idx = self.tsel;
            if self.apply_move(&f.path, &new_path) {
                // the folder now lives at its new path; stay on it
                if let Some(i) = self.tree.folders.iter().position(|x| x.path == new_path) {
                    self.tsel = i;
                    let _ = idx;
                }
            } else {
                self.tree.folders[self.tsel] = f; // keep the tree as it was
            }
            return;
        }
        if self.save_tree() {
            self.msg = "saved".into();
        } else {
            self.tree.folders[self.tsel] = f;
        }
    }
    /// The one way a folder is moved or renamed: the tree and the real folder together.
    fn apply_move(&mut self, from: &str, to: &str) -> bool {
        if let Err(e) = self.tree.move_folder(from, to) {
            self.msg = format!("not moved: {e:#}");
            return false;
        }
        if !self.save_tree() {
            return false;
        }
        let Some(stream) = self.stream.clone() else { return true };
        let excluded = tree::excluded(&self.repo_dir).unwrap_or_default();
        match tree::move_on_disk(&super::files::home(), from, to, &excluded, &stream) {
            Ok(()) => {
                self.recs = lifestream::load_all(&stream).unwrap_or_default();
                self.refresh_derived();
                self.refresh_plan();
                self.msg = format!("moved ~/{from} → ~/{to}, the folder and the tree together");
            }
            Err(e) => self.msg = format!("the tree now says ~/{to}, but the real folder was not moved: {e:#}"),
        }
        true
    }
    fn add_folder(&mut self) {
        let path = self.f_new.text.trim().trim_matches('/').to_string();
        if path.is_empty() {
            return;
        }
        match self.tree.add_folder(Folder { path, ..Default::default() }) {
            Ok(()) if self.save_tree() => {
                let _ = std::fs::create_dir_all(super::files::home().join(self.tree.folders.last().map(|f| f.path.as_str()).unwrap_or("")));
                self.f_new.clear();
                let last = self.tree.folders.len() - 1;
                self.select_folder(last);
                self.msg = "folder added".into();
            }
            Ok(()) => {
                self.tree.folders.pop();
            }
            Err(e) => self.msg = format!("{e:#}"),
        }
    }
    fn delete_folder(&mut self) {
        if self.tsel < self.tree.folders.len() {
            let gone = self.tree.folders.remove(self.tsel);
            if self.save_tree() {
                self.select_folder(self.tsel.min(self.tree.folders.len().saturating_sub(1)));
                self.msg = format!("{} is no longer used by Hestia (your files were not touched)", gone.path);
            } else {
                self.tree.folders.insert(self.tsel, gone);
            }
        }
    }
    fn add_inbox(&mut self) {
        let p = self.f_inbox.text.trim().trim_matches('/').to_string();
        if p.is_empty() || self.tree.inbox.contains(&p) {
            return;
        }
        let scanned = self.settings.roots.iter().any(|r| p == *r || p.starts_with(&format!("{r}/")));
        if !scanned {
            self.msg = format!("~/{p} is not read by Hestia yet. Add it under Settings, Scanning first; an inbox must be inside a scanned folder.");
            return;
        }
        self.tree.inbox.push(p);
        self.f_inbox.clear();
        self.save_tree();
    }
    fn remove_inbox(&mut self, i: usize) {
        if i < self.tree.inbox.len() {
            self.tree.inbox.remove(i);
            self.save_tree();
        }
    }
    /// Accept a suggested folder. If that folder already exists in the tree it learns the kind;
    /// otherwise it is added with a rule for the kind and created on disk. Either way you land on it.
    fn accept(&mut self, sg: &Suggestion) {
        let (idx, existing) = match accept_into(&mut self.tree, sg) {
            Ok(r) => r,
            Err(e) => {
                self.msg = e;
                return;
            }
        };
        let _ = std::fs::create_dir_all(super::files::home().join(&sg.path));
        if !self.save_tree() {
            return;
        }
        self.select_folder(idx);
        let here = self.tplan.as_ref().map(|p| p.moves.iter().filter(|m| m.to.starts_with(super::files::home().join(&sg.path))).count()).unwrap_or(0);
        self.msg = if existing {
            format!("{} now accepts {} · {} files would land there tonight", sg.path, sg.kind, here)
        } else {
            format!("added {} (accepts {}) · {} files would land there tonight", sg.path, sg.kind, here)
        };
    }
    fn dismiss(&mut self, kind: String) {
        self.change_settings(|st| st.dismissed.push(kind));
    }
    /// Click-to-move: choose where the folder goes, then confirm once. The tree and the real folder move together.
    fn finish_move(&mut self, dest: &str) {
        let Some(i) = self.moving.take() else { return };
        let Some(from) = self.tree.folders.get(i).map(|f| f.path.clone()) else { return };
        let name = from.rsplit('/').next().unwrap_or(&from).to_string();
        let to = if dest.is_empty() { name } else { format!("{dest}/{name}") };
        if to != from {
            self.pending_move = Some((from, to));
        }
    }
    fn confirm_move_folder(&mut self) {
        if let Some((from, to)) = self.pending_move.take() {
            self.apply_move(&from, &to);
        }
    }
    fn create_dirs(&mut self) {
        let home = super::files::home();
        for d in self.tree.dirs() {
            let _ = std::fs::create_dir_all(home.join(d));
        }
    }
    /// File everything the plan says, as a job: the screen stays alive and the button says what it is doing.
    fn file_now(&mut self) {
        let Some(stream) = self.stream.clone() else { return };
        self.confirm_move = false;
        self.undo_lines.clear();
        self.create_dirs();
        let (recs, tree, repo, home) = (self.recs.clone(), self.tree.clone(), self.repo_dir.clone(), super::files::home());
        self.msg = "moving your files…".into();
        self.jobs.start("file", Some(reload_after_filing), move || {
            let (n, failed) = tree::preview(&recs, &tree, &repo).and_then(|p| tree::apply(&p, &stream, &home)).map_err(|e| format!("could not move: {e:#}"))?;
            Ok(format!("{n}\u{1f}{}", failed.len()))
        });
    }

    /// Put the last batch back, as a job. What could not go back is listed, one calm line each.
    pub(crate) fn undo_filing(&mut self) {
        let Some(stream) = self.stream.clone() else { return };
        self.undo_lines.clear();
        self.msg = "putting files back…".into();
        let inbox = self.undo_inbox.clone();
        self.jobs.start("undo", Some(after_undo), move || {
            let u = crate::engine::undo(&stream)?;
            *inbox.lock().unwrap() = u.skipped.iter().map(|(p, why)| format!("{p}: {why}")).collect();
            Ok(if u.restored == 0 && u.skipped.is_empty() { "Nothing to put back.".to_string() } else { format!("Put back {}", crate::today::count(u.restored, "file", "files")) })
        });
    }
}

/// After a filing job: the engine moved the files and wrote the stream; load it and show Undo.
fn reload_after_filing(s: &mut State) {
    if let Some(stream) = s.stream.clone() {
        s.recs = lifestream::load_all(&stream).unwrap_or_default();
    }
    s.refresh_derived();
    s.refresh_plan();
    let (n, failed) = s.msg.split_once('\u{1f}').map(|(a, b)| (a.parse().unwrap_or(0), b.parse().unwrap_or(0))).unwrap_or((0usize, 0usize));
    s.last_filed = n;
    s.msg = format!("Moved {}{}. You can put them back.", crate::today::count(n, "file", "files"), if failed > 0 { format!(", {failed} could not be moved") } else { String::new() });
}

fn after_undo(s: &mut State) {
    if let Some(stream) = s.stream.clone() {
        s.recs = lifestream::load_all(&stream).unwrap_or_default();
    }
    s.refresh_derived();
    s.refresh_plan();
    s.last_filed = 0;
    s.undo_lines = std::mem::take(&mut *s.undo_inbox.lock().unwrap());
}

fn toggle(d: &Design, on: bool) -> El {
    let t = d.theme();
    row().gap(d.gap()).center_y()
        .child(text(if on { "☑" } else { "☐" }).font(style(d, type_::TITLE)).color(d.accent_color()))
        .child(text("File into a folder per year").font(style(d, type_::LABEL)).color(t.text))
}

pub fn panels(s: &State) -> Vec<El> {
    let d = s.design;
    let t = d.theme();
    let acc = d.accent_color();
    let scroll = || ScrollSpec { stick_bottom: false };
    let home = super::files::home();

    // how many files would land in each folder tonight
    let mut tonight: BTreeMap<String, usize> = BTreeMap::new();
    if let Some(p) = &s.tplan {
        for m in &p.moves {
            if let Some(dest) = m.to.parent().and_then(|x| x.strip_prefix(&home).ok()) {
                let mut acc_path = String::new();
                for part in dest.to_string_lossy().split('/') {
                    if !acc_path.is_empty() {
                        acc_path.push('/');
                    }
                    acc_path.push_str(part);
                    *tonight.entry(acc_path.clone()).or_default() += 1;
                }
            }
        }
    }

    // ---- the tree
    let mut inbox = row().w_full().shrink().min_w(0.0).gaps(d.gap(), d.gap() * 0.5).flex_wrap();
    for (i, p) in s.tree.inbox.iter().enumerate() {
        inbox = inbox.child(
            row().key(format!("inbox-{i}")).h(d.size(26.0)).px(d.gap()).gap(4.0).center_y().radius(d.r(tok::radius::SM)).border(1.0, t.card_border)
                .on_press(move |s: &mut State| s.remove_inbox(i))
                .child(text(format!("{p}  ×")).font(style(&d, type_::LABEL)).color(t.text)),
        );
    }
    let mut tree_col = col().w_full().gap(1.0);
    let all_dirs = s.tree.dirs();
    let mut shown: std::collections::HashSet<String> = std::collections::HashSet::new();
    // domains first, each with the folders under it; anything left over goes under "Other"
    let mut groups: Vec<(String, String, Vec<String>)> = Vec::new();
    for dm in s.domains.iter().filter(|x| !x.folder.is_empty()) {
        let list: Vec<String> = all_dirs.iter().filter(|p| **p == dm.folder || p.starts_with(&format!("{}/", dm.folder))).cloned().collect();
        groups.push((dm.name.clone(), dm.description.clone(), list));
    }
    let rest: Vec<String> = all_dirs.iter().filter(|p| !groups.iter().any(|g| g.2.contains(p))).cloned().collect();
    if !rest.is_empty() {
        groups.push(("Other".into(), "Folders that belong to no domain yet.".into(), rest));
    }
    let mut n = 0usize;
    for (gname, gdesc, list) in groups {
        if list.is_empty() {
            continue;
        }
        tree_col = tree_col.child(col().w_full().px(d.item_px()).pt(d.pad()).pb(d.gap() * 0.5).gap(2.0)
            .child(text(gname.clone()).font(style(&d, type_::SECTION)).color(t.text_secondary))
            .child(text(gdesc).font(style(&d, type_::LABEL)).color(t.text_secondary).wrap_anywhere().shrink()));
        for dir in list {
            if !shown.insert(dir.clone()) {
                continue;
            }
            n += 1;
            let idx = s.tree.folders.iter().position(|f| f.path == dir);
            let on = idx.is_some() && idx == Some(s.tsel);
            let depth = dir.matches('/').count() as f32;
            let name = dir.rsplit('/').next().unwrap_or(&dir).to_string();
            let count = tonight.get(&dir).copied().unwrap_or(0);
            let moving_here = s.moving.is_some();
            let target = dir.clone();
            tree_col = tree_col.child(
                row().key(format!("dir-{n}")).w_full().min_h(d.row_h()).py(d.gap() * 0.5).pl(d.item_px() + depth * 16.0).pr(d.item_px()).gap(d.gap()).center_y().radius(d.r(tok::radius::SM))
                    .fill(if on { d.select() } else { rune::Color::TRANSPARENT })
                    .border(1.0, if moving_here { acc.alpha(0.45) } else { rune::Color::TRANSPARENT })
                    .on_press(move |s: &mut State| {
                        if s.moving.is_some() { s.finish_move(&target) } else if let Some(i) = idx { s.select_folder(i) }
                    })
                    .child(rune::icon(&icons::FOLDER, d.size(16.0)).color(if idx.is_some() { acc } else { t.text_secondary }))
                    .child(col().grow(1.0).shrink().min_w(0.0).gap(1.0)
                        .child(text(name).font(style(&d, type_::BODY)).color(if idx.is_some() { t.text } else { t.text_secondary }).truncate())
                        .child(if let Some(i) = idx { text(summary(&s.tree.folders[i])).font(style(&d, type_::LABEL)).color(t.text_secondary).truncate() } else { text(String::new()).font(style(&d, type_::LABEL)) }))
                    .child(text(if count > 0 { format!("{count} tonight") } else { String::new() }).font(style(&d, type_::LABEL)).color(acc)),
            );
        }
    }
    // suggested folders: dashed, one click to accept
    let sugg = suggestions(s);
    let mut sugg_col = col().w_full().gap(d.gap());
    if !sugg.is_empty() {
        sugg_col = sugg_col.child(row().w_full().gap(d.gap()).center_y()
            .child(heading(&d, "Suggested folders").grow(1.0))
            .child(button(&d, "sg-all", format!("Accept all {}", sugg.len()), false, |s: &mut State| {
                for sg in suggestions(s) { s.accept(&sg); }
            })));
    }
    for sg in sugg.iter() {
        let (k1, k2) = (sg.kind.clone(), sg.kind.clone());
        let sg2 = Suggestion { kind: sg.kind.clone(), path: sg.path.clone(), folders: sg.folders };
        sugg_col = sugg_col.child(
            row().key(format!("sg-{}", sg.kind)).w_full().px(d.item_px()).py(d.gap()).gap(d.gap()).center_y().radius(d.r(tok::radius::SM)).border(1.0, acc).border_dash(4.0, 3.0)
                .child(rune::icon(&icons::FOLDER, d.size(16.0)).color(acc))
                .child(col().grow(1.0).shrink().min_w(0.0).gap(2.0)
                    .child(text(sg.path.clone()).font(style(&d, type_::BODY)).color(t.text).truncate())
                    .child(text(format!("suggested for {} · {} folders agree", sg.kind, sg.folders)).font(style(&d, type_::LABEL)).color(t.text_secondary).truncate()))
                .child(button(&d, &format!("sg-accept-{}", sg.kind), "Accept".into(), true, move |s: &mut State| s.accept(&sg2)))
                .child(button(&d, &format!("sg-dismiss-{}", sg.kind), "Dismiss".into(), false, move |s: &mut State| { let _ = &k1; s.dismiss(k2.clone()) })),
        );
    }

    // the bottom bar: what "file now" will do
    let n_moves = s.tplan.as_ref().map(|p| p.moves.len()).unwrap_or(0);
    let bar = row().w_full().px(d.item_px()).py(d.gap()).gap(d.gap()).center_y()
        .child(text(if n_moves == 0 { "Nothing to file".to_string() } else { format!("{} would move tonight", crate::today::count(n_moves, "file", "files")) }).font(style(&d, type_::LABEL)).color(t.text_secondary).grow(1.0).shrink().min_w(0.0).truncate())
        .child(crate::questions::action(&d, "f-now", "File them now…", "Moving files…", n_moves > 0, s.jobs.running("file"), |s: &mut State| s.confirm_move = true));

    let banner = s.moving.and_then(|i| s.tree.folders.get(i)).map(|f| row().w_full().px(d.item_px()).py(d.gap()).gap(d.gap()).center_y().tile(&d)
            .child(text(format!("Click the folder to put “{}” inside, or the top level:", f.path)).font(style(&d, type_::LABEL)).color(t.text).grow(1.0).shrink().min_w(0.0).wrap_anywhere())
            .child(button(&d, "mv-top", "Top level".into(), false, |s: &mut State| s.finish_move("")))
            .child(button(&d, "mv-cancel", "Cancel".into(), false, |s: &mut State| s.moving = None)));

    let mut center = col().grow(1.0).shrink().min_w(0.0).padding(d.gap()).gap(d.gap()).panel(&d)
        .child(col().w_full().px(d.item_px()).py(d.gap() * 0.5).gap(d.gap())
            .child(text("Rules").font(style(&d, type_::TITLE)).color(t.text))
            .child(text("Choose a folder to see what goes in it. Projects and special folders are left alone.").font(style(&d, type_::LABEL)).color(t.text_secondary).wrap_anywhere().shrink())
            .child(heading(&d, "Watched folders"))
            .child(text("New files that land here get sorted. Click one to stop watching it; nothing is moved.".to_string()).font(style(&d, type_::LABEL)).color(t.text_secondary).wrap_anywhere().shrink())
            .child(inbox)
            .child(field(&d, "inbox-add", &s.f_inbox, "Add a folder to watch, like Downloads", |s: &mut State| &mut s.f_inbox).on_submit(|s: &mut State, _: &mut Tx| s.add_inbox())));
    if let Some(b) = banner {
        center = center.child(b);
    }
    let center = center
        .child(col().w_full().scroll(scroll()).shrink().min_h(0.0).grow(1.0).child(col().w_full().gap(d.pad()).child(tree_col).child(sugg_col)))
        .child(bar);

    // ---- the folder, and what would land in it
    let mut body = col().w_full().shrink().min_w(0.0).gap(d.pad());
    if let Some(f) = s.tree.folders.get(s.tsel) {
        let here = tonight.get(&f.path).copied().unwrap_or(0);
        body = body.child(col().w_full().gap(d.gap())
            .child(text(f.path.clone()).font(TextStyle { size: d.size(16.0), ..style(&d, type_::TITLE) }).color(t.text).wrap_anywhere().shrink())
            .child(text(if here > 0 { format!("{here} files will be moved here tonight") } else { "Nothing will be moved here tonight.".to_string() }).font(style(&d, type_::LABEL)).color(if here > 0 { acc } else { t.text_secondary }))
            .child(heading(&d, "What goes in this folder"))
            .child(text(format!("~/{}", f.path)).font(style(&d, type_::MONO_CAPTION)).color(t.text_secondary).wrap_anywhere().shrink())
            .child(field(&d, "f-path", &s.f_path, "rename this folder (its contents come along)", |s: &mut State| &mut s.f_path))
            .child(field(&d, "f-kind", &s.f_kind, "kinds, like image, video/camera", |s: &mut State| &mut s.f_kind))
            .child(field(&d, "f-tag", &s.f_tag, "Only files with these labels (optional)", |s: &mut State| &mut s.f_tag))
            .child(field(&d, "f-name", &s.f_name, "Only files whose names match (optional)", |s: &mut State| &mut s.f_name))
            .child(row().key("f-year").w_full().px(d.item_px()).py(d.gap()).radius(d.r(tok::radius::SM)).fill(t.control_track)
                .on_press(|s: &mut State| s.f_year = !s.f_year).child(toggle(&d, s.f_year)))
            .child(row().w_full().gap(d.gap()).flex_wrap()
                .child(button(&d, "f-save", "Save".into(), true, |s: &mut State| s.save_folder()))
                .child(button(&d, "f-move", "Move to another folder…".into(), false, |s: &mut State| s.moving = Some(s.tsel)))
                .child(button(&d, "f-delete", "Stop using this folder".into(), false, |s: &mut State| s.delete_folder()))));
    }
    body = body.child(col().w_full().gap(d.gap()).child(heading(&d, "Contents")).child(super::contents::list(s)));
    body = body.child(col().w_full().gap(d.gap())
        .child(heading(&d, "Add a folder"))
        .child(field(&d, "f-new", &s.f_new, "Documents/Taxes ↵", |s: &mut State| &mut s.f_new).on_submit(|s: &mut State, _: &mut Tx| s.add_folder())));

    if let Some((from, to)) = &s.pending_move {
        body = body.child(col().w_full().padding(d.item_px()).gap(d.gap()).tile(&d)
            .child(text(format!("Move ~/{from} to ~/{to}? The folder and everything in it move on disk and in the tree. There is no undo.")).font(style(&d, type_::LABEL)).color(t.text).wrap_anywhere().shrink())
            .child(row().w_full().gap(d.gap())
                .child(button(&d, "mv-yes", "Move it".into(), true, |s: &mut State| s.confirm_move_folder()))
                .child(button(&d, "mv-no", "Cancel".into(), false, |s: &mut State| s.pending_move = None))));
    }

    // what would move, and the confirmation
    let mut prev = col().w_full().gap(d.gap()).child(heading(&d, "What would move"));
    match &s.tplan {
        Some(p) => {
            let mut by: BTreeMap<String, (usize, u64)> = BTreeMap::new();
            for m in &p.moves {
                let dest = m.to.parent().map(|x| lifestream::tilde(x, &home)).unwrap_or_default();
                let e = by.entry(dest).or_default();
                e.0 += 1;
                e.1 += m.size;
            }
            let mut dests: Vec<_> = by.into_iter().collect();
            dests.sort_by_key(|x| std::cmp::Reverse(x.1 .0));
            prev = prev.child(text(format!("{} files · {}", p.moves.len(), human(p.moves.iter().map(|m| m.size).sum()))).font(TextStyle { size: d.size(16.0), ..style(&d, type_::TITLE) }).color(t.text));
            for (dest, (n, sz)) in dests.iter().take(8) {
                prev = prev.child(row().w_full().gap(d.gap())
                    .child(text(dest.clone()).font(style(&d, type_::LABEL)).color(t.text).grow(1.0).shrink().min_w(0.0).truncate())
                    .child(text(format!("{n} · {}", human(*sz))).font(style(&d, type_::LABEL)).color(t.text_secondary)));
            }
            let mut why: BTreeMap<&str, usize> = BTreeMap::new();
            for (w, _) in &p.skipped {
                *why.entry(*w).or_default() += 1;
            }
            if !why.is_empty() {
                let mut chips = row().w_full().shrink().min_w(0.0).gaps(d.gap(), d.gap() * 0.5).flex_wrap();
                for (w, n) in why {
                    chips = chips.child(chip(&d, format!("{n} stay: {w}"), false));
                }
                prev = prev.child(chips);
            }
            if s.confirm_move {
                prev = prev.child(col().w_full().padding(d.item_px()).gap(d.gap()).tile(&d)
                    .child(text(format!("Move {} files now? You can put them back right after.", p.moves.len())).font(style(&d, type_::LABEL)).color(t.text).wrap_anywhere().shrink())
                    .child(row().w_full().gap(d.gap())
                        .child(crate::questions::action(&d, "f-confirm", &format!("Yes, move {} files", p.moves.len()), "Moving files…", true, s.jobs.running("file"), |s: &mut State| s.file_now()))
                        .child(button(&d, "f-cancel", "Cancel".into(), false, |s: &mut State| s.confirm_move = false))));
            }
        }
        None => prev = prev.child(text("Preview unavailable: the sensitive-path list could not be read.").font(style(&d, type_::LABEL)).color(t.text_secondary).wrap_anywhere().shrink()),
    }
    body = body.child(prev);
    if s.last_filed > 0 || s.jobs.running("undo") {
        body = body.child(crate::questions::action(&d, "f-undo", "Put them back", "Putting files back…", false, s.jobs.running("undo"), |s: &mut State| s.undo_filing()));
    }
    if !s.undo_lines.is_empty() {
        let mut left = col().w_full().gap(d.gap() * 0.5).child(text("These stayed where they are:").font(style(&d, type_::LABEL)).color(t.text));
        for l in s.undo_lines.iter().take(8) {
            left = left.child(text(l.clone()).font(style(&d, type_::LABEL)).color(t.text_secondary).wrap_anywhere().shrink());
        }
        if s.undo_lines.len() > 8 {
            left = left.child(text(format!("and {} more", s.undo_lines.len() - 8)).font(style(&d, type_::LABEL)).color(t.text_secondary));
        }
        body = body.child(left);
    }
    if !s.msg.is_empty() {
        body = body.child(text(s.msg.clone()).font(style(&d, type_::LABEL)).color(acc).wrap_anywhere().shrink());
    }
    let detail = col().w(460.0).padding(d.pad()).gap(d.pad()).panel(&d)
        .child(col().w_full().scroll(scroll()).shrink().min_h(0.0).grow(1.0).child(body));
    vec![center, detail]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepting_never_fails_on_a_folder_that_already_exists() {
        let mut t = tree::Tree { inbox: vec![], folders: vec![Folder { path: "Archive".into(), ..Default::default() }] };
        let sg = Suggestion { kind: "archive".into(), path: "Archive".into(), folders: 3 };
        assert_eq!(accept_into(&mut t, &sg).unwrap(), (0, true), "an existing structure-only folder learns the kind");
        assert_eq!(t.folders[0].kind, vec!["archive".to_string()]);
        assert_eq!(accept_into(&mut t, &sg).unwrap(), (0, true), "accepting twice changes nothing and does not fail");
        assert_eq!(t.folders[0].kind.len(), 1);
        let new = Suggestion { kind: "audio".into(), path: "Audio".into(), folders: 1 };
        assert_eq!(accept_into(&mut t, &new).unwrap(), (1, false));
    }

    #[test]
    fn suggested_paths_read_like_folders() {
        assert_eq!(path_for("software/app-bundle"), "Software/App-Bundle");
        assert_eq!(path_for("audio"), "Audio");
        assert_eq!(path_for("game/save-backup"), "Game/Save-Backup");
    }


    #[test]
    fn filing_says_what_moved_and_offers_undo() {
        let mut s = State { msg: "3\u{1f}0".into(), ..State::default() };
        reload_after_filing(&mut s);
        assert_eq!((s.last_filed, s.msg.as_str()), (3, "Moved 3 files. You can put them back."));
        s.msg = "1\u{1f}2".into();
        reload_after_filing(&mut s);
        assert_eq!(s.msg, "Moved 1 file, 2 could not be moved. You can put them back.");
    }

    #[test]
    fn undoing_clears_the_offer_and_lists_what_stayed() {
        let mut s = State { last_filed: 3, ..State::default() };
        *s.undo_inbox.lock().unwrap() = vec!["~/Pictures/a.png: something is already at the old place".into()];
        after_undo(&mut s);
        assert_eq!(s.last_filed, 0);
        assert_eq!(s.undo_lines.len(), 1);
    }
}
