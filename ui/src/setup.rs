//! First-run setup that touches the desktop: switching on the Hestia shell extension, which draws the
//! launcher (Alt+Space) and records the clipboard. The welcome tour and Settings → Launcher both call this.

use std::process::Command;

pub const SHORTCUT_NAME: &str = "Alt+Space";
const UUID: &str = "hestia-clipboard@hestia.app";

fn gs(args: &[&str]) -> Option<String> {
    let o = Command::new("gsettings").args(args).output().ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// `['a', 'b']`, `[]` or `@as []` as a list of strings.
pub fn parse_list(s: &str) -> Vec<String> {
    s.split('\'').skip(1).step_by(2).map(String::from).collect()
}

pub fn render_list(v: &[String]) -> String {
    format!("[{}]", v.iter().map(|x| format!("'{x}'")).collect::<Vec<_>>().join(", "))
}

fn with(mut v: Vec<String>, item: &str) -> Vec<String> {
    if !v.iter().any(|x| x == item) {
        v.push(item.to_string());
    }
    v
}

/// Turn the extension on. A newly installed one needs one log out and in first.
pub fn enable_extension() -> String {
    let uuid = UUID;
    let list = gs(&["get", "org.gnome.shell", "enabled-extensions"]).map(|t| parse_list(&t)).unwrap_or_default();
    let ok = gs(&["set", "org.gnome.shell", "enabled-extensions", &render_list(&with(list, uuid))]).is_some();
    let _ = Command::new("gnome-extensions").args(["enable", uuid]).status();
    match (ok, crate::clipboard::extension_state()) {
        (false, _) => "could not turn it on: this desktop is not GNOME, or gsettings is missing".into(),
        (true, "recording") => "the launcher and clipboard history are on".into(),
        (true, _) => "turned on. Log out and back in once and it starts recording".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_round_trip_through_gsettings_text() {
        assert_eq!(parse_list("@as []"), Vec::<String>::new());
        assert_eq!(parse_list("['<Alt>space', '<Super>x']"), vec!["<Alt>space", "<Super>x"]);
        assert_eq!(render_list(&[]), "[]");
        assert_eq!(render_list(&["a".into(), "b".into()]), "['a', 'b']");
    }

    #[test]
    fn adding_twice_adds_once() {
        assert_eq!(with(with(vec!["x".into()], "p"), "p"), vec!["x", "p"]);
    }
}
