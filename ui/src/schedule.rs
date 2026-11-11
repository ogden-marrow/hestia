//! Applies the schedule settings to the system: the overnight scan and morning briefing timers,
//! which folders the scan reads, and the menu bar icon. Hestia never edits the unit files it ships;
//! it writes small drop-in overrides next to them, so removing a drop-in restores the defaults.

use crate::settings::Settings;
use std::path::PathBuf;
use std::process::Command;

fn units_dir() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config/systemd/user")
}

fn drop_in(unit: &str, body: &str) -> std::io::Result<()> {
    let dir = units_dir().join(format!("{unit}.d"));
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("hestia.conf"), body)
}

fn systemctl(args: &[&str]) -> bool {
    Command::new("systemctl").arg("--user").args(args).status().map(|s| s.success()).unwrap_or(false)
}

fn timer(unit: &str, (h, m): (u8, u8)) -> std::io::Result<()> {
    drop_in(unit, &format!("[Timer]\nOnCalendar=\nOnCalendar=*-*-* {h:02}:{m:02}:00\n"))?;
    Ok(())
}

/// Write everything the settings say, reload, and report what happened in one line.
pub fn apply(s: &Settings) -> String {
    let steps = (|| -> std::io::Result<()> {
        timer("lifestream-nightly.timer", s.scan_time)?;
        timer("lifestream-morning.timer", s.brief_time)?;
        drop_in("hestia-watch.timer", &format!("[Timer]\nOnUnitActiveSec=\nOnUnitActiveSec={}min\n", s.watch_mins))?;
        let roots: Vec<String> = s.roots.iter().map(|r| format!("%h/{r}")).collect();
        drop_in(
            "lifestream-nightly.service",
            &format!("[Service]\nExecStart=\nExecStart=%h/.local/bin/lifestream scan {}\n", roots.join(" ")),
        )
    })();
    if let Err(e) = steps {
        return format!("could not write the schedule: {e}");
    }
    let mut ok = systemctl(&["daemon-reload"]);
    for (unit, on) in [("lifestream-nightly.timer", s.scan_on), ("lifestream-morning.timer", s.brief_on), ("hestia-watch.timer", s.watch_on)] {
        ok &= if on { systemctl(&["enable", "--now", unit]) && systemctl(&["restart", unit]) } else { systemctl(&["disable", "--now", unit]) };
    }
    ok &= if s.tray { systemctl(&["enable", "--now", "hestia-tray.service"]) } else { systemctl(&["disable", "--now", "hestia-tray.service"]) };
    if ok { "saved and applied".into() } else { "saved, but the system did not accept every change".into() }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Touches the real user units (writes drop-ins equal to the defaults).
    /// Run with: cargo test --release schedule -- --ignored --nocapture
    #[test]
    #[ignore]
    fn applying_the_defaults_reaches_systemd() {
        let result = apply(&Settings::default());
        eprintln!("{result}");
        let cat = |u: &str| String::from_utf8_lossy(&Command::new("systemctl").args(["--user", "cat", u]).output().unwrap().stdout).into_owned();
        let timer = cat("lifestream-nightly.timer");
        assert!(timer.contains("OnCalendar=*-*-* 02:30:00"), "{timer}");
        let service = cat("lifestream-nightly.service");
        assert!(service.contains("lifestream scan %h/Documents %h/Downloads %h/Pictures %h/Videos"), "{service}");
        assert_eq!(result, "saved and applied");
    }
}
