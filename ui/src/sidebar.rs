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
        .fill(if on { acc } else { rune::Color::TRANSPARENT })
        .on_press(move |s: &mut State| { s.msg.clear(); press(s) })
        .child(match glyph {
            Some(g) => rune::icon(g, d.size(16.0)).color(if on { d.on_accent() } else { t.text_secondary }),
            None => col().size(d.size(16.0), d.size(16.0)),
        })
        .child(text(label.to_string()).font(style(d, type_::LABEL)).color(if on { d.on_accent() } else { t.text }).grow(1.0))
        .child(text(badge).font(style(d, type_::LABEL)).color(if on { d.on_accent() } else { t.text_secondary }))
}

fn heading(d: &Design, label: &str) -> El {
    text(label.to_string()).font(style(d, type_::SECTION)).color(d.theme().text_secondary).px(d.gap()).py(d.gap() * 0.6)
}

pub fn panel(s: &State) -> El {
    let d = s.design;
    let p = s.pane;
    let c = col().w_full().gap(2.0)
        .child(nav(&d, "nav-today", Some(&icons::TODAY), "Today", String::new(), p == Pane::Today, |s| s.pane = Pane::Today))
        .child(nav(&d, "view-ask", Some(&icons::ASK), "Search", String::new(), p == Pane::Ask, |s| { s.pane = Pane::Ask; s.ask.focused = true }))
        .child(heading(&d, "EXPLORE"))
        .child(nav(&d, "view-time", Some(&icons::TIME), "Time", String::new(), p == Pane::Time, |s| s.pane = Pane::Time))
        .child(nav(&d, "view-files", Some(&icons::FOLDER), "Files", s.tree.folders.len().to_string(), p == Pane::Files, |s| { s.pane = Pane::Files; s.open_files() }))
        .child(nav(&d, "view-hist", Some(&icons::HISTORY), "History", "git".into(), p == Pane::History, |s| { s.pane = Pane::History; s.reload_repo() }))
        .child(nav(&d, "view-machine", Some(&icons::LAPTOP), "Machine", String::new(), p == Pane::Machine, |s| { s.pane = Pane::Machine; s.reload_machine() }))
        .child(heading(&d, "ORGANIZE"))
        .child(nav(&d, "view-q", Some(&icons::QUESTIONS), "Questions", s.bundles.len().to_string(), p == Pane::Questions, |s| s.pane = Pane::Questions))
        .child(nav(&d, "view-sys", Some(&icons::SYSTEM), "System", if s.status.ollama_up { "●".into() } else { "○".into() }, p == Pane::System, |s| s.pane = Pane::System))
        ;
    col().w(220.0).padding(d.gap()).gap(d.gap()).side(&d)
        .child(c.scroll(ScrollSpec { stick_bottom: false }).shrink().min_h(0.0).grow(1.0))
        .child(nav(&d, "nav-settings", Some(&icons::SETTINGS), "Settings", String::new(), p == Pane::Settings, |s| s.pane = Pane::Settings))
}
