mod cpu;
mod ec;
mod load;

use clap::{Parser, Subcommand};
use std::fs::OpenOptions;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use cpu::{compute_usage, read_cpu_times};
use ec::{CROS_EC_DEV, NUM_LEDS, RgbS, load_to_color, print_color_bar, set_fan_colors};

static RUNNING: AtomicBool = AtomicBool::new(true);

const UPDATE_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Parser)]
#[command(about = "Framework Desktop fan LED controller")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Show LED colors based on CPU load
    Run {
        /// Dry-run mode (no EC commands sent, shows colored blocks in terminal)
        #[arg(long)]
        dry_run: bool,
    },
    /// Generate CPU load for testing
    Load {
        /// Target CPU load percentage (0-100)
        #[arg(value_parser = clap::value_parser!(u8).range(0..=100))]
        percent: u8,
        /// Number of cores to load (defaults to all)
        #[arg(long)]
        cores: Option<usize>,
    },
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

        // Group cores into LED buckets, show max load per bucket
        let num_cores = usage.len();
        let mut colors = [RgbS { r: 0, g: 0, b: 0 }; NUM_LEDS];
        for (i, color) in colors.iter_mut().enumerate() {
            let start = i * num_cores / NUM_LEDS;
            let end = (i + 1) * num_cores / NUM_LEDS;
            let max_load = usage[start..end].iter().copied().fold(0.0_f64, f64::max);
            *color = load_to_color(max_load);
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
    let cli = Cli::parse();
    match cli.command {
        None | Some(Command::Run { dry_run: false }) => run_loop(false),
        Some(Command::Run { dry_run: true }) => run_loop(true),
        Some(Command::Load { percent, cores }) => {
            ctrlc::set_handler(|| {
                RUNNING.store(false, Ordering::Relaxed);
            })
            .expect("Failed to set Ctrl+C handler");
            load::generate_load(percent, cores, &RUNNING);
            Ok(())
        }
    }
}
