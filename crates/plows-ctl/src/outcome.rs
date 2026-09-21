//! Machine-readable results of a control operation.
//!
//! Every write reports what was asked for *and* what the hardware reports
//! afterwards. Drivers round, clamp, and occasionally ignore a write; a result
//! has to say what *is*, not what was requested.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// What happened to one setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Written, and read back.
    Applied,
    /// Already at the target; nothing was written.
    Unchanged,
    /// Would be written (`--dry-run`).
    Planned,
    /// Rejected before any write: outside the hardware's own limits, or not a
    /// value this node offers.
    Refused,
    /// The hardware or driver has no such control.
    Unsupported,
    /// The write was attempted and the driver returned an error.
    Failed,
}

/// One setting on one target (`cpu`, `gpu0`, …).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Outcome {
    pub target: String,
    pub setting: String,
    /// The value before, when it could be read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<Value>,
    /// The value asked for.
    pub to: Value,
    /// The value read back afterwards. `None` when nothing was written, or
    /// when the driver cannot report it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual: Option<Value>,
    pub status: Status,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl Outcome {
    pub fn new(
        target: impl Into<String>,
        setting: impl Into<String>,
        to: impl Into<Value>,
    ) -> Self {
        Self {
            target: target.into(),
            setting: setting.into(),
            from: None,
            to: to.into(),
            actual: None,
            status: Status::Planned,
            reason: None,
        }
    }

    pub fn from_value(mut self, from: Option<Value>) -> Self {
        self.from = from;
        self
    }

    pub fn with(mut self, status: Status, reason: Option<String>) -> Self {
        self.status = status;
        self.reason = reason;
        self
    }
}
