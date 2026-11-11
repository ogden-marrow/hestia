//! Machines: a small fingerprint of every computer you own, kept in `machines/<name>/machine.toml`.
//! It says what the machine is (hardware, operating system) and which application profile under
//! `applications/<os>/…` describes how it is set up. The raw machine id is never stored, only a short hash.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Machine {
    pub name: String,
    /// a short hash of the machine id, product and CPU: stable for this computer, meaningless elsewhere
    pub fingerprint: String,
    pub os: String,
    pub distro: String,
    pub version: String,
    pub arch: String,
    pub kernel: String,
    pub hardware: String,
    pub cpu: String,
    pub memory_gb: u32,
    pub gpu: String,
    pub first_seen: String,
    /// where its applications and settings are captured, relative to the repository
    pub profile: String,
}

fn read(p: &str) -> String {
    std::fs::read_to_string(p).unwrap_or_default().trim().to_string()
}

fn run(cmd: &str, args: &[&str]) -> String {
    std::process::Command::new(cmd).args(args).output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default()
}

pub fn os_release(text: &str) -> (String, String) {
    let get = |k: &str| text.lines().find_map(|l| l.strip_prefix(&format!("{k}="))).map(|v| v.trim_matches('"').to_string()).unwrap_or_default();
    (get("ID"), get("VERSION_ID"))
}

pub fn slug(s: &str) -> String {
    let s: String = s.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    s.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-")
}

/// Look at this computer (Linux for now; macOS and Windows get their own readers when those machines exist).
pub fn probe(today: &str) -> Machine {
    let (distro, version) = os_release(&read("/etc/os-release"));
    let cpu = read("/proc/cpuinfo").lines().find_map(|l| l.strip_prefix("model name").map(|r| r.trim_start_matches([' ', '\t', ':']).to_string())).unwrap_or_default();
    let mem_kb: u64 = read("/proc/meminfo").lines().find_map(|l| l.strip_prefix("MemTotal:")).and_then(|r| r.trim().split(' ').next()?.parse().ok()).unwrap_or(0);
    let hardware = format!("{} {}", read("/sys/class/dmi/id/sys_vendor"), read("/sys/class/dmi/id/product_name")).trim().to_string();
    let gpu = run("nvidia-smi", &["--query-gpu=name", "--format=csv,noheader"]).lines().next().unwrap_or("").to_string();
    let host = run("hostname", &[]);
    let mut h = Sha256::new();
    h.update(read("/etc/machine-id"));
    h.update(&hardware);
    h.update(&cpu);
    let fingerprint = format!("{:x}", h.finalize())[..12].to_string();
    let profile = format!("applications/linux/{distro}/{version}");
    Machine {
        name: slug(&host), fingerprint, os: "linux".into(), distro, version, arch: run("uname", &["-m"]), kernel: run("uname", &["-r"]),
        hardware, cpu, memory_gb: ((mem_kb as f64) / 1_048_576.0).round() as u32, gpu, first_seen: today.into(), profile,
    }
}

/// Write `machines/<name>/machine.toml` under `repo`, keeping the original first_seen. Returns the path.
pub fn write(repo: &Path, m: &Machine) -> Result<std::path::PathBuf> {
    let dir = repo.join("machines").join(&m.name);
    std::fs::create_dir_all(&dir)?;
    let file = dir.join("machine.toml");
    let mut m = m.clone();
    if let Some(old) = std::fs::read_to_string(&file).ok().and_then(|t| toml::from_str::<Machine>(&t).ok()) {
        m.first_seen = old.first_seen;
    }
    std::fs::write(&file, toml::to_string_pretty(&m)?)?;
    Ok(file)
}

pub fn list(repo: &Path) -> Vec<Machine> {
    let mut v: Vec<Machine> = std::fs::read_dir(repo.join("machines")).into_iter().flatten().flatten()
        .filter_map(|e| toml::from_str(&std::fs::read_to_string(e.path().join("machine.toml")).ok()?).ok()).collect();
    v.sort_by(|a, b| a.name.cmp(&b.name));
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_os_release_and_makes_safe_names() {
        assert_eq!(os_release("NAME=\"Ubuntu\"\nID=ubuntu\nVERSION_ID=\"26.10\"\n"), ("ubuntu".into(), "26.10".into()));
        assert_eq!(slug("Alice XPS_15!"), "alice-xps-15");
    }

    #[test]
    fn writing_keeps_the_first_seen_date_and_never_the_raw_machine_id() {
        let dir = std::env::temp_dir().join(format!("hestia-machine-{}", std::process::id()));
        let mut m = Machine { name: "box".into(), fingerprint: "abc".into(), first_seen: "2026-01-01".into(), ..Default::default() };
        write(&dir, &m).unwrap();
        m.first_seen = "2026-05-05".into();
        let file = write(&dir, &m).unwrap();
        let back: Machine = toml::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();
        assert_eq!(back.first_seen, "2026-01-01");
        assert_eq!(list(&dir).len(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
