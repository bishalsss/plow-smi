//! AMD SMI FFI — pure-Rust `dlopen` of `libamd_smi.so`.
//!
//! Shared by the exporter (metrics) and the control CLI (power / perf).
//! No CXX bridge and no link-time ROCm dependency: the library is resolved at
//! runtime the same way the HSA backend loads `libhsa-runtime64.so`.
//!
//! # Usage
//! ```rust,ignore
//! use is_amd_ffi::{amd_smi_init, amd_smi_get_device_count, amd_smi_collect_device};
//!
//! let rc = amd_smi_init();
//! if rc == 0 {
//!     let count = amd_smi_get_device_count();
//!     for i in 0..count {
//!         let metrics = amd_smi_collect_device(i);
//!         println!("GPU {}: {} — {}°C", i, metrics.brand, metrics.temperature_celsius);
//!     }
//! }
//! ```

#[cfg(all(feature = "amd", target_arch = "x86_64"))]
mod amdsmi;

#[cfg(all(feature = "amd", target_arch = "x86_64"))]
pub use amdsmi::{
    amd_smi_collect_device, amd_smi_control_clk_level, amd_smi_get_device_count,
    amd_smi_get_perf_level, amd_smi_init, amd_smi_set_perf_level, amd_smi_set_power_limit,
    amd_smi_shutdown, AmdDeviceMetrics,
};

// ─── Stub implementations for non-AMD platforms ─────────────────────────────

#[cfg(not(all(feature = "amd", target_arch = "x86_64")))]
#[derive(Debug, Clone)]
pub struct AmdDeviceMetrics {
    pub uuid: String,
    pub brand: String,
    pub gpu_utilization_percent: i64,
    pub memory_utilization_percent: i64,
    pub memory_total_bytes: u64,
    pub memory_used_bytes: u64,
    pub power_usage_mw: u64,
    pub power_limit_mw: u64,
    pub clock_core_mhz: u32,
    pub clock_memory_mhz: u32,
    pub temperature_celsius: i64,
    pub fan_speed_rpm: u32,
}

#[cfg(not(all(feature = "amd", target_arch = "x86_64")))]
pub fn amd_smi_init() -> i32 {
    -1
}

#[cfg(not(all(feature = "amd", target_arch = "x86_64")))]
pub fn amd_smi_shutdown() {}

#[cfg(not(all(feature = "amd", target_arch = "x86_64")))]
pub fn amd_smi_get_device_count() -> u32 {
    0
}

#[cfg(not(all(feature = "amd", target_arch = "x86_64")))]
pub fn amd_smi_collect_device(_device_index: u32) -> AmdDeviceMetrics {
    AmdDeviceMetrics {
        uuid: String::new(),
        brand: String::new(),
        gpu_utilization_percent: -1,
        memory_utilization_percent: -1,
        memory_total_bytes: 0,
        memory_used_bytes: 0,
        power_usage_mw: 0,
        power_limit_mw: 0,
        clock_core_mhz: 0,
        clock_memory_mhz: 0,
        temperature_celsius: -999,
        fan_speed_rpm: 0,
    }
}

#[cfg(not(all(feature = "amd", target_arch = "x86_64")))]
pub fn amd_smi_set_perf_level(_device_index: u32, _level: &str) -> bool {
    false
}

#[cfg(not(all(feature = "amd", target_arch = "x86_64")))]
pub fn amd_smi_set_power_limit(_device_index: u32, _power_limit_mw: u64) -> i32 {
    -1
}

#[cfg(not(all(feature = "amd", target_arch = "x86_64")))]
pub fn amd_smi_get_perf_level(_device_index: u32) -> i32 {
    -1
}

#[cfg(not(all(feature = "amd", target_arch = "x86_64")))]
pub fn amd_smi_control_clk_level(_uuid: &str, _level: i32, _expiry_secs: u64) -> i32 {
    -1
}
