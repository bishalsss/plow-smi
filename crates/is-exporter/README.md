# is-exporter

Prometheus metrics exporter backed by **`is-gpu`** (NVML / AMD SMI / Level Zero at runtime).

```bash
cargo run -p is-exporter -- --all
# http://0.0.0.0:9835/metrics
```

Flags: `--nvidia`, `--amd`, `--intel`, `--system`, `--tpu`, `--all`, `--port`, `--bind`.
