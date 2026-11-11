//! The time view: the stream as it was on a given day. Cards for earlier days recede behind the
//! selected one, and a ruler of months on the right jumps through time (Up/Left = back, Down/Right = forward).
//! "As it was" means files whose own date is on or before that day; the stream records no
//! deletion dates, so vanished files still count on the days they existed.

use super::{style, State};
use crate::design::Surface;
use chrono::{Datelike, NaiveDate};
use lifestream::{human, Record};
use rune::tok::{self, type_};
use crate::questions::button;
use rune::{col, oklcha, row, text, El, ScrollSpec, Spring, TextStyle};

#[derive(Clone, Debug)]
pub struct Day {
    pub date: String, // YYYY-MM-DD, the files' own modification date
    pub idx: Vec<usize>, // indexes into the records
}

/// Days that have files, newest first. Regenerable thumbnails are left out.
pub fn days(recs: &[Record]) -> Vec<Day> {
    let mut by: std::collections::BTreeMap<&str, Vec<usize>> = std::collections::BTreeMap::new();
    for (i, r) in recs.iter().enumerate() {
        if r.tags.iter().any(|t| t == "derived") || r.mtime.len() < 10 {
            continue;
        }
        by.entry(&r.mtime[..10]).or_default().push(i);
    }
    by.into_iter().rev().map(|(d, idx)| Day { date: d.to_string(), idx }).collect()
}

/// cum[i] = how many files exist on or before days[i] (days are newest first).
pub fn cumulative(days: &[Day]) -> Vec<usize> {
    let mut cum = vec![0; days.len()];
    let mut run = 0;
    for i in (0..days.len()).rev() {
        run += days[i].idx.len();
        cum[i] = run;
    }
    cum
}

/// (YYYY-MM, index of that month's newest day, files that month), newest month first.
pub fn months(days: &[Day]) -> Vec<(String, usize, usize)> {
    let mut out: Vec<(String, usize, usize)> = Vec::new();
    for (i, d) in days.iter().enumerate() {
        let m = &d.date[..7];
        match out.last_mut() {
            Some(last) if last.0 == m => last.2 += d.idx.len(),
            _ => out.push((m.to_string(), i, d.idx.len())),
        }
    }
    out
}

pub fn long_date(date: &str) -> String {
    NaiveDate::parse_from_str(date, "%Y-%m-%d").map(|d| d.format("%A, %B %-d, %Y").to_string()).unwrap_or_else(|_| date.to_string())
}

fn month_label(m: &str) -> String {
    let (y, mo) = (m[..4].parse::<i32>().unwrap_or(0), m[5..7].parse::<u32>().unwrap_or(1));
    NaiveDate::from_ymd_opt(y, mo, 1).map(|d| format!("{} {}", d.format("%b"), d.year())).unwrap_or_else(|| m.to_string())
}

const DEPTH: usize = 5;
const CARD_W: f32 = 860.0;
const CARD_H: f32 = 430.0;
const STRIP: f32 = 30.0; // how much of each receding window shows above the next

fn kind_icon(kind: &str) -> &'static rune::Icon {
    if kind == "project/git" { &crate::icons::FOLDER } else { &crate::icons::DOC }
}

pub fn panel(s: &State) -> El {
    let d = s.design;
    let t = d.theme();
    let acc = d.accent_color();
    let Some(day) = s.days.get(s.dsel) else {
        return col().grow(1.0).padding(d.pad()).panel(&d).child(text("Nothing in the stream yet.").font(style(&d, type_::BODY)).color(t.text_secondary));
    };
    let shadow = oklcha(0.0, 0.0, 0.0, if d.dark { 0.45 } else { 0.18 });

    // ---- the stack: later days in front, earlier days receding behind, like windows
    let mut stage = col().w(CARD_W + 72.0).h(CARD_H + STRIP * DEPTH as f32 + 40.0);
    for k in (0..=DEPTH).rev() {
        let Some(card) = s.days.get(s.dsel + k) else { continue };
        let scale = 1.0 - 0.045 * k as f32;
        let (w, h) = (CARD_W * scale, CARD_H * scale);
        let x = 36.0 + (CARD_W - w) / 2.0;
        let y = 18.0 + STRIP * (DEPTH - k) as f32;
        let opacity = [1.0, 0.92, 0.82, 0.72, 0.62, 0.5][k];
        let title_bar = row().w_full().h(d.size(STRIP)).px(d.pad()).gap(d.gap()).center_y().fill(d.side_fill())
            .border_sides([0.0, 0.0, 1.0, 0.0], t.card_border)
            .child(col().size(8.0, 8.0).radius(2.0).fill(if k == 0 { acc } else { t.text_secondary }))
            .child(text(long_date(&card.date)).font(style(&d, type_::TITLE)).color(t.text).grow(1.0).shrink().min_w(0.0).truncate())
            .child(text(format!("{} files", card.idx.len())).font(style(&d, type_::LABEL)).color(t.text_secondary));
        let mut c = col().key(format!("card-{}", card.date)).absolute(x, y).size(w, h).opacity(opacity)
            .fill(t.card).border(1.0, t.card_border).radius(d.r(tok::radius::MD)).shadow(4.0, 16.0, shadow)
            .motion(Spring::new(0.35, 0.22))
            .on_press(move |s: &mut State| s.dsel += k)
            .child(title_bar);
        if k == 0 {
            c = c.child(if s.compare { compare_body(s, &d) } else { list_body(s, &d, card) });
        }
        stage = stage.child(c);
    }
    let stage = row().w_full().grow(1.0).shrink().min_h(0.0).center().fill(t.bg).border_sides([1.0, 0.0, 1.0, 0.0], t.card_border).child(stage);

    // ---- the ruler: one tick per month, the chosen one marked
    let current = &day.date[..7];
    let mut ruler = col().w_full();
    for (n, (m, first, count)) in months(&s.days).into_iter().enumerate() {
        let on = m == current;
        let january = m.ends_with("-01");
        ruler = ruler.child(
            row().key(format!("month-{n}")).w_full().h(d.size(24.0)).pr(d.item_px()).gap(d.gap()).center_y()
                .fill(if on { d.select() } else { rune::Color::TRANSPARENT })
                .on_press(move |s: &mut State| s.dsel = first)
                .child(col().w(if on { 22.0 } else if january { 16.0 } else { 9.0 }).h(if on { 2.0 } else { 1.0 }).fill(if on { acc } else { t.text_secondary }))
                .child(text(month_label(&m)).font(style(&d, if on { type_::TITLE } else { type_::LABEL })).color(if on { acc } else { t.text }).grow(1.0))
                .child(text(count.to_string()).font(style(&d, type_::MONO_CAPTION)).color(t.text_secondary)),
        );
    }
    let ruler = col().w(190.0).py(d.gap()).side(&d).border_sides([0.0, 0.0, 0.0, 1.0], t.card_border)
        .child(text("GO BACK IN TIME").font(style(&d, type_::SECTION)).color(t.text_secondary).px(d.item_px()).py(d.gap()))
        .child(col().w_full().scroll(ScrollSpec { stick_bottom: false }).shrink().min_h(0.0).grow(1.0).child(ruler));

    // ---- heading and controls
    let total = s.cum.get(s.dsel).copied().unwrap_or(0);
    let heading = col().w_full().px(d.pad()).py(d.gap()).gap(2.0)
        .child(text("THE STREAM, AS IT WAS ON").font(style(&d, type_::SECTION)).color(t.text_secondary))
        .child(text(long_date(&day.date)).font(TextStyle { size: d.size(26.0), ..style(&d, type_::TITLE) }).color(t.text));
    let summary = if s.compare {
        let since = s.days[..s.dsel].iter().map(|x| x.idx.len()).sum::<usize>();
        format!("Since then: {since} new files")
    } else {
        format!("{} files dated this day · {} existed by then", day.idx.len(), total)
    };
    let bar = row().w_full().px(d.pad()).py(d.gap()).gap(d.gap()).center_y().border_sides([1.0, 0.0, 0.0, 0.0], t.card_border)
        .child(text(summary).font(style(&d, type_::BODY)).color(t.text_secondary).grow(1.0).shrink().min_w(0.0).truncate())
        .child(button(&d, "older", "◀  Older".into(), false, |s: &mut State| s.step_time(1)))
        .child(button(&d, "newer", "Newer  ▶".into(), false, |s: &mut State| s.step_time(-1)))
        .child(button(&d, "compare", if s.compare { "Back to this day".into() } else { "Compare with today".to_string() }, !s.compare, |s: &mut State| s.compare = !s.compare));
    let main = col().grow(1.0).shrink().min_w(0.0).panel(&d).child(heading).child(stage).child(bar);
    row().w_full().grow(1.0).min_h(0.0).child(main).child(ruler)
}

/// The day's files as a Finder-style list: Name, Kind, Size, with alternating rows.
fn list_body(s: &State, d: &crate::design::Design, card: &Day) -> El {
    let t = d.theme();
    let mut ix = card.idx.clone();
    ix.sort_by_key(|&i| std::cmp::Reverse(s.recs[i].size));
    let head = row().w_full().h(d.size(24.0)).px(d.pad()).gap(d.pad()).center_y().border_sides([0.0, 0.0, 1.0, 0.0], t.card_border)
        .child(text("Name").font(style(d, type_::LABEL)).color(t.text_secondary).grow(1.0))
        .child(text("Kind").font(style(d, type_::LABEL)).color(t.text_secondary).w(d.size(170.0)))
        .child(text("Size").font(style(d, type_::LABEL)).color(t.text_secondary).w(d.size(80.0)));
    let mut list = col().w_full().child(head);
    for (n, &i) in ix.iter().take(12).enumerate() {
        let r = &s.recs[i];
        let src = r.sources.first().or(r.was.first()).map(|x| x.as_str()).unwrap_or("");
        list = list.child(
            row().w_full().h(d.row_h() - 2.0).px(d.pad()).gap(d.pad()).center_y()
                .fill(if n % 2 == 1 { t.control_track } else { rune::Color::TRANSPARENT })
                .child(rune::icon(kind_icon(&r.kind), d.size(14.0)).color(t.text_secondary))
                .child(text(src.rsplit('/').next().unwrap_or(src).to_string()).font(style(d, type_::BODY)).color(t.text).grow(1.0).shrink().min_w(0.0).truncate())
                .child(text(r.kind.clone()).font(style(d, type_::LABEL)).color(t.text_secondary).w(d.size(170.0)))
                .child(text(human(r.size)).font(style(d, type_::MONO_CAPTION)).color(t.text_secondary).w(d.size(80.0))),
        );
    }
    if ix.len() > 12 {
        list = list.child(text(format!("+ {} more that day", ix.len() - 12)).font(style(d, type_::LABEL)).color(t.text_secondary).px(d.pad()).py(d.gap()));
    }
    list
}

/// What has been added since this day: the kinds, as bars.
fn compare_body(s: &State, d: &crate::design::Design) -> El {
    let t = d.theme();
    let mut kinds: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for x in &s.days[..s.dsel] {
        for &i in &x.idx {
            *kinds.entry(s.recs[i].kind.as_str()).or_default() += 1;
        }
    }
    let mut top: Vec<_> = kinds.into_iter().collect();
    top.sort_by_key(|k| std::cmp::Reverse(k.1));
    let max = top.first().map(|k| k.1).unwrap_or(1) as f32;
    let mut c = col().w_full().padding(d.pad()).gap(d.gap())
        .child(text("What arrived since this day").font(style(d, type_::TITLE)).color(t.text));
    for (kind, n) in top.into_iter().take(10) {
        c = c.child(row().w_full().gap(d.pad()).center_y()
            .child(text(kind.to_string()).font(style(d, type_::LABEL)).color(t.text).w(d.size(170.0)))
            .child(col().w(420.0 * n as f32 / max).h(10.0).radius(2.0).fill(d.accent_color()))
            .child(text(n.to_string()).font(style(d, type_::MONO_CAPTION)).color(t.text_secondary)));
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;
    use lifestream::By;

    fn rec(id: &str, mtime: &str, tags: &[&str]) -> Record {
        Record {
            id: id.into(), first_seen: "n".into(), mtime: mtime.into(), kind: "document/text".into(), size: 5,
            sources: vec![format!("~/{id}")], tags: tags.iter().map(|s| s.to_string()).collect(), summary: None,
            by: By { tier: "rule".into(), name: "x".into(), confidence: 1.0 }, status: "filed".into(), store: None, future_at: None, was: vec![],
        }
    }

    #[test]
    fn days_are_newest_first_and_skip_thumbnails() {
        let recs = vec![
            rec("a", "2026-10-18T10:00:00Z", &[]), rec("b", "2026-10-18T11:00:00Z", &[]),
            rec("c", "2026-11-02T10:00:00Z", &[]), rec("t", "2026-11-02T12:00:00Z", &["derived"]),
        ];
        let d = days(&recs);
        assert_eq!(d.iter().map(|x| x.date.as_str()).collect::<Vec<_>>(), ["2026-11-02", "2026-10-18"]);
        assert_eq!((d[0].idx.len(), d[1].idx.len()), (1, 2));
        assert_eq!(cumulative(&d), vec![3, 2]);
        let m = months(&d);
        assert_eq!(m, vec![("2026-11".to_string(), 0, 1), ("2026-10".to_string(), 1, 2)]);
        assert_eq!(long_date("2026-10-18"), "Sunday, October 18, 2026");
    }
}
