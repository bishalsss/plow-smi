//! Minimal discovery example for `is-gpu`.
//!
//! ```bash
//! cargo run -p is-gpu --example discover
//! ```

use is_gpu::GpuManager;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let mut mgr = GpuManager::discover();
    println!(
        "backends={} devices={}",
        mgr.backend_count(),
        mgr.all_devices().len()
    );

    mgr.refresh_all();
    for backend in mgr.backends() {
        println!(
            "=== {} ({} device(s)) ===",
            backend.vendor(),
            backend.device_count()
        );
        for (id, dev) in backend.devices().into_iter().enumerate() {
            println!(
                "  [{id}] {} uuid={} pci={} mem_total={:?} util={:?}% temp={:?}C power={:?}W",
                dev.model,
                dev.uuid,
                if dev.pci_bus_id.is_empty() {
                    "-"
                } else {
                    &dev.pci_bus_id
                },
                backend.memory_total(id),
                backend.utilization(id),
                backend.temperature(id),
                backend.power_usage(id),
            );
        }
    }
}
