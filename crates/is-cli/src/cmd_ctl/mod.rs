//! Ctl subcommand — GPU power & clock control.
//! Delegates entirely to the `is-ctl` library (DRY principle).

use anyhow::Result;
use crate::CtlCommands;

pub fn run(cmd: CtlCommands) -> Result<()> {
    match cmd {
        CtlCommands::List { format } => is_ctl::list::list_all(&format),
        CtlCommands::NvidiaList { format } => is_ctl::nvidia::list_gpus(&format),
        CtlCommands::NvidiaInfo { gpu, format } => is_ctl::nvidia::gpu_info(gpu, &format),
        CtlCommands::NvidiaSetClocks {
            gpu,
            mem,
            graphics,
            all,
        } => {
            if all {
                is_ctl::nvidia::set_clocks_all(mem, graphics)
            } else {
                let idx = gpu.ok_or_else(|| anyhow::anyhow!("Specify GPU index or use --all"))?;
                is_ctl::nvidia::set_clocks(idx, mem, graphics)
            }
        }
        CtlCommands::NvidiaSetPowerLimit { gpu, watts } => {
            is_ctl::nvidia::set_power_limit(gpu, watts)
        }
        CtlCommands::NvidiaSetPerf { gpu, level, all } => {
            if all {
                is_ctl::nvidia::set_perf_all(&level)
            } else {
                let idx = gpu.ok_or_else(|| anyhow::anyhow!("Specify GPU index or use --all"))?;
                is_ctl::nvidia::set_perf(idx, &level)
            }
        }
        CtlCommands::NvidiaReset { gpu, all } => {
            if all {
                is_ctl::nvidia::reset_all()
            } else {
                let idx = gpu.ok_or_else(|| anyhow::anyhow!("Specify GPU index or use --all"))?;
                is_ctl::nvidia::reset_clocks(idx)
            }
        }
        CtlCommands::NvidiaSupportedClocks { gpu, format } => {
            is_ctl::nvidia::supported_clocks(gpu, &format)
        }
        CtlCommands::AmdList { format } => is_ctl::amd::list_gpus(&format),
        CtlCommands::AmdInfo { gpu, format } => is_ctl::amd::gpu_info(gpu, &format),
        CtlCommands::AmdSetPowerLimit { gpu, watts } => is_ctl::amd::set_power_limit(gpu, watts),
        CtlCommands::AmdSetPerf { gpu, level, all } => {
            if all {
                is_ctl::amd::set_perf_all(&level)
            } else {
                let idx = gpu.ok_or_else(|| anyhow::anyhow!("Specify GPU index or use --all"))?;
                is_ctl::amd::set_perf(idx, &level)
            }
        }
        CtlCommands::AmdReset { gpu, all } => {
            if all {
                is_ctl::amd::reset_all()
            } else {
                let idx = gpu.ok_or_else(|| anyhow::anyhow!("Specify GPU index or use --all"))?;
                is_ctl::amd::reset_clocks(idx)
            }
        }
        CtlCommands::IntelList { format } => is_ctl::intel::list_gpus(&format),
        CtlCommands::IntelInfo { gpu, format } => is_ctl::intel::gpu_info(gpu, &format),
    }
}
