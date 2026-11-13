//! The Applications screen: everything installed that can be opened, with its icon, searchable,
//! filterable (All, Games, Apps), grouped and sortable. Click to open.

use super::{style, State};
use crate::apps::{self, Item};
use crate::design::Surface;
use rune::tok::{self, type_};
use rune::{col, row, text, El, ScrollSpec};

fn today_count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

const SHOW: [&str; 3] = ["All", "Games", "Apps"];
const SORT: [&str; 4] = ["Name", "Most used", "By kind", "By source"];

/// The shelf an item sits on for a sort: kind and source group the list, name and most-used do not.
fn shelf(it: &Item, sort: usize) -> &'static str {
    match sort { 2 => it.group, 3 => it.source.label(), _ => "" }
}

/// A muted hue per app, so two apps with the same first letter still look different.
fn tint(name: &str) -> rune::Color {
    let h = name.bytes().fold(7u32, |a, b| a.wrapping_mul(31).wrapping_add(b as u32)) % 360;
    rune::oklch(0.62, 0.07, h as f32)
}

fn icon(s: &State, it: &Item) -> El {
    let d = s.design;
    let size = d.size(32.0);
    match &it.icon {
        Some(p) => col().size(size, size).pixels(s.icons.app_icon(p, 32.0).pixels()),
        None => col().size(size, size).center().radius(d.r(tok::radius::MD)).fill(tint(&it.name))
            .child(text(it.name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default()).font(style(&d, type_::TITLE)).color(d.on_accent())),
    }
}

pub fn panel(s: &State) -> El {
    let d = s.design;
    let t = d.theme();
    let idx = apps::visible(&s.library, &s.app_q.text, s.app_show, s.app_sort, &s.launches);
    let games = s.library.iter().filter(|a| a.game).count();

    let controls = row().w_full().gap(d.pad()).center_y().flex_wrap()
        .child(super::prefs::segmented(&d, "app-show", &SHOW, s.app_show, |s, i| s.app_show = i))
        .child(super::prefs::segmented(&d, "app-sort", &SORT, s.app_sort, |s, i| s.app_sort = i));

    let mut list = col().w_full().gap(2.0);
    let mut last = "";
    for &i in &idx {
        let it = &s.library[i];
        let head = shelf(it, s.app_sort);
        if s.app_sort >= 2 && head != last {
            list = list.child(text(head.to_string()).font(style(&d, type_::SECTION)).color(t.text_secondary).px(d.item_px()).pt(d.pad()).pb(d.gap() * 0.5));
            last = head;
        }
        let sub = match (it.about.as_str(), it.source) { ("", crate::apps::Source::Native) => it.group.to_string(), ("", src) => src.label().to_string(), (a, crate::apps::Source::Native) => a.to_string(), (a, src) if a == "Steam game" && src == crate::apps::Source::Steam => "Steam".to_string(), (a, src) => format!("{a} · {}", src.label()) };
        list = list.child(
            row().key(format!("app-{}", it.id)).w_full().min_h(d.row_h() + d.gap()).px(d.item_px()).py(d.gap() * 0.5).gap(d.pad()).center_y().radius(d.r(tok::radius::SM))
                .on_press(move |s: &mut State| s.open_item(i))
                .child(icon(s, it))
                .child(col().grow(1.0).shrink().min_w(0.0).gap(1.0)
                    .child(text(it.name.clone()).font(style(&d, type_::BODY)).color(t.text).truncate())
                    .child(text(sub).font(style(&d, type_::LABEL)).color(t.text_secondary).truncate())),
        );
    }
    if idx.is_empty() {
        list = list.child(text(if s.library.is_empty() { "No applications found." } else { "Nothing matches. Try fewer letters, or choose All." }.to_string())
            .font(style(&d, type_::LABEL)).color(t.text_secondary).px(d.item_px()).py(d.pad()));
    }

    col().grow(1.0).shrink().min_w(0.0).padding(d.pad()).gap(d.pad()).panel(&d)
        .child(text(format!("{} installed: {} and {}.", s.library.len(), today_count(s.library.len() - games, "app", "apps"), today_count(games, "game", "games"))).font(style(&d, type_::LABEL)).color(t.text_secondary))
        .child(super::field(&d, "app-search", &s.app_q, "Find an app or game", |s: &mut State| &mut s.app_q))
        .child(controls)
        .child(col().w_full().scroll(ScrollSpec { stick_bottom: false }).shrink().min_h(0.0).grow(1.0).child(list))
}
