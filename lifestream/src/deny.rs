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
    (".docker", "container registry sign-ins"),
    (".kube", "cluster credentials"),
    (".aws", "cloud credentials"),
    (".azure", "cloud credentials"),
    (".config/gcloud", "cloud credentials"),
    (".config/rclone", "cloud storage sign-ins"),
    (".cargo/credentials", "package registry token"),
    (".terraform.d/credentials", "infrastructure tokens"),
    (".config/pip", "package index sign-ins"),
    (".config/openvpn", "VPN keys"),
    (".config/wireguard", "VPN keys"),
    (".openvpn", "VPN keys"),
    (".thunderbird", "mail profile"),
    (".config/google-chrome", "browser profile"),
    (".config/chromium", "browser profile"),
    (".config/bravesoftware", "browser profile"),
    (".config/microsoft-edge", "browser profile"),
    (".config/vivaldi", "browser profile"),
    (".config/opera", "browser profile"),
    (".config/slack", "chat sessions"),
    (".config/signal", "chat sessions and keys"),
    (".config/discord", "chat sessions"),
    (".config/bitwarden", "password manager"),
    (".config/keepassxc", "password manager"),
    (".var/app/org.mozilla.firefox", "browser profile"),
    (".var/app/com.google.chrome", "browser profile"),
    (".var/app/com.slack.slack", "chat sessions"),
    (".var/app/org.signal.signal", "chat sessions and keys"),
    (".var/app/com.bitwarden.desktop", "password manager"),
    (".var/app/org.keepassxc.keepassxc", "password manager"),
    ("snap/firefox", "browser profile"),
    ("snap/chromium", "browser profile"),
];

/// A path containing any of these words is refused wherever it is.
const SENSITIVE_WORDS: &[&str] = &["password", "secret", "token", "bookmarks", ".pem", ".key", "id_rsa", "id_ed25519"];

/// Should this path (relative to the home folder) never be captured? That includes a folder that holds a refused
/// path (`.config`, `.local/share`, `.docker`): adding a parent wholesale would carry its secrets along.
pub fn capture_refused(rel: &str) -> bool {
    let lower = rel.to_lowercase().split('/').filter(|c| !c.is_empty() && *c != ".").collect::<Vec<_>>().join("/");
    lower.is_empty()
        || lower.contains("..")
        || SENSITIVE_DIRS.iter().any(|(d, _)| lower.starts_with(d) || d.strip_prefix(lower.as_str()).is_some_and(|r| r.starts_with('/')))
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
    fn refuses_credential_files_profiles_and_histories() {
        for p in [".docker/config.json", ".kube/config", ".aws/credentials", ".config/gcloud/credentials.db", ".config/rclone/rclone.conf",
                  ".cargo/credentials.toml", ".cargo/credentials", ".terraform.d/credentials.tfrc.json", ".npmrc", ".pypirc", ".my.cnf",
                  ".bash_history", ".zsh_history", ".python_history", ".local/share/fish/fish_history", ".viminfo", ".lesshst",
                  ".config/google-chrome/Default/Cookies", ".config/BraveSoftware/Brave-Browser/Local State", ".config/Slack/Cookies",
                  ".config/Signal/config.json", ".config/Bitwarden/data.json", ".config/KeePassXC/keepassxc.ini", "Documents/vault.kdbx",
                  ".Xauthority", ".ICEauthority", ".config/openvpn/client.conf", ".config/wireguard/wg0.conf", "vpn/work.ovpn", ".ssh/id_dsa", "id_dsa",
                  ".var/app/org.mozilla.firefox/.mozilla", "snap/firefox/common/.mozilla", ".config//gcloud", ".config/./rclone/x"] {
            assert!(capture_refused(p), "{p} must be refused");
        }
    }

    #[test]
    fn refuses_a_parent_that_holds_a_refused_path_and_the_home_folder() {
        for p in [".config", ".config/", "./.config", ".local", ".local/share", ".var", ".var/app", ".cargo", ".docker", ".docker/", "", ".", "./"] {
            assert!(capture_refused(p), "{p:?} would carry secrets along: refuse it wholesale");
        }
    }

    #[test]
    fn allows_ordinary_config() {
        for p in [".bashrc", ".profile", ".gitconfig", ".config/htop/htoprc", ".config/mimeapps.list", ".config/gtk-4.0/settings.ini", ".config/nautilus/x", ".config/gtk-3.0", ".config/hestia", ".config/systemd/user", ".local/share/applications/x.desktop", ".id_dsa.pub", "id_dsa.pub",
                  ".cargo/config.toml", ".config/mpv", "projects/history-of-rome.txt"] {
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
