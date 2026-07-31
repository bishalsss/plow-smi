//! Pure-Rust AMD SMI telemetry via `dlopen` of `libamd_smi.so`.
//!
//! Same loading discipline as the HSA backend: resolve symbols at runtime,
//! fail gracefully if the library is absent. No CXX / link-time ROCm dependency.

use std::ffi::c_void;
use std::sync::Mutex;

use libloading::Library;

// ─── ABI constants (from amdsmi.h) ───────────────────────────────────────────

const AMDSMI_STATUS_SUCCESS: i32 = 0;
const AMDSMI_INIT_AMD_GPUS: u64 = 1 << 1;
const AMDSMI_PROCESSOR_TYPE_AMD_GPU: u32 = 1;
const AMDSMI_MEM_TYPE_VRAM: u32 = 0;
const AMDSMI_CLK_TYPE_SYS: u32 = 0;
const AMDSMI_CLK_TYPE_DF: u32 = 1;
const AMDSMI_TEMPERATURE_TYPE_EDGE: u32 = 0;
const AMDSMI_TEMPERATURE_TYPE_JUNCTION: u32 = 1;
const AMDSMI_TEMPERATURE_TYPE_VRAM: u32 = 2;
const AMDSMI_TEMP_CURRENT: u32 = 0;
const AMDSMI_MAX_STRING_LENGTH: usize = 256;
const AMDSMI_MAX_NUM_FREQUENCIES: usize = 33;
const AMDSMI_DEV_PERF_LEVEL_AUTO: u32 = 0;
const AMDSMI_DEV_PERF_LEVEL_LOW: u32 = 1;
const AMDSMI_DEV_PERF_LEVEL_HIGH: u32 = 2;
const AMDSMI_DEV_PERF_LEVEL_MANUAL: u32 = 3;

type AmdsmiStatus = i32;
type ProcessorHandle = *mut c_void;
type SocketHandle = *mut c_void;

// ─── ABI structs ─────────────────────────────────────────────────────────────

#[repr(C)]
#[derive(Clone, Copy)]
struct AmdsmiPowerInfo {
    socket_power: u64,
    current_socket_power: u32,
    average_socket_power: u32,
    gfx_voltage: u64,
    soc_voltage: u64,
    mem_voltage: u64,
    power_limit: u32,
    _pad: u32,
    reserved: [u64; 18],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct AmdsmiFrequencies {
    has_deep_sleep: u8,
    _pad0: [u8; 3],
    num_supported: u32,
    current: u32,
    _pad1: u32,
    frequency: [u64; AMDSMI_MAX_NUM_FREQUENCIES],
}

#[repr(C)]
struct AmdsmiBoardInfo {
    model_number: [u8; AMDSMI_MAX_STRING_LENGTH],
    product_serial: [u8; AMDSMI_MAX_STRING_LENGTH],
    fru_id: [u8; AMDSMI_MAX_STRING_LENGTH],
    product_name: [u8; AMDSMI_MAX_STRING_LENGTH],
    manufacturer_name: [u8; AMDSMI_MAX_STRING_LENGTH],
    reserved: [u64; 64],
}

// ─── Resolved driver ─────────────────────────────────────────────────────────

struct AmdSmiDriver {
    amdsmi_init: unsafe extern "C" fn(u64) -> AmdsmiStatus,
    amdsmi_shut_down: unsafe extern "C" fn() -> AmdsmiStatus,
    amdsmi_get_socket_handles:
        unsafe extern "C" fn(*mut u32, *mut SocketHandle) -> AmdsmiStatus,
    amdsmi_get_processor_handles:
        unsafe extern "C" fn(SocketHandle, *mut u32, *mut ProcessorHandle) -> AmdsmiStatus,
    amdsmi_get_processor_type:
        unsafe extern "C" fn(ProcessorHandle, *mut u32) -> AmdsmiStatus,
    amdsmi_get_gpu_busy_percent:
        unsafe extern "C" fn(ProcessorHandle, *mut u32) -> AmdsmiStatus,
    amdsmi_get_gpu_memory_total:
        unsafe extern "C" fn(ProcessorHandle, u32, *mut u64) -> AmdsmiStatus,
    amdsmi_get_gpu_memory_usage:
        unsafe extern "C" fn(ProcessorHandle, u32, *mut u64) -> AmdsmiStatus,
    amdsmi_get_power_info:
        unsafe extern "C" fn(ProcessorHandle, *mut AmdsmiPowerInfo) -> AmdsmiStatus,
    amdsmi_get_clk_freq:
        unsafe extern "C" fn(ProcessorHandle, u32, *mut AmdsmiFrequencies) -> AmdsmiStatus,
    amdsmi_get_temp_metric:
        unsafe extern "C" fn(ProcessorHandle, u32, u32, *mut i64) -> AmdsmiStatus,
    amdsmi_get_gpu_fan_rpms:
        unsafe extern "C" fn(ProcessorHandle, u32, *mut i64) -> AmdsmiStatus,
    amdsmi_get_gpu_device_uuid:
        unsafe extern "C" fn(ProcessorHandle, *mut u32, *mut u8) -> AmdsmiStatus,
    amdsmi_get_gpu_board_info:
        unsafe extern "C" fn(ProcessorHandle, *mut AmdsmiBoardInfo) -> AmdsmiStatus,
    amdsmi_get_gpu_perf_level:
        unsafe extern "C" fn(ProcessorHandle, *mut u32) -> AmdsmiStatus,
    amdsmi_set_gpu_perf_level:
        unsafe extern "C" fn(ProcessorHandle, u32) -> AmdsmiStatus,
    amdsmi_set_power_cap:
        unsafe extern "C" fn(ProcessorHandle, u32, u64) -> AmdsmiStatus,
    /// Keep the library mapped for the lifetime of the function pointers.
    _lib: Library,
}

impl AmdSmiDriver {
    fn open() -> Result<Self, String> {
        let lib = unsafe {
            Library::new("libamd_smi.so")
                .or_else(|_| Library::new("libamd_smi.so.1"))
                .or_else(|_| Library::new("/opt/rocm/lib/libamd_smi.so"))
                .or_else(|_| Library::new("/opt/rocm/lib/libamd_smi.so.1"))
        }
        .map_err(|e| format!("dlopen libamd_smi: {e}"))?;

        macro_rules! resolve {
            ($lib:expr, $name:expr) => {{
                *unsafe { $lib.get($name) }.map_err(|e| {
                    format!(
                        "resolve {}: {e}",
                        std::str::from_utf8($name).unwrap_or("?")
                    )
                })?
            }};
        }

        Ok(Self {
            amdsmi_init: resolve!(lib, b"amdsmi_init\0"),
            amdsmi_shut_down: resolve!(lib, b"amdsmi_shut_down\0"),
            amdsmi_get_socket_handles: resolve!(lib, b"amdsmi_get_socket_handles\0"),
            amdsmi_get_processor_handles: resolve!(lib, b"amdsmi_get_processor_handles\0"),
            amdsmi_get_processor_type: resolve!(lib, b"amdsmi_get_processor_type\0"),
            amdsmi_get_gpu_busy_percent: resolve!(lib, b"amdsmi_get_gpu_busy_percent\0"),
            amdsmi_get_gpu_memory_total: resolve!(lib, b"amdsmi_get_gpu_memory_total\0"),
            amdsmi_get_gpu_memory_usage: resolve!(lib, b"amdsmi_get_gpu_memory_usage\0"),
            amdsmi_get_power_info: resolve!(lib, b"amdsmi_get_power_info\0"),
            amdsmi_get_clk_freq: resolve!(lib, b"amdsmi_get_clk_freq\0"),
            amdsmi_get_temp_metric: resolve!(lib, b"amdsmi_get_temp_metric\0"),
            amdsmi_get_gpu_fan_rpms: resolve!(lib, b"amdsmi_get_gpu_fan_rpms\0"),
            amdsmi_get_gpu_device_uuid: resolve!(lib, b"amdsmi_get_gpu_device_uuid\0"),
            amdsmi_get_gpu_board_info: resolve!(lib, b"amdsmi_get_gpu_board_info\0"),
            amdsmi_get_gpu_perf_level: resolve!(lib, b"amdsmi_get_gpu_perf_level\0"),
            amdsmi_set_gpu_perf_level: resolve!(lib, b"amdsmi_set_gpu_perf_level\0"),
            amdsmi_set_power_cap: resolve!(lib, b"amdsmi_set_power_cap\0"),
            _lib: lib,
        })
    }
}

// ─── Global context ──────────────────────────────────────────────────────────

struct GpuContext {
    driver: Option<AmdSmiDriver>,
    handles: Vec<ProcessorHandle>,
    initialized: bool,
}

impl GpuContext {
    const fn new() -> Self {
        Self {
            driver: None,
            handles: Vec::new(),
            initialized: false,
        }
    }
}

// SAFETY: processor handles are opaque SMI pointers; access is serialized by CTX.
unsafe impl Send for GpuContext {}

static CTX: Mutex<GpuContext> = Mutex::new(GpuContext::new());

/// Metrics snapshot for a single AMD GPU — public contract for exporter/ctl.
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

impl Default for AmdDeviceMetrics {
    fn default() -> Self {
        Self {
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
}

fn c_string_from_buf(buf: &[u8]) -> String {
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..end]).into_owned()
}

fn enumerate_gpus(drv: &AmdSmiDriver) -> Result<Vec<ProcessorHandle>, i32> {
    let mut socket_count: u32 = 0;
    let st = unsafe { (drv.amdsmi_get_socket_handles)(&mut socket_count, std::ptr::null_mut()) };
    if st != AMDSMI_STATUS_SUCCESS || socket_count == 0 {
        return Err(if st != AMDSMI_STATUS_SUCCESS { st } else { -1 });
    }

    let mut sockets = vec![std::ptr::null_mut(); socket_count as usize];
    let st =
        unsafe { (drv.amdsmi_get_socket_handles)(&mut socket_count, sockets.as_mut_ptr()) };
    if st != AMDSMI_STATUS_SUCCESS {
        return Err(st);
    }

    let mut handles = Vec::new();
    for &socket in &sockets[..socket_count as usize] {
        let mut device_count: u32 = 0;
        let st = unsafe {
            (drv.amdsmi_get_processor_handles)(socket, &mut device_count, std::ptr::null_mut())
        };
        if st != AMDSMI_STATUS_SUCCESS || device_count == 0 {
            continue;
        }

        let mut procs = vec![std::ptr::null_mut(); device_count as usize];
        let st = unsafe {
            (drv.amdsmi_get_processor_handles)(socket, &mut device_count, procs.as_mut_ptr())
        };
        if st != AMDSMI_STATUS_SUCCESS {
            continue;
        }

        for &proc in &procs[..device_count as usize] {
            let mut ptype: u32 = 0;
            let st = unsafe { (drv.amdsmi_get_processor_type)(proc, &mut ptype) };
            if st == AMDSMI_STATUS_SUCCESS && ptype == AMDSMI_PROCESSOR_TYPE_AMD_GPU {
                handles.push(proc);
            }
        }
    }

    if handles.is_empty() {
        Err(-1)
    } else {
        Ok(handles)
    }
}

fn collect_gpu_busy(drv: &AmdSmiDriver, h: ProcessorHandle) -> i64 {
    let mut busy: u32 = 0;
    if unsafe { (drv.amdsmi_get_gpu_busy_percent)(h, &mut busy) } == AMDSMI_STATUS_SUCCESS {
        busy as i64
    } else {
        -1
    }
}

fn collect_memory(drv: &AmdSmiDriver, h: ProcessorHandle) -> (u64, u64) {
    let mut total = 0u64;
    let mut used = 0u64;
    let mut val = 0u64;
    if unsafe { (drv.amdsmi_get_gpu_memory_total)(h, AMDSMI_MEM_TYPE_VRAM, &mut val) }
        == AMDSMI_STATUS_SUCCESS
    {
        total = val;
    }
    if unsafe { (drv.amdsmi_get_gpu_memory_usage)(h, AMDSMI_MEM_TYPE_VRAM, &mut val) }
        == AMDSMI_STATUS_SUCCESS
    {
        used = val;
    }
    (total, used)
}

fn collect_power(drv: &AmdSmiDriver, h: ProcessorHandle) -> (u64, u64) {
    let mut info = unsafe { std::mem::zeroed::<AmdsmiPowerInfo>() };
    if unsafe { (drv.amdsmi_get_power_info)(h, &mut info) } == AMDSMI_STATUS_SUCCESS {
        // current_socket_power / power_limit are in watts on linux BM.
        (
            info.current_socket_power as u64 * 1000,
            info.power_limit as u64 * 1000,
        )
    } else {
        (0, 0)
    }
}

fn collect_clock(drv: &AmdSmiDriver, h: ProcessorHandle, clk_type: u32) -> u32 {
    let mut freq = unsafe { std::mem::zeroed::<AmdsmiFrequencies>() };
    if unsafe { (drv.amdsmi_get_clk_freq)(h, clk_type, &mut freq) } != AMDSMI_STATUS_SUCCESS {
        return 0;
    }
    let level = freq.current as usize;
    if level < freq.num_supported as usize && freq.num_supported > 0 {
        // Match prior C++ wrapper: treat raw values as Hz when large.
        let raw = freq.frequency[level];
        if raw > 10_000 {
            (raw / 1_000_000) as u32
        } else {
            raw as u32
        }
    } else {
        0
    }
}

fn collect_temperature(drv: &AmdSmiDriver, h: ProcessorHandle) -> i64 {
    for sensor in [
        AMDSMI_TEMPERATURE_TYPE_JUNCTION,
        AMDSMI_TEMPERATURE_TYPE_EDGE,
        AMDSMI_TEMPERATURE_TYPE_VRAM,
    ] {
        let mut temp: i64 = 0;
        if unsafe {
            (drv.amdsmi_get_temp_metric)(h, sensor, AMDSMI_TEMP_CURRENT, &mut temp)
        } == AMDSMI_STATUS_SUCCESS
            && temp > 0
        {
            return if temp > 1000 { temp / 1000 } else { temp };
        }
    }
    -999
}

fn collect_fan_rpm(drv: &AmdSmiDriver, h: ProcessorHandle) -> u32 {
    let mut rpm: i64 = 0;
    if unsafe { (drv.amdsmi_get_gpu_fan_rpms)(h, 0, &mut rpm) } == AMDSMI_STATUS_SUCCESS {
        rpm as u32
    } else {
        0
    }
}

fn collect_uuid(drv: &AmdSmiDriver, h: ProcessorHandle) -> String {
    let mut buf = [0u8; 64];
    let mut len = buf.len() as u32;
    if unsafe { (drv.amdsmi_get_gpu_device_uuid)(h, &mut len, buf.as_mut_ptr()) }
        == AMDSMI_STATUS_SUCCESS
    {
        c_string_from_buf(&buf)
    } else {
        "UNKNOWN".into()
    }
}

fn collect_brand(drv: &AmdSmiDriver, h: ProcessorHandle) -> String {
    let mut info = unsafe { std::mem::zeroed::<AmdsmiBoardInfo>() };
    if unsafe { (drv.amdsmi_get_gpu_board_info)(h, &mut info) } == AMDSMI_STATUS_SUCCESS {
        let name = c_string_from_buf(&info.product_name);
        if name.is_empty() {
            "AMD GPU".into()
        } else {
            name
        }
    } else {
        "AMD GPU".into()
    }
}

// ─── Public API ──────────────────────────────────────────────────────────────

pub fn amd_smi_init() -> i32 {
    let mut ctx = match CTX.lock() {
        Ok(g) => g,
        Err(_) => return -1,
    };

    if ctx.initialized {
        return 0;
    }

    let drv = match AmdSmiDriver::open() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("[amd_smi] {e}");
            return -1;
        }
    };

    let st = unsafe { (drv.amdsmi_init)(AMDSMI_INIT_AMD_GPUS) };
    if st != AMDSMI_STATUS_SUCCESS {
        eprintln!("[amd_smi] amdsmi_init failed with code: {st}");
        return st;
    }

    match enumerate_gpus(&drv) {
        Ok(handles) => {
            eprintln!(
                "[amd_smi] Initialized successfully with {} GPU(s)",
                handles.len()
            );
            ctx.handles = handles;
            ctx.driver = Some(drv);
            ctx.initialized = true;
            0
        }
        Err(code) => {
            eprintln!("[amd_smi] No AMD GPUs found during enumeration");
            let _ = unsafe { (drv.amdsmi_shut_down)() };
            code
        }
    }
}

pub fn amd_smi_shutdown() {
    let Ok(mut ctx) = CTX.lock() else {
        return;
    };
    if ctx.initialized {
        if let Some(drv) = ctx.driver.take() {
            let _ = unsafe { (drv.amdsmi_shut_down)() };
        }
        ctx.handles.clear();
        ctx.initialized = false;
    }
}

pub fn amd_smi_get_device_count() -> u32 {
    let Ok(ctx) = CTX.lock() else {
        return 0;
    };
    ctx.handles.len() as u32
}

pub fn amd_smi_collect_device(device_index: u32) -> AmdDeviceMetrics {
    let mut metrics = AmdDeviceMetrics::default();

    let Ok(ctx) = CTX.lock() else {
        metrics.uuid = "INVALID_HANDLE".into();
        metrics.brand = "Unknown".into();
        return metrics;
    };

    let Some(drv) = ctx.driver.as_ref() else {
        metrics.uuid = "INVALID_HANDLE".into();
        metrics.brand = "Unknown".into();
        return metrics;
    };

    let Some(&handle) = ctx.handles.get(device_index as usize) else {
        metrics.uuid = "INVALID_HANDLE".into();
        metrics.brand = "Unknown".into();
        return metrics;
    };

    metrics.uuid = collect_uuid(drv, handle);
    metrics.brand = collect_brand(drv, handle);
    metrics.gpu_utilization_percent = collect_gpu_busy(drv, handle);

    let (total, used) = collect_memory(drv, handle);
    metrics.memory_total_bytes = total;
    metrics.memory_used_bytes = used;
    metrics.memory_utilization_percent = if total == 0 {
        -1
    } else {
        ((100.0 * used as f64) / total as f64) as i64
    };

    let (usage_mw, limit_mw) = collect_power(drv, handle);
    metrics.power_usage_mw = usage_mw;
    metrics.power_limit_mw = limit_mw;

    metrics.clock_core_mhz = collect_clock(drv, handle, AMDSMI_CLK_TYPE_SYS);
    metrics.clock_memory_mhz = collect_clock(drv, handle, AMDSMI_CLK_TYPE_DF);
    metrics.temperature_celsius = collect_temperature(drv, handle);
    metrics.fan_speed_rpm = collect_fan_rpm(drv, handle);

    metrics
}

pub fn amd_smi_set_perf_level(device_index: u32, level: &str) -> bool {
    let perf = match level.to_ascii_lowercase().as_str() {
        "auto" => AMDSMI_DEV_PERF_LEVEL_AUTO,
        "low" => AMDSMI_DEV_PERF_LEVEL_LOW,
        "high" => AMDSMI_DEV_PERF_LEVEL_HIGH,
        "manual" => AMDSMI_DEV_PERF_LEVEL_MANUAL,
        _ => return false,
    };

    let Ok(ctx) = CTX.lock() else {
        return false;
    };
    let Some(drv) = ctx.driver.as_ref() else {
        return false;
    };
    let Some(&handle) = ctx.handles.get(device_index as usize) else {
        return false;
    };

    unsafe { (drv.amdsmi_set_gpu_perf_level)(handle, perf) == AMDSMI_STATUS_SUCCESS }
}

pub fn amd_smi_set_power_limit(device_index: u32, power_limit_mw: u64) -> i32 {
    let Ok(ctx) = CTX.lock() else {
        return -1;
    };
    let Some(drv) = ctx.driver.as_ref() else {
        return -1;
    };
    let Some(&handle) = ctx.handles.get(device_index as usize) else {
        return -1;
    };

    // amdsmi_set_power_cap expects microwatts.
    let cap_uw = power_limit_mw.saturating_mul(1000);
    unsafe { (drv.amdsmi_set_power_cap)(handle, 0, cap_uw) }
}

pub fn amd_smi_get_perf_level(device_index: u32) -> i32 {
    let Ok(ctx) = CTX.lock() else {
        return -1;
    };
    let Some(drv) = ctx.driver.as_ref() else {
        return -1;
    };
    let Some(&handle) = ctx.handles.get(device_index as usize) else {
        return -1;
    };

    let mut perf: u32 = 0;
    if unsafe { (drv.amdsmi_get_gpu_perf_level)(handle, &mut perf) } == AMDSMI_STATUS_SUCCESS {
        perf as i32
    } else {
        -1
    }
}

pub fn amd_smi_control_clk_level(_uuid: &str, _level: i32, _expiry_secs: u64) -> i32 {
    // Not used by current callers; reserved for future clk policy.
    -1
}
