//! Where you are: four places in the sidebar (Stream, Library, Computer, Settings), and inside the first
//! three a row of tabs for the screens that belong to them. One list says which screen is in which place.

use super::{style, Pane, State};
use crate::design::Design;
use rune::tok::type_;
use rune::{col, row, text, El};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Stream,
    Library,
    Computer,
    Settings,
}

/// The screens of each place, in tab order, with the tab's label.
pub const STREAM: [(Pane, &str); 3] = [(Pane::Today, "Today"), (Pane::Questions, "Questions"), (Pane::Ask, "Search")];
pub const LIBRARY: [(Pane, &str); 4] = [(Pane::Apps, "Applications"), (Pane::Folders, "Folders"), (Pane::Files, "Rules"), (Pane::Time, "Time")];
pub const COMPUTER: [(Pane, &str); 3] = [(Pane::Machine, "Backup"), (Pane::History, "Saves"), (Pane::System, "Helpers")];

pub fn tabs(place: Place) -> &'static [(Pane, &'static str)] {
    match place {
        Place::Stream => &STREAM,
        Place::Library => &LIBRARY,
        Place::Computer => &COMPUTER,
        Place::Settings => &[],
    }
}

pub fn place_of(p: Pane) -> Place {
    match p {
        Pane::Today | Pane::Questions | Pane::Ask => Place::Stream,
        Pane::Apps | Pane::Folders | Pane::Files | Pane::Time => Place::Library,
        Pane::Machine | Pane::History | Pane::System => Place::Computer,
        Pane::Settings | Pane::Welcome => Place::Settings,
    }
}

impl State {
    /// Go to a screen, loading whatever it shows. The one way to change screens.
    pub(crate) fn go_pane(&mut self, p: Pane) {
        self.msg.clear();
        self.pane = p;
        match p {
            Pane::Ask => self.ask.focused = true,
            Pane::Files => self.open_files(),
            Pane::History => self.reload_repo(),
            Pane::Machine => self.reload_machine(),
            Pane::Folders => self.reload_tree(),
            _ => {}
        }
    }

    /// Go to a place: stay on the current screen if it already belongs there, else its first tab.
    pub(crate) fn go_place(&mut self, place: Place) {
        if place_of(self.pane) == place {
            return;
        }
        match tabs(place).first() {
            Some((p, _)) => self.go_pane(*p),
            None => self.go_pane(Pane::Settings),
        }
    }
}

/// The row of tabs above a place's screens; nothing for Settings.
pub fn tab_bar(s: &State, d: &Design) -> Option<El> {
    let t = d.theme();
    let list = tabs(place_of(s.pane));
    if list.is_empty() {
        return None;
    }
    let mut r = row().w_full().gap(d.gap()).px(d.pad()).pt(d.gap()).center_y().fill(t.card).border_sides([0.0, 0.0, 1.0, 0.0], t.card_border);
    for (p, label) in list {
        let on = s.pane == *p;
        let p = *p;
        let badge = if p == Pane::Questions && !s.bundles.is_empty() { format!("  {}", s.bundles.len()) } else { String::new() };
        r = r.child(
            col().key(format!("tab-{label}")).px(d.item_px()).h(d.row_h())
                .border_sides([0.0, 0.0, if on { 2.0 } else { 0.0 }, 0.0], d.accent_color())
                .on_press(move |s: &mut State| s.go_pane(p))
                .child(text(format!("{label}{badge}")).font(style(d, if on { type_::TITLE } else { type_::LABEL })).color(if on { t.text } else { t.text_secondary })),
        );
    }
    Some(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_screen_is_in_exactly_one_place_and_in_its_places_tabs() {
        for p in [Pane::Today, Pane::Questions, Pane::Ask, Pane::Apps, Pane::Folders, Pane::Files, Pane::Time, Pane::Machine, Pane::History, Pane::System] {
            let place = place_of(p);
            assert!(tabs(place).iter().any(|(q, _)| *q == p), "{p:?} is not a tab of {place:?}");
        }
        for place in [Place::Stream, Place::Library, Place::Computer] {
            for (p, _) in tabs(place) {
                assert_eq!(place_of(*p), place);
            }
        }
    }
}
