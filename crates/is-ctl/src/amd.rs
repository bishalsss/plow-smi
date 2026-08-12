//! AMD GPU control via `is-gpu` (runtime AMD SMI).

use anyhow::{Result, bail};
use colored::Colorize;
use is_gpu::{AmdBackend, GpuBackend};

use crate::{format_bytes, GpuListEntry, OutputFormat, PerfLevel};

fn init_backend() -> Result<AmdBackend> {
    AmdBackend::try_load().map_err(|e| {
        anyhow::anyhow!(
            "Failed to initialize AMD SMI via is-gpu.\n\
             Ensure AMD GPU drivers and ROCm are installed.\n\
             Hint: Run 'rocm-smi' to check driver status.\nError: {e}"
        )
    })
}

pub fn list_gpus(format: &OutputFormat) -> Result<()> {
    let mut backend = init_backend()?;
    backend.refresh();
    let count = backend.device_count() as u32;
    if count == 0 {
        bail!("No AMD GPUs detected");
    }

    let mut entries = Vec::new();
    for (i, device) in backend.devices().into_iter().enumerate() {
        let m = backend.snapshot(i).unwrap_or_default();
        entries.push(GpuListEntry {
            index: i as u32,
            name: device.model,
            uuid: device.uuid,
            temperature_c: m.temperature.map(|t| t as i64).unwrap_or(0),
            power_w: m.power_usage.map(|w| w as u64).unwrap_or(0),
            power_limit_w: m.power_limit.map(|w| w as u64).unwrap_or(0),
            memory_used: m.memory_used.map(format_bytes).unwrap_or_else(|| "N/A".into()),
            memory_total: m
                .memory_total
                .map(format_bytes)
                .unwrap_or_else(|| "N/A".into()),
        });
    }

    match format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&entries)?),
        OutputFormat::Text => {
            println!("{}", format!("AMD GPUs detected: {count}").green());
            println!("{}", "─".repeat(80));
            println!(
                "{:>3} {:30} {:>6} {:>12} {:>16}",
                "ID".bold(),
                "Name".bold(),
                "Temp".bold(),
                "Power".bold(),
                "Memory".bold()
            );
            println!("{}", "─".repeat(80));
            for e in &entries {
                println!(
                    "{:>3} {:30} {:>4}°C {:>5}/{:<5}W {:>16}",
                    e.index.to_string().cyan(),
                    e.name,
                    e.temperature_c,
                    e.power_w,
                    e.power_limit_w,
                    format!("{}/{}", e.memory_used, e.memory_total),
                );
            }
        }
    }
    Ok(())
}

pub fn gpu_info(index: u32, format: &OutputFormat) -> Result<()> {
    let mut backend = init_backend()?;
    backend.refresh();
    if index as usize >= backend.device_count() {
        bail!(
            "AMD GPU {index} not found (detected {} devices)",
            backend.device_count()
        );
    }
    let device = backend.devices().into_iter().nth(index as usize).unwrap();
    let m = backend.snapshot(index as usize).unwrap_or_default();
    let perf_str = match backend.perf_level(index) {
        Some(0) => "auto",
        Some(1) => "low",
        Some(2) => "high",
        Some(3) => "manual",
        _ => "unknown",
    };

    match format {
        OutputFormat::Json => {
            let info = serde_json::json!({
                "index": index,
                "name": device.model,
                "uuid": device.uuid,
                "temperature_c": m.temperature,
                "power_w": m.power_usage,
                "power_limit_w": m.power_limit,
                "memory_used_bytes": m.memory_used,
                "memory_total_bytes": m.memory_total,
                "clock_graphics_mhz": m.clock_graphics,
                "clock_memory_mhz": m.clock_memory,
                "fan_speed_rpm": m.fan_speed,
                "perf_level": perf_str,
            });
            println!("{}", serde_json::to_string_pretty(&info)?);
        }
        OutputFormat::Text => {
            println!(
                "{}",
                format!("AMD GPU {index}: {}", device.model).cyan().bold()
            );
            println!("{}", "─".repeat(50));
            println!("  UUID:          {}", device.uuid);
            println!("  Temperature:   {}°C", m.temperature.unwrap_or(0.0));
            println!(
                "  Power:         {:.0} / {:.0} W",
                m.power_usage.unwrap_or(0.0),
                m.power_limit.unwrap_or(0.0)
            );
            if let (Some(used), Some(total)) = (m.memory_used, m.memory_total) {
                println!(
                    "  Memory:        {} / {}",
                    format_bytes(used),
                    format_bytes(total)
                );
            }
            println!("  Core Clock:    {} MHz", m.clock_graphics.unwrap_or(0));
            println!("  Memory Clock:  {} MHz", m.clock_memory.unwrap_or(0));
            if let Some(f) = m.fan_speed {
                println!("  Fan:           {f} RPM");
            }
            println!("  Perf Level:    {perf_str}");
        }
    }
    Ok(())
}

pub fn set_power_limit(index: u32, watts: u32) -> Result<()> {
    let backend = init_backend()?;
    if index as usize >= backend.device_count() {
        bail!(
            "AMD GPU {index} not found (detected {} devices)",
            backend.device_count()
        );
    }
    let milliwatts = watts as u64 * 1000;
    backend.set_power_limit(index, milliwatts).map_err(|e| {
        anyhow::anyhow!(
            "Failed to set power limit for AMD GPU {index}: {e}\n\
             Do you have root/sudo permissions?"
        )
    })?;
    println!(
        "{}",
        format!("✓ AMD GPU {index}: Power limit set to {watts}W").green()
    );
    Ok(())
}

pub fn set_perf(index: u32, level: &PerfLevel) -> Result<()> {
    let backend = init_backend()?;
    if index as usize >= backend.device_count() {
        bail!(
            "AMD GPU {index} not found (detected {} devices)",
            backend.device_count()
        );
    }
    let level_str = match level {
        PerfLevel::Auto => "auto",
        PerfLevel::Low => "low",
        PerfLevel::High => "high",
    };
    backend.set_perf_level(index, level_str).map_err(|e| {
        anyhow::anyhow!(
            "Failed to set performance level for AMD GPU {index}: {e}\n\
             Do you have root/sudo permissions?"
        )
    })?;
    println!(
        "{}",
        format!(
            "✓ AMD GPU {index}: Performance set to {}",
            level_str.to_uppercase()
        )
        .green()
    );
    Ok(())
}

pub fn set_perf_all(level: &PerfLevel) -> Result<()> {
    let backend = init_backend()?;
    for i in 0..backend.device_count() as u32 {
        if let Err(e) = set_perf(i, level) {
            eprintln!("{}", format!("✗ AMD GPU {i}: {e}").red());
        }
    }
    Ok(())
}

pub fn reset_clocks(index: u32) -> Result<()> {
    let backend = init_backend()?;
    if index as usize >= backend.device_count() {
        bail!(
            "AMD GPU {index} not found (detected {} devices)",
            backend.device_count()
        );
    }
    backend.set_perf_level(index, "auto").map_err(|e| {
        anyhow::anyhow!(
            "Failed to reset AMD GPU {index}: {e}. Do you have root/sudo permissions?"
        )
    })?;
    println!(
        "{}",
        format!("✓ AMD GPU {index}: Reset to defaults (auto perf level)").green()
    );
    Ok(())
}

pub fn reset_all() -> Result<()> {
    let backend = init_backend()?;
    for i in 0..backend.device_count() as u32 {
        if let Err(e) = reset_clocks(i) {
            eprintln!("{}", format!("✗ AMD GPU {i}: {e}").red());
        }
    }
    println!("{}", "✓ All AMD GPUs reset to defaults".green());
    Ok(())
}
