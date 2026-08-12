//! Application-level error types.

use thiserror::Error;

/// Top-level error type for the GPU exporter.
#[derive(Debug, Error)]
pub enum ExporterError {
    /// Unified `is-gpu` discovery / init failure.
    #[error("GPU initialization failed: {0}")]
    GpuInit(String),

    /// Legacy NVIDIA init error (kept for compatibility).
    #[error("NVIDIA initialization failed: {0}")]
    NvidiaInit(String),

    /// Legacy NVIDIA device error.
    #[error("NVIDIA device error (index={index}): {message}")]
    NvidiaDevice { index: u32, message: String },

    /// Legacy AMD init error.
    #[error("AMD SMI initialization failed (code={0})")]
    AmdInit(i32),

    /// Legacy AMD sysfs error.
    #[error("AMD sysfs error: {0}")]
    AmdSysfs(String),

    /// Legacy AMD device error.
    #[error("AMD SMI device error (index={index}): {message}")]
    AmdDevice { index: u32, message: String },

    /// System collector failure.
    #[error("System metrics collection failed: {0}")]
    System(String),

    /// Prometheus registry failure.
    #[error("Prometheus registry error: {0}")]
    Registry(#[from] prometheus::Error),

    /// HTTP server failure.
    #[error("HTTP server error: {0}")]
    Http(String),

    /// Invalid configuration.
    #[error("Configuration error: {0}")]
    Config(String),
}

/// Convenience type alias.
pub type Result<T> = std::result::Result<T, ExporterError>;
