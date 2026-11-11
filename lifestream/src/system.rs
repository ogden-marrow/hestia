//! What is running right now: timers, services, models, GPU, pipeline progress.
//! Read-only and fast (a few ms); the screen polls it from a background thread.

use crate::{bundle, semantic};
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, SystemTime};

pub const UNITS: &[(&str, &str)] = &[
    ("lifestream-nightly.timer", "overnight scan timer"),
    ("lifestream-nightly.service", "overnight scan + suggestions"),
    ("lifestream-morning.timer", "morning notification timer"),
    ("hestia-watch.timer", "capture watcher timer"),
    ("ollama.service", "local model server"),
    ("hestia-tray.service", "menu bar icon"),
];

#[derive(Clone, Debug, Default)]
pub struct Unit {
    pub name: String,
    pub what: String,
    pub active: String,
    pub result: String,
    pub next: String,
    pub last: String,
}

#[derive(Clone, Debug, Default)]
pub struct Loaded {
    pub name: String,
    pub vram_mb: u64,
    pub expires: String,
}

#[derive(Clone, Debug, Default)]
pub struct Gpu {
    pub used_mb: u64,
    pub total_mb: u64,
    pub util: u64,
    pub temp: u64,
}

#[derive(Clone, Debug, Default)]
pub struct Status {
    pub units: Vec<Unit>,
    pub ollama_up: bool,
    pub installed: Vec<(String, f64)>, // name, GB
    pub loaded: Vec<Loaded>,
    pub gpu: Option<Gpu>,
    pub stream_age: String,
    pub pending: usize,
    pub folders: usize,
    pub suggested: usize,
}

fn show(unit: &str) -> HashMap<String, String> {
    let out = Command::new("systemctl")
        .args(["--user", "show", unit, "-p", "ActiveState", "-p", "SubState", "-p", "Result", "-p", "NextElapseUSecRealtime", "-p", "LastTriggerUSec", "-p", "ExecMainExitTimestamp"])
        .output();
    let text = out.map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    text.lines().filter_map(|l| l.split_once('=')).map(|(k, v)| (k.to_string(), v.trim().to_string())).collect()
}

/// "Wed 2026-11-11 02:32:10 EST" -> "Wed 02:32"; empty or "n/a" -> "-"
fn short_time(s: &str) -> String {
    let parts: Vec<&str> = s.split_whitespace().collect();
    match parts.as_slice() {
        [day, date, time, ..] if date.len() == 10 && time.len() >= 5 => format!("{day} {date} {}", &time[..5]),
        _ => "-".into(),
    }
}

fn units() -> Vec<Unit> {
    UNITS
        .iter()
        .map(|(name, what)| {
            let p = show(name);
            let g = |k: &str| p.get(k).cloned().unwrap_or_default();
            let is_timer = name.ends_with(".timer");
            Unit {
                name: name.to_string(),
                what: what.to_string(),
                active: if is_timer || g("ActiveState") != "inactive" { format!("{} ({})", g("ActiveState"), g("SubState")) } else { "idle".into() },
                result: g("Result"),
                next: if is_timer { short_time(&g("NextElapseUSecRealtime")) } else { "-".into() },
                last: if is_timer { short_time(&g("LastTriggerUSec")) } else { short_time(&g("ExecMainExitTimestamp")) },
            }
        })
        .collect()
}

fn get_json(path: &str) -> Option<serde_json::Value> {
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_millis(800)).build();
    agent.get(&format!("{}{path}", semantic::DEFAULT_HOST)).call().ok()?.into_json().ok()
}

fn gpu() -> Option<Gpu> {
    let o = Command::new("nvidia-smi")
        .args(["--query-gpu=memory.used,memory.total,utilization.gpu,temperature.gpu", "--format=csv,noheader,nounits"])
        .output()
        .ok()?;
    let line = String::from_utf8_lossy(&o.stdout).lines().next()?.to_string();
    let n: Vec<u64> = line.split(',').filter_map(|x| x.trim().parse().ok()).collect();
    (n.len() == 4).then(|| Gpu { used_mb: n[0], total_mb: n[1], util: n[2], temp: n[3] })
}

fn age(t: SystemTime) -> String {
    let s = SystemTime::now().duration_since(t).map(|d| d.as_secs()).unwrap_or(0);
    match s {
        0..=89 => format!("{s}s ago"),
        90..=5399 => format!("{} min ago", s / 60),
        5400..=172_799 => format!("{} h ago", s / 3600),
        _ => format!("{} days ago", s / 86400),
    }
}

fn newest_shard(stream: &Path) -> Option<SystemTime> {
    std::fs::read_dir(stream).ok()?.flatten().filter_map(|y| std::fs::read_dir(y.path()).ok()).flatten().flatten()
        .filter_map(|f| f.metadata().ok()?.modified().ok()).max()
}

pub fn gather(recs: &[crate::Record], stream: &Path) -> Status {
    let ps = get_json("/api/ps");
    let tags = get_json("/api/tags");
    let loaded = ps.as_ref().and_then(|v| v["models"].as_array().cloned()).unwrap_or_default().iter()
        .map(|m| Loaded {
            name: m["name"].as_str().unwrap_or("?").to_string(),
            vram_mb: m["size_vram"].as_u64().unwrap_or(0) / 1_000_000,
            expires: m["expires_at"].as_str().and_then(|e| e.get(11..16)).unwrap_or("").to_string(),
        })
        .collect();
    let installed = tags.as_ref().and_then(|v| v["models"].as_array().cloned()).unwrap_or_default().iter()
        .map(|m| (m["name"].as_str().unwrap_or("?").to_string(), m["size"].as_f64().unwrap_or(0.0) / 1e9))
        .collect();
    let dir = stream.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let bundles = bundle::bundles(recs);
    let cache = bundle::load_cache(&dir);
    let suggested = bundles.iter().filter(|b| cache.get(&b.root).is_some_and(|c| c.fingerprint == b.fingerprint())).count();
    Status {
        units: units(),
        ollama_up: ps.is_some(),
        installed,
        loaded,
        gpu: gpu(),
        stream_age: newest_shard(stream).map(age).unwrap_or_else(|| "never".into()),
        pending: recs.iter().filter(|r| r.status == "pending").count(),
        folders: bundles.len(),
        suggested,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_time_trims_the_zone_and_seconds() {
        assert_eq!(short_time("Wed 2026-11-11 02:32:10 EST"), "Wed 2026-11-11 02:32");
        assert_eq!(short_time(""), "-");
        assert_eq!(short_time("n/a"), "-");
    }

    #[test]
    fn age_reads_naturally() {
        assert_eq!(age(SystemTime::now()), "0s ago");
        assert!(age(SystemTime::now() - Duration::from_secs(7200)).ends_with("h ago"));
    }
}
