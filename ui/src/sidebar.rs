//! The sidebar: where you are, and where you can go. Today and Ask on top, then what you can
//! explore (time, files, history), what Hestia organizes (questions, system), then the stream's saved views.

use super::{icons, style, Pane, State};
use crate::design::{Design, Surface};
use rune::tok::{self, type_};
use rune::{col, row, text, El, ScrollSpec};

fn nav<F: Fn(&mut State) + 'static>(d: &Design, key: &str, glyph: Option<&'static rune::Icon>, label: &str, badge: String, on: bool, press: F) -> El {
    let t = d.theme();
    let acc = d.accent_color();
    row().key(key.to_string()).h(d.row_h()).px(d.gap()).gap(d.gap()).center_y().radius(d.r(tok::radius::SM))
        .fill(if on { d.select() } else { rune::Color::TRANSPARENT })
        .on_press(move |s: &mut State| { s.msg.clear(); press(s) })
        .child(match glyph {
            Some(g) => rune::icon(g, d.size(16.0)).color(if on { acc } else { t.text_secondary }),
            None => col().size(d.size(16.0), d.size(16.0)),
        })
        .child(text(label.to_string()).font(style(d, type_::LABEL)).color(t.text).grow(1.0))
        .child(text(badge).font(style(d, type_::LABEL)).color(t.text_secondary))
}

pub fn panel(s: &State) -> El {
    let d = s.design;
    let here = crate::nav::place_of(s.pane);
    use crate::nav::Place;
    let c = col().w_full().gap(2.0)
        .child(nav(&d, "nav-stream", Some(&icons::TODAY), "Stream", if s.bundles.is_empty() { String::new() } else { s.bundles.len().to_string() }, here == Place::Stream, |s| s.go_place(Place::Stream)))
        .child(nav(&d, "nav-library", Some(&icons::FOLDER), "Library", String::new(), here == Place::Library, |s| s.go_place(Place::Library)))
        .child(nav(&d, "nav-computer", Some(&icons::LAPTOP), "Computer", String::new(), here == Place::Computer, |s| s.go_place(Place::Computer)));
    col().w(220.0).padding(d.gap()).gap(d.gap()).side(&d)
        .child(c.scroll(ScrollSpec { stick_bottom: false }).shrink().min_h(0.0).grow(1.0))
        .child(nav(&d, "nav-settings", Some(&icons::SETTINGS), "Settings", String::new(), here == Place::Settings, |s| s.go_pane(Pane::Settings)))
}
