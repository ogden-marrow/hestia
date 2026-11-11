//! The GitHub sign-in flow, as a pure state machine plus its screens.
//!
//! One calm sequence: sign in, look for your home repository, use the one found or create one,
//! ready. Every state has a design, including each way it can fail. The engine (`gh auth`, home
//! discovery, adopt, create) feeds this machine `Event`s; the machine never talks to a process,
//! so every state can be rendered and tested without a network. See issue #110.

use super::State;
use crate::design::{Design, Role, Surface};
use crate::questions::{primary, quiet};
use rune::{col, el, row, text, El};

/// The owner's requirement (#110): said plainly on the sign-in screen, not behind a link.
pub const NO_ACCOUNT: &str = "Hestia doesn't have accounts. Signing in to GitHub only lets Hestia keep a private copy of how your computer is set up, in a repository that only you can see. Nothing is sent to us, because there is no us.";
/// What is asked for, shown beside the approval code.
pub const PERMISSION: &str = "Hestia asks GitHub for permission to create and use a private repository. GitHub's approval page words this more broadly; Hestia only touches your home repository.";
/// The short form, repeated in Settings > Account.
pub const SHORT: &str = "No Hestia account and no online service. GitHub only stores your private home repository.";

const KEEPER: &str = "Your sign-in stays in this computer's password keeper, encrypted. You can sign out any time in Settings.";

/// The privacy statements each screen must show on the screen itself (checked by a test).
pub fn statements(a: &Auth) -> &'static [&'static str] {
    match a {
        Auth::SignedOut => &[NO_ACCOUNT, PERMISSION, KEEPER],
        Auth::Waiting { .. } => &[PERMISSION],
        Auth::NoKeyring => &[NO_ACCOUNT],
        _ => &[],
    }
}

/// A repository that could be the home repository.
#[derive(Clone, Debug, PartialEq)]
pub struct Repo {
    pub name: String, // owner/name
    pub updated: String, // "3 days ago"
}

#[derive(Clone, Debug, PartialEq)]
pub enum Fail {
    Offline,
    Denied,
    Expired,
    /// The organisation that owns the repository needs the person to authorise Hestia.
    Sso(String),
    /// A repository with this name already exists (so "create" would clash).
    Exists(String),
    /// No system keyring: Hestia refuses to keep a sign-in in plain text.
    NoKeyring,
    Other(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Auth {
    /// No system keyring to hold the sign-in encrypted. Hestia never stores it any other way (#110).
    NoKeyring,
    SignedOut,
    /// A code to type on github.com; the app notices the approval by itself.
    Waiting { code: String },
    Looking { user: String },
    Found { user: String, repos: Vec<Repo>, pick: usize },
    None { user: String },
    Creating { user: String },
    Cloning { user: String, repo: String },
    Ready { user: String, repo: String },
    Failed { why: Fail, user: Option<String> },
}

/// What happens to the flow: a button press or news from the engine.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    SignIn,
    Code(String),
    Approved(String),
    Found(Vec<Repo>),
    NoneFound,
    Pick(usize),
    UseIt,
    CreateIt,
    Created(String),
    Cloned(String),
    Cancel,
    Retry,
    Failed(Fail),
    /// Already signed in when the app looked (engine `status`).
    Known(String),
    SignedOut,
}

fn user_of(a: &Auth) -> Option<String> {
    match a {
        Auth::Looking { user } | Auth::Found { user, .. } | Auth::None { user } | Auth::Creating { user } | Auth::Cloning { user, .. } | Auth::Ready { user, .. } => Some(user.clone()),
        Auth::Failed { user, .. } => user.clone(),
        _ => None,
    }
}

/// The whole flow in one function. Anything not listed leaves the state as it is.
pub fn next(a: &Auth, e: Event) -> Auth {
    use Auth as A;
    let user = user_of(a);
    match (a, e) {
        (_, Event::Failed(Fail::NoKeyring)) => A::NoKeyring,
        (A::SignedOut, Event::Known(user)) => A::Looking { user },
        (_, Event::SignedOut) => A::SignedOut,
        (A::SignedOut, Event::SignIn) => A::Waiting { code: String::new() },
        (A::Waiting { .. }, Event::Code(code)) => A::Waiting { code },
        (A::Waiting { .. }, Event::Approved(user)) => A::Looking { user },
        (A::Looking { user }, Event::Found(repos)) if !repos.is_empty() => A::Found { user: user.clone(), repos, pick: 0 },
        (A::Looking { user }, Event::Found(_) | Event::NoneFound) => A::None { user: user.clone() },
        (A::Found { user, repos, .. }, Event::Pick(i)) => A::Found { user: user.clone(), repos: repos.clone(), pick: i.min(repos.len().saturating_sub(1)) },
        (A::Found { user, repos, pick }, Event::UseIt) => match repos.get(*pick) {
            Some(r) => A::Cloning { user: user.clone(), repo: r.name.clone() },
            None => a.clone(),
        },
        (A::Found { user, .. }, Event::CreateIt) | (A::None { user }, Event::CreateIt) => A::Creating { user: user.clone() },
        (A::Creating { user }, Event::Created(repo)) => A::Ready { user: user.clone(), repo },
        (A::Cloning { user, .. }, Event::Cloned(repo)) => A::Ready { user: user.clone(), repo },
        (A::Waiting { .. } | A::Looking { .. } | A::Creating { .. } | A::Cloning { .. }, Event::Cancel) => match user {
            Some(user) => A::None { user },
            None => A::SignedOut,
        },
        (A::Failed { why: Fail::Exists(name), user }, Event::UseIt) => A::Cloning { user: user.clone().unwrap_or_default(), repo: name.clone() },
        (A::Failed { user: Some(user), .. }, Event::Retry) => A::Looking { user: user.clone() },
        (A::Failed { .. } | A::NoKeyring, Event::Retry) => A::SignedOut,
        (_, Event::Failed(why)) => A::Failed { why, user },
        _ => a.clone(),
    }
}

/// Canned states for snapshots (`HESTIA_AUTH=<name>`) and tests, so every screen can be seen without a network.
pub fn canned(name: &str) -> Option<Auth> {
    let user = || "ada".to_string();
    let repo = |n: &str, u: &str| Repo { name: n.to_string(), updated: u.to_string() };
    Some(match name {
        "no-keyring" => Auth::NoKeyring,
        "signed-out" => Auth::SignedOut,
        "waiting" => Auth::Waiting { code: "WDJB-MJHT".into() },
        "looking" => Auth::Looking { user: user() },
        "found" => Auth::Found { user: user(), repos: vec![repo("ada/home", "3 days ago")], pick: 0 },
        "found-many" => Auth::Found { user: user(), repos: vec![repo("ada/home", "3 days ago"), repo("ada/hestia-home", "2 months ago"), repo("ada-lab/home", "last year")], pick: 0 },
        "none" => Auth::None { user: user() },
        "creating" => Auth::Creating { user: user() },
        "cloning" => Auth::Cloning { user: user(), repo: "ada/home".into() },
        "ready" => Auth::Ready { user: user(), repo: "ada/home".into() },
        "offline" => Auth::Failed { why: Fail::Offline, user: None },
        "denied" => Auth::Failed { why: Fail::Denied, user: None },
        "expired" => Auth::Failed { why: Fail::Expired, user: None },
        "sso" => Auth::Failed { why: Fail::Sso("ada-lab".into()), user: Some(user()) },
        "exists" => Auth::Failed { why: Fail::Exists("ada/home".into()), user: Some(user()) },
        "other" => Auth::Failed { why: Fail::Other("GitHub said no (HTTP 502). Try again in a minute.".into()), user: Some(user()) },
        _ => return None,
    })
}

#[cfg(test)]
pub const ALL: [&str; 15] = ["no-keyring", "signed-out", "waiting", "looking", "found", "found-many", "none", "creating", "cloning", "ready", "offline", "denied", "expired", "sso", "exists"];

/// An event arrives (a button, or a line from the engine): move the flow, then start whatever the
/// new state needs. The engine is only ever started from here, so there is one place to read.
pub fn apply(s: &mut State, e: Event) {
    let Some(before) = s.auth.clone() else { return };
    let after = next(&before, e.clone());
    if after == before {
        return;
    }
    s.auth = Some(after.clone());
    match (&before, &after, &e) {
        (_, _, Event::Cancel) => crate::engine::cancel(s),
        (_, Auth::Waiting { .. }, Event::SignIn) => crate::engine::run(s, &["auth", "login", "--json"]),
        (b, Auth::Looking { .. }, _) if !matches!(b, Auth::Looking { .. }) => crate::engine::run(s, &["home", "discover", "--json"]),
        (_, Auth::Cloning { repo, .. }, _) => crate::engine::run(s, &["home", "adopt", repo, "--json"]),
        (_, Auth::Creating { .. }, _) => crate::engine::run(s, &["home", "create", "--json"]),
        (_, Auth::Ready { .. }, _) => s.adopted(),
        _ => {}
    }
}

/// Start the flow when the person reaches it: look at whether they are already signed in.
pub fn begin(s: &mut State) {
    if s.auth.is_none() {
        s.auth = Some(Auth::SignedOut);
        crate::engine::run(s, &["auth", "status", "--json"]);
    }
}

/// One screen: (eyebrow if it is a problem, title, body, actions with the primary last).
pub struct View {
    pub problem: Option<&'static str>,
    pub title: String,
    pub body: El,
    pub actions: Vec<El>,
}

fn para(d: &Design, s: impl Into<String>) -> El {
    text(s.into()).font(d.text(Role::Body)).color(d.theme().text_secondary).wrap_anywhere().shrink()
}

fn card(d: &Design, lines: Vec<El>) -> El {
    let mut c = col().w_full().padding(d.pad()).gap(d.gap()).tile(d);
    for l in lines {
        c = c.child(l);
    }
    c
}

fn status(d: &Design, label: &str) -> El {
    // a quiet "working" line; the screen never freezes and always says what it is doing
    row().gap(d.gap()).center_y()
        .child(el().size(8.0, 8.0).radius(4.0).fill(d.accent_color()))
        .child(text(label.to_string()).font(d.text(Role::Label)).color(d.theme().text_secondary))
}

pub fn view(s: &State) -> Option<View> {
    let signed_out = Auth::SignedOut;
    let a = s.auth.as_ref().unwrap_or(&signed_out);
    let d = s.design;
    let t = d.theme();
    let cancel = || quiet(&d, "acct-cancel", "Cancel", |s: &mut State| apply(s, Event::Cancel));
    let retry = |label: &'static str| primary(&d, "acct-retry", label, |s: &mut State| apply(s, Event::Retry));
    let signed_in = |u: &str| text(format!("Signed in as {u}")).font(d.text(Role::Label)).color(t.text_secondary);
    Some(match a {
        Auth::NoKeyring => View { problem: Some("Sign-in is off"), title: "Can't keep a sign-in safe here".into(),
            body: col().w_full().gap(d.pad())
                .child(para(&d, "Hestia keeps your GitHub sign-in in this computer's password keeper, encrypted, and will not store it any other way. The keeper isn't available right now."))
                .child(para(&d, "Start or unlock it (on GNOME it is called Passwords and Keys), then check again."))
                .child(para(&d, statements(a)[0])),
            actions: vec![retry("Check again")] },
        Auth::SignedOut => View { problem: None, title: "Sign in to GitHub".into(),
            body: col().w_full().gap(d.pad())
                .child(para(&d, statements(a)[0]))
                .child(para(&d, statements(a)[1]))
                .child(para(&d, statements(a)[2])),
            actions: vec![primary(&d, "acct-signin", "Sign in with GitHub", |s: &mut State| apply(s, Event::SignIn))] },
        Auth::Waiting { code } => View { problem: None, title: "Approve in your browser".into(),
            body: col().w_full().gap(d.pad())
                .child(para(&d, "Open GitHub, type this code, and approve. Hestia notices by itself."))
                .child(para(&d, statements(a)[0]))
                .child(card(&d, vec![
                    text(if code.is_empty() { "Getting your code…".to_string() } else { code.clone() }).font(mono(d.text(Role::Display))).color(t.text),
                    row().gap(d.gap())
                        .child(crate::questions::button(&d, "acct-copy", "Copy code".into(), false, {
                            let c = code.clone();
                            move |s: &mut State| { s.copy_text = Some(c.clone()); s.msg = "code copied".into() }
                        }))
                        .child(crate::questions::button(&d, "acct-open", "Open GitHub".into(), false, |_: &mut State| open_url("https://github.com/login/device"))),
                ]))
                .child(status(&d, "Waiting for you to approve. The code works for 15 minutes."))
                ,
            actions: vec![cancel()] },
        Auth::Looking { user } => View { problem: None, title: "Looking for your repository".into(),
            body: col().w_full().gap(d.pad()).child(signed_in(user)).child(status(&d, "Checking your GitHub account…")),
            actions: vec![cancel()] },
        Auth::Found { user, repos, pick } => {
            let one = repos.len() == 1;
            let mut list = col().w_full().tile(&d);
            for (i, r) in repos.iter().enumerate() {
                list = list.child(
                    row().key(format!("acct-repo-{i}")).w_full().h(d.row_h() + d.gap()).px(d.item_px()).gap(d.pad()).center_y()
                        .fill(if i == *pick { d.select() } else { rune::Color::TRANSPARENT })
                        .on_press(move |s: &mut State| apply(s, Event::Pick(i)))
                        .child(text(r.name.clone()).font(d.text(Role::Body)).color(t.text).grow(1.0).shrink().min_w(0.0).truncate())
                        .child(text(format!("updated {}", r.updated)).font(d.text(Role::Label)).color(t.text_secondary)),
                );
            }
            let actions = vec![primary(&d, "acct-use", if one { "Use this repository" } else { "Use the selected one" }, |s: &mut State| apply(s, Event::UseIt))];
            View { problem: None, title: if one { "Found your repository".into() } else { "Which one is yours?".into() },
                body: col().w_full().gap(d.pad()).child(signed_in(user))
                    .child(para(&d, if one { "Hestia can use it as the home for your computer's setup and files. Everything in it comes down to this computer, and Hestia keeps it up to date." } else { "These look like Hestia repositories. Pick the one to use; the most recently updated is first." }))
                    .child(list)
                    .child(row().child(quiet(&d, "acct-new", "Make a new one instead", |s: &mut State| apply(s, Event::CreateIt)))),
                actions }
        }
        Auth::None { user } => View { problem: None, title: "Make your private repository".into(),
            body: col().w_full().gap(d.pad()).child(signed_in(user))
                .child(para(&d, "You don't have one yet. Hestia will make a private repository on your GitHub account. Only you can see it. It holds your computer's setup and your organised files, so a new computer can be set up from it."))
                ,
            actions: vec![primary(&d, "acct-create", "Create my private repository", |s: &mut State| apply(s, Event::CreateIt))] },
        Auth::Creating { user } => View { problem: None, title: "Creating your repository".into(),
            body: col().w_full().gap(d.pad()).child(signed_in(user)).child(status(&d, "Making a private repository and saving the first copy…")),
            actions: vec![cancel()] },
        Auth::Cloning { user, repo } => View { problem: None, title: "Bringing your files here".into(),
            body: col().w_full().gap(d.pad()).child(signed_in(user)).child(status(&d, &format!("Downloading {repo}…"))),
            actions: vec![cancel()] },
        Auth::Ready { user, repo } => View { problem: None, title: "You're all set".into(),
            body: col().w_full().gap(d.pad()).child(signed_in(user))
                .child(card(&d, vec![text(repo.clone()).font(d.text(Role::Title)).color(t.text), text("Private. Hestia keeps it up to date.".to_string()).font(d.text(Role::Label)).color(t.text_secondary)]))
                .child(para(&d, SHORT))
                .child(para(&d, "You can switch repositories or sign out any time in Settings, under Account.")),
            actions: vec![primary(&d, "acct-done", "Continue", |s: &mut State| crate::welcome::advance(s))] },
        Auth::Failed { why, user } => {
            let (problem, title, body, action): (&'static str, &str, String, El) = match why {
                Fail::Offline => ("No connection", "You're offline", "Hestia can't reach GitHub. Check your internet connection, then try again. Nothing was changed.".into(), retry("Try again")),
                Fail::Denied => ("Not approved", "Approval was declined", "GitHub says the approval was declined, so nothing was signed in. You can try again whenever you're ready.".into(), retry("Try again")),
                Fail::Expired => ("Code expired", "That code ran out", "Codes work for 15 minutes. Get a new one and approve it a little sooner.".into(), retry("Get a new code")),
                Fail::Sso(org) => ("One more step", "Your organisation needs to approve", format!("{org} asks people to authorise apps before they can use its repositories. Open GitHub, authorise Hestia for {org}, then try again."), retry("Try again")),
                Fail::Exists(name) => ("Already there", "A repository with that name exists", format!("{name} is already on your account. Hestia can use it instead of making a new one."), primary(&d, "acct-use-existing", "Use the existing one", |s: &mut State| apply(s, Event::UseIt))),
                Fail::NoKeyring => ("Sign-in is off", "Can't keep a sign-in safe here", "Hestia keeps your GitHub sign-in in this computer's password keeper and will not store it any other way. Start or unlock the keeper, then try again.".into(), retry("Try again")),
                Fail::Other(msg) => ("Something went wrong", "That didn't work", msg.clone(), retry("Try again")),
            };
            let mut actions = Vec::new();
            if matches!(why, Fail::Sso(_)) {
                actions.push(quiet(&d, "acct-open-org", "Open GitHub", |_: &mut State| open_url("https://github.com/settings/applications")));
            }
            actions.push(action);
            let mut b = col().w_full().gap(d.pad());
            if let Some(u) = user {
                b = b.child(signed_in(u));
            }
            View { problem: Some(problem), title: title.to_string(), body: b.child(para(&d, body)), actions }
        }
    })
}

fn mono(style: rune::TextStyle) -> rune::TextStyle {
    rune::TextStyle { family: rune::tok::Family::SystemMono, ..style }
}

/// Open an address in the person's browser. Only fixed github.com addresses are ever passed in.
fn open_url(url: &str) {
    if url.starts_with("https://github.com/") {
        let _ = std::process::Command::new("xdg-open").arg(url).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Auth as A;

    fn r(n: &str) -> Repo {
        Repo { name: n.into(), updated: "today".into() }
    }

    #[test]
    fn happy_path_with_a_repository_found() {
        let mut a = A::SignedOut;
        for (e, want) in [
            (Event::SignIn, "waiting"),
            (Event::Code("ABCD-1234".into()), "waiting"),
            (Event::Approved("ada".into()), "looking"),
            (Event::Found(vec![r("ada/home")]), "found"),
            (Event::UseIt, "cloning"),
            (Event::Cloned("ada/home".into()), "ready"),
        ] {
            a = next(&a, e);
            let got = match &a { A::Waiting { .. } => "waiting", A::Looking { .. } => "looking", A::Found { .. } => "found", A::Cloning { .. } => "cloning", A::Ready { .. } => "ready", _ => "other" };
            assert_eq!(got, want, "{a:?}");
        }
        assert_eq!(a, A::Ready { user: "ada".into(), repo: "ada/home".into() });
    }

    #[test]
    fn nothing_found_suggests_creating_one() {
        let a = next(&A::Looking { user: "ada".into() }, Event::NoneFound);
        assert_eq!(a, A::None { user: "ada".into() });
        let a = next(&a, Event::CreateIt);
        assert_eq!(a, A::Creating { user: "ada".into() });
        assert_eq!(next(&a, Event::Created("ada/home".into())), A::Ready { user: "ada".into(), repo: "ada/home".into() });
    }

    #[test]
    fn picking_is_clamped_and_use_takes_the_picked_one() {
        let a = A::Found { user: "ada".into(), repos: vec![r("a/one"), r("a/two")], pick: 0 };
        let a = next(&a, Event::Pick(99));
        assert!(matches!(&a, A::Found { pick: 1, .. }));
        assert_eq!(next(&a, Event::UseIt), A::Cloning { user: "ada".into(), repo: "a/two".into() });
    }

    #[test]
    fn cancel_never_strands_the_person() {
        assert_eq!(next(&A::Waiting { code: "x".into() }, Event::Cancel), A::SignedOut);
        assert_eq!(next(&A::Creating { user: "ada".into() }, Event::Cancel), A::None { user: "ada".into() });
    }

    #[test]
    fn every_failure_has_a_way_forward() {
        for f in [Fail::Offline, Fail::Denied, Fail::Expired, Fail::Sso("org".into()), Fail::Other("x".into())] {
            let failed = next(&A::SignedOut, Event::Failed(f));
            assert!(matches!(failed, A::Failed { .. }));
            assert!(!matches!(next(&failed, Event::Retry), A::Failed { .. }), "retry must leave the failure");
        }
        let exists = A::Failed { why: Fail::Exists("ada/home".into()), user: Some("ada".into()) };
        assert_eq!(next(&exists, Event::UseIt), A::Cloning { user: "ada".into(), repo: "ada/home".into() });
    }

    #[test]
    fn sign_in_screens_say_there_is_no_account_and_what_is_asked() {
        // the owner's acceptance criteria (#110): on the screen itself, beside the code
        assert!(statements(&canned("signed-out").unwrap()).contains(&NO_ACCOUNT));
        assert!(statements(&canned("signed-out").unwrap()).contains(&PERMISSION));
        assert!(statements(&canned("waiting").unwrap()).contains(&PERMISSION), "the permission sits beside the code");
        assert!(statements(&canned("no-keyring").unwrap()).contains(&NO_ACCOUNT));
        assert!(NO_ACCOUNT.contains("no us") && SHORT.contains("No Hestia account"));
        // calm words: none of these appear in what the person reads
        for line in [NO_ACCOUNT, PERMISSION, SHORT, KEEPER] {
            for bad in ["token", "OAuth", "scope", "git ", "repo ", "keyring"] {
                assert!(!line.contains(bad), "{bad:?} in {line:?}");
            }
        }
    }

    #[test]
    fn irrelevant_events_change_nothing() {
        let a = A::Ready { user: "ada".into(), repo: "ada/home".into() };
        assert_eq!(next(&a, Event::SignIn), a);
        assert_eq!(next(&A::SignedOut, Event::Cloned("x".into())), A::SignedOut);
    }

    #[test]
    fn every_canned_state_exists_and_renders() {
        for name in ALL.iter().chain(["other"].iter()) {
            let mut s = State::default();
            s.auth = canned(name);
            assert!(s.auth.is_some(), "{name}");
            assert!(view(&s).is_some(), "{name}");
        }
    }
}
