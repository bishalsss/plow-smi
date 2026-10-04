# Apple Silicon support

Status: implemented on `feat/apple-silicon-metrics`; hardware validated on Apple M4 Pro,
macOS 26.5.1. Other hardware remains unverified.

## Architecture

`plows-gpu` remains the reusable Rust integration layer. `Vendor::Apple` and
`AppleBackend` implement the existing `GpuBackend` interface. One integrated
GPU represents the host SoC, with host-local identity `apple-soc-0` and no PCI
address or dedicated-memory capacity. `GpuManager::discover_filtered` avoids
probing unselected vendors and starting unused sampler threads.

`DeviceMetrics::soc` holds optional `SocMetrics`. Reusable `EngineMetrics`
describe CPU/NPU measurements; CPU tiers carry OS-provided names. Adding a
default `soc_metrics` trait method preserves existing backend implementations.
Adding a public struct field requires downstream struct literals to use
`..Default::default()`; the workspace remains version 0.1.0.

The Apple sampler owns native handles on one background thread. It samples
once per second and publishes an immutable `Arc<DeviceMetrics>`. `refresh()`
only reads the cached pointer; getters never call native APIs or spawn commands.
Owned `snapshot()` copies optional sensor vectors for consumers that need them;
scalar getters avoid those copies. Samples older than three seconds disappear.
Shutdown interrupts the timer and joins the worker. A blocked OS call cannot
be forcibly interrupted. Consumers should retain one backend/manager, not
rediscover on each refresh.

## Native sources

- IOReport: runtime-loaded `/usr/lib/libIOReport.dylib`, filtered CPU core/GPU
  residency and CPU/GPU/ANE energy channels. Energy uses actual elapsed time
  and explicit J/mJ/uJ/nJ units. No frequency-times-voltage power estimates.
- IOKit: runtime-loaded OS framework, registry DVFS tables read once; SMC
  read-only sensor requests with cached key metadata.
- IOHID: optional runtime-loaded temperature fallback when SMC sensors are
  unavailable; service list and sensor names cached.
- sysctl/Mach: CPU topology and physical RAM, CPU-load fallback, system-wide
  unified-memory use. Missing private APIs do not disable these sources.
- CoreFoundation and libSystem: linked macOS system dependencies, not vendor
  SDKs. The executable is native ARM64, not fully static.

Raw pointers stay within `ffi/apple.rs`. Create/Copy CF objects, registry
handles, SMC connections and Mach send rights are released. No unsafe
`Send`/`Sync` implementations are used for the Apple backend.

## Metric semantics

GPU metrics reuse existing `gpu_*` names. CPU/NPU use `apple_cpu_*` and
`apple_npu_*`; tiers use `apple_cpu_cluster_*{cluster=...}`. Sensor temperatures
use `apple_temperature_celsius{sensor=...}`. Standard device/host labels apply.
GPU power gauges preserve fractional watts, unlike the previous integer gauges.
Existing GPU utilization/temperature snapshot fields still have integer precision.

Unified RAM uses `apple_unified_memory_{used,total}_bytes`, never GPU VRAM
gauges. CPU+GPU+ANE power is `apple_compute_power_watts`; it is not whole-system
power. `apple_system_power_watts` requires the separate SMC PSTR sensor.
Temperature averages follow commonly used SMC CPU/GPU key prefixes; individual
sensor names remain available and private key attribution is not guaranteed.

Unsupported counters are `None`, JSON `null`, TUI `N/A`, and absent Prometheus
series. ANE power is available on the tested host; ANE utilization, frequency,
temperature and core count are unavailable and are not inferred from power.
Optional ANE residency channels are accepted only when the OS exposes them.
Clock tables with mismatched state counts omit frequency rather than guess.
CPU residency aggregates require complete core coverage, otherwise Mach CPU
load is used. Power sums require all contributing channel readings to be valid.

## Applications

- `plow-smi export --apple --system` / `plows-exporter --apple --system`:
  existing collection loop publishes SoC metrics; scrapes only read gauges.
- `plow-smi top`: CPU/GPU/ANE rows, explicitly shared RAM and unavailable values.
  The TUI reuses its Tokio runtime instead of rebuilding it on every refresh.
- `plow-smi ctl apple-info --format json` / `plows-ctl apple info`:
  read-only JSON/text inspection. One-shot commands wait 1.1 seconds for a
  complete energy delta; they may report null if sampling is delayed.
- `plows-ctl capabilities`: Apple appears as read-only; apply rejects power
  control. No clock limits, power limits, persistence or GPU process accounting.
- `plows-gpu`: direct Rust integration for Plower/PlowRT, with no subprocess or
  HTTP dependency. No C ABI is provided.

## Compatibility and validation

The backend is gated to `aarch64-apple-darwin`; unsupported hosts return an
explicit error. M1–M3 CPU tables use Hz, M4+ use kHz, GPUs use Hz; newer tier
tables can be located through `acc-clusters`. Ultra-prefixed CPU/energy channels
are accepted. Unknown layouts degrade to CPU/memory and optional power metrics.
M1/M2/M3/M5 and Ultra variants are intended compatibility, not hardware-certified.
Intel Macs do not gain an Apple backend. Nix package metadata includes ARM64 Darwin.

Tests cover energy units and invalid deltas, residency and table mismatch,
vendor/filter flags, unsupported hosts, missing metric removal and precision.
An ignored live Apple test checks valid CPU/RAM, no fabricated VRAM,
non-blocking refresh and interruptible shutdown. The `apple` example measures
cached refresh plus owned snapshot overhead. Private Apple APIs are not stable
contracts; further chip/macOS validation should compare aligned sampling windows
against `powermetrics` and use real GPU/Core ML workloads, not guessed thresholds.

### Local review results

- Workspace all-features tests: 43 passed, 4 hardware tests skipped; the Apple
  hardware test was additionally run explicitly and passed.
- `plows-gpu` all-target Clippy with warnings denied passed. Workspace Clippy
  completed with existing style warnings in other modules.
- Full CLI and HTTP exporter validated on the M4 Pro without sudo; median
  local HTTP scrape was approximately 0.69 ms for the debug executable.
- Release microbenchmark: cached refresh plus scalar getter approximately
  93 ns/call; refresh plus owned sensor snapshot approximately 3.8 microseconds.
  These are local measurements, not performance guarantees.
- Linux cross-build and Nix package derivation were not run locally.
