//! plows-ctl — GPU power & clock control CLI (via plows-gpu).
//!
//! Usage:
//!   plows-ctl list                             # All vendors
//!   plows-ctl nvidia list | amd list | intel list
//!   plows-ctl nvidia set-power-limit 0 --watts 300
//!   plows-ctl amd set-perf 0 --level high

use clap::{Parser, Subcommand};
use plows_ctl::{OutputFormat, PerfLevel};

#[derive(Parser)]
#[command(
    name = "plows-ctl",
    version,
    about = "GPU list & control CLI (NVIDIA / AMD / Intel via plows-gpu)",
    long_about = "List GPUs across vendors and control NVIDIA/AMD power & clocks. \
                  Requires root/sudo for most set operations."
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List every GPU discovered (NVIDIA + AMD + Intel)
    List {
        #[arg(long, default_value = "text")]
        format: OutputFormat,
    },

    /// NVIDIA GPU control operations
    #[command(subcommand)]
    Nvidia(NvidiaCommands),

    /// AMD GPU control operations
    #[command(subcommand)]
    Amd(AmdCommands),

    /// Intel GPU info (metrics; Level Zero)
    #[command(subcommand)]
    Intel(IntelCommands),
}

#[derive(Subcommand)]
enum NvidiaCommands {
    /// List all detected NVIDIA GPUs
    List {
        /// Output format
        #[arg(long, default_value = "text")]
        format: OutputFormat,
    },

    /// Show detailed info for a specific GPU
    Info {
        /// GPU index (0-based)
        gpu: u32,
        /// Output format
        #[arg(long, default_value = "text")]
        format: OutputFormat,
    },

    /// Set application clock speeds
    SetClocks {
        /// GPU index (0-based), or use --all
        gpu: Option<u32>,
        /// Target memory clock (MHz)
        #[arg(long)]
        mem: u32,
        /// Target graphics/core clock (MHz)
        #[arg(long)]
        graphics: u32,
        /// Apply to all GPUs
        #[arg(long)]
        all: bool,
    },

    /// Set power limit (watts)
    SetPowerLimit {
        /// GPU index (0-based)
        gpu: u32,
        /// Power limit in watts
        #[arg(long)]
        watts: u32,
    },

    /// Set performance level
    SetPerf {
        /// GPU index (0-based), or use --all
        gpu: Option<u32>,
        /// Performance level: auto, low, high
        #[arg(long)]
        level: PerfLevel,
        /// Apply to all GPUs
        #[arg(long)]
        all: bool,
    },

    /// Reset clocks to default
    Reset {
        /// GPU index (0-based)
        gpu: Option<u32>,
        /// Reset all GPUs
        #[arg(long)]
        all: bool,
    },

    /// Show supported clock speeds for a GPU
    SupportedClocks {
        /// GPU index (0-based)
        gpu: u32,
        /// Output format
        #[arg(long, default_value = "text")]
        format: OutputFormat,
    },
}

#[derive(Subcommand)]
enum AmdCommands {
    /// List all detected AMD GPUs
    List {
        /// Output format
        #[arg(long, default_value = "text")]
        format: OutputFormat,
    },

    /// Show detailed info for a specific GPU
    Info {
        /// GPU index (0-based)
        gpu: u32,
        /// Output format
        #[arg(long, default_value = "text")]
        format: OutputFormat,
    },

    /// Set power limit (watts)
    SetPowerLimit {
        /// GPU index (0-based)
        gpu: u32,
        /// Power limit in watts
        #[arg(long)]
        watts: u32,
    },

    /// Set performance level
    SetPerf {
        /// GPU index (0-based), or use --all
        gpu: Option<u32>,
        /// Performance level: auto, low, high
        #[arg(long)]
        level: PerfLevel,
        /// Apply to all GPUs
        #[arg(long)]
        all: bool,
    },

    /// Reset to default settings
    Reset {
        /// GPU index (0-based)
        gpu: Option<u32>,
        /// Reset all GPUs
        #[arg(long)]
        all: bool,
    },
}

#[derive(Subcommand)]
enum IntelCommands {
    /// List all detected Intel GPUs
    List {
        #[arg(long, default_value = "text")]
        format: OutputFormat,
    },

    /// Show detailed info for a specific GPU
    Info {
        gpu: u32,
        #[arg(long, default_value = "text")]
        format: OutputFormat,
    },
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "warn".into()),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::List { format } => plows_ctl::list::list_all(&format),
        Commands::Nvidia(cmd) => run_nvidia(cmd),
        Commands::Amd(cmd) => run_amd(cmd),
        Commands::Intel(cmd) => run_intel(cmd),
    }
}

fn run_nvidia(cmd: NvidiaCommands) -> anyhow::Result<()> {
    use plows_ctl::nvidia;

    match cmd {
        NvidiaCommands::List { format } => nvidia::list_gpus(&format),
        NvidiaCommands::Info { gpu, format } => nvidia::gpu_info(gpu, &format),
        NvidiaCommands::SetClocks {
            gpu,
            mem,
            graphics,
            all,
        } => {
            if all {
                nvidia::set_clocks_all(mem, graphics)
            } else {
                let idx = gpu.ok_or_else(|| anyhow::anyhow!("Specify GPU index or use --all"))?;
                nvidia::set_clocks(idx, mem, graphics)
            }
        }
        NvidiaCommands::SetPowerLimit { gpu, watts } => nvidia::set_power_limit(gpu, watts),
        NvidiaCommands::SetPerf { gpu, level, all } => {
            if all {
                nvidia::set_perf_all(&level)
            } else {
                let idx = gpu.ok_or_else(|| anyhow::anyhow!("Specify GPU index or use --all"))?;
                nvidia::set_perf(idx, &level)
            }
        }
        NvidiaCommands::Reset { gpu, all } => {
            if all {
                nvidia::reset_all()
            } else {
                let idx = gpu.ok_or_else(|| anyhow::anyhow!("Specify GPU index or use --all"))?;
                nvidia::reset_clocks(idx)
            }
        }
        NvidiaCommands::SupportedClocks { gpu, format } => nvidia::supported_clocks(gpu, &format),
    }
}

fn run_amd(cmd: AmdCommands) -> anyhow::Result<()> {
    use plows_ctl::amd;

    match cmd {
        AmdCommands::List { format } => amd::list_gpus(&format),
        AmdCommands::Info { gpu, format } => amd::gpu_info(gpu, &format),
        AmdCommands::SetPowerLimit { gpu, watts } => amd::set_power_limit(gpu, watts),
        AmdCommands::SetPerf { gpu, level, all } => {
            if all {
                amd::set_perf_all(&level)
            } else {
                let idx = gpu.ok_or_else(|| anyhow::anyhow!("Specify GPU index or use --all"))?;
                amd::set_perf(idx, &level)
            }
        }
        AmdCommands::Reset { gpu, all } => {
            if all {
                amd::reset_all()
            } else {
                let idx = gpu.ok_or_else(|| anyhow::anyhow!("Specify GPU index or use --all"))?;
                amd::reset_clocks(idx)
            }
        }
    }
}

fn run_intel(cmd: IntelCommands) -> anyhow::Result<()> {
    use plows_ctl::intel;
    match cmd {
        IntelCommands::List { format } => intel::list_gpus(&format),
        IntelCommands::Info { gpu, format } => intel::gpu_info(gpu, &format),
    }
}
