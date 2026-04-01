use std::env;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::unix::io::AsRawFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

static RUNNING: AtomicBool = AtomicBool::new(true);

const CROS_EC_DEV: &str = "/dev/cros_ec";
const EC_CMD_RGBKBD_SET_COLOR: u32 = 0x013A;
const NUM_LEDS: usize = 8;
const UPDATE_INTERVAL: Duration = Duration::from_millis(500);

// ioctl definition: magic 0xEC, command 0, read/write CrosEcCommandV2
// The kernel struct uses a flexible array member (data[]) which has size 0,
// so the ioctl number must encode only the 5×u32 header size (20 bytes),
// not our Rust struct's full size which includes the data buffer.
nix::ioctl_readwrite_bad!(cros_ec_cmd, nix::request_code_readwrite!(0xEC, 0, 20), CrosEcCommandV2);

#[repr(C)]
struct CrosEcCommandV2 {
    version: u32,
    command: u32,
    outsize: u32,
    insize: u32,
    result: u32,
    data: [u8; 256],
}

#[repr(C, packed)]
#[derive(Copy, Clone, Debug, PartialEq)]
struct RgbS {
    r: u8,
    g: u8,
    b: u8,
}

impl std::fmt::Display for RgbS {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }
}

fn set_fan_colors(dev: &File, colors: &[RgbS; NUM_LEDS]) -> io::Result<()> {
    let mut cmd = CrosEcCommandV2 {
        version: 0,
        command: EC_CMD_RGBKBD_SET_COLOR,
        outsize: (2 + NUM_LEDS * 3) as u32, // start_key + length + color data
        insize: 0,
        result: 0,
        data: [0u8; 256],
    };

    // Pack the request: start_key=0, length=NUM_LEDS, then RGB triplets
    cmd.data[0] = 0; // start_key
    cmd.data[1] = NUM_LEDS as u8; // length
    for (i, color) in colors.iter().enumerate() {
        cmd.data[2 + i * 3] = color.r;
        cmd.data[2 + i * 3 + 1] = color.g;
        cmd.data[2 + i * 3 + 2] = color.b;
    }

    // Safety: we're passing a properly initialized struct to the kernel ioctl
    unsafe {
        cros_ec_cmd(dev.as_raw_fd(), &mut cmd)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    }

    if cmd.result != 0 {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("EC returned error: {}", cmd.result),
        ));
    }

    Ok(())
}

/// Read per-CPU jiffies from /proc/stat.
/// Returns Vec of (total_jiffies, idle_jiffies) per core.
fn read_cpu_times() -> io::Result<Vec<(u64, u64)>> {
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
fn compute_usage(prev: &[(u64, u64)], curr: &[(u64, u64)]) -> Vec<f64> {
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

/// Map a load value (0.0 - 1.0) to a color: blue → red.
fn load_to_color(load: f64) -> RgbS {
    let load = load.clamp(0.0, 1.0);
    RgbS {
        r: (load * 255.0) as u8,
        g: 0,
        b: ((1.0 - load) * 255.0) as u8,
    }
}

fn print_color_bar(colors: &[RgbS; NUM_LEDS], avg_load: f64) {
    // Print colored blocks using ANSI true color escape codes
    print!("\r  ");
    for color in colors {
        print!("\x1b[48;2;{};{};{}m   \x1b[0m", color.r, color.g, color.b);
    }
    print!("  avg: {:.0}%  ", avg_load * 100.0);
    use std::io::Write;
    std::io::stdout().flush().ok();
}

fn run_loop(dry_run: bool) -> io::Result<()> {
    let dev = if dry_run {
        None
    } else {
        Some(OpenOptions::new().read(true).write(true).open(CROS_EC_DEV).map_err(|e| {
            eprintln!(
                "Failed to open {CROS_EC_DEV}: {e}\nMake sure you have permission (try running with sudo)."
            );
            e
        })?)
    };

    ctrlc::set_handler(|| {
        RUNNING.store(false, Ordering::Relaxed);
    })
    .expect("Failed to set Ctrl+C handler");

    println!("Framework Desktop fan LED controller started");
    println!("Blue = low load, Red = high load");
    if dry_run {
        println!("Running in dry-run mode (no EC commands sent)");
    }
    println!("Press Ctrl+C to stop");

    let mut prev_times = read_cpu_times()?;
    thread::sleep(UPDATE_INTERVAL);

    while RUNNING.load(Ordering::Relaxed) {
        let curr_times = read_cpu_times()?;
        let usage = compute_usage(&prev_times, &curr_times);
        prev_times = curr_times;

        if usage.is_empty() {
            thread::sleep(UPDATE_INTERVAL);
            continue;
        }

        // Overall average load
        let avg_load: f64 = usage.iter().sum::<f64>() / usage.len() as f64;

        // Switch to per-core view when any core is pegged (>= 98%)
        let any_core_maxed = usage.iter().any(|&u| u >= 0.98);

        let mut colors = [load_to_color(avg_load); NUM_LEDS];

        if any_core_maxed {
            // Distribute per-core loads across LEDs for visual feedback.
            // If more cores than LEDs, show the hottest cores.
            // If fewer cores than LEDs, spread them out.
            let num_cores = usage.len();
            for i in 0..NUM_LEDS {
                let core_idx = i * num_cores / NUM_LEDS;
                colors[i] = load_to_color(usage[core_idx]);
            }
        }

        if dry_run {
            print_color_bar(&colors, avg_load);
        } else if let Err(e) = set_fan_colors(dev.as_ref().unwrap(), &colors) {
            eprintln!("Failed to set LED colors: {e}");
        }

        thread::sleep(UPDATE_INTERVAL);
    }

    // Turn off LEDs on shutdown
    println!("\nShutting down, turning off LEDs...");
    let off = [RgbS { r: 0, g: 0, b: 0 }; NUM_LEDS];
    if dry_run {
        print_color_bar(&off, 0.0);
        println!();
    } else if let Err(e) = set_fan_colors(dev.as_ref().unwrap(), &off) {
        eprintln!("Failed to turn off LEDs: {e}");
    }

    Ok(())
}

fn main() -> io::Result<()> {
    let dry_run = env::args().any(|a| a == "--dry-run");
    run_loop(dry_run)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_to_color_zero() {
        let c = load_to_color(0.0);
        assert_eq!(c, RgbS { r: 0, g: 0, b: 255 });
    }

    #[test]
    fn test_load_to_color_full() {
        let c = load_to_color(1.0);
        assert_eq!(c, RgbS { r: 255, g: 0, b: 0 });
    }

    #[test]
    fn test_load_to_color_mid() {
        let c = load_to_color(0.5);
        assert_eq!(c, RgbS { r: 127, g: 0, b: 127 });
    }

    #[test]
    fn test_load_to_color_clamps() {
        let low = load_to_color(-0.5);
        assert_eq!(low, RgbS { r: 0, g: 0, b: 255 });
        let high = load_to_color(1.5);
        assert_eq!(high, RgbS { r: 255, g: 0, b: 0 });
    }

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
