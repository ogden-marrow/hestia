//! The first-run tour: what Hestia does in four short screens, and the two desktop steps
//! (the launcher shortcut and clipboard history) done for you. Shown once; Settings → About reopens it.

use super::{style, Pane, State};
use crate::design::{Role, Surface};
use rune::tok::type_;
use crate::questions::button;
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
    if s.wstep + 1 >= STEPS { finish(s) } else { s.wstep += 1; }
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
            .child(line(s, "Everything stays here.", "Nothing leaves this computer, and Hestia never deletes a file."))),
        1 => ("Where things are", col().w_full().gap(d.section())
            .child(line(s, "Questions", "Folders it could not place. Your answer teaches it."))
            .child(line(s, "Files", "Everything you own, grouped by what it is for, with each folder's rule and contents."))
            .child(line(s, "Time and History", "Look back at any day, or at the history of a project."))
            .child(line(s, "Machine", "Your settings and installed apps, captured so a new computer can be rebuilt."))),
        2 => {
            let found = lifestream::home::is_home(&s.repo_dir);
            let folder = s.home_field.text.trim().trim_start_matches("~/").trim_matches('/').to_string();
            let local = if found { s.repo_dir.clone() } else { crate::files::home().join(if folder.is_empty() { "Home" } else { &folder }) };
            let (acct, repo) = (s.gh_account.text.trim().to_string(), s.gh_repo.text.trim().to_string());
            let remote = if found { lifestream::home::remote(&s.repo_dir) } else { None };
            let target = if acct.is_empty() || repo.is_empty() { "choose an account and a name".to_string() } else { format!("github.com/{acct}/{repo}  ·  private") };
            let url = remote.clone().unwrap_or_else(|| if acct.is_empty() || repo.is_empty() { "not set".to_string() } else { lifestream::home::remote_url(&acct, &repo) });
            let row_kv = |k: &str, v: String| row().w_full().gap(d.pad()).child(text(k.to_string()).font(style(&d, type_::LABEL)).color(t.text_secondary).w(d.size(120.0)))
                .child(text(v).font(style(&d, type_::LABEL)).color(t.text).grow(1.0).shrink().min_w(0.0).wrap_anywhere());
            let plan = col().w_full().padding(d.pad()).gap(d.gap()).tile(&d)
                .child(text("Where it will live").font(style(&d, type_::BODY)).color(t.text))
                .child(row_kv("On this computer", crate::files::tilde(&local)))
                .child(row_kv("On GitHub", if remote.is_some() { "already connected".to_string() } else { target }))
                .child(row_kv("Remote (origin)", url))
                .child(row_kv("Pushes", "automatically, every few minutes, once published. Only to a private repository, and never if something looks like a secret.".to_string()));
            let labeled = |label: &str, f: El| col().w_full().gap(3.0).child(text(label.to_string()).font(style(&d, type_::LABEL)).color(t.text_secondary)).child(f);
            let mut b = col().w_full().gap(d.pad());
            b = b.child(line(s, if found { "Found your home repository." } else { "Everything of yours lives in one private git repository." },
                if found { "Hestia keeps it up to date. Here is where it is, and where it publishes." } else { "Documents, pictures, machines, applications and the folder tree, with Hestia itself added as a submodule so the tool is tracked too. Nothing is created until you press the button." }));
            b = b.child(plan);
            if !found {
                b = b.child(labeled("Folder on this computer (inside your home folder)", super::field(&d, "w-home", &s.home_field, "like Home", |s: &mut State| &mut s.home_field)))
                    .child(labeled("GitHub account or organisation", super::field(&d, "w-acct", &s.gh_account, "your GitHub name", |s: &mut State| &mut s.gh_account)))
                    .child(labeled("Repository name on GitHub", super::field(&d, "w-repo", &s.gh_repo, "like home", |s: &mut State| &mut s.gh_repo)))
                    .child(labeled("Hestia's own repository (optional until it is published)", super::field(&d, "w-hestia", &s.hestia_field, "https://github.com/…", |s: &mut State| &mut s.hestia_field)))
                    .child(button(&d, "w-create", "Create on this computer".into(), true, |s: &mut State| s.create_home()));
            } else if remote.is_none() {
                b = b.child(labeled("GitHub account or organisation", super::field(&d, "w-acct", &s.gh_account, "your GitHub name", |s: &mut State| &mut s.gh_account)))
                    .child(labeled("Repository name on GitHub", super::field(&d, "w-repo", &s.gh_repo, "like home", |s: &mut State| &mut s.gh_repo)));
                if s.publish_confirm {
                    b = b.child(col().w_full().padding(d.pad()).gap(d.gap()).tile(&d)
                        .child(text(format!("Create the private repository {acct}/{repo} on GitHub and push your first commit to it?")).font(style(&d, type_::LABEL)).color(t.text).wrap_anywhere().shrink())
                        .child(row().gap(d.gap())
                            .child(crate::questions::action(&d, "w-yes", "Yes, publish", "Publishing…", true, s.jobs.running("publish"), |s: &mut State| s.publish_home()))
                            .child(button(&d, "w-no", "Cancel".into(), false, |s: &mut State| s.publish_confirm = false))));
                } else {
                    b = b.child(crate::questions::action(&d, "w-publish", "Publish to GitHub…", "Publishing…", false, s.jobs.running("publish"), |s: &mut State| s.publish_confirm = true));
                }
            }
            if !s.gh_note.is_empty() {
                b = b.child(text(s.gh_note.clone()).font(style(&d, type_::LABEL)).color(t.text_secondary).wrap_anywhere().shrink());
            }
            ("Your home repository", b)
        }
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
                .child(row().child(crate::prefs::on_off(&d, "w-clip", s.settings.clipboard_on, |s, on| crate::prefs::set_clipboard(s, on)))))
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
        .child(text(format!("Step {} of {}", step + 1, STEPS)).font(d.text(Role::Label)).color(t.text_secondary))
        .child(el().grow(1.0));
    nav = match actions {
        Some(acts) => {
            // the flow's own buttons, with a way to leave it for later
            nav = nav.child(crate::questions::quiet(&d, "w-later", "Skip for now", |s: &mut State| advance(s)));
            acts.into_iter().fold(nav, |n, a| n.child(a))
        }
        None => nav
            .child(crate::questions::quiet(&d, "w-skip", if last { "Back" } else { "Skip" }, move |s: &mut State| if last { s.wstep -= 1 } else { finish(s) }))
            .child(crate::questions::primary(&d, "w-next", if last { "Open Hestia" } else { "Continue" }, move |s: &mut State| {
                s.msg.clear();
                if last { finish(s) } else { s.wstep += 1; if s.wstep == 2 && s.auth.is_none() { s.detect_github(); } }
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
            let mut s = State::default();
            s.wstep = step;
            let _ = screen(&s);
        }
    }
}
