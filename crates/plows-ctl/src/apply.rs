//! `plows-ctl apply`: validate every setting, then write, then read back.
//!
//! - **Validate everything, then write.** A value outside the node's own
//!   limits is refused even if asked for, and a request containing any
//!   refusal writes nothing, so a bad request is never half-applied.
//! - **Compare before writing.** A setting already at its target is
//!   `unchanged` and not written, so re-asserting desired state is free.
//! - **Read back.** Every result carries what the hardware reports afterwards.
//!
//! The logic runs against the [`PowerControl`] trait, so it is tested with a
//! fake that records writes and scripts read-backs.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::caps::{NodeCaps, CONTRACT_VERSION};
use crate::outcome::{Outcome, Status};
use crate::profile::{self, Profile, Settings, Unsupported};

/// The hardware operations `apply` needs.
pub trait PowerControl {
    /// A fresh read of everything.
    fn read(&mut self) -> NodeCaps;
    fn set_cpu_governor(&mut self, governor: &str) -> Result<(), String>;
    fn set_cpu_max_freq_khz(&mut self, khz: u64) -> Result<(), String>;
    fn set_cpu_epp(&mut self, epp: &str) -> Result<(), String>;
    /// `index` is the cross-vendor GPU index.
    fn set_gpu_power_limit_mw(&mut self, index: u32, mw: u64) -> Result<(), String>;
}

/// The request on stdin: a profile, or explicit settings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplyRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<Profile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<Settings>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplyReport {
    pub contract_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<Profile>,
    pub dry_run: bool,
    pub resolved: Settings,
    /// Written and read back — or, with `--dry-run`, what would be written.
    pub applied: Vec<Outcome>,
    pub unchanged: Vec<Outcome>,
    pub refused: Vec<Outcome>,
    pub unsupported: Vec<Outcome>,
    pub failed: Vec<Outcome>,
    /// The node as read after the writes.
    pub actual: NodeCaps,
}

impl ApplyReport {
    /// `0` everything applied or already so; `2` partial; `1` nothing done.
    pub fn exit_code(&self) -> i32 {
        let done = self.applied.len() + self.unchanged.len();
        let bad = self.refused.len() + self.unsupported.len() + self.failed.len();
        match (done, bad) {
            (_, 0) => 0,
            (0, _) => 1,
            _ => 2,
        }
    }
}

/// A write that passed validation.
enum Write {
    Governor(String),
    MaxFreq(u64),
    Epp(String),
    GpuLimit { index: u32, mw: u64 },
}

struct Planned {
    outcome: Outcome,
    write: Write,
    /// Already at the target.
    same: bool,
}

fn refuse(o: Outcome, why: String) -> Outcome {
    o.with(Status::Refused, Some(why))
}

/// Check each requested setting against what the node reports.
fn plan(settings: &Settings, caps: &NodeCaps) -> (Vec<Planned>, Vec<Outcome>, Vec<Outcome>) {
    let (mut ok, mut refused, mut unsupported) = (vec![], vec![], vec![]);
    let c = &settings.cpu;

    if !c.is_empty() {
        match &caps.cpu {
            None => {
                for (s, v) in [
                    ("governor", c.governor.clone().map(Value::from)),
                    ("max_freq_khz", c.max_freq_khz.map(Value::from)),
                    ("epp", c.epp.clone().map(Value::from)),
                ] {
                    if let Some(v) = v {
                        unsupported.push(Outcome::new("cpu", s, v).with(
                            Status::Unsupported,
                            Some("no cpufreq on this machine".into()),
                        ));
                    }
                }
            }
            Some(cpu) => {
                if let Some(g) = &c.governor {
                    let o = Outcome::new("cpu", "governor", g.clone())
                        .from_value(cpu.governor.clone().map(Value::from));
                    if cpu.available_governors.iter().any(|x| x == g) {
                        ok.push(Planned {
                            same: cpu.governor.as_ref() == Some(g),
                            outcome: o,
                            write: Write::Governor(g.clone()),
                        });
                    } else {
                        refused.push(refuse(
                            o,
                            format!(
                                "not offered here (has: {})",
                                cpu.available_governors.join(" ")
                            ),
                        ));
                    }
                }
                if let Some(k) = c.max_freq_khz {
                    let o = Outcome::new("cpu", "max_freq_khz", k)
                        .from_value(cpu.max_freq_khz.map(Value::from));
                    match (cpu.hw_min_khz, cpu.hw_max_khz) {
                        (Some(lo), Some(hi)) if k < lo || k > hi => {
                            refused.push(refuse(o, format!("outside {lo}–{hi} kHz")))
                        }
                        (Some(_), Some(_)) => ok.push(Planned {
                            same: cpu.max_freq_khz == Some(k),
                            outcome: o,
                            write: Write::MaxFreq(k),
                        }),
                        _ => unsupported.push(o.with(
                            Status::Unsupported,
                            Some("the driver reports no frequency range".into()),
                        )),
                    }
                }
                if let Some(e) = &c.epp {
                    let o = Outcome::new("cpu", "epp", e.clone())
                        .from_value(cpu.epp.clone().map(Value::from));
                    if cpu.available_epp.is_empty() {
                        let d = cpu.driver.clone().unwrap_or_else(|| "this driver".into());
                        unsupported
                            .push(o.with(Status::Unsupported, Some(format!("{d} exposes no EPP"))));
                    } else if cpu.available_epp.iter().any(|x| x == e) {
                        ok.push(Planned {
                            same: cpu.epp.as_ref() == Some(e),
                            outcome: o,
                            write: Write::Epp(e.clone()),
                        });
                    } else {
                        refused.push(refuse(
                            o,
                            format!("not offered here (has: {})", cpu.available_epp.join(" ")),
                        ));
                    }
                }
            }
        }
    }

    for (&index, g) in &settings.gpus {
        let Some(w) = g.power_limit_w else { continue };
        let target = format!("gpu{index}");
        let mw = w as u64 * 1000;
        let Some(gpu) = caps.gpus.iter().find(|x| x.index == index) else {
            refused.push(refuse(
                Outcome::new(&target, "power_limit_w", w),
                format!("no GPU {index} (this node has {})", caps.gpus.len()),
            ));
            continue;
        };
        let lim = gpu.power_limit_mw;
        let o = Outcome::new(&target, "power_limit_w", w)
            .from_value(lim.current.map(|m| Value::from(m / 1000)));
        if !gpu.power_limit_settable {
            unsupported.push(o.with(
                Status::Unsupported,
                Some("the power limit cannot be set".into()),
            ));
            continue;
        }
        // A zero cap is never meaningful, whatever the reported minimum.
        let lo = lim.min.unwrap_or(0).max(1);
        match lim.max {
            _ if mw < lo => refused.push(refuse(o, format!("below min {} W", lo.div_ceil(1000)))),
            Some(hi) if mw > hi => refused.push(refuse(o, format!("above max {} W", hi / 1000))),
            _ => ok.push(Planned {
                // Compared in whole watts: drivers report in mW or µW and
                // round, and the request is in watts.
                same: lim.current.map(|c| c / 1000) == Some(w as u64),
                outcome: o,
                write: Write::GpuLimit { index, mw },
            }),
        }
    }
    (ok, refused, unsupported)
}

fn unsupported_outcome(u: Unsupported) -> Outcome {
    Outcome::new(u.target, u.setting, Value::Null).with(Status::Unsupported, Some(u.reason))
}

/// What the node reports now for one outcome's setting.
fn read_back(o: &Outcome, caps: &NodeCaps) -> Option<Value> {
    if o.target == "cpu" {
        let cpu = caps.cpu.as_ref()?;
        return match o.setting.as_str() {
            "governor" => cpu.governor.clone().map(Value::from),
            "max_freq_khz" => cpu.max_freq_khz.map(Value::from),
            "epp" => cpu.epp.clone().map(Value::from),
            _ => None,
        };
    }
    let index: u32 = o.target.strip_prefix("gpu")?.parse().ok()?;
    let g = caps.gpus.iter().find(|g| g.index == index)?;
    g.power_limit_mw.current.map(|m| json!(m / 1000))
}

/// Apply a request. Nothing is written when `dry_run` is set.
pub fn apply(req: &ApplyRequest, ctl: &mut dyn PowerControl, dry_run: bool) -> ApplyReport {
    let before = ctl.read();
    let (resolved, mut unsupported) = match (&req.profile, &req.settings) {
        (Some(p), _) => {
            let r = profile::resolve(*p, &before);
            (
                r.settings,
                r.unsupported.into_iter().map(unsupported_outcome).collect(),
            )
        }
        (None, Some(s)) => (s.clone(), Vec::new()),
        (None, None) => (Settings::default(), Vec::new()),
    };

    let (planned, refused, more_unsupported) = plan(&resolved, &before);
    unsupported.extend(more_unsupported);

    let mut report = ApplyReport {
        contract_version: CONTRACT_VERSION,
        profile: req.profile,
        dry_run,
        resolved,
        applied: vec![],
        unchanged: vec![],
        refused,
        unsupported,
        failed: vec![],
        actual: before.clone(),
    };

    // Any refusal means the request is wrong: write nothing.
    if !report.refused.is_empty() {
        return report;
    }

    let mut wrote = false;
    for Planned {
        outcome,
        write,
        same,
    } in planned
    {
        if same {
            report.unchanged.push(outcome.with(Status::Unchanged, None));
            continue;
        }
        if dry_run {
            report.applied.push(outcome.with(Status::Planned, None));
            continue;
        }
        let res = match &write {
            Write::Governor(g) => ctl.set_cpu_governor(g),
            Write::MaxFreq(k) => ctl.set_cpu_max_freq_khz(*k),
            Write::Epp(e) => ctl.set_cpu_epp(e),
            Write::GpuLimit { index, mw } => ctl.set_gpu_power_limit_mw(*index, *mw),
        };
        wrote = true;
        match res {
            Ok(()) => report.applied.push(outcome.with(Status::Applied, None)),
            Err(e) => report.failed.push(outcome.with(Status::Failed, Some(e))),
        }
    }

    if wrote {
        report.actual = ctl.read();
        for o in report.applied.iter_mut().chain(report.failed.iter_mut()) {
            o.actual = read_back(o, &report.actual);
            if o.status == Status::Applied && o.actual.is_some() && o.actual.as_ref() != Some(&o.to)
            {
                // Accepted but not what was asked: say so rather than claim it.
                o.reason = Some("the driver adjusted the value".into());
            }
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::caps::GpuCaps;
    use crate::profile::tests::{acpi_cpu, mi350x, pstate_cpu};
    use crate::profile::{CpuSettings, GpuSettings};

    /// Records writes and applies them to its own state; can be told to clamp
    /// GPU limits or fail a write.
    struct Fake {
        caps: NodeCaps,
        writes: Vec<String>,
        clamp_gpu_mw: Option<u64>,
        fail_epp: bool,
    }

    impl Fake {
        fn new(caps: NodeCaps) -> Self {
            Self {
                caps,
                writes: vec![],
                clamp_gpu_mw: None,
                fail_epp: false,
            }
        }
    }

    impl PowerControl for Fake {
        fn read(&mut self) -> NodeCaps {
            self.caps.clone()
        }
        fn set_cpu_governor(&mut self, g: &str) -> Result<(), String> {
            self.writes.push(format!("governor={g}"));
            self.caps.cpu.as_mut().unwrap().governor = Some(g.into());
            Ok(())
        }
        fn set_cpu_max_freq_khz(&mut self, k: u64) -> Result<(), String> {
            self.writes.push(format!("max={k}"));
            self.caps.cpu.as_mut().unwrap().max_freq_khz = Some(k);
            Ok(())
        }
        fn set_cpu_epp(&mut self, e: &str) -> Result<(), String> {
            self.writes.push(format!("epp={e}"));
            if self.fail_epp {
                return Err("Device or resource busy".into());
            }
            self.caps.cpu.as_mut().unwrap().epp = Some(e.into());
            Ok(())
        }
        fn set_gpu_power_limit_mw(&mut self, i: u32, mw: u64) -> Result<(), String> {
            self.writes.push(format!("gpu{i}={mw}"));
            let v = self.clamp_gpu_mw.map_or(mw, |c| mw.min(c));
            self.caps
                .gpus
                .iter_mut()
                .find(|g| g.index == i)
                .unwrap()
                .power_limit_mw
                .current = Some(v);
            Ok(())
        }
    }

    fn node() -> NodeCaps {
        NodeCaps {
            cpu: Some(acpi_cpu()),
            gpus: vec![mi350x(0), mi350x(1)],
        }
    }

    fn profile(p: Profile) -> ApplyRequest {
        ApplyRequest {
            profile: Some(p),
            settings: None,
        }
    }

    #[test]
    fn dry_run_writes_nothing() {
        let mut f = Fake::new(node());
        let r = apply(&profile(Profile::PowerSaving), &mut f, true);
        assert!(f.writes.is_empty());
        assert!(r.applied.iter().all(|o| o.status == Status::Planned));
        assert_eq!(r.applied.len(), 3); // max freq + two GPUs; governor unchanged
        assert_eq!(r.exit_code(), 0);
    }

    #[test]
    fn applies_reads_back_then_is_idempotent() {
        let mut f = Fake::new(node());
        let r = apply(&profile(Profile::PowerSaving), &mut f, false);
        assert_eq!(r.exit_code(), 0);
        assert_eq!(f.writes, vec!["max=2400000", "gpu0=600000", "gpu1=600000"]);
        let g0 = r.applied.iter().find(|o| o.target == "gpu0").unwrap();
        assert_eq!(g0.from, Some(json!(1000)));
        assert_eq!(g0.actual, Some(json!(600)));
        assert_eq!(g0.reason, None);

        // Re-asserting the same state writes nothing.
        f.writes.clear();
        let again = apply(&profile(Profile::PowerSaving), &mut f, false);
        assert!(f.writes.is_empty());
        assert!(again.applied.is_empty());
        assert_eq!(again.unchanged.len(), 4);
        assert_eq!(again.exit_code(), 0);
    }

    #[test]
    fn one_refusal_writes_nothing() {
        let mut f = Fake::new(node());
        let req = ApplyRequest {
            profile: None,
            settings: Some(Settings {
                cpu: CpuSettings {
                    governor: Some("ondemand".into()),
                    ..Default::default()
                },
                gpus: [(
                    0,
                    GpuSettings {
                        power_limit_w: Some(1500),
                    },
                )]
                .into(),
            }),
        };
        let r = apply(&req, &mut f, false);
        assert!(f.writes.is_empty(), "{:?}", f.writes);
        assert_eq!(r.refused[0].reason.as_deref(), Some("above max 1000 W"));
        assert_eq!(r.exit_code(), 1);
    }

    #[test]
    fn a_zero_watt_limit_is_refused_even_when_min_is_zero() {
        let mut f = Fake::new(node());
        let req = ApplyRequest {
            profile: None,
            settings: Some(Settings {
                gpus: [(
                    1,
                    GpuSettings {
                        power_limit_w: Some(0),
                    },
                )]
                .into(),
                ..Default::default()
            }),
        };
        let r = apply(&req, &mut f, false);
        assert!(f.writes.is_empty());
        assert!(r.refused[0]
            .reason
            .as_deref()
            .unwrap()
            .starts_with("below min"));
    }

    #[test]
    fn unknown_gpu_and_governor_are_refused() {
        let mut f = Fake::new(node());
        let req = ApplyRequest {
            profile: None,
            settings: Some(Settings {
                cpu: CpuSettings {
                    governor: Some("turbo".into()),
                    ..Default::default()
                },
                gpus: [(
                    9,
                    GpuSettings {
                        power_limit_w: Some(500),
                    },
                )]
                .into(),
            }),
        };
        let r = apply(&req, &mut f, false);
        assert_eq!(r.refused.len(), 2);
        assert!(f.writes.is_empty());
    }

    #[test]
    fn a_frequency_outside_the_range_is_refused() {
        let mut f = Fake::new(node());
        let req = ApplyRequest {
            profile: None,
            settings: Some(Settings {
                cpu: CpuSettings {
                    max_freq_khz: Some(5_008_007),
                    ..Default::default()
                },
                ..Default::default()
            }),
        };
        assert_eq!(apply(&req, &mut f, false).refused.len(), 1);
    }

    #[test]
    fn a_clamping_driver_is_reported_as_it_is() {
        let mut f = Fake::new(node());
        f.clamp_gpu_mw = Some(700_000);
        let req = ApplyRequest {
            profile: None,
            settings: Some(Settings {
                gpus: [(
                    0,
                    GpuSettings {
                        power_limit_w: Some(900),
                    },
                )]
                .into(),
                ..Default::default()
            }),
        };
        let r = apply(&req, &mut f, false);
        assert_eq!(r.applied[0].actual, Some(json!(700)));
        assert_eq!(
            r.applied[0].reason.as_deref(),
            Some("the driver adjusted the value")
        );
    }

    #[test]
    fn a_failed_write_is_partial() {
        let mut f = Fake::new(NodeCaps {
            cpu: Some(pstate_cpu()),
            gpus: vec![],
        });
        f.fail_epp = true;
        let r = apply(&profile(Profile::PowerSaving), &mut f, false);
        assert_eq!(r.failed.len(), 1);
        assert_eq!(r.failed[0].setting, "epp");
        assert_eq!(r.failed[0].actual, Some(json!("balance_performance")));
        assert_eq!(r.exit_code(), 2);
        // The governor and frequency still went through.
        assert_eq!(r.applied.len(), 1);
    }

    #[test]
    fn an_unsettable_gpu_is_partial_not_fatal() {
        let mut caps = node();
        caps.gpus[1].power_limit_settable = false;
        let mut f = Fake::new(caps);
        let r = apply(&profile(Profile::PowerSaving), &mut f, false);
        assert_eq!(r.unsupported.len(), 1);
        assert_eq!(r.exit_code(), 2);
        assert!(f.writes.contains(&"gpu0=600000".to_string()));
    }

    #[test]
    fn nothing_possible_is_exit_1() {
        let mut f = Fake::new(NodeCaps {
            cpu: None,
            gpus: vec![GpuCaps {
                power_limit_settable: false,
                ..mi350x(0)
            }],
        });
        let r = apply(&profile(Profile::Balanced), &mut f, false);
        assert_eq!(r.exit_code(), 1);
    }

    #[test]
    fn the_request_rejects_unknown_fields() {
        assert!(serde_json::from_str::<ApplyRequest>(r#"{"profile":"balanced"}"#).is_ok());
        assert!(serde_json::from_str::<ApplyRequest>(r#"{"profle":"balanced"}"#).is_err());
        let s: ApplyRequest = serde_json::from_str(
            r#"{"settings":{"cpu":{"governor":"schedutil"},"gpus":{"0":{"power_limit_w":250}}}}"#,
        )
        .unwrap();
        assert_eq!(s.settings.unwrap().gpus[&0].power_limit_w, Some(250));
    }
}
