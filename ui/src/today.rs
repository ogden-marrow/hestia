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

/// A new install has nothing to show yet: a short checklist of what to do first, gone once it is all done.
fn get_started(s: &State) -> Option<El> {
    let d = s.design;
    let t = d.theme();
    let acc = d.accent_color();
    let home = lifestream::home::is_home(&s.repo_dir);
    let scanned = s.total > 0;
    let launcher = crate::clipboard::extension_state() == "recording";
    if home && scanned && launcher {
        return None;
    }
    let step = |n: u8, done: bool, title: &str, why: &str, action: Option<El>| -> El {
        row().w_full().gap(d.pad()).center_y().py(d.gap() * 0.6)
            .child(text(if done { "✓".to_string() } else { n.to_string() }).font(style(&d, type_::TITLE)).color(if done { acc } else { t.text_secondary }).w(d.size(22.0)))
            .child(col().grow(1.0).shrink().min_w(0.0).gap(2.0)
                .child(text(title.to_string()).font(style(&d, type_::BODY)).color(if done { t.text_secondary } else { t.text }))
                .child(text(why.to_string()).font(style(&d, type_::LABEL)).color(t.text_secondary).wrap_anywhere().shrink()))
            .child(action.unwrap_or_else(|| col().size(1.0, 1.0)))
    };
    let go = |key: &str, label: &str, f: fn(&mut State)| crate::questions::button(&d, key, label.to_string(), true, f);
    Some(col().w_full().padding(d.pad()).gap(d.gap() * 0.4).tile(&d)
        .child(text("Get started").font(style(&d, type_::TITLE)).color(t.text))
        .child(step(1, home, "Create your home repository", "The private git repository that holds everything of yours. Hestia will not scan until it exists, because it holds the list of places that must never be read.",
            (!home).then(|| go("gs-home", "Set it up", |s| { crate::welcome::open(s); s.wstep = 2; s.detect_github(); }))))
        .child(step(2, scanned, "Run your first scan", "Reads your folders and builds the stream. It never changes a file, and it runs by itself every night.",
            (home && !scanned).then(|| crate::questions::action(&d, "gs-scan", "Scan now", "Scanning…", true, s.jobs.running("scan"), |s: &mut State| s.start_scan()))))
        .child(step(3, launcher, "Turn on Hestia Search", "Alt+Space from anywhere, and clipboard history. GNOME needs one log out and in after you turn it on.",
            (!launcher).then(|| crate::questions::action(&d, "gs-ext", "Turn on", "Turning on…", true, s.jobs.running("search-on"), |s: &mut State| s.turn_on_search())))))
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
        .child(tile(&d, "tile-files", &icons::FOLDER, "Files", format!("{} folders in the tree", s.tree.folders.len()), |s| { s.pane = Pane::Files; s.open_files() }))
        .child(tile(&d, "tile-history", &icons::HISTORY, "History", commit, |s| { s.pane = Pane::History; s.reload_repo() }))
        .child(tile(&d, "tile-ask", &icons::ASK, "Search", "apps, files, clipboard and Hestia in one box".into(), |s| { s.pane = Pane::Ask; s.ask.focused = true }));

    let mut body = col().w_full().gap(d.pad() * 1.3);
    if let Some(g) = get_started(s) {
        body = body.child(g);
    }
    if !s.msg.is_empty() {
        body = body.child(text(s.msg.clone()).font(style(&d, type_::LABEL)).color(d.accent_color()).wrap_anywhere().shrink());
    }
    let body = body.child(hero).child(stats).child(needs)
        .child(col().w_full().gap(d.gap()).child(text("Jump back in").font(style(&d, type_::TITLE)).color(t.text)).child(tiles));
    col().grow(1.0).shrink().min_w(0.0).padding(d.pad() * 1.3).panel(&d)
        .child(col().w_full().scroll(ScrollSpec { stick_bottom: false }).shrink().min_h(0.0).grow(1.0).child(body))
}
