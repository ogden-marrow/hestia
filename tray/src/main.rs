//! Menu bar icon: how many files need you, a click to open the screen, a scan-now item.
//! Refreshes every minute from the stream; touches nothing else.

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
        match self.frame {
            1 => vec![flame!(16, 1), flame!(32, 1), flame!(48, 1)],
            2 => vec![flame!(16, 2), flame!(32, 2), flame!(48, 2)],
            3 => vec![flame!(16, 3), flame!(32, 3), flame!(48, 3)],
            _ => vec![flame!(16, 0), flame!(32, 0), flame!(48, 0)],
        }
    }
    fn title(&self) -> String {
        format!("Hestia: {} need you", self.needs)
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
        vec![
            StandardItem { label: format!("{} need you · {} files", self.needs, self.total), enabled: false, ..Default::default() }.into(),
            MenuItem::Separator,
            item("Search…".into(), |_| spawn("gdbus", &["call", "--session", "--dest", "org.gnome.Shell", "--object-path", "/app/hestia/Launcher", "--method", "app.hestia.Launcher.Toggle"])),
            item(format!("Answer the questions ({})", self.folders), |_| spawn("hestia", &["--questions"])),
            item("Open Hestia".into(), |_| spawn("hestia", &[])),
            item("Scan now".into(), |_| spawn("systemctl", &["--user", "start", "lifestream-nightly.service"])),
            MenuItem::Separator,
            item("Quit".into(), |_| std::process::exit(0)),
        ]
    }
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

/// Is the Hestia window open? (The small launcher window does not count.)
fn app_open() -> bool {
    let Ok(dir) = std::fs::read_dir("/proc") else { return false };
    dir.flatten().any(|e| {
        let p = e.path();
        std::fs::read_to_string(p.join("comm")).map(|c| c.trim() == "hestia").unwrap_or(false)
            && !std::fs::read(p.join("cmdline")).map(|c| String::from_utf8_lossy(&c).contains("--launch")).unwrap_or(true)
    })
}

fn main() {
    let (needs, folders, total) = counts();
    let handle = Tray { needs, folders, total, frame: 0 }.spawn().expect("no tray host: is the GNOME appindicator extension on?");
    let mut tick = 0u32;
    let mut stats = (needs, folders, total);
    let (mut open, mut last, mut shown) = (false, 0usize, stats);
    loop {
        std::thread::sleep(TICK);
        tick = tick.wrapping_add(1);
        if tick % REFRESH_EVERY == 0 {
            stats = counts();
        }
        // it only wiggles while the app is open; checked about once a second
        if tick % 5 == 0 {
            open = app_open();
        }
        let frame = if open { FLICKER[tick as usize % FLICKER.len()] } else { 0 };
        if frame == last && stats == shown {
            continue;
        }
        last = frame;
        shown = stats;
        if handle.update(|t| { t.needs = stats.0; t.folders = stats.1; t.total = stats.2; t.frame = frame; }).is_none() {
            std::process::exit(1); // the tray service went away: exit non-zero so systemd restarts us
        }
    }
}
