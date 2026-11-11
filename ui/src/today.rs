//! Today: the screen Hestia opens on. What happened while you were away, what needs you,
//! and where to jump back in.

use super::{greeting, icons, style, Pane, State};
use crate::design::{Design, Surface};
use crate::timeview;
use lifestream::human;
use rune::tok::type_;
use rune::{col, row, text, El, ScrollSpec, TextStyle};

/// (new in the last 24 hours, how many of those are filed, how many files vanished)
pub fn overnight(recs: &[lifestream::Record]) -> (usize, usize, usize) {
    let now = chrono::Utc::now();
    let recent = |r: &&lifestream::Record| {
        chrono::DateTime::parse_from_rfc3339(&r.first_seen).map(|t| (now - t.with_timezone(&chrono::Utc)).num_hours() < 24).unwrap_or(false)
    };
    let new: Vec<&lifestream::Record> = recs.iter().filter(recent).collect();
    (new.len(), new.iter().filter(|r| r.status == "filed").count(), recs.iter().filter(|r| r.status == "gone").count())
}

fn tile<F: Fn(&mut State) + 'static>(d: &Design, key: &str, glyph: &'static rune::Icon, title: &str, line: String, press: F) -> El {
    let t = d.theme();
    col().key(key.to_string()).grow(1.0).shrink().min_w(0.0).padding(d.pad()).gap(d.gap()).tile(d).on_press(press)
        .child(rune::icon(glyph, d.size(22.0)).color(d.accent_color()))
        .child(text(title.to_string()).font(style(d, type_::TITLE)).color(t.text))
        .child(text(line).font(style(d, type_::LABEL)).color(t.text_secondary).wrap_anywhere().shrink())
}

fn stat(d: &Design, big: String, label: &str) -> El {
    let t = d.theme();
    col().grow(1.0).shrink().min_w(0.0).padding(d.pad()).gap(2.0).tile(d)
        .child(text(big).font(TextStyle { size: d.size(26.0), ..style(d, type_::TITLE) }).color(d.accent_color()))
        .child(text(label.to_string()).font(style(d, type_::LABEL)).color(t.text_secondary))
}

pub fn panel(s: &State) -> El {
    let d = s.design;
    let t = d.theme();
    let (_, filed, vanished) = s.overnight;
    let pending = s.recs.iter().filter(|r| r.status == "pending").count();
    let live = s.recs.iter().filter(|r| r.status != "gone").count();
    let agreed = s.suggestions.values().filter(|c| c.agree()).count();

    let big = TextStyle { size: d.size(34.0), ..style(&d, type_::TITLE) };
    let hero = col().gap(d.gap())
        .child(text(greeting().to_string()).font(big).color(t.text))
        .child(text(format!("While you were away, Hestia filed {} files. {} files in {} folders need you; everything else is handled.", filed, pending, s.bundles.len()))
            .font(TextStyle { size: d.size(15.0), ..style(&d, type_::BODY) }).color(t.text_secondary).wrap_anywhere().shrink());

    let stats = row().w_full().gap(d.pad())
        .child(stat(&d, format!("{live}"), "files in your stream"))
        .child(stat(&d, format!("{pending}"), "waiting for you"))
        .child(stat(&d, format!("{agreed} of {}", s.bundles.len().max(s.suggestions.len())), "folders both models agree on"))
        .child(stat(&d, format!("{vanished}"), "files gone, kept in history"));

    // ---- needs you: what is waiting. Answering happens in one place, Questions.
    let mut asks = col().w_full().gap(d.gap());
    for b in s.bundles.iter().take(3) {
        asks = asks.child(
            row().w_full().px(d.item_px()).py(d.gap()).gap(d.pad()).center_y().tile(&d)
                .child(text(b.root.trim_start_matches("~/").to_string()).font(style(&d, type_::BODY)).color(t.text).grow(1.0).shrink().min_w(0.0).truncate())
                .child(text(format!("{} files · {}", b.ids.len(), human(b.size))).font(style(&d, type_::LABEL)).color(t.text_secondary)),
        );
    }
    let needs = col().w_full().gap(d.gap())
        .child(row().w_full().center_y()
            .child(text("Needs you").font(style(&d, type_::TITLE)).color(t.text).grow(1.0))
            .child(text(format!("{} folders wait in Questions", s.bundles.len())).font(style(&d, type_::LABEL)).color(t.text_secondary)))
        .child(asks);

    // ---- jump back in
    let day = s.days.first().map(|x| format!("{} · {} files", timeview::long_date(&x.date), x.idx.len())).unwrap_or_else(|| "nothing yet".into());
    let commit = s.commits.iter().find(|c| c.hash != crate::repo::WORKTREE).map(|c| format!("{} · {}", c.subject, c.when)).unwrap_or_else(|| "no commits".into());
    let tiles = row().w_full().gap(d.pad())
        .child(tile(&d, "tile-time", &icons::TIME, "Time", day, |s| s.pane = Pane::Time))
        .child(tile(&d, "tile-files", &icons::FOLDER, "Files", format!("{} items in your home folder, kept current", s.entries.len()), |s| { s.pane = Pane::Files; s.reload_dir() }))
        .child(tile(&d, "tile-history", &icons::HISTORY, "History", commit, |s| { s.pane = Pane::History; s.reload_repo() }))
        .child(tile(&d, "tile-ask", &icons::ASK, "Search", "apps, files, clipboard and Hestia in one box".into(), |s| { s.pane = Pane::Ask; s.ask.focused = true }));

    let body = col().w_full().gap(d.pad() * 1.3).child(hero).child(stats).child(needs)
        .child(col().w_full().gap(d.gap()).child(text("Jump back in").font(style(&d, type_::TITLE)).color(t.text)).child(tiles));
    col().grow(1.0).shrink().min_w(0.0).padding(d.pad() * 1.3).panel(&d)
        .child(col().w_full().scroll(ScrollSpec { stick_bottom: false }).shrink().min_h(0.0).grow(1.0).child(body))
}
