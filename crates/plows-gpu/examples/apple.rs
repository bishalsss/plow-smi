//! One-shot Apple Silicon inspection and cached refresh microbenchmark.
use plows_gpu::{AppleBackend, GpuBackend};
use std::time::{Duration, Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut backend = AppleBackend::try_load()?;
    std::thread::sleep(Duration::from_millis(1200));
    backend.refresh();
    println!("{:#?}", backend.snapshot(0));
    let start = Instant::now();
    for _ in 0..100_000 {
        backend.refresh();
        std::hint::black_box(backend.power_usage(0));
    }
    println!(
        "cached refresh + scalar getter: {:?}/call",
        start.elapsed() / 100_000
    );
    let start = Instant::now();
    for _ in 0..10_000 {
        backend.refresh();
        std::hint::black_box(backend.snapshot(0));
    }
    println!(
        "cached refresh + snapshot: {:?}/call",
        start.elapsed() / 10_000
    );
    Ok(())
}
