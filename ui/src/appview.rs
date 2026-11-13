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
const GROUP: [&str; 3] = ["No grouping", "By kind", "By source"];
const SORT: [&str; 2] = ["Name", "Most used"];

fn shelf(it: &Item, group: usize) -> &'static str {
    match group { 1 => it.group, 2 => it.source.label(), _ => "" }
}

fn icon(s: &State, it: &Item) -> El {
    let d = s.design;
    let t = d.theme();
    let size = d.size(32.0);
    match &it.icon {
        Some(p) => col().size(size, size).pixels(s.icons.app_icon(p, 32.0).pixels()),
        None => col().size(size, size).center().radius(d.r(tok::radius::SM)).fill(t.control_off)
            .child(text(it.name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default()).font(style(&d, type_::TITLE)).color(t.text_secondary)),
    }
}

pub fn panel(s: &State) -> El {
    let d = s.design;
    let t = d.theme();
    let idx = apps::visible(&s.library, &s.app_q.text, s.app_show, s.app_sort, &s.launches);
    let games = s.library.iter().filter(|a| a.game).count();

    let controls = row().w_full().gap(d.pad()).center_y().flex_wrap()
        .child(super::prefs::segmented(&d, "app-show", &SHOW, s.app_show, |s, i| s.app_show = i))
        .child(super::prefs::segmented(&d, "app-group", &GROUP, s.app_group, |s, i| s.app_group = i))
        .child(super::prefs::segmented(&d, "app-sort", &SORT, s.app_sort, |s, i| s.app_sort = i));

    let mut list = col().w_full().gap(2.0);
    let mut last = "";
    for &i in &idx {
        let it = &s.library[i];
        let head = shelf(it, s.app_group);
        if s.app_group > 0 && head != last {
            list = list.child(text(head.to_string()).font(style(&d, type_::SECTION)).color(t.text_secondary).px(d.item_px()).pt(d.pad()).pb(d.gap() * 0.5));
            last = head;
        }
        let sub = if it.about.is_empty() { it.source.label().to_string() } else { format!("{} · {}", it.about, it.source.label()) };
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
        .child(col().w_full().gap(2.0)
            .child(text("Applications").font(style(&d, type_::TITLE)).color(t.text))
            .child(text(format!("{} installed: {} and {}. Click one to open it.", s.library.len(), today_count(s.library.len() - games, "app", "apps"), today_count(games, "game", "games"))).font(style(&d, type_::LABEL)).color(t.text_secondary)))
        .child(super::field(&d, "app-search", &s.app_q, "Find an app or game", |s: &mut State| &mut s.app_q))
        .child(controls)
        .child(col().w_full().scroll(ScrollSpec { stick_bottom: false }).shrink().min_h(0.0).grow(1.0).child(list))
}
