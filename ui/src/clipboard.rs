//! Clipboard history. GNOME on Wayland does not let a background program watch the clipboard, so a
//! tiny Shell extension (packaging/gnome-extension) records it into a private file; this
//! reads that file. Opt-in and off by default. Text only, newest first, capped; the extension skips
//! password managers and text that looks like a key or a code, which is a filter, not a guarantee.

/// What the person is told before they turn it on (Welcome and Settings say the same thing).
pub const PRIVACY: &str = "Off until you turn it on. Text only, kept only on this computer, and deleted when you turn it off. It skips password managers and anything that looks like a key or a code, but that is a filter, not a guarantee.";

use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub at: u64,
    pub text: String,
}

pub fn file() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/share/hestia/clipboard.jsonl")
}

pub fn parse(text: &str) -> Vec<Entry> {
    let mut v: Vec<Entry> = text.lines().filter_map(|l| {
        let j: serde_json::Value = serde_json::from_str(l).ok()?;
        Some(Entry { at: j["t"].as_u64()?, text: j["text"].as_str()?.to_string() })
    }).collect();
    v.reverse(); // the file is oldest first
    v
}

pub fn load() -> Vec<Entry> {
    std::fs::read_to_string(file()).map(|t| parse(&t)).unwrap_or_default()
}

pub fn clear() {
    let _ = std::fs::remove_file(file());
}

/// One line for a list: the first non-empty line, squashed.
pub fn preview(e: &Entry) -> String {
    let first = e.text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    let mut s: String = first.chars().take(90).collect();
    if first.chars().count() > 90 || e.text.lines().count() > 1 {
        s.push('…');
    }
    s
}

/// Is the GNOME Shell extension that records the history installed, and is it switched on?
pub fn extension_state() -> &'static str {
    let installed = PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/share/gnome-shell/extensions/hestia-clipboard@hestia.app").exists();
    if !installed {
        return "not installed";
    }
    let out = std::process::Command::new("gnome-extensions").args(["info", "hestia-clipboard@hestia.app"]).output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    if out.contains("State: ACTIVE") || out.contains("State: ENABLED") { "recording" } else { "installed, not enabled yet (enable it, then log out and in)" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newest_first_and_previews_are_short() {
        let v = parse("{\"t\":1,\"text\":\"old\"}\n{\"t\":2,\"text\":\"line one\\nline two\"}\nnot json\n");
        assert_eq!((v[0].at, v[1].at), (2, 1));
        assert_eq!(preview(&v[0]), "line one…");
        assert_eq!(preview(&v[1]), "old");
    }
}
