//! GPU Exporter Library
//!
//! Professional GPU metrics exporter supporting NVIDIA, AMD, and Intel via
//! `is-gpu` (runtime dlopen), plus optional system and TPU metrics, exposed as
//! Prometheus metrics.

pub mod collector;
pub mod config;
pub mod error;
pub mod exporter;
pub mod metrics;
