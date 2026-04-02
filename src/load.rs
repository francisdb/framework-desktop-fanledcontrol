use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

pub fn generate_load(percent: u8, cores: Option<usize>, running: &'static AtomicBool) {
    let available = thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let num_cores = cores.unwrap_or(available).min(available);
    println!("Generating {percent}% load on {num_cores}/{available} cores");
    println!("Press Ctrl+C to stop");

    // Short cycle (10ms) so load averages out within each LED sample window
    let busy = Duration::from_micros(u64::from(percent) * 100);
    let idle = Duration::from_micros(u64::from(100 - percent) * 100);

    let handles: Vec<_> = (0..num_cores)
        .map(|_| {
            thread::spawn(move || {
                while running.load(Ordering::Relaxed) {
                    let start = Instant::now();
                    while start.elapsed() < busy && running.load(Ordering::Relaxed) {
                        std::hint::spin_loop();
                    }
                    if !idle.is_zero() {
                        thread::sleep(idle);
                    }
                }
            })
        })
        .collect();

    for h in handles {
        h.join().ok();
    }
}
