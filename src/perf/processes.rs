//! Heavy consumers: processes using the most CPU and memory.

use crate::model::{Item, Target};
use crate::util::fmt::bytes;
use std::time::Duration;
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

pub const TOP_N: usize = 30;

pub fn scan() -> Vec<Item> {
    let kind = ProcessRefreshKind::nothing().with_cpu().with_memory();
    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, kind);
    std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL.max(Duration::from_millis(300)));
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, kind);
    let me = std::process::id();
    let mut procs: Vec<_> = sys
        .processes()
        .values()
        .filter(|p| p.pid().as_u32() != me && p.pid().as_u32() > 1)
        .map(|p| (p.pid().as_u32(), p.name().to_string_lossy().into_owned(), p.cpu_usage(), p.memory()))
        .collect();
    // Rank by a blend of CPU (%) and memory (GB) so both heavy kinds surface.
    procs.sort_by(|a, b| score(b.2, b.3).total_cmp(&score(a.2, a.3)));
    procs
        .into_iter()
        .take(TOP_N)
        .map(|(pid, name, cpu, mem)| Item {
            group: "Processes",
            label: name,
            detail: format!("pid {pid} · CPU {cpu:5.1}% · RAM {}", bytes(mem)),
            size: mem,
            selected: false,
            target: Target::Process { pid },
        })
        .collect()
}

fn score(cpu: f32, mem: u64) -> f64 {
    cpu as f64 + mem as f64 / 50_000_000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn score_weights_cpu_and_memory() {
        assert!(score(50.0, 0) > score(1.0, 100_000_000));
        assert!(score(0.0, 4_000_000_000) > score(10.0, 0));
    }

    #[test]
    fn lists_processes_without_self() {
        let items = scan();
        assert!(!items.is_empty());
        let me = std::process::id();
        assert!(items.iter().all(|i| i.target != Target::Process { pid: me }));
    }
}
