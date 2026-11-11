//! The git viewer: commits on the left, what a commit changed in the middle, the diff on the right.

use super::{style, State};
use crate::design::Surface;
use crate::repo::{Kind, WORKTREE};
use rune::tok::{self, type_};
use rune::{col, el, oklcha, oklch, row, text, El, ScrollSpec};

pub fn panels(s: &State) -> Vec<El> {
    let d = s.design;
    let t = d.theme();
    let acc = d.accent_color();
    let dark = d.dark;
    let (add_bg, del_bg) = (oklcha(0.72, 0.15, 150.0, 0.17), oklcha(0.62, 0.2, 25.0, 0.17));
    let (add_fg, del_fg) = if dark { (oklch(0.86, 0.1, 150.0), oklch(0.82, 0.1, 25.0)) } else { (oklch(0.38, 0.11, 150.0), oklch(0.45, 0.15, 25.0)) };
    let scroll = || ScrollSpec { stick_bottom: false };

    // ---- commits
    let mut list = col().w_full().gap(2.0);
    for (i, c) in s.commits.iter().enumerate() {
        let on = i == s.csel;
        let work = c.hash == WORKTREE;
        list = list.child(
            row().key(format!("commit-{i}")).px(d.gap()).py(d.gap()).gap(d.gap()).radius(d.r(tok::radius::SM))
                .fill(if on { d.select() } else { rune::Color::TRANSPARENT })
                .on_press(move |s: &mut State| s.select_commit(i))
                .child(col().w(14.0).child(el().size(9.0, 9.0).radius(5.0).fill(if on || work { acc } else { t.text_secondary })))
                .child(col().grow(1.0).shrink().min_w(0.0).gap(2.0)
                    .child(text(c.subject.clone()).font(style(&d, type_::BODY)).color(t.text).truncate())
                    .child(text(if work { "not committed yet".to_string() } else { format!("{} · {}", c.short, c.when) }).font(style(&d, type_::MONO_CAPTION)).color(t.text_secondary).truncate())),
        );
    }
    let commits = col().w(250.0).shrink().min_w(200.0).padding(d.gap()).gap(d.gap()).panel(&d)
        .child(text(format!("{} · {}", s.repo_dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), s.repo_status)).font(style(&d, type_::SECTION)).color(t.text_secondary).truncate())
        .child(col().w_full().scroll(scroll()).shrink().min_h(0.0).grow(1.0).child(list));

    // ---- changed files
    let mut files = col().w_full().gap(2.0);
    for (i, c) in s.changes.iter().enumerate() {
        let on = i == s.fsel;
        let (dir, name) = c.path.rsplit_once('/').map(|(a, b)| (a.to_string(), b.to_string())).unwrap_or((String::new(), c.path.clone()));
        files = files.child(
            row().key(format!("change-{i}")).px(d.gap()).py(d.gap()).gap(d.gap()).center_y().radius(d.r(tok::radius::SM))
                .fill(if on { d.select() } else { rune::Color::TRANSPARENT })
                .on_press(move |s: &mut State| { s.fsel = i; s.load_diff(); })
                .child(col().grow(1.0).shrink().min_w(0.0).gap(2.0)
                    .child(text(name).font(style(&d, type_::BODY)).color(t.text).truncate())
                    .child(text(dir).font(style(&d, type_::MONO_CAPTION)).color(t.text_secondary).truncate()))
                .child(text(format!("+{}", c.add)).font(style(&d, type_::MONO_CAPTION)).color(add_fg))
                .child(text(format!("−{}", c.del)).font(style(&d, type_::MONO_CAPTION)).color(del_fg)),
        );
    }
    let (total_add, total_del): (u32, u32) = s.changes.iter().fold((0, 0), |a, c| (a.0 + c.add, a.1 + c.del));
    let msg = s.commits.get(s.csel).map(|c| c.subject.clone()).unwrap_or_default();
    let changed = col().w(230.0).shrink().min_w(200.0).padding(d.pad()).gap(d.gap()).panel(&d)
        .child(text(msg).font(style(&d, type_::TITLE)).color(t.text).wrap_anywhere().shrink())
        .child(text(format!("{} files · +{total_add} −{total_del}", s.changes.len())).font(style(&d, type_::LABEL)).color(t.text_secondary))
        .child(col().w_full().scroll(scroll()).shrink().min_h(0.0).grow(1.0).child(files));

    // ---- diff
    let mut lines = col().w_full();
    for (i, l) in s.diff.iter().enumerate() {
        let (bg, fg, sign) = match l.kind {
            Kind::Add => (add_bg, add_fg, "+"),
            Kind::Del => (del_bg, del_fg, "−"),
            Kind::Hunk => (t.control_track, acc, " "),
            Kind::Ctx => (rune::Color::TRANSPARENT, t.text_secondary, " "),
        };
        let num = |n: Option<u32>| n.map(|n| n.to_string()).unwrap_or_default();
        lines = lines.child(
            row().key(format!("dl-{i}")).w_full().gap(d.gap()).px(d.gap()).fill(bg)
                .child(text(num(l.old)).font(style(&d, type_::MONO_CAPTION)).color(t.text_secondary).w(d.size(40.0)))
                .child(text(num(l.new)).font(style(&d, type_::MONO_CAPTION)).color(t.text_secondary).w(d.size(40.0)))
                .child(text(sign).font(style(&d, type_::MONO_CAPTION)).color(fg).w(d.size(12.0)))
                .child(text(l.text.clone()).font(style(&d, type_::MONO_CAPTION)).color(if matches!(l.kind, Kind::Ctx) { t.text_secondary } else { fg }).grow(1.0).shrink().min_w(0.0).wrap_anywhere()),
        );
    }
    let path = s.changes.get(s.fsel).map(|c| c.path.clone()).unwrap_or_default();
    let diff = col().grow(1.0).shrink().min_w(0.0).padding(d.pad()).gap(d.gap()).panel(&d)
        .child(text(if path.is_empty() { "pick a file".to_string() } else { path }).font(style(&d, type_::TITLE)).color(t.text).truncate())
        .child(col().w_full().scroll(scroll()).shrink().min_h(0.0).grow(1.0).child(lines));

    vec![commits, changed, diff]
}
