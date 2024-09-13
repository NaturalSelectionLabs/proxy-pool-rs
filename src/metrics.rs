use std::future::ready;

use axum::{routing::get, Router};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};

pub fn setup_metrics_recorder() -> PrometheusHandle {
    PrometheusBuilder::new().install_recorder().unwrap()
}

pub fn routes() -> Router {
    let record_handle = setup_metrics_recorder();
    Router::new()
        .route("/healthz", get(|| async { "OK" }))
        .route("/metrics", get(move || ready(record_handle.render())))
}
