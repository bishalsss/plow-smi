//! Runtime-loaded Apple APIs. All raw handles are owned and thread-confined here.
//!
//! IOReport is private: channel absence, malformed tables, and missing symbols
//! leave individual metrics unavailable. Public sysctl/Mach counters remain usable.

use crate::backend::apple::{energy_watts, residency};
use crate::{
    CpuClusterMetrics, DeviceMetrics, EngineMetrics, GpuDevice, GpuError, Result, SocMetrics,
    TemperatureSensor, Vendor,
};
use core_foundation_sys::{
    array::{
        kCFTypeArrayCallBacks, CFArrayAppendValue, CFArrayCreateMutable, CFArrayGetCount,
        CFArrayGetTypeID, CFArrayGetValueAtIndex,
    },
    base::{CFGetTypeID, CFRelease},
    data::{CFDataGetBytePtr, CFDataGetLength, CFDataGetTypeID},
    dictionary::{
        kCFTypeDictionaryKeyCallBacks, kCFTypeDictionaryValueCallBacks, CFDictionaryCreate,
        CFDictionaryCreateMutableCopy, CFDictionaryGetTypeID, CFDictionaryGetValue,
        CFDictionarySetValue,
    },
    number::{kCFNumberSInt32Type, CFNumberCreate},
    string::{
        kCFStringEncodingUTF8, CFStringCreateWithCString, CFStringGetCString, CFStringGetTypeID,
    },
};
use libloading::Library;
use std::{
    ffi::{c_void, CStr, CString},
    mem::size_of,
    ptr::{null, null_mut},
    time::Instant,
};

extern "C" {
    fn mach_port_deallocate(task: u32, name: u32) -> i32;
}

type Ref = *const c_void;

/// Own one Create/Copy-rule CoreFoundation reference.
struct Owned(Ref);
impl Owned {
    fn new(raw: Ref) -> Option<Self> {
        (!raw.is_null()).then_some(Self(raw))
    }
}
impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: every Owned originates from a Create/Copy API returning a CF object.
        unsafe {
            CFRelease(self.0);
        }
    }
}

fn cf_string(value: &str) -> Option<Owned> {
    let value = CString::new(value).ok()?;
    // SAFETY: CString is terminated and copied by CoreFoundation.
    Owned::new(
        unsafe { CFStringCreateWithCString(null(), value.as_ptr(), kCFStringEncodingUTF8) }.cast(),
    )
}
fn string(raw: Ref) -> Option<String> {
    if raw.is_null() {
        return None;
    }
    let mut buf = [0i8; 512];
    // SAFETY: pointer is a live borrowed CF value; verify type before conversion.
    unsafe {
        if CFGetTypeID(raw) != CFStringGetTypeID()
            || CFStringGetCString(
                raw.cast(),
                buf.as_mut_ptr(),
                buf.len() as isize,
                kCFStringEncodingUTF8,
            ) == 0
        {
            return None;
        }
        Some(CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned())
    }
}
fn get(dict: Ref, key: &str) -> Ref {
    let Some(key) = cf_string(key) else {
        return null();
    };
    // SAFETY: callers provide a live CFDictionary; returned value is borrowed.
    unsafe {
        if dict.is_null() || CFGetTypeID(dict) != CFDictionaryGetTypeID() {
            return null();
        }
        CFDictionaryGetValue(dict.cast(), key.0)
    }
}

macro_rules! dynamic_api {
    ($name:ident { $($field:ident : $symbol:literal : $ty:ty),* $(,)? }) => {
        struct $name { $($field: $ty,)* _library: Library }
        impl $name {
            fn load(path: &str) -> Result<Self> {
                let library = super::dynlib::open_first(&[path])?;
                // SAFETY: typed signatures match the OS ABI; the library outlives pointers.
                $(let $field = unsafe { super::dynlib::resolve_required::<$ty>(&library, concat!($symbol, "\0").as_bytes(), path) }?;)*
                Ok(Self { $($field,)* _library: library })
            }
        }
    }
}

dynamic_api!(ReportApi {
    channels: "IOReportCopyAllChannels": unsafe extern "C" fn(u64, u64) -> Ref,
    subscribe: "IOReportCreateSubscription": unsafe extern "C" fn(Ref, Ref, *mut Ref, u64, Ref) -> Ref,
    sample: "IOReportCreateSamples": unsafe extern "C" fn(Ref, Ref, Ref) -> Ref,
    delta: "IOReportCreateSamplesDelta": unsafe extern "C" fn(Ref, Ref, Ref) -> Ref,
    group: "IOReportChannelGetGroup": unsafe extern "C" fn(Ref) -> Ref,
    subgroup: "IOReportChannelGetSubGroup": unsafe extern "C" fn(Ref) -> Ref,
    name: "IOReportChannelGetChannelName": unsafe extern "C" fn(Ref) -> Ref,
    unit: "IOReportChannelGetUnitLabel": unsafe extern "C" fn(Ref) -> Ref,
    integer: "IOReportSimpleGetIntegerValue": unsafe extern "C" fn(Ref, i32) -> i64,
    state_count: "IOReportStateGetCount": unsafe extern "C" fn(Ref) -> i32,
    state_name: "IOReportStateGetNameForIndex": unsafe extern "C" fn(Ref, i32) -> Ref,
    state_time: "IOReportStateGetResidency": unsafe extern "C" fn(Ref, i32) -> i64,
});

dynamic_api!(KitApi {
    matching: "IOServiceMatching": unsafe extern "C" fn(*const i8) -> Ref,
    services: "IOServiceGetMatchingServices": unsafe extern "C" fn(u32, Ref, *mut u32) -> i32,
    next: "IOIteratorNext": unsafe extern "C" fn(u32) -> u32,
    release: "IOObjectRelease": unsafe extern "C" fn(u32) -> i32,
    entry_name: "IORegistryEntryGetName": unsafe extern "C" fn(u32, *mut i8) -> i32,
    properties: "IORegistryEntryCreateCFProperties": unsafe extern "C" fn(u32, *mut Ref, Ref, u32) -> i32,
    open: "IOServiceOpen": unsafe extern "C" fn(u32, u32, u32, *mut u32) -> i32,
    close: "IOServiceClose": unsafe extern "C" fn(u32) -> i32,
    call: "IOConnectCallStructMethod": unsafe extern "C" fn(u32, u32, Ref, usize, *mut c_void, *mut usize) -> i32,
});

dynamic_api!(HidApi {
    create: "IOHIDEventSystemClientCreate": unsafe extern "C" fn(Ref) -> Ref,
    matching: "IOHIDEventSystemClientSetMatching": unsafe extern "C" fn(Ref, Ref),
    services: "IOHIDEventSystemClientCopyServices": unsafe extern "C" fn(Ref) -> Ref,
    property: "IOHIDServiceClientCopyProperty": unsafe extern "C" fn(Ref, Ref) -> Ref,
    event: "IOHIDServiceClientCopyEvent": unsafe extern "C" fn(Ref, i64, i32, i64) -> Ref,
    value: "IOHIDEventGetFloatValue": unsafe extern "C" fn(Ref, i64) -> f64,
});

struct Hid {
    services: Owned,
    _client: Owned,
    api: HidApi,
    sensors: Vec<(isize, String)>,
}
impl Hid {
    fn new() -> Option<Self> {
        let api = HidApi::load("/System/Library/Frameworks/IOKit.framework/IOKit").ok()?;
        let keys = [cf_string("PrimaryUsagePage")?, cf_string("PrimaryUsage")?];
        let values = [0xff00i32, 5i32];
        // SAFETY: CF constructors copy integer values; client and services follow Copy ownership.
        unsafe {
            let numbers = [
                Owned::new(
                    CFNumberCreate(
                        null(),
                        kCFNumberSInt32Type,
                        (&values[0] as *const i32).cast(),
                    )
                    .cast(),
                )?,
                Owned::new(
                    CFNumberCreate(
                        null(),
                        kCFNumberSInt32Type,
                        (&values[1] as *const i32).cast(),
                    )
                    .cast(),
                )?,
            ];
            let raw_keys = [keys[0].0, keys[1].0];
            let raw_values = [numbers[0].0, numbers[1].0];
            let matching = Owned::new(
                CFDictionaryCreate(
                    null(),
                    raw_keys.as_ptr(),
                    raw_values.as_ptr(),
                    2,
                    &kCFTypeDictionaryKeyCallBacks,
                    &kCFTypeDictionaryValueCallBacks,
                )
                .cast(),
            )?;
            let client = Owned::new((api.create)(null()))?;
            (api.matching)(client.0, matching.0);
            let services = Owned::new((api.services)(client.0))?;
            if CFGetTypeID(services.0) != CFArrayGetTypeID() {
                return None;
            }
            let product = cf_string("Product")?;
            let mut sensors = Vec::new();
            for i in 0..CFArrayGetCount(services.0.cast()) {
                let service = CFArrayGetValueAtIndex(services.0.cast(), i);
                let Some(name) = Owned::new((api.property)(service, product.0)) else {
                    continue;
                };
                if let Some(name) = string(name.0) {
                    sensors.push((i, name));
                }
            }
            Some(Self {
                services,
                _client: client,
                api,
                sensors,
            })
        }
    }
    fn refresh(&self, m: &mut DeviceMetrics) {
        let soc = m.soc.as_mut().unwrap();
        let mut cpu = (0.0f32, 0u32);
        let mut gpu = (0.0f32, 0u32);
        for (index, name) in &self.sensors {
            // SAFETY: services array keeps each borrowed service alive; CopyEvent is released.
            let value = unsafe {
                let service = CFArrayGetValueAtIndex(self.services.0.cast(), *index);
                let Some(event) = Owned::new((self.api.event)(service, 15, 0, 0)) else {
                    continue;
                };
                (self.api.value)(event.0, 15 << 16) as f32
            };
            if !value.is_finite() || value <= 0.0 || value > 150.0 {
                continue;
            }
            soc.temperatures.push(TemperatureSensor {
                name: name.clone(),
                celsius: value,
            });
            if name.starts_with("pACC MTR Temp Sensor") || name.starts_with("eACC MTR Temp Sensor")
            {
                cpu.0 += value;
                cpu.1 += 1;
            }
            if name.starts_with("GPU MTR Temp Sensor") {
                gpu.0 += value;
                gpu.1 += 1;
            }
        }
        if cpu.1 > 0 {
            soc.cpu.temperature_celsius = Some(cpu.0 / cpu.1 as f32);
        }
        if gpu.1 > 0 {
            m.temperature = Some(gpu.0 / gpu.1 as f32);
        }
    }
}

fn registry(api: &KitApi, service: &CStr, mut visit: impl FnMut(u32, &str, Ref)) {
    // SAFETY: matching dictionary is consumed by services; every iterator/entry is released.
    unsafe {
        let matching = (api.matching)(service.as_ptr());
        if matching.is_null() {
            return;
        }
        let mut iterator = 0;
        if (api.services)(0, matching, &mut iterator) != 0 {
            return;
        }
        loop {
            let entry = (api.next)(iterator);
            if entry == 0 {
                break;
            }
            let mut name = [0i8; 128];
            let mut properties = null();
            if (api.entry_name)(entry, name.as_mut_ptr()) == 0
                && (api.properties)(entry, &mut properties, null(), 0) == 0
            {
                if let Some(properties) = Owned::new(properties) {
                    visit(
                        entry,
                        &CStr::from_ptr(name.as_ptr()).to_string_lossy(),
                        properties.0,
                    );
                }
            }
            (api.release)(entry);
        }
        (api.release)(iterator);
    }
}

fn sysctl(name: &CStr) -> Option<Vec<u8>> {
    let mut len = 0;
    // SAFETY: sysctlbyname first obtains length, then writes into an allocated buffer.
    unsafe {
        if libc::sysctlbyname(name.as_ptr(), null_mut(), &mut len, null_mut(), 0) != 0 || len > 4096
        {
            return None;
        }
        let mut buf = vec![0; len];
        if libc::sysctlbyname(
            name.as_ptr(),
            buf.as_mut_ptr().cast(),
            &mut len,
            null_mut(),
            0,
        ) != 0
        {
            return None;
        }
        buf.truncate(len);
        Some(buf)
    }
}
fn sys_string(name: &CStr) -> Option<String> {
    let buf = sysctl(name)?;
    Some(
        String::from_utf8_lossy(&buf)
            .trim_end_matches('\0')
            .to_owned(),
    )
}
fn sys_u32(name: &CStr) -> Option<u32> {
    Some(u32::from_ne_bytes(sysctl(name)?.try_into().ok()?))
}
fn sys_u64(name: &CStr) -> Option<u64> {
    Some(u64::from_ne_bytes(sysctl(name)?.try_into().ok()?))
}

fn frequencies(dict: Ref, key: &str, scale: u32) -> Vec<u32> {
    let data = get(dict, key);
    // SAFETY: type checked before CFData APIs; byte slice borrows the live properties object.
    unsafe {
        if data.is_null() || CFGetTypeID(data) != CFDataGetTypeID() {
            return vec![];
        }
        let len = CFDataGetLength(data.cast());
        if len <= 0 || len > 8192 || len % 8 != 0 {
            return vec![];
        }
        let bytes = std::slice::from_raw_parts(CFDataGetBytePtr(data.cast()), len as usize);
        bytes
            .chunks_exact(8)
            .map(|pair| u32::from_le_bytes(pair[..4].try_into().unwrap()) / scale)
            .collect()
    }
}

fn cpu_table_keys(dict: Ref) -> Option<[String; 2]> {
    let data = get(dict, "acc-clusters");
    // SAFETY: validate CFData and its length before borrowing bytes.
    unsafe {
        if data.is_null() || CFGetTypeID(data) != CFDataGetTypeID() {
            return None;
        }
        let len = CFDataGetLength(data.cast());
        if !(16..=8192).contains(&len) || len % 8 != 0 {
            return None;
        }
        let bytes = std::slice::from_raw_parts(CFDataGetBytePtr(data.cast()), len as usize);
        let mut tiers: Vec<_> = bytes
            .chunks_exact(8)
            .map(|chunk| (chunk[1], chunk[0]))
            .collect();
        tiers.sort_unstable();
        tiers.dedup_by_key(|entry| entry.0);
        if tiers.len() < 2 {
            return None;
        }
        Some([
            format!("voltage-states{}-sram", tiers[tiers.len() - 2].1),
            format!("voltage-states{}-sram", tiers[tiers.len() - 1].1),
        ])
    }
}

#[derive(Clone, Copy)]
enum Kind {
    Cpu,
    Gpu,
    Npu,
    CpuEnergy,
    GpuEnergy,
    NpuEnergy,
}
struct Channel {
    index: isize,
    kind: Kind,
    unit: String,
    frequencies: Vec<u32>,
    tier: usize,
    states: Vec<(String, i64)>,
}
struct Report {
    subscription: Owned,
    channels: Owned,
    previous: Option<(Owned, Instant)>,
    metadata: Vec<Channel>,
    api: ReportApi,
}
impl Report {
    fn new(tables: &[Vec<u32>; 3]) -> Result<Self> {
        let api = ReportApi::load("/usr/lib/libIOReport.dylib")?;
        // SAFETY: all function pointers match the OS ABI; Owned manages Create/Copy results.
        unsafe {
            let all = Owned::new((api.channels)(0, 0)).ok_or_else(|| {
                GpuError::InitializationFailed("IOReport channels unavailable".into())
            })?;
            let array = get(all.0, "IOReportChannels");
            if array.is_null() || CFGetTypeID(array) != CFArrayGetTypeID() {
                return Err(GpuError::InitializationFailed(
                    "invalid IOReport channels".into(),
                ));
            }
            let selected =
                Owned::new(CFArrayCreateMutable(null(), 0, &kCFTypeArrayCallBacks).cast())
                    .ok_or_else(|| GpuError::InitializationFailed("CF allocation failed".into()))?;
            let mut metadata = Vec::new();
            for i in 0..CFArrayGetCount(array.cast()) {
                let item = CFArrayGetValueAtIndex(array.cast(), i);
                let group = string((api.group)(item)).unwrap_or_default();
                let sub = string((api.subgroup)(item)).unwrap_or_default();
                let name = string((api.name)(item)).unwrap_or_default();
                let unit = string((api.unit)(item)).unwrap_or_default();
                let tier = usize::from(name.contains("PCPU"));
                let kind = match (group.as_str(), sub.as_str()) {
                    ("CPU Stats", "CPU Core Performance States")
                        if name.contains("PCPU")
                            || name.contains("ECPU")
                            || name.contains("MCPU") =>
                    {
                        Kind::Cpu
                    }
                    ("GPU Stats", "GPU Performance States")
                        if name == "GPUPH" || name.ends_with("_GPUPH") =>
                    {
                        Kind::Gpu
                    }
                    ("ANE Stats", _) | ("NPU Stats", _) if sub.contains("Performance States") => {
                        Kind::Npu
                    }
                    ("Energy Model", _) if name.ends_with("CPU Energy") => Kind::CpuEnergy,
                    ("Energy Model", _) if name.ends_with("GPU Energy") => Kind::GpuEnergy,
                    ("Energy Model", _) if name.starts_with("ANE") => Kind::NpuEnergy,
                    _ => continue,
                };
                let freqs = match kind {
                    Kind::Cpu => tables[tier].clone(),
                    Kind::Gpu => tables[2].iter().copied().filter(|f| *f > 0).collect(),
                    _ => vec![],
                };
                let count = if matches!(kind, Kind::Cpu | Kind::Gpu | Kind::Npu) {
                    (api.state_count)(item)
                } else {
                    0
                };
                let states = if matches!(kind, Kind::Cpu | Kind::Gpu | Kind::Npu)
                    && (1..=256).contains(&count)
                {
                    let names: Option<Vec<_>> = (0..count)
                        .map(|i| string((api.state_name)(item, i)).map(|name| (name, 0)))
                        .collect();
                    names.unwrap_or_default()
                } else {
                    vec![]
                };
                metadata.push(Channel {
                    index: metadata.len() as isize,
                    kind,
                    unit,
                    frequencies: freqs,
                    tier,
                    states,
                });
                CFArrayAppendValue(selected.0.cast_mut().cast(), item);
            }
            let channels =
                Owned::new(CFDictionaryCreateMutableCopy(null(), 0, all.0.cast()).cast())
                    .ok_or_else(|| GpuError::InitializationFailed("CF allocation failed".into()))?;
            let key = cf_string("IOReportChannels")
                .ok_or_else(|| GpuError::InitializationFailed("CF allocation failed".into()))?;
            CFDictionarySetValue(channels.0.cast_mut().cast(), key.0, selected.0);
            let mut subscribed = null();
            let subscription = Owned::new((api.subscribe)(
                null(),
                channels.0,
                &mut subscribed,
                0,
                null(),
            ))
            .ok_or_else(|| GpuError::InitializationFailed("IOReport subscription denied".into()))?;
            // IOReport returns the actual subscribed channel dictionary (+1 ownership).
            let channels = Owned::new(subscribed).unwrap_or(channels);
            Ok(Self {
                subscription,
                channels,
                previous: None,
                metadata,
                api,
            })
        }
    }
    fn refresh(&mut self, m: &mut DeviceMetrics) {
        // SAFETY: subscription/channel references remain alive; samples follow Copy ownership.
        unsafe {
            let Some(next) = Owned::new((self.api.sample)(
                self.subscription.0,
                self.channels.0,
                null(),
            )) else {
                self.previous = None;
                return;
            };
            let now = Instant::now();
            let previous = self.previous.replace((next, now));
            let Some((previous, started)) = previous else {
                return;
            };
            let seconds = now.duration_since(started).as_secs_f64();
            let next = &self.previous.as_ref().unwrap().0;
            let Some(delta) = Owned::new((self.api.delta)(previous.0, next.0, null())) else {
                return;
            };
            let array = get(delta.0, "IOReportChannels");
            if array.is_null()
                || CFGetTypeID(array) != CFArrayGetTypeID()
                || CFArrayGetCount(array.cast()) != self.metadata.len() as isize
            {
                return;
            }
            let mut cpu = [(0.0f64, 0u32, 0u64, 0u32); 2];
            let mut gpu = (0.0f64, 0u32, 0u64, 0u32);
            let mut invalid_energy = [false; 3];
            let soc = m.soc.as_mut().unwrap();
            for channel in &mut self.metadata {
                let item = CFArrayGetValueAtIndex(array.cast(), channel.index);
                match channel.kind {
                    Kind::CpuEnergy | Kind::GpuEnergy | Kind::NpuEnergy => {
                        let value =
                            energy_watts((self.api.integer)(item, 0), &channel.unit, seconds);
                        let energy_index = match channel.kind {
                            Kind::CpuEnergy => 0,
                            Kind::GpuEnergy => 1,
                            _ => 2,
                        };
                        invalid_energy[energy_index] |= value.is_none();
                        let dest = match channel.kind {
                            Kind::CpuEnergy => &mut soc.cpu.power_watts,
                            Kind::GpuEnergy => &mut m.power_usage,
                            _ => &mut soc.npu.power_watts,
                        };
                        if let Some(value) = value {
                            *dest = Some(dest.unwrap_or(0.0) + value);
                        }
                    }
                    _ => {
                        let count = (self.api.state_count)(item);
                        if !(1..=256).contains(&count) {
                            continue;
                        }
                        // Some OS versions expose state names only on samples, not discovery
                        // dictionaries. Initialize lazily and cache after the first valid sample.
                        if count as usize != channel.states.len() || channel.states.is_empty() {
                            let names: Option<Vec<_>> = (0..count)
                                .map(|i| {
                                    string((self.api.state_name)(item, i)).map(|name| (name, 0))
                                })
                                .collect();
                            let Some(names) = names else {
                                continue;
                            };
                            channel.states = names;
                        }
                        for (i, (_, time)) in channel.states.iter_mut().enumerate() {
                            *time = (self.api.state_time)(item, i as i32);
                        }
                        let (util, freq) = residency(&channel.states, &channel.frequencies);
                        match channel.kind {
                            Kind::Cpu => {
                                if let Some(util) = util {
                                    cpu[channel.tier].0 += util as f64;
                                    cpu[channel.tier].1 += 1;
                                }
                                if let Some(freq) = freq {
                                    cpu[channel.tier].2 += freq as u64;
                                    cpu[channel.tier].3 += 1;
                                }
                            }
                            Kind::Gpu => {
                                if let Some(util) = util {
                                    gpu.0 += util as f64;
                                    gpu.1 += 1;
                                }
                                if let Some(freq) = freq {
                                    gpu.2 += freq as u64;
                                    gpu.3 += 1;
                                }
                            }
                            Kind::Npu => {
                                soc.npu.utilization = util;
                                soc.npu.clock_mhz = freq;
                            }
                            _ => {}
                        }
                    }
                }
            }
            if invalid_energy[0] {
                soc.cpu.power_watts = None;
            }
            if invalid_energy[1] {
                m.power_usage = None;
            }
            if invalid_energy[2] {
                soc.npu.power_watts = None;
            }
            m.utilization = (gpu.1 > 0).then(|| (gpu.0 / gpu.1 as f64) as f32);
            m.clock_graphics = (gpu.3 > 0).then(|| (gpu.2 / gpu.3 as u64) as u32);
            let total: u32 = cpu.iter().map(|c| c.1).sum();
            // Do not present a subset of discovered CPU counters as a complete CPU average.
            if total > 0 && Some(total) == soc.cpu.cores {
                soc.cpu.utilization =
                    Some((cpu.iter().map(|c| c.0).sum::<f64>() / total as f64) as f32);
            }
            let clocks: u32 = cpu.iter().map(|c| c.3).sum();
            if clocks > 0 {
                soc.cpu.clock_mhz =
                    Some((cpu.iter().map(|c| c.2).sum::<u64>() / clocks as u64) as u32);
            }
            for (i, (util, count, freq, freq_count)) in cpu.into_iter().enumerate() {
                if let Some(tier) = soc.cpu_clusters.get_mut(i) {
                    tier.metrics.utilization = (count > 0 && Some(count) == tier.metrics.cores)
                        .then(|| (util / count as f64) as f32);
                    tier.metrics.clock_mhz =
                        (freq_count > 0).then(|| (freq / freq_count as u64) as u32);
                }
            }
            if let (Some(cpu), Some(gpu), Some(npu)) =
                (soc.cpu.power_watts, m.power_usage, soc.npu.power_watts)
            {
                soc.compute_power_watts = Some(cpu + gpu + npu);
            }
        }
    }
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct KeyInfo {
    size: u32,
    kind: u32,
    attributes: u8,
}
#[repr(C)]
#[derive(Default)]
struct SmcData {
    key: u32,
    version: [u8; 6],
    limits: [u32; 4],
    info: KeyInfo,
    result: u8,
    status: u8,
    command: u8,
    value: u32,
    bytes: [u8; 32],
}
struct Smc {
    conn: u32,
    api: KitApi,
    sensors: Vec<(String, KeyInfo)>,
}
impl Smc {
    fn new() -> Option<Self> {
        let api = KitApi::load("/System/Library/Frameworks/IOKit.framework/IOKit").ok()?;
        let mut conn = 0;
        registry(&api, c"AppleSMC", |entry, name, _| {
            if conn == 0 && (name == "AppleSMCKeysEndpoint" || name == "AppleSMC") {
                // SAFETY: valid registry entry; connection is owned until Drop.
                #[allow(deprecated)]
                unsafe {
                    (api.open)(entry, libc::mach_task_self(), 0, &mut conn);
                }
            }
        });
        if conn == 0 {
            return None;
        }
        let mut smc = Self {
            conn,
            api,
            sensors: vec![],
        };
        let count = smc.read("#KEY", smc.info("#KEY")?)?;
        let count = u32::from_be_bytes(count[..4].try_into().ok()?).min(16384);
        for i in 0..count {
            let result = smc.call(SmcData {
                command: 8,
                value: i,
                ..Default::default()
            });
            let Some(result) = result else {
                continue;
            };
            let bytes = result.key.to_be_bytes();
            let Ok(key) = std::str::from_utf8(&bytes) else {
                continue;
            };
            if key.starts_with("Tp")
                || key.starts_with("Te")
                || key.starts_with("Ts")
                || key.starts_with("Tg")
                || key == "PSTR"
            {
                if let Some(info) = smc.info(key) {
                    if info.size == 4 && info.kind == u32::from_be_bytes(*b"flt ") {
                        smc.sensors.push((key.into(), info));
                    }
                }
            }
        }
        Some(smc)
    }
    fn call(&self, request: SmcData) -> Option<SmcData> {
        let mut output = SmcData::default();
        let mut len = size_of::<SmcData>();
        // SAFETY: repr(C) SMC ABI buffers with exact sizes; method 2 performs read requests only.
        let result = unsafe {
            (self.api.call)(
                self.conn,
                2,
                (&request as *const SmcData).cast(),
                len,
                (&mut output as *mut SmcData).cast(),
                &mut len,
            )
        };
        (result == 0 && len == size_of::<SmcData>() && output.result == 0).then_some(output)
    }
    fn info(&self, key: &str) -> Option<KeyInfo> {
        Some(
            self.call(SmcData {
                key: u32::from_be_bytes(key.as_bytes().try_into().ok()?),
                command: 9,
                ..Default::default()
            })?
            .info,
        )
    }
    fn read(&self, key: &str, info: KeyInfo) -> Option<[u8; 32]> {
        Some(
            self.call(SmcData {
                key: u32::from_be_bytes(key.as_bytes().try_into().ok()?),
                command: 5,
                info,
                ..Default::default()
            })?
            .bytes,
        )
    }
    fn refresh(&self, m: &mut DeviceMetrics) {
        let mut cpu = (0.0, 0u32);
        let mut gpu = (0.0, 0u32);
        let soc = m.soc.as_mut().unwrap();
        for (name, info) in &self.sensors {
            let Some(bytes) = self.read(name, *info) else {
                continue;
            };
            let value = f32::from_le_bytes(bytes[..4].try_into().unwrap());
            if !value.is_finite() {
                continue;
            }
            if name == "PSTR" {
                if value >= 0.0 {
                    soc.system_power_watts = Some(value);
                }
                continue;
            }
            if value <= 0.0 || value > 150.0 {
                continue;
            }
            soc.temperatures.push(TemperatureSensor {
                name: name.clone(),
                celsius: value,
            });
            let aggregate = if name.starts_with("Tg") {
                &mut gpu
            } else {
                &mut cpu
            };
            aggregate.0 += value;
            aggregate.1 += 1;
        }
        soc.cpu.temperature_celsius = (cpu.1 > 0).then(|| cpu.0 / cpu.1 as f32);
        m.temperature = (gpu.1 > 0).then(|| gpu.0 / gpu.1 as f32);
    }
}
impl Drop for Smc {
    fn drop(&mut self) {
        // SAFETY: connection obtained from IOServiceOpen, closed exactly once.
        unsafe {
            (self.api.close)(self.conn);
        }
    }
}

pub(crate) struct AppleSampler {
    identity: GpuDevice,
    base: SocMetrics,
    report: Option<Report>,
    smc: Option<Smc>,
    hid: Option<Hid>,
    host: u32,
    previous_cpu: Option<[u32; 4]>,
}
impl AppleSampler {
    pub(crate) fn new() -> Result<Self> {
        let model = sys_string(c"machdep.cpu.brand_string")
            .filter(|s| s.starts_with("Apple "))
            .ok_or_else(|| GpuError::Unsupported("not an Apple Silicon host".into()))?;
        let mut identity = GpuDevice::new(Vendor::Apple, 0, "apple-soc-0".into(), model.clone());
        identity.architecture = "arm64".into();
        let kit = KitApi::load("/System/Library/Frameworks/IOKit.framework/IOKit").ok();
        let mut tables = [vec![], vec![], vec![]];
        if let Some(kit) = &kit {
            // Platform UUID is locally useful but not exported; avoid leaking hardware identifiers.
            registry(kit, c"AppleARMIODevice", |_, name, props| {
                if name != "pmgr" && name != "pmgr-child" {
                    return;
                }
                let scale = if ["M1", "M2", "M3"]
                    .iter()
                    .any(|chip| model.split_whitespace().any(|part| part == *chip))
                {
                    1_000_000
                } else {
                    1000
                };
                for (i, (key, scale)) in [
                    ("voltage-states1-sram", scale),
                    ("voltage-states5-sram", scale),
                    ("voltage-states9", 1_000_000),
                ]
                .into_iter()
                .enumerate()
                {
                    if tables[i].is_empty() {
                        tables[i] = frequencies(props, key, scale);
                    }
                }
                if let Some(keys) = cpu_table_keys(props) {
                    for (i, key) in keys.iter().enumerate() {
                        if tables[i].is_empty() {
                            tables[i] = frequencies(props, key, scale);
                        }
                    }
                }
            });
        }
        let mut clusters = Vec::new();
        for level in (0..sys_u32(c"hw.nperflevels").unwrap_or(0).min(8)).rev() {
            let name = CString::new(format!("hw.perflevel{level}.name")).unwrap();
            let cores = CString::new(format!("hw.perflevel{level}.physicalcpu")).unwrap();
            clusters.push(CpuClusterMetrics {
                name: sys_string(&name).unwrap_or_else(|| format!("tier{level}")),
                metrics: EngineMetrics {
                    cores: sys_u32(&cores),
                    ..Default::default()
                },
            });
        }
        let base = SocMetrics {
            cpu: EngineMetrics {
                cores: sys_u32(c"hw.physicalcpu"),
                ..Default::default()
            },
            cpu_clusters: clusters,
            memory_total_bytes: sys_u64(c"hw.memsize"),
            ..Default::default()
        };
        let report = match Report::new(&tables) {
            Ok(report) => Some(report),
            Err(e) => {
                tracing::warn!(error = %e, "Apple IOReport unavailable; retaining CPU/unified-memory telemetry");
                None
            }
        };
        let smc = Smc::new();
        let hid = if smc
            .as_ref()
            .is_none_or(|s| !s.sensors.iter().any(|(name, _)| name.starts_with('T')))
        {
            Hid::new()
        } else {
            None
        };
        // SAFETY: Mach returns a send right owned until AppleSampler::drop.
        #[allow(deprecated)]
        let host = unsafe { libc::mach_host_self() };
        Ok(Self {
            identity,
            base,
            report,
            smc,
            hid,
            host,
            previous_cpu: None,
        })
    }
    pub(crate) fn identity(&self) -> &GpuDevice {
        &self.identity
    }
    pub(crate) fn sample(&mut self) -> DeviceMetrics {
        let mut m = DeviceMetrics {
            soc: Some(self.base.clone()),
            ..Default::default()
        };
        if let Some(report) = &mut self.report {
            report.refresh(&mut m);
        }
        if let Some(smc) = &self.smc {
            smc.refresh(&mut m);
        }
        if let Some(hid) = &self.hid {
            hid.refresh(&mut m);
        }
        let soc = m.soc.as_mut().unwrap();
        // SAFETY: zero-initialized Mach ABI structs; count specifies allocated buffer size.
        #[allow(deprecated)]
        unsafe {
            let mut cpu: libc::host_cpu_load_info = std::mem::zeroed();
            let mut count = (size_of::<libc::host_cpu_load_info>() / size_of::<i32>()) as u32;
            if libc::host_statistics(
                self.host,
                libc::HOST_CPU_LOAD_INFO,
                (&mut cpu as *mut libc::host_cpu_load_info).cast(),
                &mut count,
            ) == 0
            {
                if let Some(previous) = self.previous_cpu.replace(cpu.cpu_ticks) {
                    let delta: [u64; 4] =
                        std::array::from_fn(|i| cpu.cpu_ticks[i].wrapping_sub(previous[i]) as u64);
                    let total: u64 = delta.iter().sum();
                    if soc.cpu.utilization.is_none() && total > 0 {
                        soc.cpu.utilization = Some(
                            100.0 * (total - delta[libc::CPU_STATE_IDLE as usize]) as f32
                                / total as f32,
                        );
                    }
                }
            }
            let mut stats: libc::vm_statistics64 = std::mem::zeroed();
            let mut count = libc::HOST_VM_INFO64_COUNT;
            if libc::host_statistics64(
                self.host,
                libc::HOST_VM_INFO64,
                (&mut stats as *mut libc::vm_statistics64).cast(),
                &mut count,
            ) == 0
            {
                let pages = (stats.active_count as u64
                    + stats.inactive_count as u64
                    + stats.wire_count as u64
                    + stats.speculative_count as u64
                    + stats.compressor_page_count as u64)
                    .saturating_sub(
                        stats.purgeable_count as u64 + stats.external_page_count as u64,
                    );
                let page_size = libc::sysconf(libc::_SC_PAGESIZE);
                if page_size > 0 {
                    soc.memory_used_bytes = Some(
                        pages
                            .saturating_mul(page_size as u64)
                            .min(soc.memory_total_bytes.unwrap_or(u64::MAX)),
                    );
                }
            }
        }
        m
    }
}
impl Drop for AppleSampler {
    fn drop(&mut self) {
        // SAFETY: release the send right obtained by mach_host_self.
        #[allow(deprecated)]
        unsafe {
            mach_port_deallocate(libc::mach_task_self(), self.host);
        }
    }
}
