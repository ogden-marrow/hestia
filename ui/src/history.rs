//! The git viewer: commits on the left, what a commit changed in the middle, the diff on the right.

use super::{style, State};
use crate::design::Surface;
use crate::repo::{Kind, WORKTREE};
use rune::tok::{self, type_};
use rune::{col, el, oklcha, oklch, row, text, El, ScrollSpec};

/// A commit subject as a person would say it: no "machine:" prefix, no trailing "(path/to/key)".
pub fn plain(subject: &str) -> String {
    let s = subject.strip_prefix("machine: ").unwrap_or(subject).trim();
    let s = match s.rsplit_once(" (") {
        Some((head, tail)) if tail.ends_with(')') && tail.contains('/') && !tail.contains(' ') => head,
        _ => s,
    };
    let s = s.replace(" 1 files ", " 1 file ").replace(", 1 files", ", 1 file");
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

/// "3 files changed · 12 lines added", or nothing at all when no file is listed.
fn tally(files: usize, add: u32, del: u32) -> Option<String> {
    if files == 0 {
        return None;
    }
    let lines = |n: u32| if n == 1 { "1 line".to_string() } else { format!("{n} lines") };
    let what = if del == 0 { format!("{} added", lines(add)) } else { format!("{add} added, {del} removed") };
    Some(format!("{} · {what}", crate::today::count(files, "file changed", "files changed")))
}

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
                    .child(text(plain(&c.subject)).font(style(&d, type_::BODY)).color(t.text).truncate())
                    .child(text(if work { "not saved yet".to_string() } else { c.when.clone() }).font(style(&d, type_::LABEL)).color(t.text_secondary).truncate())),
        );
    }
    let commits = col().w(250.0).shrink().min_w(200.0).padding(d.gap()).gap(d.gap()).panel(&d)
        .child(text("Your saved changes".to_string()).font(style(&d, type_::SECTION)).color(t.text_secondary).truncate())
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
                .child(text(if c.del == 0 { format!("{} added", c.add) } else { format!("{} added, {} removed", c.add, c.del) }).font(style(&d, type_::LABEL)).color(t.text_secondary)),
        );
    }
    let (total_add, total_del): (u32, u32) = s.changes.iter().fold((0, 0), |a, c| (a.0 + c.add, a.1 + c.del));
    let msg = s.commits.get(s.csel).map(|c| plain(&c.subject)).unwrap_or_default();
    let changed = col().w(230.0).shrink().min_w(200.0).padding(d.pad()).gap(d.gap()).panel(&d)
        .child(text(msg).font(style(&d, type_::TITLE)).color(t.text).wrap_anywhere().shrink())
        .child(text(tally(s.changes.len(), total_add, total_del).unwrap_or_else(|| "Nothing to show for this one.".into())).font(style(&d, type_::LABEL)).color(t.text_secondary))
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
        .child(text(if path.is_empty() { "Choose a file to see what changed".to_string() } else { path }).font(style(&d, type_::TITLE)).color(t.text).truncate())
        .child(col().w_full().scroll(scroll()).shrink().min_h(0.0).grow(1.0).child(lines));

    vec![commits, changed, diff]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subjects_read_like_a_person_wrote_them() {
        assert_eq!(plain("machine: GNOME settings, 5 lines changed (org/gnome/mutter/keybindings)"), "GNOME settings, 5 lines changed");
        assert_eq!(plain("Uncommitted changes"), "Uncommitted changes");
        assert_eq!(plain("machine: fonts (user)"), "Fonts (user)");
        assert_eq!(plain("machine: config, 1 files changed"), "Config, 1 file changed");
    }

    #[test]
    fn the_tally_is_silent_without_files_and_singular_when_one() {
        assert_eq!(tally(0, 0, 0), None);
        assert_eq!(tally(1, 1, 0).unwrap(), "1 file changed · 1 line added");
        assert_eq!(tally(2, 4, 3).unwrap(), "2 files changed · 4 added, 3 removed");
    }
}
