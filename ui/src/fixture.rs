//! Synthetic data for snapshots. `HESTIA_FIXTURE=1 hestia --snap out.png --scene today` renders
//! a believable, generic home (no real names, paths or counts), so snapshot boards can live in the
//! repository and look the same on every machine. Panes that read the live machine (Files, History,
//! Machine, System, Ask) show their empty state in fixture mode, which is itself worth a snapshot.

use super::State;
use lifestream::{By, Record};

fn rec(i: usize, kind: &str, path: &str, hours_ago: i64, status: &str, size: u64) -> Record {
    let seen = chrono::Utc::now() - chrono::Duration::hours(hours_ago);
    Record {
        id: format!("fixture-{i:04}"),
        first_seen: seen.to_rfc3339(),
        mtime: seen.to_rfc3339(),
        kind: kind.to_string(),
        size,
        sources: vec![path.to_string()],
        was: vec![],
        tags: vec![],
        summary: None,
        by: By { tier: "rule".into(), name: "fixture".into(), confidence: 0.9 },
        status: status.to_string(),
        store: None,
        future_at: None,
    }
}

/// The fixture records: filed, waiting and gone, spread over the last few days.
pub fn records(pending_folders: bool) -> Vec<Record> {
    let mut v = Vec::new();
    let mut n = 0;
    let mut add = |kind: &str, path: String, hours: i64, status: &str, size: u64| {
        v.push(rec(n, kind, &path, hours, status, size));
        n += 1;
    };
    for i in 0..48 {
        let (kind, dir, ext) = match i % 4 {
            0 => ("document/finance", "Documents/Taxes", "pdf"),
            1 => ("picture/photo", "Pictures/Camera", "jpg"),
            2 => ("document/manual", "Documents/Manuals", "pdf"),
            _ => ("audio/music", "Music/Albums", "mp3"),
        };
        add(kind, format!("/home/you/{dir}/item-{i}.{ext}"), (i as i64 * 5) % 70, "filed", 120_000 + i as u64 * 3_000);
    }
    for i in 0..5 {
        add("document/finance", format!("/home/you/Documents/Old/gone-{i}.pdf"), 30 + i as i64, "gone", 80_000);
    }
    if pending_folders {
        for i in 0..6 {
            add("unknown/pdf", format!("/home/you/Downloads/Scans/scan-{i}.pdf"), 3, "pending", 410_000);
        }
        for i in 0..4 {
            add("unknown/image", format!("/home/you/Pictures/Trip/IMG_{i:03}.jpg"), 6, "pending", 2_400_000);
        }
        for i in 0..3 {
            add("unknown/doc", format!("/home/you/Desktop/Notes/draft-{i}.txt"), 9, "pending", 12_000);
        }
    }
    v
}

/// Synthetic applications for snapshots: no icons from this machine, no real names.
pub fn apps() -> Vec<crate::apps::Item> {
    use crate::apps::{Item, Open, Source};
    let mk = |name: &str, about: &str, group: &'static str, game: bool, source: Source| Item {
        id: name.to_lowercase(), name: name.into(), about: about.into(), group, game, source, open: Open::Steam(1), icon: None, keywords: String::new(), last_played: None,
    };
    vec![
        mk("Calculator", "Do arithmetic", "Utilities", false, Source::Native), mk("Files", "Browse your files", "Utilities", false, Source::Native),
        mk("Firefox", "Browse the web", "Internet", false, Source::Snap), mk("Image Editor", "Edit pictures", "Graphics", false, Source::Flatpak),
        mk("Music Player", "Play your music", "Media", false, Source::Flatpak), mk("Text Editor", "Edit text files", "Office", false, Source::Native),
        mk("Terminal", "Use the command line", "Development", false, Source::Native), mk("Lantern Keep", "Steam game", "Games", true, Source::Steam),
        mk("Orbit Garden", "Steam game", "Games", true, Source::Steam), mk("Chess", "Play chess", "Games", true, Source::Native),
    ]
}

/// Replace everything the live machine would supply with the fixture.
pub fn apply(s: &mut State, pending_folders: bool) {
    s.settings = crate::settings::Settings::default();
    s.settings.welcomed = true;
    s.recs = records(pending_folders);
    s.suggestions.clear();
    s.clips.clear();
    s.apps.clear();
    s.skipped.clear();
    s.repo_dir = std::path::PathBuf::from("/nonexistent/fixture-home"); // never show this machine's repository
    s.account_user = None;
    s.refresh_derived();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_has_no_personal_paths_and_covers_every_status() {
        let r = records(true);
        assert!(r.iter().all(|x| x.sources.iter().all(|p| p.starts_with("/home/you/"))));
        for st in ["filed", "pending", "gone"] {
            assert!(r.iter().any(|x| x.status == st), "{st}");
        }
        assert!(records(false).iter().all(|x| x.status != "pending"));
    }
}
