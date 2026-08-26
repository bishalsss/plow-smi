//! Shared `libloading` helpers for opening libraries and resolving symbols once.

use libloading::{Library, Symbol};

use crate::error::{GpuError, Result};

/// Open the first candidate path/soname that succeeds.
pub fn open_first(candidates: &[&str]) -> Result<Library> {
    let mut last_err = String::new();
    for path in candidates {
        match unsafe { Library::new(path) } {
            Ok(lib) => return Ok(lib),
            Err(e) => {
                last_err = e.to_string();
            }
        }
    }
    let _ = last_err;
    Err(GpuError::LibraryNotFound {
        candidates: candidates.join(", "),
    })
}

/// Resolve a required symbol and copy the function pointer out of the `Symbol`
/// guard. The library must outlive any use of the returned pointer.
///
/// # Safety
///
/// `T` must match the actual C ABI of `name`.
pub unsafe fn resolve_required<T: Copy>(lib: &Library, name: &[u8], library: &str) -> Result<T> {
    let sym: Symbol<T> = unsafe { lib.get(name) }.map_err(|_| {
        let display = std::str::from_utf8(name)
            .unwrap_or("?")
            .trim_end_matches('\0');
        GpuError::MissingSymbol {
            name: display.to_string(),
            library: library.to_string(),
        }
    })?;
    Ok(*sym)
}

/// Resolve an optional symbol. Missing symbols yield `None` without error.
///
/// # Safety
///
/// `T` must match the actual C ABI of `name`.
pub unsafe fn resolve_optional<T: Copy>(lib: &Library, name: &[u8]) -> Option<T> {
    let sym: Symbol<T> = unsafe { lib.get(name) }.ok()?;
    Some(*sym)
}

/// Candidate library names for a vendor on the current platform.
pub fn nvml_candidates() -> &'static [&'static str] {
    &[
        "libnvidia-ml.so.1",
        "libnvidia-ml.so",
        #[cfg(target_os = "windows")]
        "nvml.dll",
    ]
}

/// AMD SMI sonames / common absolute ROCm install paths.
pub fn amdsmi_candidates() -> &'static [&'static str] {
    &[
        "libamd_smi.so",
        "libamd_smi.so.1",
        "/opt/rocm/lib/libamd_smi.so",
        "/opt/rocm/lib/libamd_smi.so.1",
        #[cfg(target_os = "windows")]
        "amd_smi_dll.dll",
    ]
}

/// Level Zero loader sonames.
pub fn level_zero_candidates() -> &'static [&'static str] {
    &[
        "libze_loader.so.1",
        "libze_loader.so",
        #[cfg(target_os = "windows")]
        "ze_loader.dll",
    ]
}

/// Decode a NUL-terminated C buffer into an owned Rust string.
pub fn c_string_from_buf(buf: &[u8]) -> String {
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..end]).into_owned()
}
