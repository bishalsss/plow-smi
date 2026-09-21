//! Named power profiles, resolved into concrete settings for one node.
//!
//! Profiles live here, next to the code that applies them, because what they
//! mean is hardware knowledge: which governors a driver offers, where a GPU's
//! default limit sits. A consumer names a profile and gets back exactly what
//! it becomes on this machine — including what it cannot do, and why.
//!
//! | Profile | CPU | GPU |
//! |---|---|---|
//! | performance | `performance` governor, max frequency = top of range | power limit = default |
//! | balanced | `schedutil` (else `ondemand`); pstate drivers: `powersave` + EPP `balance_performance` | power limit = default |
//! | power-saving | max frequency ≈ 70 % of range, snapped to a real step; pstate drivers: `powersave` + EPP `power` | power limit = max(min, 60 % of default) |
//!
//! Power-saving never uses a fixed-low governor or `set-perf low`. The
//! `acpi-cpufreq` `powersave` governor pins every core at its lowest step, and
//! `set-perf low` pins the lowest memory clock — on an inference GPU that cuts
//! decode throughput by an order of magnitude. The lever is a *ceiling*
//! (max frequency, power limit), below which the hardware still boosts.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::caps::NodeCaps;
use crate::cpu::CpuCaps;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Profile {
    Performance,
    Balanced,
    PowerSaving,
}

impl Profile {
    pub const ALL: [Profile; 3] = [
        Profile::Performance,
        Profile::Balanced,
        Profile::PowerSaving,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Profile::Performance => "performance",
            Profile::Balanced => "balanced",
            Profile::PowerSaving => "power-saving",
        }
    }
}

/// Concrete CPU settings. `None` means "leave as it is".
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CpuSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub governor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_freq_khz: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub epp: Option<String>,
}

impl CpuSettings {
    pub fn is_empty(&self) -> bool {
        self.governor.is_none() && self.max_freq_khz.is_none() && self.epp.is_none()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GpuSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub power_limit_w: Option<u32>,
}

/// Explicit settings, or what a profile resolved to.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default, skip_serializing_if = "CpuSettings::is_empty")]
    pub cpu: CpuSettings,
    /// Keyed by the cross-vendor GPU index, as a string in JSON.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub gpus: BTreeMap<u32, GpuSettings>,
}

/// A part of a profile this node cannot honour.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Unsupported {
    pub target: String,
    pub setting: String,
    pub reason: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resolution {
    pub settings: Settings,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unsupported: Vec<Unsupported>,
}

/// Drivers where `powersave` is a dynamic governor steered by EPP, not a
/// fixed-lowest-frequency one.
fn is_pstate(cpu: &CpuCaps) -> bool {
    matches!(
        cpu.driver.as_deref(),
        Some("intel_pstate" | "amd-pstate" | "amd-pstate-epp" | "intel_cpufreq")
    ) && cpu.available_governors.iter().any(|g| g == "powersave")
}

fn has_gov(cpu: &CpuCaps, g: &str) -> bool {
    cpu.available_governors.iter().any(|x| x == g)
}

/// The highest real step at or below `target`; the lowest step if none is.
fn snap_down(steps: &[u64], target: u64) -> Option<u64> {
    steps
        .iter()
        .copied()
        .filter(|&f| f <= target)
        .max()
        .or_else(|| steps.iter().copied().min())
}

/// ≈70 % of the way from min to max, snapped to a step the driver offers.
fn power_saving_max_khz(cpu: &CpuCaps) -> Option<u64> {
    let (min, max) = (cpu.hw_min_khz?, cpu.hw_max_khz?);
    let target = min + (max.saturating_sub(min)) * 7 / 10;
    if cpu.available_frequencies_khz.is_empty() {
        // Drivers without steps take any value; round to whole MHz.
        Some(target / 1000 * 1000)
    } else {
        snap_down(&cpu.available_frequencies_khz, target)
    }
}

fn resolve_cpu(profile: Profile, cpu: &CpuCaps, out: &mut Resolution) {
    let mut unsupported = |setting: &str, reason: String| {
        out.unsupported.push(Unsupported {
            target: "cpu".into(),
            setting: setting.into(),
            reason,
        })
    };
    let driver = cpu.driver.clone().unwrap_or_else(|| "this driver".into());
    let pstate = is_pstate(cpu);

    let governor = match profile {
        Profile::Performance => has_gov(cpu, "performance").then(|| "performance".to_string()),
        Profile::Balanced | Profile::PowerSaving if pstate => Some("powersave".into()),
        Profile::Balanced | Profile::PowerSaving => ["schedutil", "ondemand"]
            .into_iter()
            .find(|g| has_gov(cpu, g))
            .map(str::to_string),
    };
    match governor {
        Some(g) => out.settings.cpu.governor = Some(g),
        None => unsupported(
            "governor",
            format!(
                "{driver} offers none of the governors {} uses (has: {})",
                profile.as_str(),
                cpu.available_governors.join(" ")
            ),
        ),
    }

    let max = match profile {
        Profile::Performance | Profile::Balanced => cpu.hw_max_khz,
        Profile::PowerSaving => power_saving_max_khz(cpu),
    };
    match max {
        Some(k) => out.settings.cpu.max_freq_khz = Some(k),
        None => unsupported(
            "max_freq_khz",
            format!("{driver} reports no frequency range"),
        ),
    }

    // EPP only steers the dynamic pstate governors. Under `performance` the
    // driver fixes it, and a write is refused (EBUSY), so it is left alone.
    if profile != Profile::Performance {
        let want = match profile {
            Profile::PowerSaving => "power",
            _ => "balance_performance",
        };
        if cpu.available_epp.iter().any(|e| e == want) {
            out.settings.cpu.epp = Some(want.into());
        } else if pstate {
            unsupported("epp", format!("{driver} does not offer EPP `{want}`"));
        }
        // Non-pstate drivers have no EPP at all, and this profile does not
        // need one there: nothing to report.
    }
}

/// Resolve a profile against a node's capabilities.
pub fn resolve(profile: Profile, node: &NodeCaps) -> Resolution {
    let mut out = Resolution::default();

    match &node.cpu {
        Some(cpu) => resolve_cpu(profile, cpu, &mut out),
        None => out.unsupported.push(Unsupported {
            target: "cpu".into(),
            setting: "cpufreq".into(),
            reason: "no cpufreq on this machine".into(),
        }),
    }

    for g in &node.gpus {
        let target = format!("gpu{}", g.index);
        let unsupported = |reason: String| Unsupported {
            target: target.clone(),
            setting: "power_limit_w".into(),
            reason,
        };
        if !g.power_limit_settable {
            out.unsupported
                .push(unsupported("the power limit cannot be set".into()));
            continue;
        }
        let Some(default) = g.power_limit_mw.default else {
            out.unsupported.push(unsupported(
                "the driver does not report a default power limit".into(),
            ));
            continue;
        };
        let mw = match profile {
            Profile::Performance | Profile::Balanced => default,
            Profile::PowerSaving => {
                let floor = g.power_limit_mw.min.unwrap_or(0);
                (default * 6 / 10).max(floor)
            }
        };
        out.settings.gpus.insert(
            g.index,
            GpuSettings {
                power_limit_w: Some((mw / 1000) as u32),
            },
        );
    }
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::caps::{GpuCaps, PowerLimitsMw};

    pub(crate) fn acpi_cpu() -> CpuCaps {
        CpuCaps {
            driver: Some("acpi-cpufreq".into()),
            policies: 256,
            available_governors: "conservative ondemand userspace powersave performance schedutil"
                .split(' ')
                .map(str::to_string)
                .collect(),
            governor: Some("schedutil".into()),
            hw_min_khz: Some(1_500_000),
            hw_max_khz: Some(3_300_000),
            max_freq_khz: Some(3_300_000),
            available_frequencies_khz: vec![3_300_000, 2_400_000, 1_500_000],
            ..Default::default()
        }
    }

    pub(crate) fn pstate_cpu() -> CpuCaps {
        CpuCaps {
            driver: Some("intel_pstate".into()),
            policies: 8,
            available_governors: vec!["performance".into(), "powersave".into()],
            governor: Some("powersave".into()),
            hw_min_khz: Some(800_000),
            hw_max_khz: Some(4_000_000),
            max_freq_khz: Some(4_000_000),
            epp: Some("balance_performance".into()),
            available_epp: "default performance balance_performance balance_power power"
                .split(' ')
                .map(str::to_string)
                .collect(),
            ..Default::default()
        }
    }

    pub(crate) fn mi350x(index: u32) -> GpuCaps {
        GpuCaps {
            index,
            vendor: "amd".into(),
            vendor_index: index,
            name: "AMD Instinct MI350X".into(),
            power_limit_mw: PowerLimitsMw {
                current: Some(1_000_000),
                default: Some(1_000_000),
                min: Some(0),
                max: Some(1_000_000),
            },
            power_limit_settable: true,
            ..Default::default()
        }
    }

    #[test]
    fn acpi_power_saving_caps_frequency_and_keeps_a_dynamic_governor() {
        let r = resolve(
            Profile::PowerSaving,
            &NodeCaps {
                cpu: Some(acpi_cpu()),
                gpus: vec![],
            },
        );
        // Not `powersave`: under acpi-cpufreq that pins every core at 1.5 GHz.
        assert_eq!(r.settings.cpu.governor.as_deref(), Some("schedutil"));
        // 1.5 + 0.7 × 1.8 = 2.76 GHz, snapped down to the 2.4 GHz step.
        assert_eq!(r.settings.cpu.max_freq_khz, Some(2_400_000));
        assert_eq!(r.settings.cpu.epp, None);
        assert!(r.unsupported.is_empty(), "{:?}", r.unsupported);
    }

    #[test]
    fn acpi_performance_and_balanced() {
        let node = NodeCaps {
            cpu: Some(acpi_cpu()),
            gpus: vec![],
        };
        let p = resolve(Profile::Performance, &node);
        assert_eq!(p.settings.cpu.governor.as_deref(), Some("performance"));
        assert_eq!(p.settings.cpu.max_freq_khz, Some(3_300_000));
        let b = resolve(Profile::Balanced, &node);
        assert_eq!(b.settings.cpu.governor.as_deref(), Some("schedutil"));
    }

    #[test]
    fn pstate_uses_powersave_with_epp() {
        let node = NodeCaps {
            cpu: Some(pstate_cpu()),
            gpus: vec![],
        };
        let b = resolve(Profile::Balanced, &node);
        assert_eq!(b.settings.cpu.governor.as_deref(), Some("powersave"));
        assert_eq!(b.settings.cpu.epp.as_deref(), Some("balance_performance"));
        let s = resolve(Profile::PowerSaving, &node);
        assert_eq!(s.settings.cpu.epp.as_deref(), Some("power"));
        // No steps: 0.8 + 0.7 × 3.2 = 3.04 GHz, whole MHz.
        assert_eq!(s.settings.cpu.max_freq_khz, Some(3_040_000));
        // Performance never writes EPP: the driver fixes it and refuses.
        assert_eq!(resolve(Profile::Performance, &node).settings.cpu.epp, None);
    }

    #[test]
    fn a_missing_governor_is_reported_not_guessed() {
        let mut cpu = acpi_cpu();
        cpu.available_governors = vec!["userspace".into()];
        let r = resolve(
            Profile::Balanced,
            &NodeCaps {
                cpu: Some(cpu),
                gpus: vec![],
            },
        );
        assert_eq!(r.settings.cpu.governor, None);
        assert_eq!(r.unsupported[0].setting, "governor");
    }

    #[test]
    fn no_cpufreq_is_unsupported() {
        let r = resolve(
            Profile::Performance,
            &NodeCaps {
                cpu: None,
                gpus: vec![],
            },
        );
        assert!(r.settings.cpu.is_empty());
        assert_eq!(r.unsupported[0].setting, "cpufreq");
    }

    #[test]
    fn gpu_limits_follow_the_default() {
        let node = NodeCaps {
            cpu: None,
            gpus: vec![mi350x(0), mi350x(1)],
        };
        let p = resolve(Profile::Performance, &node);
        assert_eq!(p.settings.gpus[&1].power_limit_w, Some(1000));
        let s = resolve(Profile::PowerSaving, &node);
        assert_eq!(s.settings.gpus[&0].power_limit_w, Some(600));
    }

    #[test]
    fn power_saving_never_goes_below_the_minimum() {
        let mut g = mi350x(0);
        g.power_limit_mw.min = Some(800_000);
        let s = resolve(
            Profile::PowerSaving,
            &NodeCaps {
                cpu: None,
                gpus: vec![g],
            },
        );
        assert_eq!(s.settings.gpus[&0].power_limit_w, Some(800));
    }

    #[test]
    fn an_unsettable_gpu_is_reported() {
        let mut g = mi350x(0);
        g.power_limit_settable = false;
        let r = resolve(
            Profile::Balanced,
            &NodeCaps {
                cpu: None,
                gpus: vec![g],
            },
        );
        assert!(r.settings.gpus.is_empty());
        assert!(r.unsupported.iter().any(|u| u.target == "gpu0"));
    }

    #[test]
    fn a_gpu_without_a_default_is_reported() {
        let mut g = mi350x(0);
        g.power_limit_mw.default = None;
        let r = resolve(
            Profile::Balanced,
            &NodeCaps {
                cpu: None,
                gpus: vec![g],
            },
        );
        assert!(r.settings.gpus.is_empty());
        assert!(r.unsupported[1].reason.contains("default"));
    }
}
