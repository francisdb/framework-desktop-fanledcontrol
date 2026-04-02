use std::fs;
use std::io;

/// Read per-CPU jiffies from /proc/stat.
/// Returns Vec of (total_jiffies, idle_jiffies) per core.
pub fn read_cpu_times() -> io::Result<Vec<(u64, u64)>> {
    let stat = fs::read_to_string("/proc/stat")?;
    Ok(parse_cpu_times(&stat))
}

/// Parse per-CPU jiffies from /proc/stat content.
/// Returns Vec of (total_jiffies, idle_jiffies) per core.
fn parse_cpu_times(stat: &str) -> Vec<(u64, u64)> {
    let mut cores = Vec::new();
    for line in stat.lines() {
        // Skip the aggregate "cpu " line, only read per-core "cpu0", "cpu1", etc.
        if line.starts_with("cpu") && !line.starts_with("cpu ") {
            let fields: Vec<u64> = line
                .split_whitespace()
                .skip(1) // skip "cpuN"
                .filter_map(|s| s.parse().ok())
                .collect();
            if fields.len() >= 5 {
                let total: u64 = fields.iter().sum();
                let idle = fields[3] + fields[4]; // idle + iowait
                cores.push((total, idle));
            }
        }
    }
    cores
}

/// Compute per-core usage (0.0 - 1.0) from two snapshots.
pub fn compute_usage(prev: &[(u64, u64)], curr: &[(u64, u64)]) -> Vec<f64> {
    prev.iter()
        .zip(curr.iter())
        .map(|(&(pt, pi), &(ct, ci))| {
            let dt = ct.saturating_sub(pt);
            let di = ci.saturating_sub(pi);
            if dt == 0 {
                0.0
            } else {
                1.0 - (di as f64 / dt as f64)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_usage_idle() {
        let prev = vec![(1000, 900)];
        let curr = vec![(2000, 1900)];
        let usage = compute_usage(&prev, &curr);
        assert!((usage[0] - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_compute_usage_full() {
        let prev = vec![(1000, 500)];
        let curr = vec![(2000, 500)]; // no idle time added
        let usage = compute_usage(&prev, &curr);
        assert!((usage[0] - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_compute_usage_half() {
        let prev = vec![(1000, 500)];
        let curr = vec![(2000, 1000)]; // half the delta was idle
        let usage = compute_usage(&prev, &curr);
        assert!((usage[0] - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_compute_usage_zero_delta() {
        let prev = vec![(1000, 500)];
        let curr = vec![(1000, 500)];
        let usage = compute_usage(&prev, &curr);
        assert_eq!(usage[0], 0.0);
    }

    #[test]
    fn test_parse_cpu_times_includes_iowait_as_idle() {
        // Fields: user nice system idle iowait irq softirq steal guest guest_nice
        let stat = "\
cpu  100 200 300 400 500 600 700 800 900 1000
cpu0 10 20 30 400 500 60 70 80 90 100
cpu1 50 50 50 200 300 50 50 50 50 50
";
        let cores = parse_cpu_times(stat);
        assert_eq!(cores.len(), 2);
        // cpu0: total = sum of all fields, idle = 400 + 500 = 900
        let (total0, idle0) = cores[0];
        assert_eq!(total0, 10 + 20 + 30 + 400 + 500 + 60 + 70 + 80 + 90 + 100);
        assert_eq!(idle0, 400 + 500);
        // cpu1: idle = 200 + 300 = 500
        let (total1, idle1) = cores[1];
        assert_eq!(total1, 50 + 50 + 50 + 200 + 300 + 50 + 50 + 50 + 50 + 50);
        assert_eq!(idle1, 200 + 300);
    }

    #[test]
    fn test_parse_cpu_times_skips_aggregate_line() {
        let stat = "\
cpu  100 200 300 400 500 600 700 800 900 1000
";
        let cores = parse_cpu_times(stat);
        assert!(cores.is_empty());
    }
}
