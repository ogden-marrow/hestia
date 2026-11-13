//! Applications: everything installed that can be opened, as one list. Desktop entries (native, Flatpak,
//! Snap, the person's own and Steam/Proton shortcuts) plus installed Steam games read from Steam's own
//! manifests. Read-only; opening goes through `gio`, which is how `launcher` already opens things.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Native,
    Flatpak,
    Snap,
    Steam,
}

impl Source {
    pub fn label(self) -> &'static str {
        match self {
            Source::Native => "Installed",
            Source::Flatpak => "Flatpak",
            Source::Snap => "Snap",
            Source::Steam => "Steam",
        }
    }
}

/// How an item is opened.
#[derive(Clone, Debug, PartialEq)]
pub enum Open {
    /// A `.desktop` entry (`gio launch`).
    Entry(PathBuf),
    /// A Steam game by id (`gio open steam://rungameid/ID`).
    Steam(u64),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub id: String,
    pub name: String,
    pub about: String,
    pub group: &'static str,
    pub game: bool,
    pub source: Source,
    pub open: Open,
    /// Where the icon comes from: the `.desktop` file itself, or a Steam cache image.
    pub icon: Option<PathBuf>,
    pub keywords: String,
    /// Seconds since 1970 of the last time it was played (Steam), when known.
    pub last_played: Option<u64>,
}

/// The one-word shelf for a `Categories=` value, by the first main category that is recognised.
pub fn group_of(categories: &str) -> &'static str {
    for c in categories.split(';').map(str::trim) {
        let g = match c {
            "Game" => "Games",
            "AudioVideo" | "Audio" | "Video" | "Player" | "Music" => "Media",
            "Graphics" | "Photography" => "Graphics",
            "Network" | "WebBrowser" | "Email" | "Chat" | "InstantMessaging" => "Internet",
            "Office" | "TextEditor" => "Office",
            "Development" | "IDE" | "Debugger" | "TerminalEmulator" => "Development",
            "Education" | "Science" | "Math" => "Education",
            "System" | "Settings" | "Utility" | "Monitor" | "PackageManager" => "Utilities",
            _ => continue,
        };
        return g;
    }
    "Other"
}

fn source_of(path: &Path) -> Source {
    let p = path.to_string_lossy();
    if p.contains("/flatpak/") { Source::Flatpak } else if p.contains("/snapd/") || p.contains("/snap/") { Source::Snap } else { Source::Native }
}

/// The Steam app id in `steam://rungameid/123` (an entry Steam wrote for a game or a non-Steam shortcut).
pub fn steam_id_in(exec: &str) -> Option<u64> {
    let rest = exec.split("steam://rungameid/").nth(1)?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// One `.desktop` file as an item; `None` for hidden entries and anything that is not an application.
pub fn parse_entry(id: &str, path: &Path, text: &str) -> Option<Item> {
    let (mut name, mut about, mut keywords, mut cats, mut exec) = (String::new(), String::new(), String::new(), String::new(), String::new());
    let (mut hidden, mut is_app, mut in_main, mut icon) = (false, false, false, String::new());
    for l in text.lines().map(str::trim) {
        if l.starts_with('[') {
            in_main = l == "[Desktop Entry]";
            continue;
        }
        let Some((k, v)) = l.split_once('=').filter(|_| in_main) else { continue };
        match k {
            "Name" => name = v.into(),
            "Comment" => about = v.into(),
            "GenericName" if about.is_empty() => about = v.into(),
            "Keywords" => keywords = v.into(),
            "Categories" => cats = v.into(),
            "Exec" => exec = v.into(),
            "Icon" => icon = v.into(),
            "NoDisplay" | "Hidden" => hidden |= v.eq_ignore_ascii_case("true"),
            "Type" => is_app = v == "Application",
            _ => {}
        }
    }
    if !is_app || hidden || name.is_empty() {
        return None;
    }
    let steam = steam_id_in(&exec);
    let group = if steam.is_some() { "Games" } else { group_of(&cats) };
    let icon_path = Path::new(&icon).is_absolute().then(|| PathBuf::from(&icon)).filter(|p| p.exists()).unwrap_or_else(|| path.to_path_buf());
    Some(Item {
        id: id.to_string(), name, about, group, game: group == "Games",
        source: if steam.is_some() { Source::Steam } else { source_of(path) },
        open: Open::Entry(path.to_path_buf()), icon: Some(icon_path), keywords, last_played: None,
    })
}

/// Values of `"key"  "value"` lines in Valve's text format; enough for manifests and library lists.
fn vdf_pairs(text: &str) -> Vec<(String, String)> {
    text.lines().filter_map(|l| {
        let mut parts = l.split('"').skip(1).step_by(2);
        let (k, v) = (parts.next()?, parts.next()?);
        Some((k.to_string(), v.to_string()))
    }).collect()
}

/// Steam's runtimes and tools are installed like games but are not something to play.
fn is_steam_tool(name: &str) -> bool {
    ["Proton", "Steam Linux Runtime", "Steamworks Common Redistributables", "Steam Controller Configs"].iter().any(|t| name.starts_with(t))
}

/// An installed game from an `appmanifest_*.acf`; `None` for tools and anything not fully installed.
pub fn parse_manifest(text: &str, cache: &Path) -> Option<Item> {
    let kv: HashMap<String, String> = vdf_pairs(text).into_iter().collect();
    let id: u64 = kv.get("appid")?.parse().ok()?;
    let name = kv.get("name")?.clone();
    let installed = kv.get("StateFlags").and_then(|f| f.parse::<u32>().ok()).is_some_and(|f| f & 4 != 0);
    if !installed || is_steam_tool(&name) {
        return None;
    }
    let dir = cache.join(id.to_string());
    let icon = ["library_600x900.jpg", "header.jpg"].iter().map(|f| dir.join(f)).find(|p| p.exists());
    Some(Item {
        id: format!("steam:{id}"), name, about: "Steam game".into(), group: "Games", game: true, source: Source::Steam,
        open: Open::Steam(id), icon, keywords: String::new(), last_played: kv.get("LastPlayed").and_then(|v| v.parse().ok()).filter(|n| *n > 0),
    })
}

fn steam_roots(home: &Path) -> Vec<PathBuf> {
    [".local/share/Steam", ".steam/steam", "snap/steam/common/.local/share/Steam", ".var/app/com.valvesoftware.Steam/.local/share/Steam"]
        .iter().map(|r| home.join(r)).filter(|p| p.join("steamapps").is_dir()).collect()
}

/// Every Steam library folder (the main one and extra drives), de-duplicated by real path.
fn steam_libraries(root: &Path) -> Vec<PathBuf> {
    let mut out = vec![root.join("steamapps")];
    let list = std::fs::read_to_string(root.join("steamapps/libraryfolders.vdf")).unwrap_or_default();
    for (k, v) in vdf_pairs(&list) {
        if k == "path" {
            out.push(PathBuf::from(v).join("steamapps"));
        }
    }
    out.retain(|p| p.is_dir());
    let real = |p: &PathBuf| std::fs::canonicalize(p).unwrap_or_else(|_| p.clone());
    let mut seen = Vec::new();
    out.retain(|p| { let r = real(p); let new = !seen.contains(&r); seen.push(r); new });
    out
}

pub fn steam_games(home: &Path) -> Vec<Item> {
    let mut out = Vec::new();
    for root in steam_roots(home) {
        let cache = root.join("appcache/librarycache");
        for lib in steam_libraries(&root) {
            for e in std::fs::read_dir(&lib).into_iter().flatten().flatten() {
                let n = e.file_name().to_string_lossy().into_owned();
                if n.starts_with("appmanifest_") && n.ends_with(".acf") {
                    out.extend(parse_manifest(&std::fs::read_to_string(e.path()).unwrap_or_default(), &cache));
                }
            }
        }
    }
    out
}

/// Desktop entries and Steam games as one list, A to Z. A game that is both a Steam shortcut entry and a
/// manifest shows once (the manifest wins, since it knows when it was played).
pub fn load(entry_dirs: &[PathBuf], home: &Path) -> Vec<Item> {
    let mut by_id: HashMap<String, Item> = HashMap::new();
    for d in entry_dirs {
        for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
            let path = e.path();
            if path.extension().and_then(|x| x.to_str()) != Some("desktop") {
                continue;
            }
            let id = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if by_id.contains_key(&id) {
                continue;
            }
            if let Some(it) = parse_entry(&id, &path, &std::fs::read_to_string(&path).unwrap_or_default()) {
                by_id.insert(id, it);
            }
        }
    }
    let steam = steam_games(home);
    let steam_ids: Vec<u64> = steam.iter().filter_map(|g| if let Open::Steam(i) = g.open { Some(i) } else { None }).collect();
    let mut all: Vec<Item> = by_id.into_values().filter(|a| {
        let exec_id = if let Open::Entry(p) = &a.open { std::fs::read_to_string(p).ok().and_then(|t| t.lines().find_map(|l| l.strip_prefix("Exec=").and_then(steam_id_in))) } else { None };
        !exec_id.is_some_and(|i| steam_ids.contains(&i))
    }).collect();
    all.extend(steam);
    all.sort_by_key(|a| a.name.to_lowercase());
    all
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHELF: &str = "[Desktop Entry]\nType=Application\nName=Firefox\nComment=Browse the web\nExec=firefox %u\nIcon=firefox\nCategories=Network;WebBrowser;\n";

    #[test]
    fn a_desktop_entry_becomes_an_item_on_the_right_shelf() {
        let it = parse_entry("firefox.desktop", Path::new("/usr/share/applications/firefox.desktop"), SHELF).unwrap();
        assert_eq!((it.name.as_str(), it.group, it.game, it.source), ("Firefox", "Internet", false, Source::Native));
        assert_eq!(it.about, "Browse the web");
    }

    #[test]
    fn hidden_entries_and_non_applications_are_left_out() {
        assert!(parse_entry("a", Path::new("/x"), "[Desktop Entry]\nType=Application\nName=A\nNoDisplay=true\n").is_none());
        assert!(parse_entry("a", Path::new("/x"), "[Desktop Entry]\nType=Link\nName=A\n").is_none());
        assert!(parse_entry("a", Path::new("/x"), "[Desktop Entry]\nType=Application\n").is_none());
    }

    #[test]
    fn flatpak_and_snap_are_told_apart_by_where_the_entry_lives() {
        let t = "[Desktop Entry]\nType=Application\nName=Thing\n";
        assert_eq!(parse_entry("t", Path::new("/var/lib/flatpak/exports/share/applications/t.desktop"), t).unwrap().source, Source::Flatpak);
        assert_eq!(parse_entry("t", Path::new("/var/lib/snapd/desktop/applications/t.desktop"), t).unwrap().source, Source::Snap);
    }

    #[test]
    fn games_are_found_by_category_and_by_a_steam_launch_line() {
        let g = "[Desktop Entry]\nType=Application\nName=Chess\nCategories=Game;BoardGame;\n";
        assert!(parse_entry("g", Path::new("/x/g.desktop"), g).unwrap().game);
        let s = "[Desktop Entry]\nType=Application\nName=Some Game\nExec=steam steam://rungameid/12345\n";
        let it = parse_entry("s", Path::new("/x/s.desktop"), s).unwrap();
        assert!(it.game && it.source == Source::Steam);
        assert_eq!(steam_id_in("steam steam://rungameid/12345"), Some(12345));
        assert_eq!(steam_id_in("firefox"), None);
    }

    #[test]
    fn shelves_follow_the_first_known_category() {
        assert_eq!(group_of("GTK;Office;TextEditor;"), "Office");
        assert_eq!(group_of("X-Foo;"), "Other");
        assert_eq!(group_of("Utility;Game;"), "Utilities");
    }

    fn manifest(id: u64, name: &str, flags: u32) -> String {
        format!("\"AppState\"\n{{\n\t\"appid\"\t\t\"{id}\"\n\t\"name\"\t\t\"{name}\"\n\t\"StateFlags\"\t\t\"{flags}\"\n\t\"LastPlayed\"\t\t\"1792303666\"\n}}\n")
    }

    #[test]
    fn steam_manifests_give_installed_games_only() {
        let c = Path::new("/nonexistent/cache");
        let g = parse_manifest(&manifest(2142030, "Fishton", 4), c).unwrap();
        assert_eq!((g.name.as_str(), g.open.clone(), g.last_played), ("Fishton", Open::Steam(2142030), Some(1792303666)));
        assert!(parse_manifest(&manifest(1, "Half-installed", 1026), c).is_none());
        assert!(parse_manifest(&manifest(228980, "Steamworks Common Redistributables", 4), c).is_none());
        assert!(parse_manifest(&manifest(1493710, "Proton Experimental", 4), c).is_none());
    }

    #[test]
    fn a_game_that_is_both_a_shortcut_and_a_manifest_shows_once() {
        let tmp = std::env::temp_dir().join(format!("hestia-apps-{}", std::process::id()));
        let (apps, lib) = (tmp.join("applications"), tmp.join(".local/share/Steam/steamapps"));
        std::fs::create_dir_all(&apps).unwrap();
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(apps.join("fishton.desktop"), "[Desktop Entry]\nType=Application\nName=Fishton\nExec=steam steam://rungameid/2142030\n").unwrap();
        std::fs::write(apps.join("calc.desktop"), "[Desktop Entry]\nType=Application\nName=Calculator\nCategories=Utility;\n").unwrap();
        std::fs::write(lib.join("appmanifest_2142030.acf"), manifest(2142030, "Fishton", 4)).unwrap();
        let all = load(std::slice::from_ref(&apps), &tmp);
        let names: Vec<&str> = all.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["Calculator", "Fishton"]);
        assert!(matches!(all[1].open, Open::Steam(2142030)));
        let _ = std::fs::remove_dir_all(&tmp);
    }
}

/// Which items to show, and in what order. `show`: 0 all, 1 games, 2 apps (not games).
/// `sort`: 1 most used first (our own launch counts, then Steam play time); 0, 2 and 3 are by name (the view
/// groups 2 by kind and 3 by source, so those shelves are listed together). Names break every tie.
pub fn visible(items: &[Item], q: &str, show: usize, sort: usize, counts: &HashMap<String, u32>) -> Vec<usize> {
    let q = q.trim().to_lowercase();
    let mut out: Vec<(u32, usize)> = items.iter().enumerate().filter(|(_, a)| match show { 1 => a.game, 2 => !a.game, _ => true }).filter_map(|(i, a)| {
        if q.is_empty() {
            return Some((0, i));
        }
        let s = [crate::launcher::score(&q, &a.name), crate::launcher::score(&q, &a.keywords) / 2, crate::launcher::score(&q, &a.about) / 3].into_iter().max().unwrap_or(0);
        (s > 0).then_some((s, i))
    }).collect();
    out.sort_by(|(sa, a), (sb, b)| {
        let (x, y) = (&items[*a], &items[*b]);
        let used = |it: &Item| (counts.get(&it.id).copied().unwrap_or(0), it.last_played.unwrap_or(0));
        let shelf = |it: &Item| match sort { 2 => it.group, 3 => it.source.label(), _ => "" };
        sb.cmp(sa)
            .then_with(|| shelf(x).cmp(shelf(y)))
            .then_with(|| if sort == 1 { used(y).cmp(&used(x)) } else { std::cmp::Ordering::Equal })
            .then_with(|| x.name.to_lowercase().cmp(&y.name.to_lowercase()))
    });
    out.into_iter().map(|(_, i)| i).collect()
}

/// Open an item. A failure comes back as words a person can read; a success is counted.
pub fn open(item: &Item, counts: &mut HashMap<String, u32>) -> Result<(), String> {
    match &item.open {
        Open::Entry(path) => {
            let app = crate::launcher::App { id: item.id.clone(), name: item.name.clone(), generic: String::new(), keywords: String::new(), path: path.clone() };
            crate::launcher::launch(&app, counts)
        }
        Open::Steam(id) => {
            let out = std::process::Command::new("gio").arg("open").arg(format!("steam://rungameid/{id}")).stdin(std::process::Stdio::null()).output()
                .map_err(|e| format!("Couldn't open {}: gio is not available ({e})", item.name))?;
            if out.status.success() {
                *counts.entry(item.id.clone()).or_default() += 1;
                Ok(())
            } else {
                Err(format!("Couldn't open {}: is Steam installed?", item.name))
            }
        }
    }
}

#[cfg(test)]
mod view_tests {
    use super::*;

    fn it(name: &str, game: bool) -> Item {
        Item { id: name.into(), name: name.into(), about: String::new(), group: if game { "Games" } else { "Other" }, game, source: Source::Native, open: Open::Steam(1), icon: None, keywords: String::new(), last_played: None }
    }

    #[test]
    fn filters_search_and_sort_agree() {
        let items = vec![it("Zed", false), it("Alpha", true), it("Beta", true), it("Firefox", false)];
        let none = HashMap::new();
        assert_eq!(visible(&items, "", 0, 0, &none), [1, 2, 3, 0]);
        assert_eq!(visible(&items, "", 1, 0, &none), [1, 2]);
        assert_eq!(visible(&items, "", 2, 0, &none), [3, 0]);
        assert_eq!(visible(&items, "fire", 0, 0, &none), [3]);
        let used: HashMap<String, u32> = [("Zed".to_string(), 9)].into();
        assert_eq!(visible(&items, "", 0, 1, &used)[0], 0);
        // by kind lists the shelves together: Games (Alpha, Beta) before Other (Firefox, Zed)
        assert_eq!(visible(&items, "", 0, 2, &none), [1, 2, 3, 0]);
    }
}

#[cfg(test)]
mod machine_smoke {
    /// Reads this computer's real applications and prints what it found: `cargo test -p hestia real_apps -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_apps() {
        let home = std::path::PathBuf::from(std::env::var("HOME").unwrap());
        let all = super::load(&crate::launcher::dirs(), &home);
        let games = all.iter().filter(|a| a.game).count();
        eprintln!("{} items, {games} games, {} from Steam", all.len(), all.iter().filter(|a| a.source == super::Source::Steam).count());
        assert!(!all.is_empty());
    }
}
