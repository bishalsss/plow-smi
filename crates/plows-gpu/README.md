# plows-gpu

Cross-platform GPU metrics and control via **runtime** `dlopen` of:

| Vendor | Library |
|--------|---------|
| NVIDIA | `libnvidia-ml.so.1` |
| AMD | `libamd_smi.so` |
| Intel | `libze_loader.so` |

No compile-time CUDA / ROCm / oneAPI dependency.

```rust
use plows_gpu::{GpuBackend, GpuManager};

let mut mgr = GpuManager::discover();
mgr.refresh_all();
for b in mgr.backends() {
    println!("{}: {} device(s)", b.vendor(), b.device_count());
}
```

Control example:

```rust
use plows_gpu::NvidiaBackend;

let nv = NvidiaBackend::try_load()?;
nv.set_power_limit(0, 250_000)?; // milliwatts
```

See the workspace [ARCHITECTURE.md](../../ARCHITECTURE.md) and crate docs (`cargo doc -p plows-gpu --open`).
