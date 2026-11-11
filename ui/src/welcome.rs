//! The first-run tour: what Hestia does in four short screens, and the two desktop steps
//! (the launcher shortcut and clipboard history) done for you. Shown once; Settings → About reopens it.

use super::{style, Pane, State};
use crate::design::Surface;
use crate::questions::button;
use rune::tok::type_;
use rune::{col, el, row, text, El};

const STEPS: usize = 5;

fn line(s: &State, head: &str, body: &str) -> El {
    let d = s.design;
    let t = d.theme();
    col().w_full().gap(2.0)
        .child(text(head.to_string()).font(style(&d, type_::BODY)).color(t.text))
        .child(text(body.to_string()).font(style(&d, type_::LABEL)).color(t.text_secondary).wrap_anywhere().shrink())
}

pub fn finish(s: &mut State) {
    s.change_settings(|st| st.welcomed = true);
    s.msg.clear();
    s.pane = Pane::Today;
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

    let (title, mut body) = match step {
        0 => ("Welcome to Hestia", col().w_full().gap(d.pad())
            .child(line(s, "It files your computer for you.", "Every night it reads your folders and works out what each file is and where it belongs."))
            .child(line(s, "It only asks when it is unsure.", "Each morning it tells you what it filed and asks a few questions about the rest."))
            .child(line(s, "Everything stays here.", "Nothing leaves this computer, and Hestia never deletes a file."))),
        1 => ("Where things are", col().w_full().gap(d.pad())
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
                .child(row_kv("Pushes", "only when you press Publish. Nothing is pushed by itself.".to_string()));
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
                            .child(button(&d, "w-yes", "Yes, publish".into(), true, |s: &mut State| s.publish_home()))
                            .child(button(&d, "w-no", "Cancel".into(), false, |s: &mut State| s.publish_confirm = false))));
                } else {
                    b = b.child(button(&d, "w-publish", "Publish to GitHub…".into(), false, |s: &mut State| s.publish_confirm = true));
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
            ("Hestia Search and clipboard", col().w_full().gap(d.pad())
                .child(line(s, &format!("Press {} anywhere and type.", crate::setup::SHORTCUT_NAME), "Find apps, files, things you copied and Hestia itself. Type clipboard to browse what you copied: text only, kept on this computer, never passwords."))
                .child(line(s, "A small part of Hestia lives in the desktop.", "GNOME only lets the shell draw the launcher and watch the clipboard, so Hestia adds one extension. Turn it on here; you log out and in once."))
                .child(row().gap(d.pad()).center_y()
                    .child(button(&d, "w-ext", "Turn on".into(), !on, |s: &mut State| s.msg = crate::setup::enable_extension()))
                    .child(text(if on { "on".to_string() } else { format!("not yet: {state}") }).font(style(&d, type_::LABEL)).color(if on { acc } else { t.text_secondary }))))
        }
        _ => ("You are set", col().w_full().gap(d.pad())
            .child(line(s, "That is all.", "Your first scan runs tonight. Until then Hestia shows what it already knows."))
            .child(line(s, "Everything you chose is in Settings.", "You can run this tour again from Settings, About."))),
    };
    if !s.msg.is_empty() && (step == 2 || step == 3) {
        body = body.child(text(s.msg.clone()).font(style(&d, type_::LABEL)).color(acc).wrap_anywhere().shrink());
    }

    let mut dots = row().gap(6.0).center_y();
    for i in 0..STEPS {
        dots = dots.child(el().size(8.0, 8.0).radius(4.0).fill(if i == step { acc } else { t.card_border }));
    }
    let last = step == STEPS - 1;
    let nav = row().w_full().gap(d.gap()).center_y()
        .child(dots)
        .child(el().grow(1.0))
        .child(button(&d, "w-skip", if last { "Back".into() } else { "Skip".into() }, false, move |s: &mut State| if last { s.wstep -= 1 } else { finish(s) }))
        .child(button(&d, "w-next", if last { "Open Hestia".into() } else { "Continue".into() }, true, move |s: &mut State| {
            s.msg.clear();
            if last { finish(s) } else { s.wstep += 1; if s.wstep == 2 { s.detect_github(); } }
        }));

    let card = col().w(540.0).padding(d.pad() * 2.0).gap(d.pad() * 1.5).panel(&d)
        .child(text(title.to_string()).font(style(&d, type_::TITLE)).color(t.text))
        .child(body)
        .child(nav);
    col().w_full().h_full().backdrop(&d)
        .child(el().grow(1.0))
        .child(row().w_full().child(el().grow(1.0)).child(card).child(el().grow(1.0)))
        .child(el().grow(1.0))
}
