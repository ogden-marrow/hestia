//! Kinds as people say them. The engine's names (`unknown/image`, `document/finance`) are keys, not
//! words: they are shown through `label` and never typed back by the app on the person's behalf.

const KNOWN: &[(&str, &str)] = &[
    ("image/thumbnail", "Thumbnail"), ("image", "Image"), ("image/raw", "Camera original"), ("image/screenshot", "Screenshot"),
    ("video/camera", "Camera video"), ("video/screen-recording", "Screen recording"), ("audio", "Audio"), ("audio/music", "Music"),
    ("document/text", "Text document"), ("document/pdf", "PDF"), ("document/finance", "Finance document"), ("document/manual", "Manual"),
    ("picture/photo", "Photo"), ("code", "Code"), ("data/config", "Settings file"), ("data/database", "Database"), ("log", "Log"),
    ("archive", "Archive"), ("software/installer", "Installer"), ("software/library", "Library"), ("software/app-bundle", "App"),
    ("game/save-backup", "Game save"), ("game/disc-dump", "Game disc image"), ("project/git", "Git project"), ("pdf", "PDF"),
];

fn words(s: &str) -> String {
    let w = s.replace(['-', '_'], " ");
    let mut c = w.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

/// "unknown/image" -> "Image", "document/finance" -> "Finance document", "game/save-backup" -> "Game save".
pub fn label(kind: &str) -> String {
    let k = kind.strip_prefix("unknown/").unwrap_or(kind);
    if let Some((_, l)) = KNOWN.iter().find(|(raw, _)| *raw == k) {
        return (*l).to_string();
    }
    match k.split_once('/') {
        Some(("document", rest)) => format!("{} document", words(rest)),
        Some((_, rest)) => words(rest),
        None => words(k),
    }
}

#[cfg(test)]
mod tests {
    use super::label;

    #[test]
    fn engine_names_become_words() {
        assert_eq!(label("unknown/image"), "Image");
        assert_eq!(label("unknown/pdf"), "PDF");
        assert_eq!(label("document/finance"), "Finance document");
        assert_eq!(label("document/tax-form"), "Tax form document");
        assert_eq!(label("video/screen-recording"), "Screen recording");
        assert_eq!(label("misc/old-stuff"), "Old stuff");
        assert_eq!(label("weird"), "Weird");
    }

    #[test]
    fn every_kind_the_engine_offers_has_a_plain_label() {
        for k in lifestream::bundle::KINDS {
            assert!(!label(k).contains('/') && !label(k).is_empty(), "{k}");
        }
    }
}
