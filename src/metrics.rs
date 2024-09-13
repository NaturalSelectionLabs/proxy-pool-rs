use axum::{routing::get, Router};
use lazy_static::lazy_static;
use prometheus::{
    proto::MetricFamily, register_int_counter, register_int_counter_vec, Encoder, IntCounter,
    IntCounterVec, TextEncoder,
};
use std::net::SocketAddr;
use tokio::net::TcpListener;

lazy_static! {
    pub static ref HTTP_REQUEST_COUNTER: IntCounterVec = register_int_counter_vec!(
        "http_requests_total",
        "Total number of HTTP requests",
        &["method"]
    )
    .unwrap();
    pub static ref HTTP_ERROR_COUNTER: IntCounter =
        register_int_counter!("http_errors_total", "Total number of HTTP errors").unwrap();
    pub static ref SOCKS5_REQUEST_COUNTER: IntCounter =
        register_int_counter!("socks5_requests_total", "Total number of SOCKS5 requests").unwrap();
    pub static ref SOCKS5_ERROR_COUNTER: IntCounter =
        register_int_counter!("socks5_errors_total", "Total number of SOCKS5 errors").unwrap();
}

fn metrics_string(f: Vec<MetricFamily>) -> String {
    let encoder = TextEncoder::new();
    let mut buffer = vec![];

    match encoder.encode(&f, &mut buffer) {
        Ok(_) => (),
        Err(e) => {
            println!("Failed to encode metrics: {}", e);
            return String::from("Failed to encode metrics");
        }
    }

    String::from_utf8(buffer.clone()).unwrap_or(String::from("Failed to encode metrics"))
}
async fn metrics() -> String {
    let families = prometheus::gather();
    metrics_string(families)
}

pub fn routes() -> Router {
    Router::new()
        .route("/healthz", get(|| async { "OK" }))
        .route("/metrics", get(metrics))
}

pub async fn run(addr: SocketAddr) -> crate::error::Result<()> {
    let app = routes();
    let listener = TcpListener::bind(addr).await?;

    tracing::info!("Listening on: {}", listener.local_addr()?);
    axum::serve(listener, app).await?;

    Ok(())
}
