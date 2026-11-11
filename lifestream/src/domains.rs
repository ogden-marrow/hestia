//! Domains: the big things you do on a computer (documents, pictures, videos, music, applications,
//! games, software engineering, data, machines). Each one is plain data that says what belongs in it,
//! how it is shaped and how it is backed up. Hestia ships nine; you can edit, add or hide any of them
//! by putting a `domains/<id>.toml` in your home repository. The classifier is told about every domain,
//! so what you write here is what the local models use to decide where things go.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Backup {
    /// files up to this many MB are tracked in git; bigger ones become pointers into the encrypted store
    #[serde(default)]
    pub git_max_mb: u32,
    /// "encrypted" (client-side encrypted store you hold the keys for) or "none"
    #[serde(default)]
    pub store: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Domain {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub icon: String,
    /// where it lives, relative to $HOME ("" when it is not a folder of files)
    #[serde(default)]
    pub folder: String,
    /// told to the classifier, in plain words
    pub description: String,
    /// the file kinds this domain owns (leaf kinds from the taxonomy)
    #[serde(default)]
    pub kinds: Vec<String>,
    /// how the folder is laid out: by_topic, by_year, by_source, by_project, by_machine, flat
    #[serde(default)]
    pub shape: String,
    #[serde(default)]
    pub backup: Backup,
    /// set in a user file to hide a stock domain
    #[serde(default, skip_serializing_if = "is_false")]
    pub disabled: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

const DEFAULTS: &[&str] = &[
    include_str!("../defaults/domains/documents.toml"),
    include_str!("../defaults/domains/pictures.toml"),
    include_str!("../defaults/domains/videos.toml"),
    include_str!("../defaults/domains/music.toml"),
    include_str!("../defaults/domains/applications.toml"),
    include_str!("../defaults/domains/games.toml"),
    include_str!("../defaults/domains/software-engineering.toml"),
    include_str!("../defaults/domains/data.toml"),
    include_str!("../defaults/domains/machines.toml"),
];

fn slug_ok(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// The nine domains Hestia ships.
pub fn defaults() -> Vec<Domain> {
    DEFAULTS.iter().map(|t| toml::from_str(t).expect("a stock domain file is valid")).collect()
}

/// Stock domains plus the ones in `dir` (a user's `domains/` folder): same id replaces, a new id adds,
/// `disabled = true` hides. A kind claimed by two domains is an error: a file must have one home.
pub fn load(dir: Option<&Path>) -> Result<Vec<Domain>> {
    let mut all = defaults();
    if let Some(dir) = dir.filter(|d| d.is_dir()) {
        let mut files: Vec<_> = std::fs::read_dir(dir)?.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "toml")).collect();
        files.sort();
        for f in files {
            let d: Domain = toml::from_str(&std::fs::read_to_string(&f)?).with_context(|| format!("{}", f.display()))?;
            if !slug_ok(&d.id) {
                bail!("{}: id '{}' must be lowercase letters, digits and dashes", f.display(), d.id);
            }
            match all.iter().position(|x| x.id == d.id) {
                Some(i) => all[i] = d,
                None => all.push(d),
            }
        }
    }
    all.retain(|d| !d.disabled);
    let mut seen: Vec<(&str, &str)> = Vec::new();
    for d in &all {
        for k in &d.kinds {
            if let Some((_, other)) = seen.iter().find(|(kk, _)| kk == k) {
                bail!("kind '{k}' is claimed by both '{other}' and '{}'", d.id);
            }
            seen.push((k, &d.id));
        }
    }
    Ok(all)
}

/// Which domain owns this kind (exact match first, then the longest kind that prefixes it).
pub fn owner<'a>(all: &'a [Domain], kind: &str) -> Option<&'a Domain> {
    all.iter()
        .flat_map(|d| d.kinds.iter().map(move |k| (k, d)))
        .filter(|(k, _)| kind == k.as_str() || kind.strip_prefix(k.as_str()).is_some_and(|r| r.starts_with('/')))
        .max_by_key(|(k, _)| k.len())
        .map(|(_, d)| d)
}

/// What the classifier is told about the domains, one line each.
pub fn guide(all: &[Domain]) -> String {
    all.iter()
        .filter(|d| !d.kinds.is_empty())
        .map(|d| format!("{} ({}): {} Kinds: {}.", d.name, d.id, d.description.trim(), d.kinds.join(", ")))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stock_domains_parse_and_every_builtin_kind_has_exactly_one_home() {
        let all = load(None).unwrap();
        assert_eq!(all.len(), 9);
        for k in crate::bundle::KINDS {
            let n = all.iter().filter(|d| d.kinds.iter().any(|x| x == k)).count();
            assert_eq!(n, 1, "kind {k} must belong to exactly one domain, found {n}");
        }
    }

    #[test]
    fn owner_prefers_the_most_specific_kind() {
        let all = load(None).unwrap();
        assert_eq!(owner(&all, "image/raw").unwrap().id, "pictures");
        assert_eq!(owner(&all, "document/pdf").unwrap().id, "documents");
        assert!(owner(&all, "mystery/thing").is_none());
    }

    #[test]
    fn user_files_replace_add_and_hide() {
        let dir = std::env::temp_dir().join(format!("hestia-domains-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("music.toml"), "id=\"music\"\nname=\"Tunes\"\ndescription=\"mine\"\nkinds=[\"audio\"]\n").unwrap();
        std::fs::write(dir.join("recipes.toml"), "id=\"recipes\"\nname=\"Recipes\"\ndescription=\"food\"\nkinds=[\"document/recipe\"]\n").unwrap();
        std::fs::write(dir.join("machines.toml"), "id=\"machines\"\nname=\"Machines\"\ndescription=\"x\"\ndisabled=true\n").unwrap();
        let all = load(Some(&dir)).unwrap();
        assert_eq!(all.iter().find(|d| d.id == "music").unwrap().name, "Tunes");
        assert!(all.iter().any(|d| d.id == "recipes"));
        assert!(!all.iter().any(|d| d.id == "machines"));
        std::fs::write(dir.join("clash.toml"), "id=\"clash\"\nname=\"C\"\ndescription=\"x\"\nkinds=[\"audio\"]\n").unwrap();
        assert!(load(Some(&dir)).unwrap_err().to_string().contains("claimed by both"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_guide_names_every_domain_that_holds_files() {
        let g = guide(&load(None).unwrap());
        assert!(g.contains("Pictures (pictures)") && g.contains("image/raw"));
        assert!(!g.contains("Machines"));
    }
}
