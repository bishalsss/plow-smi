//! Control-plane facts shared by the vendor backends.

/// A GPU's power limits, in milliwatts. Each is `None` when the driver does
/// not report it — never `0`, which would read as a real (and absurd) limit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PowerLimits {
    /// The limit in effect now.
    pub current_mw: Option<u64>,
    /// The limit the board ships with, and returns to on reset.
    pub default_mw: Option<u64>,
    /// The lowest limit the driver accepts.
    pub min_mw: Option<u64>,
    /// The highest limit the driver accepts.
    pub max_mw: Option<u64>,
}
