//! Settings: the one place everything is chosen. Changes apply and save the moment you make them.

use super::{field, style, State};
use crate::design::{Density, Design, Surface};
use crate::settings::{root_ok, Accent, Scheme, Settings};
use rune::tok::{self, type_};
use rune::{col, row, text, El, ScrollSpec, Tx};

const TABS: [&str; 8] = ["Appearance", "General", "Launcher", "Scanning", "Questions", "Files", "Domains", "About"];

pub(crate) fn segmented<F: Fn(&mut State, usize) + Clone + 'static>(d: &Design, key: &str, labels: &[&str], on: usize, f: F) -> El {
    let t = d.theme();
    let acc = d.accent_color();
    let mut r = row().gap(0.0).radius(d.r(tok::radius::SM)).border(1.0, t.card_border);
    for (i, l) in labels.iter().enumerate() {
        let f = f.clone();
        r = r.child(
            row().key(format!("{key}-{i}")).h(d.row_h() - 2.0).px(d.pad()).center_y()
                .fill(if i == on { acc } else { rune::Color::TRANSPARENT })
                .on_press(move |s: &mut State| f(s, i))
                .child(text(l.to_string()).font(style(d, type_::LABEL)).color(if i == on { d.on_accent() } else { t.text })),
        );
    }
    r
}

pub(crate) fn setting(d: &Design, title: &str, desc: &str, control: El) -> El {
    let t = d.theme();
    row().w_full().gap(d.pad()).center_y().py(d.gap()).border_sides([0.0, 0.0, 1.0, 0.0], t.card_border)
        .child(col().grow(1.0).shrink().min_w(0.0).gap(2.0)
            .child(text(title.to_string()).font(style(d, type_::BODY)).color(t.text))
            .child(text(desc.to_string()).font(style(d, type_::LABEL)).color(t.text_secondary).wrap_anywhere().shrink()))
        .child(control)
}

fn heading(d: &Design, label: &str) -> El {
    text(label.to_string()).font(style(d, type_::TITLE)).color(d.theme().text).py(d.gap())
}

fn accent_picker(s: &State) -> El {
    let d = s.design;
    let t = d.theme();
    let mut r = row().gap(d.gap()).center_y();
    for a in Accent::ALL {
        let c = Design::build(&Settings { accent: a, ..s.settings.clone() }, s.desktop).accent_color();
        let on = s.settings.accent == a;
        r = r.child(
            col().key(format!("accent-{}", a.name())).size(24.0, 24.0).radius(3.0).fill(c)
                .border(if on { 2.5 } else { 1.0 }, if on { t.text } else { t.card_border })
                .on_press(move |s: &mut State| s.change_settings(|st| st.accent = a)),
        );
    }
    r
}

fn bump((h, m): (u8, u8), dh: i32, dm: i32) -> (u8, u8) {
    let total = (h as i32 * 60 + m as i32 + dh * 60 + dm).rem_euclid(24 * 60);
    ((total / 60) as u8, (total % 60) as u8)
}

/// A time of day with a button to go earlier or later by an hour or by five minutes.
fn clock<F: Fn(&mut State, i32, i32) + Clone + 'static>(d: &Design, key: &str, (h, m): (u8, u8), f: F) -> El {
    let t = d.theme();
    let step = |label: &str, k: String, dh: i32, dm: i32, f: F| {
        row().key(k).size(d.row_h() - 4.0, d.row_h() - 4.0).center().radius(d.r(tok::radius::SM)).border(1.0, t.card_border)
            .on_press(move |s: &mut State| f(s, dh, dm))
            .child(text(label.to_string()).font(style(d, type_::LABEL)).color(t.text))
    };
    row().gap(d.gap() * 0.5).center_y()
        .child(step("−", format!("{key}-h-"), -1, 0, f.clone()))
        .child(text(format!("{h:02}")).font(style(d, type_::TITLE)).color(t.text).w(d.size(22.0)))
        .child(step("+", format!("{key}-h+"), 1, 0, f.clone()))
        .child(text(":").font(style(d, type_::TITLE)).color(t.text_secondary))
        .child(step("−", format!("{key}-m-"), 0, -5, f.clone()))
        .child(text(format!("{m:02}")).font(style(d, type_::TITLE)).color(t.text).w(d.size(22.0)))
        .child(step("+", format!("{key}-m+"), 0, 5, f))
}

pub(crate) fn on_off<F: Fn(&mut State, bool) + Clone + 'static>(d: &Design, key: &str, on: bool, f: F) -> El {
    segmented(d, key, &["Off", "On"], on as usize, move |s, i| f(s, i == 1))
}

fn add_root(s: &mut State) {
    let r = s.f_root.text.trim().trim_matches('/').to_string();
    if !root_ok(&r) {
        s.msg = "use a plain folder name under your home folder, like Music or Pictures/Raw".into();
    } else if !super::files::home().join(&r).is_dir() {
        s.msg = format!("~/{r} does not exist");
    } else if !s.settings.roots.contains(&r) {
        s.f_root.clear();
        s.change_schedule(|st| st.roots.push(r));
    }
}

pub fn panels(s: &State) -> Vec<El> {
    let d = s.design;
    let t = d.theme();
    let acc = d.accent_color();
    let scroll = || ScrollSpec { stick_bottom: false };

    let mut tabs = col().w_full().gap(2.0);
    for (i, name) in TABS.iter().enumerate() {
        let on = i == s.stab;
        tabs = tabs.child(
            row().key(format!("tab-{i}")).h(d.row_h()).px(d.item_px()).center_y().radius(d.r(tok::radius::SM))
                .fill(if on { t.control_off } else { rune::Color::TRANSPARENT })
                .on_press(move |s: &mut State| { s.stab = i; if i == 6 && s.dom_edit.id.is_empty() { s.select_domain(0) } })
                .child(text(name.to_string()).font(style(&d, type_::LABEL)).color(t.text)),
        );
    }
    let tabs = col().w(220.0).padding(d.gap()).gap(d.gap()).side(&d)
        .child(text("Settings").font(style(&d, type_::TITLE)).color(t.text).px(d.item_px()).py(d.gap() * 0.5))
        .child(tabs);

    let mut body = col().w_full().gap(0.0).child(heading(&d, TABS[s.stab.min(TABS.len() - 1)]));
    match s.stab {
        0 => {
            body = body
                .child(setting(&d, "Colours", "Follow the desktop, or stay light or dark.",
                    segmented(&d, "scheme", &["Auto", "Light", "Dark"], match s.settings.scheme { Scheme::Auto => 0, Scheme::Light => 1, Scheme::Dark => 2 },
                        |s, i| s.change_settings(|st| st.scheme = [Scheme::Auto, Scheme::Light, Scheme::Dark][i]))))
                .child(setting(&d, "Accent colour", "Used for selection and main buttons. The last swatch follows your desktop.", accent_picker(s)))
                .child(setting(&d, "Spacing", "How much room rows and panels get.",
                    segmented(&d, "density", &["Compact", "Comfortable", "Spacious"], match s.settings.density { Density::Compact => 0, Density::Cozy => 1, Density::Airy => 2 },
                        |s, i| s.change_settings(|st| st.density = [Density::Compact, Density::Cozy, Density::Airy][i]))))
                .child(setting(&d, "Text size", "Scales every piece of text together.",
                    segmented(&d, "text", &["Small", "Default", "Large", "Larger"], [0.92, 1.0, 1.1, 1.2].iter().position(|v| (*v - s.settings.text).abs() < 0.01).unwrap_or(1),
                        |s, i| s.change_settings(|st| st.text = [0.92, 1.0, 1.1, 1.2][i]))));
        }
        1 => {
            body = body
                .child(setting(&d, "Menu bar icon", "Shows how many questions are waiting. Click it to open Hestia.",
                    on_off(&d, "tray", s.settings.tray, |s, on| s.change_schedule(|st| st.tray = on))))
                .child(setting(&d, "Keep my backup up to date", "Hestia notices changes to your settings, programs and config files and saves them when your computer is quiet. Nothing is sent anywhere.",
                    on_off(&d, "watch", s.settings.watch_on, |s, on| s.change_schedule(|st| st.watch_on = on))))
                .child(setting(&d, "Check for changes", "How often Hestia looks for changes.",
                    segmented(&d, "watchmins", &["Every 5 min", "Every 15 min", "Hourly"], match s.settings.watch_mins { 5 => 0, 60 => 2, _ => 1 },
                        |s, i| s.change_schedule(|st| st.watch_mins = [5, 15, 60][i]))))
                .child(setting(&d, "Morning summary", "A notification each morning showing what Hestia sorted overnight.",
                    on_off(&d, "brief", s.settings.brief_on, |s, on| s.change_schedule(|st| st.brief_on = on))))
                .child(setting(&d, "Summary time", "When the morning notification appears.",
                    clock(&d, "brief-time", s.settings.brief_time, |s, dh, dm| s.change_schedule(|st| st.brief_time = bump(st.brief_time, dh, dm)))));
        }
        3 => {
            let mut chips = row().w_full().shrink().min_w(0.0).gaps(d.gap(), d.gap() * 0.5).flex_wrap();
            for (i, r) in s.settings.roots.iter().enumerate() {
                chips = chips.child(
                    row().key(format!("root-{i}")).h(d.size(26.0)).px(d.gap()).gap(4.0).center_y().radius(d.r(tok::radius::SM)).border(1.0, t.card_border)
                        .on_press(move |s: &mut State| {
                            if s.settings.roots.len() > 1 {
                                s.change_schedule(|st| { st.roots.remove(i); });
                            } else {
                                s.msg = "keep at least one folder to scan".into();
                            }
                        })
                        .child(text(format!("~/{r}  ×")).font(style(&d, type_::LABEL)).color(t.text)),
                );
            }
            body = body
                .child(setting(&d, "Overnight scan", "Reads your folders and updates the stream. It never changes your files, and runs at low priority.",
                    on_off(&d, "scan", s.settings.scan_on, |s, on| s.change_schedule(|st| st.scan_on = on))))
                .child(setting(&d, "Scan time", "When the overnight scan starts. Its status and a Scan now button are on the System screen.",
                    clock(&d, "scan-time", s.settings.scan_time, |s, dh, dm| s.change_schedule(|st| st.scan_time = bump(st.scan_time, dh, dm)))))
                .child(col().w_full().py(d.gap()).gap(d.gap())
                    .child(text("Folders Hestia reads (click one to remove it). Inboxes in Files must be inside these.").font(style(&d, type_::BODY)).color(t.text))
                    .child(chips)
                    .child(field(&d, "root-add", &s.f_root, "add a folder under your home folder, like Music ↵", |s: &mut State| &mut s.f_root).on_submit(|s: &mut State, _: &mut Tx| add_root(s))));
        }
        4 => {
            body = body.child(setting(&d, "Answer similar folders together", "When you answer a folder, also answer the other folders in the same project.",
                on_off(&d, "siblings", s.settings.siblings, |s, on| s.change_settings(|st| st.siblings = on))));
        }
        5 => {
            body = body.child(setting(&d, "Show hidden files", "Files whose names start with a dot, like .config.",
                on_off(&d, "hidden", s.settings.hidden, |s, on| s.change_settings(|st| st.hidden = on))));
        }
        6 => {
            body = body.child(crate::domains_ui::tab(s));
        }
        2 => {
            let state = crate::clipboard::extension_state();
            let n = s.clips.len();
            body = body
                .child(setting(&d, "Clipboard history", "Hestia remembers what you copy, so you can find and paste it again from the launcher. Text only, kept on this computer. Anything a password manager marks as a password is never recorded.",
                    on_off(&d, "clip", s.settings.clipboard_on, |s, on| s.change_settings(|st| st.clipboard_on = on))))
                .child(setting(&d, "Keep the last", "How many copied items to remember.",
                    segmented(&d, "clipmax", &["50", "100", "200"], match s.settings.clipboard_max { 50 => 0, 200 => 2, _ => 1 },
                        |s, i| s.change_settings(|st| st.clipboard_max = [50, 100, 200][i]))))
                .child(setting(&d, "Keep for", "Older items are dropped.",
                    segmented(&d, "clipdays", &["1 day", "7 days", "30 days"], match s.settings.clipboard_days { 1 => 0, 30 => 2, _ => 1 },
                        |s, i| s.change_settings(|st| st.clipboard_days = [1, 7, 30][i]))))
                .child(setting(&d, &format!("Recording: {state}"), &format!("{n} items in the history. GNOME does not let apps watch the clipboard in the background, so a small Hestia extension does it. A newly turned-on extension starts after you log out and in once."),
                    row().gap(d.gap())
                        .child(crate::questions::action(&d, "clip-on", "Turn on", "Turning on…", false, s.jobs.running("search-on"), |s: &mut State| s.turn_on_search()))
                        .child(crate::questions::button(&d, "clip-clear", "Clear history".into(), false, |s: &mut State| { crate::clipboard::clear(); s.clips.clear(); s.msg = "clipboard history cleared".into(); }))))
                .child(setting(&d, "Shortcut: Alt+Space", "Opens the launcher from anywhere. It is drawn by the Hestia extension, so it works once the extension is on (use Turn on above, then log out and in once).", text("").font(style(&d, type_::LABEL))));
        }
        _ => {
            body = body
                .child(setting(&d, "Hestia", "The hearth of your computer: files, history and time. Everything stays on this machine.", text(env!("CARGO_PKG_VERSION")).font(style(&d, type_::LABEL)).color(t.text_secondary)))
                .child(setting(&d, "Your repository", &s.repo_dir.to_string_lossy(), text("").font(style(&d, type_::LABEL))))
                .child(setting(&d, "Settings file", "~/.config/hestia/settings. Schedules are applied as small overrides in ~/.config/systemd/user, so removing them restores the defaults.", text("").font(style(&d, type_::LABEL))))
                .child(setting(&d, "Welcome tour", "The short introduction shown the first time Hestia opens.",
                    crate::questions::button(&d, "tour", "Show it again".into(), false, |s: &mut State| crate::welcome::open(s))))
                .child(setting(&d, "Models", "Two small local models suggest labels for folders. Nothing leaves this computer.", text("local").font(style(&d, type_::LABEL)).color(t.text_secondary)));
        }
    }
    if !s.msg.is_empty() {
        body = body.child(text(s.msg.clone()).font(style(&d, type_::LABEL)).color(acc).py(d.gap()).wrap_anywhere().shrink());
    }
    let main = col().grow(1.0).shrink().min_w(0.0).padding(d.pad() * 1.3).gap(d.pad()).panel(&d)
        .child(col().w_full().scroll(scroll()).shrink().min_h(0.0).grow(1.0).child(body));
    vec![tabs, main]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_wrap_around_the_clock() {
        assert_eq!(bump((23, 55), 0, 5), (0, 0));
        assert_eq!(bump((0, 0), 0, -5), (23, 55));
        assert_eq!(bump((2, 30), 1, 0), (3, 30));
        assert_eq!(bump((0, 30), -1, 0), (23, 30));
    }
}
