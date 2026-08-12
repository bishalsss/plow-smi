# InferSight

Production-grade GPU observability and control for heterogeneous compute — NVIDIA, AMD, and Intel — from one Rust workspace.

**No compile-time GPU SDKs.** Vendor libraries (`libnvidia-ml`, `libamd_smi`, `libze_loader`) are loaded at runtime with `dlopen`. One binary works on CPU-only hosts and enables GPUs automatically when drivers are present.

## Crates

| Crate | Role |
|-------|------|
| **is-gpu** | Shared GPU layer: discover backends, metrics, processes, power/clock control |
| **is-exporter** | Prometheus `/metrics` HTTP server |
| **is-top** | Interactive TUI (htop for GPUs) |
| **is-ctl** | List / info / set power & clocks |
| **is-cli** | Unified `infersight` binary (`export`, `top`, `ctl`) |

See [ARCHITECTURE.md](ARCHITECTURE.md) for the design.

## Quick start

```bash
git clone https://github.com/infervisor/infersight.git
cd infersight
cargo build --release
```

```bash
# Prometheus exporter
./target/release/is-exporter --all
# → http://0.0.0.0:9835/metrics

# Terminal monitor
./target/release/is-top

# Control
./target/release/is-ctl nvidia list
./target/release/is-ctl amd list

# Unified CLI
./target/release/infersight export --all
./target/release/infersight top
./target/release/infersight ctl nvidia-list
```

Dev:

```bash
cargo run -p is-gpu --example discover
cargo run -p is-exporter -- --nvidia --system
cargo test -p is-gpu
```

## Requirements

| Need | At build | At runtime |
|------|----------|------------|
| CUDA / ROCm / oneAPI SDKs | **Not required** | Not required |
| NVIDIA driver + `libnvidia-ml.so.1` | — | Optional (auto) |
| AMD ROCm + `libamd_smi.so` | — | Optional (auto) |
| Intel Level Zero + `libze_loader.so` | — | Optional (auto) |

Missing vendors are logged and skipped — never crash the process.

## Features

- Multi-vendor metrics (util, memory, temp, power, clocks, fan)
- Runtime backend discovery (multiple vendors active at once)
- Prometheus metrics with per-device labels
- TUI with GPU + system + process view
- Power limits / clocks / perf levels (NVIDIA + AMD; needs privileges)
- Structured logging via `tracing`

## License

Licensed under Apache-2.0 or MIT, at your option.
