//! Tier-1 classifier: deterministic, data-driven rules from `rules.toml`.

use anyhow::{bail, Context, Result};
use regex::Regex;
use serde::Deserialize;
use std::collections::BTreeSet;

#[derive(Debug, Deserialize)]
struct RuleFile {
    #[serde(default)]
    rule: Vec<Rule>,
}

#[derive(Debug, Deserialize)]
struct Rule {
    name: String,
    #[serde(default)]
    ext: Vec<String>,
    name_regex: Option<String>,
    path_contains: Option<String>,
    kind: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
}

struct Compiled {
    rule: Rule,
    re: Option<Regex>,
}

pub struct RuleSet {
    rules: Vec<Compiled>,
}

#[derive(Debug, Default, PartialEq)]
pub struct Outcome {
    /// kind assigned by the first matching rule that sets one
    pub kind: Option<String>,
    /// name of the rule that set the kind
    pub kind_rule: Option<String>,
    pub tags: BTreeSet<String>,
}

impl RuleSet {
    pub fn parse(text: &str) -> Result<Self> {
        let file: RuleFile = toml::from_str(text).context("parsing rules")?;
        let mut rules = Vec::new();
        for rule in file.rule {
            if rule.ext.is_empty() && rule.name_regex.is_none() && rule.path_contains.is_none() {
                bail!("rule '{}' has no criteria (ext, name_regex or path_contains)", rule.name);
            }
            let re = match &rule.name_regex {
                Some(p) => Some(
                    Regex::new(p).with_context(|| format!("rule '{}': bad name_regex", rule.name))?,
                ),
                None => None,
            };
            rules.push(Compiled { rule, re });
        }
        Ok(Self { rules })
    }

    /// `path` is the `~`-relative path, `name` the file name, `ext` lowercase without dot.
    pub fn classify(&self, path: &str, name: &str, ext: &str) -> Outcome {
        let mut out = Outcome::default();
        for c in &self.rules {
            let r = &c.rule;
            if !r.ext.is_empty() && !r.ext.iter().any(|e| e.eq_ignore_ascii_case(ext)) {
                continue;
            }
            if let Some(re) = &c.re {
                if !re.is_match(name) {
                    continue;
                }
            }
            if let Some(s) = &r.path_contains {
                if !path.contains(s.as_str()) {
                    continue;
                }
            }
            if out.kind.is_none() {
                if let Some(k) = &r.kind {
                    out.kind = Some(k.clone());
                    out.kind_rule = Some(r.name.clone());
                }
            }
            out.tags.extend(r.tags.iter().cloned());
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defaults() -> RuleSet {
        RuleSet::parse(include_str!("../rules.toml")).unwrap()
    }

    #[test]
    fn default_rules_parse() {
        assert!(defaults().rules.len() > 20);
    }

    #[test]
    fn screen_recording_beats_generic_video() {
        let o = defaults().classify("~/Videos/2025-09-15 18-20-44.mp4", "2025-09-15 18-20-44.mp4", "mp4");
        assert_eq!(o.kind.as_deref(), Some("video/screen-recording"));
        assert!(o.tags.contains("video") && o.tags.contains("screen-recording"));
    }

    #[test]
    fn camera_clip_detected() {
        let o = defaults().classify("~/Videos/MVI_1487.MOV", "MVI_1487.MOV", "mov");
        assert_eq!(o.kind.as_deref(), Some("video/camera"));
    }

    #[test]
    fn save_backup_by_path_wins_over_ext() {
        let o = defaults().classify("~/Documents/Game Save Backups/a.sl2", "a.sl2", "sl2");
        assert_eq!(o.kind.as_deref(), Some("game/save-backup"));
        assert!(o.tags.contains("save"));
    }

    #[test]
    fn extension_is_case_insensitive() {
        let o = defaults().classify("~/Pictures/x.DNG", "x.DNG", "dng");
        assert_eq!(o.kind.as_deref(), Some("image/raw"));
    }

    #[test]
    fn unknown_gets_no_kind() {
        let o = defaults().classify("~/x/blob.zzz", "blob.zzz", "zzz");
        assert!(o.kind.is_none() && o.tags.is_empty());
    }

    #[test]
    fn rule_without_criteria_is_rejected() {
        assert!(RuleSet::parse("[[rule]]\nname='bad'\nkind='x'\n").is_err());
    }
}
