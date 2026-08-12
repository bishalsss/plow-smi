//! Unified GPU metrics trait and snapshot helper.

use crate::device::{GpuDevice, Vendor};
use crate::process::GpuProcessInfo;

/// Aggregated live metrics for a single device at a point in time.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DeviceMetrics {
    /// GPU compute utilization in percent (0–100).
    pub utilization: Option<f32>,
    /// Memory controller utilization in percent (0–100), when available.
    pub memory_utilization: Option<f32>,
    /// Memory used in bytes.
    pub memory_used: Option<u64>,
    /// Memory total in bytes.
    pub memory_total: Option<u64>,
    /// Temperature in Celsius.
    pub temperature: Option<f32>,
    /// Instantaneous power draw in watts.
    pub power_usage: Option<f32>,
    /// Enforced / configured power limit in watts.
    pub power_limit: Option<f32>,
    /// Fan speed (percent for NVML, RPM for AMD when available).
    pub fan_speed: Option<f32>,
    /// Graphics / core clock in MHz.
    pub clock_graphics: Option<u32>,
    /// Memory clock in MHz.
    pub clock_memory: Option<u32>,
}

/// Vendor-agnostic GPU telemetry backend.
///
/// Implementations load vendor libraries at runtime and must never panic because
/// a library or symbol is absent — construction fails with [`crate::GpuError`]
/// instead, and [`crate::GpuManager`] skips failed probes.
pub trait GpuBackend: Send + Sync {
    /// Vendor identity for this backend.
    fn vendor(&self) -> Vendor;

    /// Number of devices currently tracked.
    fn device_count(&self) -> usize;

    /// Static identity for every tracked device.
    fn devices(&self) -> Vec<GpuDevice>;

    /// Refresh cached live metrics from the vendor API.
    fn refresh(&mut self);

    /// GPU utilization percent (0–100).
    fn utilization(&self, id: usize) -> Option<f32>;

    /// Memory controller utilization percent (0–100).
    fn memory_utilization(&self, id: usize) -> Option<f32> {
        let _ = id;
        None
    }

    /// Bytes of device memory currently in use.
    fn memory_used(&self, id: usize) -> Option<u64>;

    /// Total device memory in bytes.
    fn memory_total(&self, id: usize) -> Option<u64>;

    /// Temperature in degrees Celsius.
    fn temperature(&self, id: usize) -> Option<f32>;

    /// Power draw in watts.
    fn power_usage(&self, id: usize) -> Option<f32>;

    /// Power limit in watts.
    fn power_limit(&self, id: usize) -> Option<f32> {
        let _ = id;
        None
    }

    /// Fan speed. Units are vendor-specific (percent vs RPM); see backend docs.
    fn fan_speed(&self, id: usize) -> Option<f32>;

    /// Graphics clock in MHz.
    fn clock_graphics(&self, id: usize) -> Option<u32>;

    /// Memory clock in MHz.
    fn clock_memory(&self, id: usize) -> Option<u32>;

    /// Driver version string when the vendor exposes one.
    fn driver_version(&self) -> Option<String> {
        None
    }

    /// CUDA driver version string (NVIDIA only).
    fn cuda_version(&self) -> Option<String> {
        None
    }

    /// Processes currently using devices on this backend (NVML today).
    fn collect_processes(&self) -> Vec<GpuProcessInfo> {
        Vec::new()
    }

    /// Collect all metric getters into one snapshot.
    fn snapshot(&self, id: usize) -> Option<DeviceMetrics> {
        if id >= self.device_count() {
            return None;
        }
        Some(DeviceMetrics {
            utilization: self.utilization(id),
            memory_utilization: self.memory_utilization(id),
            memory_used: self.memory_used(id),
            memory_total: self.memory_total(id),
            temperature: self.temperature(id),
            power_usage: self.power_usage(id),
            power_limit: self.power_limit(id),
            fan_speed: self.fan_speed(id),
            clock_graphics: self.clock_graphics(id),
            clock_memory: self.clock_memory(id),
        })
    }
}
