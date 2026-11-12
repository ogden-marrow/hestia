//! The ONE list of things that must never be captured or backed up. Everything that decides (the capture script,
//! the Machine screen's "add a path", the starter manifest of a new home repository) reads this, so there is
//! one place to add to and nothing to keep in step by hand.
//!
//! Paths here are relative to the home folder. A person's own `manifest/rules.toml` can add more; it cannot remove these.

/// Folders and files that hold keys, sessions or accounts. A path equal to one of these, or under one, is refused.
pub const SENSITIVE_DIRS: &[(&str, &str)] = &[
    (".ssh", "SSH keys and known hosts"),
    (".gnupg", "GPG keys"),
    (".config/gh", "GitHub CLI sign-in"),
    (".config/claude", "AI assistant sessions"),
    (".config/helium", "browser profile"),
    (".config/goa-1.0", "online accounts"),
    (".config/evolution", "mail and calendar"),
    (".local/share/keyrings", "the system keyring"),
    (".mozilla", "browser profile"),
    (".password-store", "password store"),
];

/// A path containing any of these words is refused wherever it is.
const SENSITIVE_WORDS: &[&str] = &["password", "secret", "token", "bookmarks", ".pem", ".key", "id_rsa", "id_ed25519"];

/// Should this path (relative to the home folder) never be captured?
pub fn capture_refused(rel: &str) -> bool {
    let lower = rel.trim_start_matches("./").to_lowercase();
    lower.contains("..")
        || SENSITIVE_DIRS.iter().any(|(d, _)| lower.starts_with(d))
        || SENSITIVE_WORDS.iter().any(|w| lower.contains(w))
        || crate::looks_secret(rel.rsplit('/').next().unwrap_or(rel))
}

/// The starter `manifest/rules.toml` rules for a new home repository, generated from the list above.
pub fn manifest_rules() -> String {
    SENSITIVE_DIRS
        .iter()
        .map(|(d, why)| format!("[[rule]]\nsensitive = true\npath = \"{d}\"\nnote = \"{why}: never read, moved or sent anywhere.\"\n"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_keys_sessions_accounts_and_secret_looking_names() {
        for p in [".ssh", ".ssh/id_ed25519", ".gnupg/pubring.kbx", ".config/gh/hosts.yml", ".config/Claude/x", ".local/share/keyrings/login.keyring",
                  "notes/my-password-list.txt", ".config/gtk-3.0/bookmarks", "a/../b", ".config/app/api_token.json", "certs/site.pem", ".env", "x/id_rsa", ".sshconfig-backup"] {
            assert!(capture_refused(p), "{p} must be refused");
        }
    }

    #[test]
    fn allows_ordinary_config() {
        for p in [".bashrc", ".profile", ".gitconfig", ".config/htop/htoprc", ".config/mimeapps.list", ".config/gtk-4.0/settings.ini", ".config/nautilus/x"] {
            assert!(!capture_refused(p), "{p} is ordinary config");
        }
    }

    #[test]
    fn the_starter_manifest_is_generated_from_the_list_and_parses() {
        let m = manifest_rules();
        assert_eq!(m.matches("[[rule]]").count(), SENSITIVE_DIRS.len());
        let v: toml::Value = toml::from_str(&m).expect("the generated rules are valid toml");
        assert!(v["rule"].as_array().unwrap().iter().all(|r| r["sensitive"].as_bool() == Some(true)));
        assert!(m.contains("path = \".ssh\""));
    }
}
