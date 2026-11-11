//! The file explorer: places on the left, the folder in the middle (kept current), details on the right.

use super::{icons, kv, style, State};
use crate::design::Surface;
use crate::files::{self, tilde};
use lifestream::human;
use rune::tok::{self, type_};
use rune::{col, row, text, El, ScrollSpec};

/// The left column of Files: your places, and the collections (saved views of the stream).
pub fn left(s: &State) -> El {
    let d = s.design;
    let t = d.theme();
    let acc = d.accent_color();
    let home = files::home();
    let places: Vec<(&str, std::path::PathBuf)> = vec![
        ("Home", home.clone()),
        ("Documents", home.join("Documents")),
        ("Downloads", home.join("Downloads")),
        ("Pictures", home.join("Pictures")),
        ("Videos", home.join("Videos")),
        ("home repo", s.repo_dir.clone()),
    ];
    let mut pl = col().w_full().gap(2.0);
    for (i, (name, path)) in places.into_iter().enumerate() {
        let on = s.pane == super::Pane::Files && s.cwd == path;
        pl = pl.child(
            row().key(format!("place-{i}")).h(d.row_h()).px(d.gap()).center_y().radius(d.r(tok::radius::SM))
                .fill(if on { acc } else { rune::Color::TRANSPARENT })
                .on_press(move |s: &mut State| { s.pane = super::Pane::Files; s.go(path.clone()) })
                .child(text(name).font(style(&d, type_::LABEL)).color(if on { d.on_accent() } else { t.text })),
        );
    }
    pl = pl.child(text("COLLECTIONS").font(style(&d, type_::SECTION)).color(t.text_secondary).px(d.gap()).py(d.gap()));
    for i in 0..=s.views.len() {
        let on = s.pane == super::Pane::Stream && s.view == i;
        let label = if i == 0 { "All files".to_string() } else { s.view_name(i) };
        pl = pl.child(
            row().key(format!("coll-{i}")).h(d.row_h()).px(d.gap()).gap(d.gap()).center_y().radius(d.r(tok::radius::SM))
                .fill(if on { acc } else { rune::Color::TRANSPARENT })
                .on_press(move |s: &mut State| { s.pane = super::Pane::Stream; s.view = i; s.sel = 0; s.recompute() })
                .child(text(label).font(style(&d, type_::LABEL)).color(if on { d.on_accent() } else { t.text }).grow(1.0))
                .child(text(s.counts[i].to_string()).font(style(&d, type_::LABEL)).color(if on { d.on_accent() } else { t.text_secondary })),
        );
    }
    col().w(210.0).padding(d.gap()).side(&d).child(pl.scroll(ScrollSpec { stick_bottom: false }).shrink().min_h(0.0))
}

impl State {
    /// Change one file's kind (and tags): the single place a single file is relabelled.
    fn label_file(&mut self) {
        let text = self.f_filekind.text.trim().to_string();
        let mut parts = text.split_whitespace();
        let Some(kind) = parts.next() else { return };
        let tags: Vec<String> = parts.map(String::from).collect();
        let Some(e) = self.entries.get(self.esel) else { return };
        let path = tilde(&e.path);
        let Some(id) = self.recs.iter().find(|r| r.sources.iter().any(|p| *p == path)).map(|r| r.id.clone()) else { return };
        let Some(stream) = self.stream.clone() else { return };
        match lifestream::teach(&stream, &[id], kind, &tags) {
            Ok(recs) => {
                self.recs = recs;
                self.refresh_derived();
                self.recompute();
                self.f_filekind.clear();
                self.msg = format!("{} is now filed as {}", e_name(&path), kind.to_lowercase());
                self.reload_dir();
            }
            Err(e) => self.msg = format!("not saved: {e}"),
        }
    }
}

fn e_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

pub fn panels(s: &State) -> Vec<El> {
    let d = s.design;
    let t = d.theme();
    let acc = d.accent_color();
    let places = left(s);

    // ---- the folder
    let mut rows = col().w_full().gap(2.0);
    for (i, e) in s.entries.iter().enumerate() {
        let on = i == s.esel;
        let path = e.path.clone();
        let is_dir = e.is_dir;
        rows = rows.child(
            row().key(format!("entry-{i}")).h(d.row_h()).px(d.gap()).gap(d.pad()).center_y().radius(d.r(tok::radius::SM))
                .fill(if on { d.select() } else { rune::Color::TRANSPARENT })
                .on_press(move |s: &mut State| { s.esel = i; s.f_filekind.clear(); s.msg.clear(); if is_dir { s.go(path.clone()) } })
                .child(rune::icon(if e.is_dir { &icons::FOLDER } else { &icons::DOC }, d.size(16.0)).color(if e.is_dir { acc } else { t.text_secondary }))
                .child(text(e.name.clone()).font(style(&d, type_::BODY)).color(t.text).grow(1.0).shrink().min_w(0.0).truncate())
                .child(text(e.kind.clone()).font(style(&d, type_::LABEL)).color(if e.kind == "not in the stream yet" { acc } else { t.text_secondary }).w(d.size(150.0)))
                .child(text(if e.is_dir { String::new() } else { human(e.size) }).font(style(&d, type_::MONO_CAPTION)).color(t.text_secondary).w(d.size(70.0)))
                .child(text(e.modified.clone()).font(style(&d, type_::MONO_CAPTION)).color(t.text_secondary).w(d.size(120.0))),
        );
    }
    let up = s.cwd.parent().map(|p| p.to_path_buf());
    let header = row().w_full().gap(d.gap()).center_y()
        .child(
            row().key("up").h(d.row_h()).px(d.pad()).center_y().radius(d.r(tok::radius::SM)).fill(t.control_off)
                .on_press(move |s: &mut State| { if let Some(p) = up.clone() { s.go(p) } })
                .child(text("↑ up").font(style(&d, type_::LABEL)).color(t.text)),
        )
        .child(text(tilde(&s.cwd)).font(style(&d, type_::BODY)).color(t.text).grow(1.0).shrink().min_w(0.0).truncate())
        .child(
            row().key("new-note").h(d.row_h()).px(d.pad()).center_y().radius(d.r(tok::radius::SM)).fill(acc)
                .on_press(|s: &mut State| s.new_note())
                .child(text("New note").font(style(&d, type_::LABEL)).color(d.on_accent())),
        );
    let folder = col().grow(1.0).shrink().min_w(0.0).padding(d.gap()).gap(d.gap()).panel(&d)
        .child(header)
        .child(text(format!("{} items · updates by itself{}", s.entries.len(), if s.msg.is_empty() { String::new() } else { format!(" · {}", s.msg) })).font(style(&d, type_::SECTION)).color(t.text_secondary))
        .child(col().w_full().scroll(ScrollSpec { stick_bottom: false }).shrink().min_h(0.0).grow(1.0).child(rows));

    // ---- details
    let mut detail = col().w(300.0).padding(d.pad()).gap(d.gap()).panel(&d);
    if let Some(e) = s.entries.get(s.esel) {
        detail = detail
            .child(text(e.name.clone()).font(style(&d, type_::TITLE)).color(t.text).wrap_anywhere().shrink())
            .child(kv(&d, "where", &tilde(&e.path)))
            .child(kv(&d, "kind", &e.kind))
            .child(kv(&d, "size", &if e.is_dir { "folder".to_string() } else { human(e.size) }))
            .child(kv(&d, "modified", &e.modified));
        if let Some(r) = s.recs.iter().find(|r| r.sources.iter().any(|p| *p == tilde(&e.path))) {
            detail = detail
                .child(kv(&d, "filed by", &format!("{} · {}", r.by.tier, r.by.name)))
                .child(kv(&d, "tags", &r.tags.join("  ")))
                .child(super::field(&d, "file-kind", &s.f_filekind, "change this file's kind ↵ (like document/manual)", |s: &mut State| &mut s.f_filekind).on_submit(|s: &mut State, _: &mut rune::Tx| s.label_file()));
        }
        let path = e.path.clone();
        detail = detail.child(
            row().key("open").h(d.row_h()).px(d.pad()).center_y().radius(d.r(tok::radius::SM)).fill(acc)
                .on_press(move |_: &mut State| super::open_folder(&path.to_string_lossy()))
                .child(text("Open").font(style(&d, type_::LABEL)).color(d.on_accent())),
        );
    }
    vec![places, folder, detail]
}
