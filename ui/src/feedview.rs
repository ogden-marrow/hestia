//! The Stream: what happened, newest first, grouped by day, in plain words. Each row says what was done, why,
//! and who did it (Hestia, you, the backup), and offers Undo where the engine can reverse it. Read from the
//! engine's own feed (`lifestream::feed`); nothing is invented here.

use super::{style, State};
use chrono::{DateTime, Local, NaiveDate};
use rune::tok::type_;
use rune::{col, row, text, El};

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub id: String,
    pub at: DateTime<Local>,
    pub kind: String,
    pub actor: String,
    pub reason: String,
    pub summary: String,
    pub count: usize,
    pub undoable: bool,
    pub batch: Option<String>,
}

impl Row {
    fn from_item(i: &lifestream::feed::Item) -> Option<Row> {
        let at = DateTime::parse_from_rfc3339(&i.at).ok()?.with_timezone(&Local);
        Some(Row { id: i.id.clone(), at, kind: i.kind.into(), actor: i.actor.into(), reason: i.reason.clone(), summary: i.summary.clone(), count: i.count, undoable: i.undoable, batch: i.batch.clone() })
    }

    pub fn failed(&self) -> bool {
        self.kind == "backup_failed"
    }
}

/// The newest `limit` events, newest first. Missing or damaged sources give fewer rows, never an error.
pub fn load(stream: &std::path::Path, limit: usize) -> Vec<Row> {
    let Ok(home) = lifestream::home() else { return vec![] };
    let history = lifestream::feed::history_path().unwrap_or_default();
    lifestream::feed::feed(&home, stream, &history, &lifestream::feed::Opts { since: None, limit })
        .iter().filter_map(Row::from_item).collect()
}

/// Who did it, as a person would say: Hestia (its rules and models), You, or Backup (the watcher).
pub fn who(actor: &str) -> &'static str {
    match actor {
        "hestia_rule" | "model" => "Hestia",
        "you" => "You",
        "watcher" => "Backup",
        _ => "",
    }
}

/// "Today", "Yesterday", or "Mon, Nov 9".
pub fn day_label(day: NaiveDate, today: NaiveDate) -> String {
    let n = (today - day).num_days();
    match n {
        0 => "Today".into(),
        1 => "Yesterday".into(),
        _ => day.format("%a, %b %-d").to_string(),
    }
}

/// The filter chips: everything, files (filed, moved, renamed, new folders, undone), backup, and what you did yourself.
pub const FILTERS: [&str; 4] = ["Everything", "Files", "Backup", "Yours"];
const PAGE: usize = 20;

pub fn matches(filter: usize, r: &Row) -> bool {
    match filter {
        1 => matches!(r.kind.as_str(), "filed" | "moved" | "renamed" | "folder_created" | "undone"),
        2 => r.kind.starts_with("backup_"),
        3 => r.actor == "you",
        _ => true,
    }
}

impl State {
    pub(crate) fn reload_feed(&mut self) {
        self.feed = match &self.stream {
            Some(p) => load(p, 150),
            None => vec![],
        };
    }

    /// Undo one event through the engine's journal, then refresh everything that shows files.
    pub(crate) fn undo_event(&mut self, batch: String) {
        let Some(stream) = self.stream.clone() else { return };
        self.msg = "Putting it back…".into();
        self.jobs.start("undo-event", Some(|s: &mut State| {
            if let Some(stream) = s.stream.clone() {
                s.recs = lifestream::load_all(&stream).unwrap_or_default();
            }
            s.refresh_derived();
            s.refresh_plan();
            s.reload_feed();
            s.reload_tree();
        }), move || {
            let home = lifestream::home().map_err(|e| e.to_string())?;
            let r = lifestream::tree::undo_batch(&stream, &home, Some(&batch)).map_err(|e| format!("Couldn't undo it: {e}"))?;
            Ok(if r.restored == 0 && !r.skipped.is_empty() { format!("Nothing was put back: {}", r.skipped[0].1) } else { format!("Put back {}", crate::today::count(r.restored, "item", "items")) })
        });
    }
}

fn one(s: &State, r: &Row) -> El {
    let d = s.design;
    let t = d.theme();
    let bad = r.failed();
    let time = r.at.format("%-I:%M %p").to_string().to_lowercase();
    let chip = crate::questions::chip(&d, who(&r.actor).to_string(), false);
    let mut line = row().w_full().gap(d.pad()).py(d.gap()).center_y()
        .child(text(time).font(style(&d, type_::LABEL)).color(t.text_secondary).w(d.size(64.0)))
        .child(col().grow(1.0).shrink().min_w(0.0).gap(2.0)
            .child(text(r.summary.clone()).font(style(&d, type_::BODY)).color(if bad { d.danger() } else { t.text }).wrap_anywhere().shrink())
            .child(text(r.reason.clone()).font(style(&d, type_::LABEL)).color(t.text_secondary).wrap_anywhere().shrink()));
    if !who(&r.actor).is_empty() {
        line = line.child(chip);
    }
    if let (true, Some(b)) = (r.undoable, r.batch.clone()) {
        line = line.child(crate::questions::button(&d, &format!("undo-{}", r.id), "Undo".into(), false, move |s: &mut State| s.undo_event(b.clone())));
    } else if bad {
        line = line.child(crate::questions::button(&d, &format!("fix-{}", r.id), "See why".into(), false, |s: &mut State| s.go_pane(super::Pane::Machine)));
    }
    line.border_sides([0.0, 0.0, 1.0, 0.0], t.card_border)
}

/// The feed as a section of the Stream: day headings, then rows. A quiet empty state when nothing has happened.
pub fn section(s: &State) -> El {
    let d = s.design;
    let t = d.theme();
    let mut c = col().w_full().gap(0.0).child(text("What happened".to_string()).font(style(&d, type_::TITLE)).color(t.text).pb(d.gap()));
    if !s.feed.is_empty() {
        c = c.child(row().child(super::prefs::segmented(&d, "feed-filter", &FILTERS, s.feed_filter, |s, i| { s.feed_filter = i; s.feed_shown = PAGE })).pb(d.gap()));
    }
    let shown: Vec<&Row> = s.feed.iter().filter(|r| matches(s.feed_filter, r)).collect();
    if shown.is_empty() && !s.feed.is_empty() {
        return c.child(text("Nothing of that kind yet.".to_string()).font(style(&d, type_::LABEL)).color(t.text_secondary));
    }
    if s.feed.is_empty() {
        return c.child(text("Nothing yet. When Hestia files a file, saves your computer, or you move something, it shows up here with a way to undo it.".to_string())
            .font(style(&d, type_::LABEL)).color(t.text_secondary).wrap_anywhere().shrink());
    }
    let today = Local::now().date_naive();
    let mut last: Option<NaiveDate> = None;
    let total = shown.len();
    for r in shown.into_iter().take(s.feed_shown.max(PAGE)) {
        let day = r.at.date_naive();
        if last != Some(day) {
            c = c.child(text(day_label(day, today)).font(style(&d, type_::SECTION)).color(t.text_secondary).pt(d.pad()).pb(d.gap() * 0.5));
            last = Some(day);
        }
        c = c.child(one(s, r));
    }
    if total > s.feed_shown.max(PAGE) {
        c = c.child(row().pt(d.pad()).child(crate::questions::button(&d, "feed-more", format!("Show {} more", total - s.feed_shown.max(PAGE)), false, |s: &mut State| s.feed_shown += 40)));
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days_read_like_a_person_says_them() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 13).unwrap();
        assert_eq!(day_label(today, today), "Today");
        assert_eq!(day_label(today.pred_opt().unwrap(), today), "Yesterday");
        assert_eq!(day_label(NaiveDate::from_ymd_opt(2026, 11, 9).unwrap(), today), "Mon, Nov 9");
    }

    #[test]
    fn filters_pick_files_backup_and_what_you_did() {
        let row = |kind: &str, actor: &str| Row { id: "x".into(), at: Local::now(), kind: kind.into(), actor: actor.into(), reason: String::new(), summary: String::new(), count: 1, undoable: false, batch: None };
        let (filed, saved, moved) = (row("filed", "hestia_rule"), row("backup_saved", "watcher"), row("moved", "you"));
        assert!([&filed, &saved, &moved].iter().all(|r| matches(0, r)));
        assert!(matches(1, &filed) && matches(1, &moved) && !matches(1, &saved));
        assert!(matches(2, &saved) && !matches(2, &filed));
        assert!(matches(3, &moved) && !matches(3, &filed));
    }

    #[test]
    fn who_is_hestia_you_or_backup() {
        assert_eq!((who("hestia_rule"), who("model"), who("you"), who("watcher"), who("unknown")), ("Hestia", "Hestia", "You", "Backup", ""));
    }

    /// The whole path: a real move through the engine shows up in the feed, undoable, and Undo puts it back.
    #[test]
    fn a_real_move_appears_in_the_feed_and_can_be_undone() {
        let home = std::env::temp_dir().join(format!("hestia-feed-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let stream = home.join(".stream");
        std::fs::create_dir_all(home.join("dir")).unwrap();
        std::fs::write(home.join("a.txt"), "x").unwrap();
        let plan = lifestream::tree::Plan { moves: vec![lifestream::tree::Move { id: String::new(), from: home.join("a.txt"), to: home.join("dir/a.txt"), size: 1 }], skipped: vec![] };
        lifestream::tree::apply(&plan, &stream, &home).unwrap();
        let items = lifestream::feed::feed(&home, &stream, std::path::Path::new("/nonexistent/history"), &lifestream::feed::Opts { since: None, limit: 10 });
        let rows: Vec<Row> = items.iter().filter_map(Row::from_item).collect();
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert!(rows[0].undoable && rows[0].batch.is_some());
        lifestream::tree::undo_batch(&stream, &home, rows[0].batch.as_deref()).unwrap();
        assert!(home.join("a.txt").exists());
        let _ = std::fs::remove_dir_all(&home);
    }
}
