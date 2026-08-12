//! Export subcommand — starts the Prometheus metrics exporter.
//! Mirrors the logic in is-exporter's main.rs.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use axum::{Router, routing::get};
use tracing::{info, warn};

use is_exporter::collector;
use is_exporter::collector::manager::CollectorManager;
use is_exporter::collector::Collector;
use is_exporter::exporter::prometheus::metrics_handler;
use is_exporter::metrics::gpu_metrics::update_gpu_metrics;

pub fn run(nvidia: bool, amd: bool, system: bool, all: bool, port: u16, bind: String) -> Result<()> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    rt.block_on(async move {
        let enable_system = system || all;
        // Default: GPU (all vendors) + system when nothing was selected.
        let (enable_nvidia, enable_amd, enable_intel, enable_system) =
            if !nvidia && !amd && !system && !all {
                (true, true, true, true)
            } else if all {
                (true, true, true, true)
            } else {
                (nvidia, amd, false, enable_system)
            };

        let mut manager = CollectorManager::new();

        let filter = collector::gpu::VendorFilter {
            nvidia: enable_nvidia,
            amd: enable_amd,
            intel: enable_intel,
        };
        if filter.nvidia || filter.amd || filter.intel {
            info!(
                "Registering is-gpu collector (nvidia={} amd={} intel={})",
                filter.nvidia, filter.amd, filter.intel
            );
            let gpu = collector::gpu::GpuCollector::new(filter);
            if let Err(e) = manager.register(Box::new(gpu)).await {
                warn!(error = %e, "GPU collector failed to initialize");
            }
        }

        let interval = Duration::from_secs(5);
        let manager = Arc::new(manager);

        if manager.collector_count() > 0 {
            let manager_clone = Arc::clone(&manager);
            tokio::spawn(async move {
                info!("GPU metrics collection loop started");
                loop {
                    let snapshots = manager_clone.collect_all().await;
                    update_gpu_metrics(&snapshots);
                    tokio::time::sleep(interval).await;
                }
            });
        } else {
            warn!("No GPU collectors active");
        }

        if enable_system {
            use is_exporter::collector::system::SystemCollector;
            use is_exporter::metrics::system_metrics::update_system_metrics;

            let mut sys = SystemCollector::new();
            match Collector::init(&mut sys).await {
                Ok(_) => {
                    tokio::spawn(async move {
                        info!("System metrics collection loop started");
                        loop {
                            let snapshot = sys.collect_system_snapshot();
                            update_system_metrics(&snapshot);
                            tokio::time::sleep(interval).await;
                        }
                    });
                }
                Err(e) => warn!("System collector failed: {e}"),
            }
        }

        async fn health_handler() -> &'static str {
            "OK"
        }

        let app = Router::new()
            .route("/metrics", get(metrics_handler))
            .route("/health", get(health_handler))
            .route("/healthz", get(health_handler));

        let addr: SocketAddr = format!("{bind}:{port}")
            .parse()
            .map_err(|e| anyhow::anyhow!("Invalid address: {e}"))?;

        info!(%addr, "HTTP server starting");
        info!("Prometheus metrics at http://{}/metrics", addr);

        let listener = tokio::net::TcpListener::bind(addr).await?;
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                tokio::signal::ctrl_c().await.ok();
                info!("Shutting down...");
            })
            .await
            .map_err(|e| anyhow::anyhow!("Server error: {e}"))?;

        Ok(())
    })
}
