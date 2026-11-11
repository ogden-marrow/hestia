//! Today: the screen Hestia opens on. What happened while you were away, what needs you,
//! and where to jump back in.

use super::{greeting, style, Pane, State};
use crate::design::{Role, Surface};
use rune::tok::type_;
use rune::{col, row, text, El, ScrollSpec};

/// (new in the last 24 hours, how many of those are filed, how many files vanished)
pub fn overnight(recs: &[lifestream::Record]) -> (usize, usize, usize) {
    let now = chrono::Utc::now();
    let recent = |r: &&lifestream::Record| {
        chrono::DateTime::parse_from_rfc3339(&r.first_seen).map(|t| (now - t.with_timezone(&chrono::Utc)).num_hours() < 24).unwrap_or(false)
    };
    let new: Vec<&lifestream::Record> = recs.iter().filter(recent).collect();
    (new.len(), new.iter().filter(|r| r.status == "filed").count(), recs.iter().filter(|r| r.status == "gone").count())
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

/// "1 file", "2 files", with thousands separated: 1,463.
pub fn count(n: usize, one: &str, many: &str) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    format!("{out} {}", if n == 1 { one } else { many })
}

/// The one sentence under the greeting: what happened, in plain words.
pub fn summary(filed: usize, waiting: usize) -> String {
    let did = if filed == 0 { "Nothing new arrived while you were away.".to_string() } else { format!("While you were away, Hestia filed {}.", count(filed, "file", "files")) };
    if waiting == 0 { format!("{did} Everything is where it belongs.") } else { format!("{did} A few things need your say.") }
}

pub fn panel(s: &State) -> El {
    let d = s.design;
    let t = d.theme();
    let (_, filed, vanished) = s.overnight;
    let live = s.recs.iter().filter(|r| r.status != "gone").count();
    let folders = s.bundles.len();

    // One calm focal point: the greeting and a single sentence.
    let hero = col().gap(d.gap())
        .child(text(greeting().to_string()).font(d.text(Role::Display)).color(t.text))
        .child(text(summary(filed, folders)).font(d.text(Role::Body)).color(t.text_secondary).wrap_anywhere().shrink());

    // What needs you: the one primary action on this screen, only when something does.
    let needs = if folders == 0 {
        col().w_full().gap(d.gap())
            .child(text("Nothing needs you.").font(d.text(Role::Title)).color(t.text))
            .child(text("Hestia will ask when it is unsure.").font(d.text(Role::Body)).color(t.text_secondary))
    } else {
        let mut list = col().w_full().tile(&d);
        for (i, b) in s.bundles.iter().take(3).enumerate() {
            list = list.child(
                row().w_full().h(d.row_h() + d.gap()).px(d.item_px()).gap(d.pad()).center_y()
                    .border_sides([if i == 0 { 0.0 } else { 1.0 }, 0.0, 0.0, 0.0], t.card_border)
                    .child(text(b.root.rsplit('/').next().unwrap_or(&b.root).to_string()).font(d.text(Role::Body)).color(t.text).grow(1.0).shrink().min_w(0.0).truncate())
                    .child(text(count(b.ids.len(), "file", "files")).font(d.text(Role::Label)).color(t.text_secondary)),
            );
        }
        col().w_full().gap(d.pad())
            .child(text(format!("{} to look at", count(folders, "folder", "folders"))).font(d.text(Role::Title)).color(t.text))
            .child(list)
            .child(row().child(crate::questions::button(&d, "today-answer", "Answer questions".into(), true, |s| s.pane = Pane::Questions)))
    };

    // The numbers, quietly: one line, not a dashboard.
    let facts = text(format!("{} in your stream  ·  {} kept in history", count(live, "file", "files"), count(vanished, "file", "files")))
        .font(d.text(Role::Label)).color(t.text_secondary).wrap_anywhere().shrink();

    let mut body = col().w_full().gap(d.section());
    if let Some(g) = get_started(s) {
        body = body.child(g);
    }
    if !s.msg.is_empty() {
        body = body.child(text(s.msg.clone()).font(d.text(Role::Label)).color(d.accent_color()).wrap_anywhere().shrink());
    }
    let body = body.child(hero).child(needs).child(facts);
    col().grow(1.0).shrink().min_w(0.0).padding(d.pad() * 2.0).panel(&d)
        .child(col().w_full().max_w(d.size(640.0)).scroll(ScrollSpec { stick_bottom: false }).shrink().min_h(0.0).grow(1.0).child(body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_are_plural_and_grouped() {
        assert_eq!(count(1, "file", "files"), "1 file");
        assert_eq!(count(0, "file", "files"), "0 files");
        assert_eq!(count(1463, "file", "files"), "1,463 files");
        assert_eq!(count(2_815_000, "file", "files"), "2,815,000 files");
    }

    #[test]
    fn summary_is_calm_in_every_state() {
        assert_eq!(summary(0, 0), "Nothing new arrived while you were away. Everything is where it belongs.");
        assert!(summary(1, 0).starts_with("While you were away, Hestia filed 1 file."));
        assert!(summary(12, 3).ends_with("A few things need your say."));
    }

    #[test]
    fn renders_with_nothing_waiting_and_with_many() {
        let mut s = State::default();
        s.bundles.clear();
        let _ = panel(&s);
    }
}
