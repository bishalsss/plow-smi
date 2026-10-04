//! Read-only Apple Silicon inspection, reusing the shared GPU/SoC snapshots.

use crate::OutputFormat;
use anyhow::{bail, Result};
use plows_gpu::{AppleBackend, EngineMetrics, GpuBackend};

pub fn list_gpus(format: &OutputFormat) -> Result<()> {
    gpu_info(0, format)
}

pub fn gpu_info(index: u32, format: &OutputFormat) -> Result<()> {
    if index != 0 {
        bail!("Apple Silicon device {index} not found (only SoC index 0 is supported)");
    }
    let mut backend = AppleBackend::try_load()?;
    // One-shot inspection waits for the initial delta; continuous consumers never wait.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    backend.refresh();
    let device = backend.devices().remove(0);
    let m = backend.snapshot(0).unwrap_or_default();
    let soc = m.soc.unwrap_or_default();
    let engine = |e: &EngineMetrics| {
        serde_json::json!({
            "utilization_percent": e.utilization, "power_watts": e.power_watts,
            "clock_mhz": e.clock_mhz, "temperature_celsius": e.temperature_celsius, "cores": e.cores,
        })
    };
    let info = serde_json::json!({
        "index": 0, "vendor": "apple", "name": device.model, "uuid": device.uuid,
        "architecture": device.architecture, "read_only": true,
        "cpu": engine(&soc.cpu),
        "cpu_clusters": soc.cpu_clusters.iter().map(|c| serde_json::json!({"name": c.name, "metrics": engine(&c.metrics)})).collect::<Vec<_>>(),
        "gpu": {"utilization_percent": m.utilization, "power_watts": m.power_usage, "clock_mhz": m.clock_graphics, "temperature_celsius": m.temperature},
        "npu": engine(&soc.npu),
        "unified_memory": {"used_bytes": soc.memory_used_bytes, "total_bytes": soc.memory_total_bytes},
        "compute_power_watts": soc.compute_power_watts, "system_power_watts": soc.system_power_watts,
        "temperatures": soc.temperatures.iter().map(|t| serde_json::json!({"name": t.name, "celsius": t.celsius})).collect::<Vec<_>>(),
    });
    match format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&info)?),
        OutputFormat::Text => {
            println!("Apple Silicon: {} (read-only)", device.model);
            for name in ["cpu", "gpu", "npu"] {
                let e = &info[name];
                let display = |key: &str, unit: &str| {
                    if e[key].is_null() {
                        "N/A".into()
                    } else {
                        format!("{}{unit}", e[key])
                    }
                };
                println!(
                    "  {}: utilization {}, power {}, clock {}, temperature {}",
                    name.to_uppercase(),
                    display("utilization_percent", "%"),
                    display("power_watts", " W"),
                    display("clock_mhz", " MHz"),
                    display("temperature_celsius", " °C")
                );
            }
            println!(
                "  Unified memory: {} / {}",
                soc.memory_used_bytes
                    .map(crate::format_bytes)
                    .unwrap_or_else(|| "N/A".into()),
                soc.memory_total_bytes
                    .map(crate::format_bytes)
                    .unwrap_or_else(|| "N/A".into())
            );
        }
    }
    Ok(())
}
