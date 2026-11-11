//! Everything the user can choose, saved in ~/.config/hestia/settings (one `key=value` per line).
//! The Settings screen is the only place these are changed.

use crate::design::Density;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Accent {
    System,
    Blue,
    Teal,
    Green,
    Orange,
    Purple,
    Pink,
    Graphite,
}

impl Accent {
    pub const ALL: [Accent; 8] = [Accent::Blue, Accent::Teal, Accent::Green, Accent::Orange, Accent::Purple, Accent::Pink, Accent::Graphite, Accent::System];
    pub fn name(self) -> &'static str {
        match self {
            Accent::System => "system",
            Accent::Blue => "blue",
            Accent::Teal => "teal",
            Accent::Green => "green",
            Accent::Orange => "orange",
            Accent::Purple => "purple",
            Accent::Pink => "pink",
            Accent::Graphite => "graphite",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Scheme {
    Auto,
    Light,
    Dark,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    // appearance
    pub scheme: Scheme,
    pub density: Density,
    pub text: f32,
    pub accent: Accent,
    // scanning
    pub scan_on: bool,
    pub scan_time: (u8, u8),
    pub roots: Vec<String>,
    // morning briefing
    pub brief_on: bool,
    pub brief_time: (u8, u8),
    // general
    pub tray: bool,
    // files
    pub hidden: bool,
    // questions
    pub siblings: bool,
    // launcher and clipboard
    pub clipboard_on: bool,
    pub clipboard_max: u32,
    pub clipboard_days: u32,
    // folders: suggested kinds the user dismissed
    pub dismissed: Vec<String>,
    // the welcome tour has been seen
    pub welcomed: bool,
    // keep the machine capture current: look every N minutes (when quiet) and commit changes
    pub watch_on: bool,
    pub watch_mins: u32,
    // where the home repository lives, relative to the home folder ("" = not chosen), and Hestia's public repository
    pub home: String,
    pub hestia_repo: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            scheme: Scheme::Auto, density: Density::Cozy, text: 1.0, accent: Accent::Blue,
            scan_on: true, scan_time: (2, 30), roots: ["Documents", "Downloads", "Pictures", "Videos"].map(String::from).to_vec(),
            brief_on: true, brief_time: (7, 30), tray: true, hidden: false, siblings: false, clipboard_on: false, clipboard_max: 100, clipboard_days: 7, dismissed: vec![], welcomed: false, watch_on: true, watch_mins: 15, home: String::new(), hestia_repo: String::new(),
        }
    }
}

fn time(v: &str) -> Option<(u8, u8)> {
    let (h, m) = v.split_once(':')?;
    let (h, m) = (h.parse::<u8>().ok()?, m.parse::<u8>().ok()?);
    (h < 24 && m < 60).then_some((h, m))
}

/// A scan folder is a plain path under the home folder: no spaces or tricks, so it is safe in a unit file.
pub fn root_ok(r: &str) -> bool {
    !r.is_empty() && !r.starts_with(['/', '~']) && !r.split('/').any(|c| c.is_empty() || c == "." || c == "..")
        && r.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_' | '.'))
}

impl Settings {
    fn path() -> Option<std::path::PathBuf> {
        Some(std::path::PathBuf::from(std::env::var("HOME").ok()?).join(".config/hestia/settings"))
    }

    pub fn parse(text: &str) -> Settings {
        let mut s = Settings::default();
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let (k, v) = (k.trim(), v.trim());
            match k {
                "scheme" => s.scheme = match v { "light" => Scheme::Light, "dark" => Scheme::Dark, _ => Scheme::Auto },
                "density" => s.density = match v { "compact" => Density::Compact, "airy" => Density::Airy, _ => Density::Cozy },
                "text" => s.text = v.parse::<f32>().map(|t| t.clamp(0.8, 1.4)).unwrap_or(1.0),
                "accent" => s.accent = Accent::ALL.into_iter().find(|a| a.name() == v).unwrap_or(Accent::Blue),
                "scan" => s.scan_on = v != "off",
                "scan_time" => s.scan_time = time(v).unwrap_or(s.scan_time),
                "roots" => s.roots = v.split(',').map(str::trim).filter(|r| root_ok(r)).map(String::from).collect(),
                "brief" => s.brief_on = v != "off",
                "brief_time" => s.brief_time = time(v).unwrap_or(s.brief_time),
                "tray" => s.tray = v != "off",
                "hidden" => s.hidden = v == "yes",
                "siblings" => s.siblings = v == "yes", // opt-in: a missing or odd value means off (#56)
                "clipboard_history" => s.clipboard_on = v == "on", // opt-in: only an explicit "on"; the old `clipboard=` key is ignored on purpose
                "clipboard_max" => s.clipboard_max = v.parse::<u32>().map(|n| n.clamp(10, 1000)).unwrap_or(100),
                "clipboard_days" => s.clipboard_days = v.parse::<u32>().map(|n| n.clamp(1, 365)).unwrap_or(7),
                "welcomed" => s.welcomed = v == "yes",
                "home" => s.home = v.trim().to_string(),
                "hestia_repo" => s.hestia_repo = v.trim().to_string(),
                "watch" => s.watch_on = v != "off",
                "watch_mins" => s.watch_mins = match v.parse::<u32>() { Ok(5) => 5, Ok(60) => 60, _ => 15 },
                "dismissed" => s.dismissed = v.split(',').map(str::trim).filter(|x| !x.is_empty()).map(String::from).collect(),
                _ => {}
            }
        }
        s
    }

    pub fn render(&self) -> String {
        let onoff = |b: bool| if b { "on" } else { "off" };
        format!(
            "scheme={}\ndensity={}\ntext={}\naccent={}\nscan={}\nscan_time={:02}:{:02}\nroots={}\nbrief={}\nbrief_time={:02}:{:02}\ntray={}\nhidden={}\nsiblings={}\nclipboard_history={}\nclipboard_max={}\nclipboard_days={}\ndismissed={}\nwelcomed={}\nwatch={}\nwatch_mins={}\nhome={}\nhestia_repo={}\n",
            match self.scheme { Scheme::Auto => "auto", Scheme::Light => "light", Scheme::Dark => "dark" },
            match self.density { Density::Compact => "compact", Density::Cozy => "cozy", Density::Airy => "airy" },
            self.text, self.accent.name(), onoff(self.scan_on), self.scan_time.0, self.scan_time.1, self.roots.join(","),
            onoff(self.brief_on), self.brief_time.0, self.brief_time.1, onoff(self.tray),
            if self.hidden { "yes" } else { "no" }, if self.siblings { "yes" } else { "no" }, onoff(self.clipboard_on), self.clipboard_max, self.clipboard_days, self.dismissed.join(","), if self.welcomed { "yes" } else { "no" }, onoff(self.watch_on), self.watch_mins, self.home, self.hestia_repo,
        )
    }

    pub fn load() -> Settings {
        if cfg!(test) {
            return Settings::default();
        }
        Self::path().and_then(|p| std::fs::read_to_string(p).ok()).map(|t| Settings::parse(&t)).unwrap_or_default()
    }

    pub fn save(&self) {
        if cfg!(test) {
            return; // tests must never touch the real config
        }
        if let Some(p) = Self::path() {
            let _ = std::fs::create_dir_all(p.parent().unwrap_or(&p));
            let _ = std::fs::write(p, self.render());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_and_tolerate_junk() {
        let s = Settings {
            scheme: Scheme::Dark, density: Density::Airy, text: 1.1, accent: Accent::Orange, scan_on: false, scan_time: (3, 15),
            roots: vec!["Documents".into(), "Music/Albums".into()], brief_on: false, brief_time: (6, 45), tray: false, hidden: true, siblings: false, clipboard_on: false, clipboard_max: 200, clipboard_days: 30, dismissed: vec!["software/app-bundle".into()], welcomed: true, watch_on: false, watch_mins: 60, home: "Home".into(), hestia_repo: "https://example.org/hestia".into(),
        };
        assert_eq!(Settings::parse(&s.render()), s);
        assert_eq!(Settings::parse("nonsense\naccent=???\ntext=abc\nscan_time=99:99\n"), Settings::default());
        assert_eq!(Settings::parse("text=9").text, 1.4, "text size is clamped");
    }

    #[test]
    fn clipboard_history_is_opt_in() {
        assert!(!Settings::default().clipboard_on);
        for v in ["", "clipboard_history=", "clipboard_history=off", "clipboard_history=yes", "clipboard_history=ON", "clipboard_history=true"] {
            assert!(!Settings::parse(v).clipboard_on, "{v:?}");
        }
        assert!(Settings::parse("clipboard_history=on").clipboard_on);
        // anyone opted in by the old default (`clipboard=on`) is off until they choose
        assert!(!Settings::parse("clipboard=on").clipboard_on);
        assert!(Settings { clipboard_on: true, ..Settings::default() }.render().contains("clipboard_history=on"));
        assert!(!Settings::default().render().contains("\nclipboard=") );
    }

    #[test]
    fn answering_similar_folders_together_is_opt_in() {
        assert!(!Settings::default().siblings);
        for v in ["", "siblings=", "siblings=no", "siblings=YES", "siblings=true", "siblings=1"] {
            assert!(!Settings::parse(v).siblings, "{v:?}");
        }
        assert!(Settings::parse("siblings=yes").siblings);
    }

    #[test]
    fn scan_folders_must_be_plain_paths_under_home() {
        for ok in ["Documents", "Pictures/Raw", "my-stuff_2"] {
            assert!(root_ok(ok), "{ok}");
        }
        for bad in ["", "/etc", "~/x", "../x", "a b", "a//b", "x;rm", "$HOME"] {
            assert!(!root_ok(bad), "{bad}");
        }
    }
}
