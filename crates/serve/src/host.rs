//! How the machine is doing, for the GUI's Servers page: load, memory, disk
//! and network, read from `/proc` and the file system. Linux only; elsewhere
//! the numbers are zero.

use std::path::Path;

use zoomriya_proto::ServerStats;

/// The machine's numbers now. `data_dir` picks the disk to measure.
pub fn stats(name: &str, role: &str, version: &str, started: u64, data_dir: &Path) -> ServerStats {
    let (mem_total, mem_used) = memory(&read("/proc/meminfo"));
    let (disk_total, disk_used) = disk(data_dir);
    let (net_rx, net_tx) = network(&read("/proc/net/dev"));
    ServerStats {
        name: name.to_string(),
        role: role.to_string(),
        version: version.to_string(),
        started,
        uptime: first_number(&read("/proc/uptime")) as u64,
        load: first_number(&read("/proc/loadavg")) as f32,
        cpus: std::thread::available_parallelism()
            .map(|n| n.get() as u32)
            .unwrap_or(1),
        mem_total,
        mem_used,
        disk_total,
        disk_used,
        net_rx,
        net_tx,
        turn: None,
    }
}

fn read(path: &str) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

fn first_number(text: &str) -> f64 {
    text.split_whitespace()
        .next()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.0)
}

/// Total and in use, in bytes, from `/proc/meminfo`.
fn memory(meminfo: &str) -> (u64, u64) {
    let field = |name: &str| {
        meminfo
            .lines()
            .find_map(|l| l.strip_prefix(name))
            .and_then(|rest| rest.split_whitespace().next())
            .and_then(|kb| kb.parse::<u64>().ok())
            .map(|kb| kb * 1024)
            .unwrap_or(0)
    };
    let total = field("MemTotal:");
    let available = field("MemAvailable:");
    (total, total.saturating_sub(available))
}

/// Received and sent, in bytes, on every interface but loopback, from
/// `/proc/net/dev`.
fn network(dev: &str) -> (u64, u64) {
    let mut rx = 0u64;
    let mut tx = 0u64;
    for line in dev.lines().skip(2) {
        let Some((name, fields)) = line.split_once(':') else {
            continue;
        };
        if name.trim() == "lo" {
            continue;
        }
        let n: Vec<u64> = fields
            .split_whitespace()
            .map(|v| v.parse().unwrap_or(0))
            .collect();
        if n.len() >= 9 {
            rx = rx.saturating_add(n[0]);
            tx = tx.saturating_add(n[8]);
        }
    }
    (rx, tx)
}

/// Total and used, in bytes, of the file system holding `path`.
fn disk(path: &Path) -> (u64, u64) {
    match rustix::fs::statvfs(path) {
        Ok(v) => {
            let total = v.f_blocks.saturating_mul(v.f_frsize);
            let available = v.f_bavail.saturating_mul(v.f_frsize);
            (total, total.saturating_sub(available))
        }
        Err(_) => (0, 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_proc() {
        let meminfo = "MemTotal:        2000 kB\nMemFree:  100 kB\nMemAvailable:    500 kB\n";
        assert_eq!(memory(meminfo), (2000 * 1024, 1500 * 1024));
        let dev = "Inter-|   Receive\n face |bytes    packets\n    lo: 100 1 0 0 0 0 0 0 100 1 0 0 0 0 0 0\n  eth0: 5000 10 0 0 0 0 0 0 7000 12 0 0 0 0 0 0\n  eth1: 1 1 0 0 0 0 0 0 2 1 0 0 0 0 0 0\n";
        assert_eq!(network(dev), (5001, 7002));
        assert_eq!(first_number("0.52 0.40 0.30 1/200 999"), 0.52);
        let s = stats("x", "relay", "1", 0, Path::new("/"));
        assert!(s.cpus >= 1);
    }
}
