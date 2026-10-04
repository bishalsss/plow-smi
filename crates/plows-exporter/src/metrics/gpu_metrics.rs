//! GPU-specific Prometheus metrics (shared across AMD and NVIDIA).

use once_cell::sync::Lazy;
use prometheus::{GaugeVec, IntGaugeVec, Opts};

use super::registry::REGISTRY;
use std::sync::Mutex;

/// Labels applied to all GPU metrics.
const GPU_LABELS: &[&str] = &["gpu_index", "vendor", "hostname", "brand", "uuid"];

/// Helper to create and register an IntGaugeVec with standard GPU labels.
fn gpu_gauge(name: &str, help: &str) -> IntGaugeVec {
    let gauge = IntGaugeVec::new(Opts::new(name, help), GPU_LABELS)
        .unwrap_or_else(|e| panic!("Failed to create metric '{name}': {e}"));
    REGISTRY
        .register(Box::new(gauge.clone()))
        .unwrap_or_else(|e| panic!("Failed to register metric '{name}': {e}"));
    gauge
}

fn gpu_float_gauge(name: &str, help: &str) -> GaugeVec {
    let gauge = GaugeVec::new(Opts::new(name, help), GPU_LABELS).expect("valid GPU metric");
    REGISTRY
        .register(Box::new(gauge.clone()))
        .expect("unique GPU metric");
    gauge
}

/// All GPU-related Prometheus metrics.
pub struct GpuMetrics {
    pub gpu_utilization_percent: IntGaugeVec,
    pub memory_utilization_percent: IntGaugeVec,
    pub memory_total_bytes: IntGaugeVec,
    pub memory_used_bytes: IntGaugeVec,
    pub memory_free_bytes: IntGaugeVec,
    pub power_usage_watts: GaugeVec,
    pub power_limit_watts: GaugeVec,
    pub clock_core_mhz: IntGaugeVec,
    pub clock_memory_mhz: IntGaugeVec,
    pub temperature_celsius: IntGaugeVec,
    pub fan_speed: IntGaugeVec,
}

impl GpuMetrics {
    fn new() -> Self {
        Self {
            gpu_utilization_percent: gpu_gauge(
                "gpu_utilization_percent",
                "GPU core utilization percentage (0-100)",
            ),
            memory_utilization_percent: gpu_gauge(
                "gpu_memory_utilization_percent",
                "GPU memory controller utilization percentage (0-100)",
            ),
            memory_total_bytes: gpu_gauge("gpu_memory_total_bytes", "Total GPU memory in bytes"),
            memory_used_bytes: gpu_gauge("gpu_memory_used_bytes", "Used GPU memory in bytes"),
            memory_free_bytes: gpu_gauge("gpu_memory_free_bytes", "Free GPU memory in bytes"),
            power_usage_watts: gpu_float_gauge(
                "gpu_power_usage_watts",
                "Current GPU power draw in watts",
            ),
            power_limit_watts: gpu_float_gauge("gpu_power_limit_watts", "GPU power limit in watts"),
            clock_core_mhz: gpu_gauge("gpu_clock_core_mhz", "GPU core/graphics clock speed in MHz"),
            clock_memory_mhz: gpu_gauge("gpu_clock_memory_mhz", "GPU memory clock speed in MHz"),
            temperature_celsius: gpu_gauge(
                "gpu_temperature_celsius",
                "GPU temperature in degrees Celsius",
            ),
            fan_speed: gpu_gauge(
                "gpu_fan_speed",
                "GPU fan speed (percentage for NVIDIA, RPM for AMD)",
            ),
        }
    }
}

/// Global GPU metrics instance.
pub static GPU_METRICS: Lazy<GpuMetrics> = Lazy::new(GpuMetrics::new);
static PREVIOUS_LABELS: Mutex<Vec<[String; 5]>> = Mutex::new(Vec::new());

/// Update all GPU Prometheus gauges from a collection of snapshots.
pub fn update_gpu_metrics(snapshots: &[crate::collector::GpuSnapshot]) {
    super::soc_metrics::update_soc_metrics(snapshots);
    let mut previous = PREVIOUS_LABELS.lock().unwrap_or_else(|e| e.into_inner());
    let integer_gauges = [
        &GPU_METRICS.gpu_utilization_percent,
        &GPU_METRICS.memory_utilization_percent,
        &GPU_METRICS.memory_total_bytes,
        &GPU_METRICS.memory_used_bytes,
        &GPU_METRICS.memory_free_bytes,
        &GPU_METRICS.clock_core_mhz,
        &GPU_METRICS.clock_memory_mhz,
        &GPU_METRICS.temperature_celsius,
        &GPU_METRICS.fan_speed,
    ];
    // Preserve registered children on normal updates; only remove missing devices/values.
    for old in previous.iter() {
        if !snapshots.iter().any(|s| {
            s.index.to_string() == old[0]
                && s.vendor == old[1]
                && s.hostname == old[2]
                && s.brand == old[3]
                && s.uuid == old[4]
        }) {
            let labels: [&str; 5] = std::array::from_fn(|i| old[i].as_str());
            for gauge in &integer_gauges {
                let _ = gauge.remove_label_values(&labels);
            }
            let _ = GPU_METRICS.power_usage_watts.remove_label_values(&labels);
            let _ = GPU_METRICS.power_limit_watts.remove_label_values(&labels);
        }
    }
    previous.clear();
    for snap in snapshots {
        let index = snap.index.to_string();
        let labels = [
            &*index,
            snap.vendor,
            &snap.hostname,
            &snap.brand,
            &snap.uuid,
        ];
        let values = [
            snap.gpu_utilization_percent,
            snap.memory_utilization_percent,
            snap.memory_total_bytes.and_then(|v| i64::try_from(v).ok()),
            snap.memory_used_bytes.and_then(|v| i64::try_from(v).ok()),
            snap.memory_free_bytes.and_then(|v| i64::try_from(v).ok()),
            snap.clock_core_mhz.map(i64::from),
            snap.clock_memory_mhz.map(i64::from),
            snap.temperature_celsius,
            snap.fan_speed.map(i64::from),
        ];
        for (gauge, value) in integer_gauges.iter().zip(values) {
            if let Some(value) = value {
                gauge.with_label_values(&labels).set(value);
            } else {
                let _ = gauge.remove_label_values(&labels);
            }
        }
        for (gauge, value) in [
            (&GPU_METRICS.power_usage_watts, snap.power_usage_mw),
            (&GPU_METRICS.power_limit_watts, snap.power_limit_mw),
        ] {
            if let Some(value) = value {
                gauge.with_label_values(&labels).set(value as f64 / 1000.0);
            } else {
                let _ = gauge.remove_label_values(&labels);
            }
        }
        previous.push([
            index,
            snap.vendor.into(),
            snap.hostname.clone(),
            snap.brand.clone(),
            snap.uuid.clone(),
        ]);
    }
}
