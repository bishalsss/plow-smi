//! Human-readable output for `capabilities`, `apply` and `cpu info`.
//! JSON is the contract; this is for someone at a shell on the node.

use colored::Colorize;

use crate::apply::ApplyReport;
use crate::caps::Capabilities;
use crate::cpu::CpuCaps;
use crate::outcome::Outcome;

fn khz(k: Option<u64>) -> String {
    k.map_or("?".into(), |k| format!("{:.2} GHz", k as f64 / 1e6))
}

fn w(mw: Option<u64>) -> String {
    mw.map_or("?".into(), |m| format!("{} W", m / 1000))
}

pub fn print_cpu(c: &CpuCaps) {
    println!("{}", "CPU (cpufreq)".cyan().bold());
    println!("  Driver:        {}", c.driver.as_deref().unwrap_or("?"));
    println!("  Policies:      {}", c.policies);
    println!(
        "  Governor:      {} (available: {})",
        c.governor.as_deref().unwrap_or("mixed"),
        c.available_governors.join(" ")
    );
    println!(
        "  Max frequency: {} (range {} – {})",
        khz(c.max_freq_khz),
        khz(c.hw_min_khz),
        khz(c.hw_max_khz)
    );
    if !c.available_frequencies_khz.is_empty() {
        let steps: Vec<String> = c
            .available_frequencies_khz
            .iter()
            .map(|k| khz(Some(*k)))
            .collect();
        println!("  Steps:         {}", steps.join(", "));
    }
    match &c.epp {
        Some(e) => println!(
            "  EPP:           {e} (available: {})",
            c.available_epp.join(" ")
        ),
        None if c.available_epp.is_empty() => {
            println!("  EPP:           not exposed by this driver")
        }
        None => println!("  EPP:           mixed"),
    }
    for z in &c.rapl {
        println!(
            "  RAPL {}: limit {} (read only)",
            z.name,
            w(z.power_limit_uw.map(|u| u / 1000))
        );
    }
}

pub fn print_capabilities(caps: &Capabilities) {
    println!(
        "{}",
        format!(
            "plows-ctl {} — contract {}",
            caps.plows_ctl_version, caps.contract_version
        )
        .dimmed()
    );
    match &caps.cpu {
        Some(c) => print_cpu(c),
        None => println!("{}", "CPU: no cpufreq".yellow()),
    }
    println!("{}", format!("GPUs: {}", caps.gpus.len()).cyan().bold());
    for g in &caps.gpus {
        let l = g.power_limit_mw;
        println!(
            "  {:>2} {:6} {:28} limit {} (default {}, {} – {}) {}",
            g.index,
            g.vendor,
            g.name,
            w(l.current),
            w(l.default),
            w(l.min),
            w(l.max),
            if g.power_limit_settable {
                "settable".green()
            } else {
                format!(
                    "not settable: {}",
                    g.power_limit_error.as_deref().unwrap_or("?")
                )
                .yellow()
            }
        );
    }
    println!("{}", "Profiles on this node".cyan().bold());
    for (name, r) in &caps.profiles {
        let c = &r.settings.cpu;
        let mut parts = vec![];
        if let Some(g) = &c.governor {
            parts.push(format!("governor {g}"));
        }
        if let Some(k) = c.max_freq_khz {
            parts.push(format!("max {}", khz(Some(k))));
        }
        if let Some(e) = &c.epp {
            parts.push(format!("EPP {e}"));
        }
        let mut limits: Vec<u32> = r
            .settings
            .gpus
            .values()
            .filter_map(|g| g.power_limit_w)
            .collect();
        limits.dedup();
        if !limits.is_empty() {
            let l: Vec<String> = limits.iter().map(|w| format!("{w} W")).collect();
            parts.push(format!("GPU {}", l.join("/")));
        }
        println!("  {:13} {}", name, parts.join(", "));
        for u in &r.unsupported {
            println!(
                "  {:13} {}",
                "",
                format!("{} {}: {}", u.target, u.setting, u.reason).yellow()
            );
        }
    }
}

fn line(o: &Outcome) -> String {
    let from = o.from.as_ref().map_or("?".into(), |v| v.to_string());
    let actual = o
        .actual
        .as_ref()
        .map_or(String::new(), |v| format!(", now {v}"));
    let why = o
        .reason
        .as_ref()
        .map_or(String::new(), |r| format!(" — {r}"));
    format!(
        "{} {}: {} → {}{}{}",
        o.target, o.setting, from, o.to, actual, why
    )
}

pub fn print_apply(r: &ApplyReport) {
    let head = if r.dry_run {
        "Dry run — nothing written"
    } else if !r.refused.is_empty() {
        "Refused — nothing written"
    } else {
        "Applied"
    };
    println!("{}", head.cyan().bold());
    for o in &r.applied {
        println!(
            "  {} {}",
            if r.dry_run {
                "~".normal()
            } else {
                "✓".green()
            },
            line(o)
        );
    }
    for o in &r.unchanged {
        println!("  {} {}", "=".dimmed(), line(o));
    }
    for o in r.refused.iter().chain(&r.failed) {
        println!("  {} {}", "✗".red(), line(o));
    }
    for o in &r.unsupported {
        println!("  {} {}", "-".yellow(), line(o));
    }
}
