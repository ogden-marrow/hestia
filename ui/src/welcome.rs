//! The first-run tour: what Hestia does in four short screens, and the two desktop steps
//! (the launcher shortcut and clipboard history) done for you. Shown once; Settings → About reopens it.

use super::{style, Pane, State};
use crate::design::Role;
use rune::tok::type_;
use rune::{col, el, row, text, El, ScrollSpec};

const STEPS: usize = 5;

fn line(s: &State, head: &str, body: &str) -> El {
    let d = s.design;
    let t = d.theme();
    col().w_full().gap(crate::design::space::XS)
        .child(text(head.to_string()).font(d.text(Role::Title)).color(t.text))
        .child(text(body.to_string()).font(d.text(Role::Body)).color(t.text_secondary).wrap_anywhere().shrink())
}

pub fn finish(s: &mut State) {
    s.change_settings(|st| st.welcomed = true);
    s.msg.clear();
    s.pane = Pane::Today;
}

/// The Continue button, from anywhere in the tour (the sign-in flow uses it when it is done).
pub fn advance(s: &mut State) {
    s.msg.clear();
    if s.wstep + 1 >= STEPS { finish(s) } else { s.wstep += 1; if s.wstep == 2 { crate::account::begin(s); } }
}

pub fn open(s: &mut State) {
    s.wstep = 0;
    s.msg.clear();
    s.pane = Pane::Welcome;
}

pub fn screen(s: &State) -> El {
    let d = s.design;
    let t = d.theme();
    let acc = d.accent_color();
    let step = s.wstep.min(STEPS - 1);

    // The sign-in flow (step 3) draws itself; every other step is plain text below.
    let mut problem = None;
    let mut actions: Option<Vec<El>> = None;
    let flow = if step == 2 { crate::account::view(s) } else { None };
    let (title, mut body): (String, El) = if let Some(v) = flow {
        problem = v.problem;
        actions = Some(v.actions);
        (v.title, v.body)
    } else { let (t, b) = match step {
        0 => ("Welcome to Hestia", col().w_full().gap(d.section())
            .child(line(s, "It files your computer for you.", "Every night it reads your folders and works out what each file is and where it belongs."))
            .child(line(s, "It only asks when it is unsure.", "Each morning it tells you what it filed and asks a few questions about the rest."))
            .child(line(s, "Your files stay here.", "They are read only by models on this computer, and Hestia never deletes a file. Only notes about how your computer is set up are saved, to your private GitHub repository."))),
        1 => ("Where things are", col().w_full().gap(d.section())
            .child(line(s, "Questions", "Folders it could not place. Your answer teaches it."))
            .child(line(s, "Files", "Everything you own, grouped by what it is for, with each folder's rule and contents."))
            .child(line(s, "Time and History", "Look back at any day, or at the history of a project."))
            .child(line(s, "Machine", "Your settings and installed apps, captured so a new computer can be rebuilt."))),
        2 => ("", col()), // drawn by the sign-in flow (account.rs)
        3 => {
            let state = crate::clipboard::extension_state();
            let on = state == "recording";
            ("Hestia Search", col().w_full().gap(d.pad())
                .child(line(s, &format!("Press {} anywhere and type.", crate::setup::SHORTCUT_NAME), "Find apps, files and Hestia itself."))
                .child(line(s, "A small part of Hestia lives in the desktop.", "GNOME only lets the shell draw the launcher, so Hestia adds one extension. Turn it on here; you log out and in once."))
                .child(row().gap(d.pad()).center_y()
                    .child(crate::questions::action(&d, "w-ext", "Turn on", "Turning on…", !on, s.jobs.running("search-on"), |s: &mut State| s.turn_on_search()))
                    .child(text(if on { "on".to_string() } else { format!("not yet: {state}") }).font(style(&d, type_::LABEL)).color(if on { acc } else { t.text_secondary })))
                .child(line(s, "Remember what you copy (optional)", crate::clipboard::PRIVACY))
                .child(row().child(crate::prefs::on_off(&d, "w-clip", s.settings.clipboard_on, crate::prefs::set_clipboard))))
        }
        _ => ("You are set", col().w_full().gap(d.section())
            .child(line(s, "That is all.", "Your first scan runs tonight. Until then Hestia shows what it already knows."))
            .child(line(s, "Everything you chose is in Settings.", "You can run this tour again from Settings, About."))),
    }; (t.to_string(), b) };
    if !s.msg.is_empty() && (step == 2 || step == 3) {
        body = body.child(text(s.msg.clone()).font(style(&d, type_::LABEL)).color(acc).wrap_anywhere().shrink());
    }

    let last = step == STEPS - 1;
    let mut nav = row().w_full().gap(d.gap()).center_y()
        .child(text(if s.flow_return.is_some() { "Settings · Account".to_string() } else { format!("Step {} of {}", step + 1, STEPS) }).font(d.text(Role::Label)).color(t.text_secondary))
        .child(el().grow(1.0));
    nav = match actions {
        Some(acts) => {
            // the flow's own buttons, with a way to leave it for later
            nav = nav.child(crate::questions::quiet(&d, "w-later", if s.flow_return.is_some() { "Back to Settings" } else { "Skip for now" }, |s: &mut State| crate::account::done(s)));
            acts.into_iter().fold(nav, |n, a| n.child(a))
        }
        None => nav
            .child(crate::questions::quiet(&d, "w-skip", if last { "Back" } else { "Skip" }, move |s: &mut State| if last { s.wstep -= 1 } else { finish(s) }))
            .child(crate::questions::primary(&d, "w-next", if last { "Open Hestia" } else { "Continue" }, move |s: &mut State| {
                s.msg.clear();
                if last { finish(s) } else { s.wstep += 1; if s.wstep == 2 { crate::account::begin(s); } }
            })),
    };

    // No floating box: the whole window is the page, with one centred column of fixed measure.
    // The heading sits at the same height on every step; the body scrolls, the buttons stay put.
    let column = col().w(d.size(520.0)).h_full().gap(d.section()).py(d.section() * 2.0)
        .child(col().w_full().gap(d.gap())
            .child(match problem {
                Some(p) => text(p.to_string()).font(d.text(Role::Label)).color(d.danger()),
                None => el().size(1.0, 1.0),
            })
            .child(text(title).font(d.text(Role::Display)).color(t.text)))
        .child(col().w_full().grow(1.0).shrink().scroll(ScrollSpec { stick_bottom: false }).child(body))
        .child(nav);
    row().w_full().h_full().fill(t.card).child(el().grow(1.0)).child(column).child(el().grow(1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skip_finishes_the_tour_and_lands_on_today() {
        let mut s = State::default();
        open(&mut s);
        assert_eq!((s.wstep, s.pane), (0, Pane::Welcome));
        finish(&mut s);
        assert_eq!(s.pane, Pane::Today);
        assert!(s.settings.welcomed);
    }

    #[test]
    fn every_step_renders() {
        for step in 0..STEPS {
            let s = State { wstep: step, ..State::default() };
            let _ = screen(&s);
        }
    }
}
