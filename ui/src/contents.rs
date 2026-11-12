//! The contents of the folder chosen in Files: its items (kept current) and the selected item's details.

use super::{icons, kv, style, State};
use crate::files::tilde;
use lifestream::human;
use rune::tok::{self, type_};
use rune::{col, row, text, El, ScrollSpec};

impl State {
    /// Change one file's kind (and tags): the single place a single file is relabelled.
    fn label_file(&mut self) {
        let text = self.f_filekind.text.trim().to_string();
        let mut parts = text.split_whitespace();
        let Some(kind) = parts.next() else { return };
        let tags: Vec<String> = parts.map(String::from).collect();
        let Some(e) = self.entries.get(self.esel) else { return };
        let path = tilde(&e.path);
        let Some(id) = self.recs.iter().find(|r| r.sources.contains(&path)).map(|r| r.id.clone()) else { return };
        let Some(stream) = self.stream.clone() else { return };
        match lifestream::teach(&stream, &[id], kind, &tags) {
            Ok(recs) => {
                self.recs = recs;
                self.refresh_derived();
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

/// What is in the selected folder, and the selected item: the contents half of the Files screen.
pub fn list(s: &State) -> El {
    let d = s.design;
    let t = d.theme();
    let acc = d.accent_color();
    let mut rows = col().w_full().gap(2.0);
    for (i, e) in s.entries.iter().enumerate() {
        let on = i == s.esel;
        let path = e.path.clone();
        let is_dir = e.is_dir;
        rows = rows.child(
            row().key(format!("entry-{i}")).h(d.row_h()).px(d.gap()).gap(d.pad()).center_y().radius(d.r(tok::radius::SM))
                .fill(if on { d.select() } else { rune::Color::TRANSPARENT })
                .on_press(move |s: &mut State| { s.esel = i; s.f_filekind.clear(); s.msg.clear(); if is_dir { s.go_folder(path.clone()) } })
                .child(rune::icon(if e.is_dir { &icons::FOLDER } else { &icons::DOC }, d.size(16.0)).color(if e.is_dir { acc } else { t.text_secondary }))
                .child(text(e.name.clone()).font(style(&d, type_::BODY)).color(t.text).grow(1.0).shrink().min_w(0.0).truncate())
                .child(text(e.kind.clone()).font(style(&d, type_::LABEL)).color(if e.kind == "not in the stream yet" { acc } else { t.text_secondary }).truncate())
                .child(text(if e.is_dir { String::new() } else { human(e.size) }).font(style(&d, type_::MONO_CAPTION)).color(t.text_secondary).w(d.size(64.0))),
        );
    }
    let up = s.cwd.parent().map(|p| p.to_path_buf());
    let header = row().w_full().gap(d.gap()).center_y()
        .child(
            row().key("up").h(d.row_h()).px(d.pad()).center_y().radius(d.r(tok::radius::SM)).fill(t.control_off)
                .on_press(move |s: &mut State| { if let Some(p) = up.clone() { s.go_folder(p) } })
                .child(text("↑ up").font(style(&d, type_::LABEL)).color(t.text)),
        )
        .child(text(tilde(&s.cwd)).font(style(&d, type_::BODY)).color(t.text).grow(1.0).shrink().min_w(0.0).truncate())
        .child(
            row().key("new-note").h(d.row_h()).px(d.pad()).center_y().radius(d.r(tok::radius::SM)).fill(t.control_off)
                .on_press(|s: &mut State| s.new_note())
                .child(text("New note").font(style(&d, type_::LABEL)).color(t.text)),
        );
    let mut c = col().w_full().gap(d.gap()).child(header)
        .child(text(format!("{} items · updates by itself", s.entries.len())).font(style(&d, type_::SECTION)).color(t.text_secondary))
        .child(col().w_full().h(d.size(220.0)).scroll(ScrollSpec { stick_bottom: false }).child(rows));
    if let Some(e) = s.entries.get(s.esel) {
        c = c
            .child(kv(&d, "kind", &e.kind))
            .child(kv(&d, "size", &if e.is_dir { "folder".to_string() } else { human(e.size) }))
            .child(kv(&d, "modified", &e.modified));
        if let Some(r) = s.recs.iter().find(|r| r.sources.iter().any(|p| *p == tilde(&e.path))) {
            c = c
                .child(kv(&d, "filed by", &format!("{} · {}", r.by.tier, r.by.name)))
                .child(kv(&d, "tags", &r.tags.join("  ")))
                .child(super::field(&d, "file-kind", &s.f_filekind, "change this file's kind ↵ (like document/manual)", |s: &mut State| &mut s.f_filekind).on_submit(|s: &mut State, _: &mut rune::Tx| s.label_file()));
        }
        let path = e.path.clone();
        c = c.child(
            row().key("open").h(d.row_h()).px(d.pad()).center_y().radius(d.r(tok::radius::SM)).fill(acc)
                .on_press(move |_: &mut State| super::open_folder(&path.to_string_lossy()))
                .child(text("Open").font(style(&d, type_::LABEL)).color(d.on_accent())),
        );
    }
    c
}
