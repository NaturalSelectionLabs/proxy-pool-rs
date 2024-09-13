use std::{future::ready, net::SocketAddr};

use axum::{routing::get, Router};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use tokio::net::TcpListener;

pub fn setup_metrics_recorder() -> PrometheusHandle {
    PrometheusBuilder::new().install_recorder().unwrap()
}

pub fn routes() -> Router {
    let record_handle = setup_metrics_recorder();
    Router::new()
        .route("/healthz", get(|| async { "OK" }))
        .route("/metrics", get(move || ready(record_handle.render())))
}

pub async fn run(addr: SocketAddr) -> crate::error::Result<()> {
    let app = routes();
    let listener = TcpListener::bind(addr).await?;

    tracing::info!("Listening on: {}", listener.local_addr()?);
    axum::serve(listener, app).await?;

    Ok(())
}
