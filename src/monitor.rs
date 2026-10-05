//! Lightweight system monitor (menu-bar style stats). Refreshes only the
//! subsystems it displays to keep CPU and memory use minimal.

use crate::util::{cmd, fmt::bytes};
use sysinfo::{Components, CpuRefreshKind, Disks, MemoryRefreshKind, Networks, RefreshKind, System};

pub struct Monitor {
    sys: System,
    disks: Disks,
    nets: Networks,
    comps: Components,
    battery_health: Option<String>,
}

#[derive(Default, Clone)]
pub struct Snapshot {
    pub cpu: f32,
    pub cores: Vec<f32>,
    pub load: [f64; 3],
    pub mem_used: u64,
    pub mem_total: u64,
    pub swap_used: u64,
    pub disk_free: u64,
    pub disk_total: u64,
    pub net_rx: u64,
    pub net_tx: u64,
    pub cpu_temp: Option<f32>,
    pub battery: Option<String>,
    pub battery_health: Option<String>,
    pub uptime_secs: u64,
}

impl Monitor {
    pub fn new() -> Self {
        let kind = RefreshKind::nothing()
            .with_cpu(CpuRefreshKind::nothing().with_cpu_usage())
            .with_memory(MemoryRefreshKind::everything());
        Self {
            sys: System::new_with_specifics(kind),
            disks: Disks::new_with_refreshed_list(),
            nets: Networks::new_with_refreshed_list(),
            comps: Components::new_with_refreshed_list(),
            battery_health: battery_health(),
        }
    }

    /// Call at most once per second; network rates are per-refresh deltas.
    pub fn sample(&mut self, with_battery: bool) -> Snapshot {
        self.sys.refresh_cpu_usage();
        self.sys.refresh_memory();
        self.disks.refresh(true);
        self.nets.refresh(true);
        self.comps.refresh(false);
        let root = self.disks.list().iter().find(|d| d.mount_point() == std::path::Path::new("/"));
        let load = System::load_average();
        Snapshot {
            cpu: self.sys.global_cpu_usage(),
            cores: self.sys.cpus().iter().map(|c| c.cpu_usage()).collect(),
            load: [load.one, load.five, load.fifteen],
            mem_used: self.sys.used_memory(),
            mem_total: self.sys.total_memory(),
            swap_used: self.sys.used_swap(),
            disk_free: root.map_or(0, |d| d.available_space()),
            disk_total: root.map_or(0, |d| d.total_space()),
            net_rx: self.nets.values().map(|n| n.received()).sum(),
            net_tx: self.nets.values().map(|n| n.transmitted()).sum(),
            cpu_temp: cpu_temperature(&self.comps),
            battery: if with_battery { battery_status() } else { None },
            battery_health: self.battery_health.clone(),
            uptime_secs: System::uptime(),
        }
    }
}

fn cpu_temperature(comps: &Components) -> Option<f32> {
    comps
        .iter()
        .filter(|c| {
            let l = c.label().to_ascii_lowercase();
            l.contains("cpu") || l.contains("package")
        })
        .filter_map(|c| c.temperature())
        .fold(None, |acc: Option<f32>, t| Some(acc.map_or(t, |a| a.max(t))))
}

/// e.g. "87%; charging; 0:45 remaining"
pub fn parse_pmset(out: &str) -> Option<String> {
    let line = out.lines().find(|l| l.contains('%'))?;
    let pct = line.find('%')?;
    let start = line[..pct].rfind(|c: char| !c.is_ascii_digit()).map_or(0, |i| i + 1);
    let text = line[start..].split(" present").next()?.trim();
    Some(text.trim_end_matches(';').to_string())
}

fn battery_status() -> Option<String> {
    parse_pmset(&cmd::run("/usr/bin/pmset", &["-g", "batt"]).ok()?)
}

fn ioreg_int(out: &str, key: &str) -> Option<u64> {
    let needle = format!("\"{key}\" = ");
    let line = out.lines().find(|l| l.trim_start().starts_with(&needle))?;
    line.split(" = ").nth(1)?.trim().parse().ok()
}

/// Cycle count and capacity vs design (read once; it barely changes).
pub fn parse_battery_health(out: &str) -> Option<String> {
    let cycles = ioreg_int(out, "CycleCount")?;
    let max = ioreg_int(out, "AppleRawMaxCapacity").or_else(|| ioreg_int(out, "MaxCapacity"))?;
    let design = ioreg_int(out, "DesignCapacity")?;
    let pct = if design > 0 { max * 100 / design } else { 0 };
    Some(format!("{cycles} cycles · {pct}% of design capacity"))
}

fn battery_health() -> Option<String> {
    parse_battery_health(&cmd::run("/usr/sbin/ioreg", &["-rn", "AppleSmartBattery"]).ok()?)
}

impl Snapshot {
    pub fn lines(&self) -> Vec<String> {
        let mut v = vec![
            format!("CPU      {:5.1}%   load {:.2} {:.2} {:.2}", self.cpu, self.load[0], self.load[1], self.load[2]),
            format!("Memory   {} / {}   swap {}", bytes(self.mem_used), bytes(self.mem_total), bytes(self.swap_used)),
            format!("Disk     {} free of {}", bytes(self.disk_free), bytes(self.disk_total)),
            format!("Network  ↓ {}/s  ↑ {}/s", bytes(self.net_rx), bytes(self.net_tx)),
        ];
        if let Some(t) = self.cpu_temp { v.push(format!("CPU temp {t:.0}°C")); }
        if let Some(b) = &self.battery { v.push(format!("Battery  {b}")); }
        if let Some(h) = &self.battery_health { v.push(format!("Health   {h}")); }
        v.push(format!("Uptime   {}h {}m", self.uptime_secs / 3600, self.uptime_secs % 3600 / 60));
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pmset_output() {
        let out = "Now drawing from 'AC Power'\n -InternalBattery-0 (id=123)\t87%; charging; 0:45 remaining present: true\n";
        assert_eq!(parse_pmset(out).unwrap(), "87%; charging; 0:45 remaining");
        assert!(parse_pmset("no battery").is_none());
    }

    #[test]
    fn parses_ioreg_battery() {
        let out = "  \"CycleCount\" = 312\n  \"AppleRawMaxCapacity\" = 4500\n  \"DesignCapacity\" = 5000\n";
        assert_eq!(parse_battery_health(out).unwrap(), "312 cycles · 90% of design capacity");
    }

    #[test]
    fn samples_without_panicking() {
        let mut m = Monitor::new();
        let s = m.sample(false);
        assert!(s.mem_total > 0);
        assert!(!s.lines().is_empty());
    }
}
