//! Menu bar icon: how many files need you, a click to open the screen, a scan-now item. When the backup needs you
//! (`lifestream backup-state`: failing or stale) the flame turns warm orange and the tooltip says why in plain words.
//! Refreshes every minute from the stream and the watcher's files. It is also the always-on backup check (#252): every minute
//! it asks the engine's one notifier (`lifestream::backup::check`), so a stopped or dead watcher timer still raises its one
//! notification; the engine's lock and `notified.json` keep the tray and the watcher from both telling the same problem.

use ksni::blocking::TrayMethods;
use ksni::menu::{MenuItem, StandardItem};
use lifestream::{load_all, stream_dir};
use std::process::Command;
use std::time::Duration;

struct Tray {
    needs: usize,
    folders: usize,
    total: usize,
    frame: usize, // which flicker frame is showing
    backup: Option<String>, // the plain sentence when the backup needs you, else None
}

/// Background services have no ~/.local/bin on PATH, so name our own programs by full path.
fn spawn(bin: &str, args: &[&str]) {
    let home = std::env::var("HOME").unwrap_or_default();
    let installed = format!("{home}/.local/bin/{bin}");
    let program = if std::path::Path::new(&installed).exists() { installed } else { bin.to_string() };
    let _ = Command::new(program).args(args).spawn();
}

impl ksni::Tray for Tray {
    fn id(&self) -> String {
        "hestia".into()
    }
    /// The icon drawn into the tray itself, so it shows even when the desktop's icon cache is stale.
    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        // a one-colour pixel flame at 1x, 2x and 3x (the shell picks the closest), in four flicker frames
        macro_rules! flame {
            ($n:literal, $f:literal) => {
                ksni::Icon { width: $n, height: $n, data: include_bytes!(concat!("../icons/flame-", $n, "-", $f, ".argb")).to_vec() }
            };
        }
        let icons = match self.frame {
            1 => vec![flame!(16, 1), flame!(32, 1), flame!(48, 1)],
            2 => vec![flame!(16, 2), flame!(32, 2), flame!(48, 2)],
            3 => vec![flame!(16, 3), flame!(32, 3), flame!(48, 3)],
            _ => vec![flame!(16, 0), flame!(32, 0), flame!(48, 0)],
        };
        if self.backup.is_some() { icons.into_iter().map(warm).collect() } else { icons }
    }
    fn title(&self) -> String {
        match self.backup {
            Some(_) => "Hestia: your backup needs you".into(),
            None => format!("Hestia: {} need you", self.needs),
        }
    }
    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip { title: self.title(), description: self.backup.clone().unwrap_or_default(), ..Default::default() }
    }
    fn icon_name(&self) -> String {
        String::new() // the picture below is drawn into the item, so no theme lookup is involved
    }
    fn activate(&mut self, _x: i32, _y: i32) {
        spawn("hestia", &[]);
    }
    fn menu(&self) -> Vec<MenuItem<Self>> {
        let item = |label: String, f: fn(&mut Self)| -> MenuItem<Self> {
            StandardItem { label, activate: Box::new(f), ..Default::default() }.into()
        };
        let mut menu: Vec<MenuItem<Self>> = vec![];
        if let Some(why) = &self.backup {
            menu.push(StandardItem { label: "Your backup needs you".into(), enabled: false, ..Default::default() }.into());
            menu.push(StandardItem { label: why.clone(), enabled: false, ..Default::default() }.into());
            menu.push(MenuItem::Separator);
        }
        menu.extend([
            StandardItem { label: format!("{} need you · {} files", self.needs, self.total), enabled: false, ..Default::default() }.into(),
            MenuItem::Separator,
            item("Search…".into(), |_| spawn("gdbus", &["call", "--session", "--dest", "org.gnome.Shell", "--object-path", "/app/hestia/Launcher", "--method", "app.hestia.Launcher.Toggle"])),
            item(format!("Answer the questions ({})", self.folders), |_| spawn("hestia", &["--questions"])),
            item("Open Hestia".into(), |_| spawn("hestia", &[])),
            item("Scan now".into(), |_| spawn("systemctl", &["--user", "start", "lifestream-nightly.service"])),
            MenuItem::Separator,
            item("Quit".into(), |_| std::process::exit(0)),
        ]);
        menu
    }
}

/// The flame in the one warm colour that means "this needs you" (every drawn pixel keeps its alpha).
fn warm(mut icon: ksni::Icon) -> ksni::Icon {
    for px in icon.data.as_chunks_mut::<4>().0 {
        if px[0] != 0 {
            *px = [px[0], 0xF0, 0x6A, 0x3C];
        }
    }
    icon
}

fn counts() -> (usize, usize, usize) {
    let recs = stream_dir(None).and_then(|d| load_all(&d)).unwrap_or_default();
    let live = recs.iter().filter(|r| r.status != "gone");
    (recs.iter().filter(|r| r.status == "pending").count(), lifestream::bundle::bundles(&recs).len(), live.count())
}

/// The order the flame's tip moves in, one step per tick.
const FLICKER: [usize; 8] = [0, 1, 0, 2, 0, 3, 2, 1];
const TICK: Duration = Duration::from_millis(220);
const REFRESH_EVERY: u32 = 60 * 1000 / 220;

/// Is the Hestia window open? (The headless `--serve` engine behind the Shell launcher does not count.)
fn app_open() -> bool {
    let Ok(dir) = std::fs::read_dir("/proc") else { return false };
    dir.flatten().any(|e| {
        let p = e.path();
        std::fs::read_to_string(p.join("comm")).map(|c| c.trim() == "hestia").unwrap_or(false)
            && !std::fs::read(p.join("cmdline")).map(|c| String::from_utf8_lossy(&c).contains("--serve")).unwrap_or(true)
    })
}

fn main() {
    let (needs, folders, total) = counts();
    // the backup check (and its one notification, if due) runs before the tray host is reached: a missing host must not stop it (#252)
    let mut backup = lifestream::backup::check();
    let handle = Tray { needs, folders, total, frame: 0, backup: backup.clone() }.spawn().expect("no tray host: is the GNOME appindicator extension on?");
    let mut tick = 0u32;
    let mut stats = (needs, folders, total);
    let (mut open, mut last, mut shown, mut shown_backup) = (false, 0usize, stats, backup.clone());
    loop {
        std::thread::sleep(TICK);
        tick = tick.wrapping_add(1);
        if tick.is_multiple_of(REFRESH_EVERY) {
            stats = counts();
            backup = lifestream::backup::check();
        }
        // it only wiggles while the app is open; checked about once a second
        if tick.is_multiple_of(5) {
            open = app_open();
        }
        let frame = if open { FLICKER[tick as usize % FLICKER.len()] } else { 0 };
        if frame == last && stats == shown && backup == shown_backup {
            continue;
        }
        last = frame;
        shown = stats;
        shown_backup = backup.clone();
        if handle.update(|t| { t.needs = stats.0; t.folders = stats.1; t.total = stats.2; t.frame = frame; t.backup = backup.clone(); }).is_none() {
            std::process::exit(1); // the tray service went away: exit non-zero so systemd restarts us
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn warm_recolours_drawn_pixels_only() {
        let icon = super::warm(ksni::Icon { width: 2, height: 1, data: vec![0, 0, 0, 0, 0xFF, 0xF2, 0xF2, 0xF2] });
        assert_eq!(icon.data, vec![0, 0, 0, 0, 0xFF, 0xF0, 0x6A, 0x3C]);
    }
}
