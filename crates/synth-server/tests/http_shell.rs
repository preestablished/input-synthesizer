//! HTTP sidecar shell tests (`/healthz`, `/metrics`) — ARCHITECTURE.md §8.
//! The gRPC surface has its own suite (grpc_integration.rs); this one pins
//! the plain-HTTP endpoints the orchestrator's probes and Prometheus scrape.

use std::sync::Arc;

use synth_server::SynthService;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct HttpServer {
    addr: std::net::SocketAddr,
    shutdown_tx: tokio::sync::oneshot::Sender<()>,
    task: tokio::task::JoinHandle<()>,
}

async fn start_http(service: Arc<SynthService>) -> HttpServer {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral port");
    let addr = listener.local_addr().expect("local_addr");
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let task = tokio::spawn(async move {
        synth_server::http::serve(listener, service, shutdown_rx)
            .await
            .expect("http server ran without accept error");
    });
    tokio::task::yield_now().await;
    HttpServer {
        addr,
        shutdown_tx,
        task,
    }
}

/// Minimal raw HTTP/1.1 GET so the test needs no client dependency; the
/// server side is real hyper.
async fn get(addr: std::net::SocketAddr, path: &str) -> String {
    let mut stream = tokio::net::TcpStream::connect(addr)
        .await
        .expect("connect to http shell");
    let request = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .await
        .expect("write request");
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .await
        .expect("read response");
    response
}

#[tokio::test]
async fn http_shell_serves_healthz_and_metrics() {
    let service = Arc::new(SynthService::new());
    let server = start_http(Arc::clone(&service)).await;

    let healthz = get(server.addr, "/healthz").await;
    assert!(
        healthz.starts_with("HTTP/1.1 200"),
        "healthz status: {healthz}"
    );
    assert!(healthz.ends_with("ok"), "healthz body: {healthz}");

    let metrics = get(server.addr, "/metrics").await;
    assert!(
        metrics.starts_with("HTTP/1.1 200"),
        "metrics status: {metrics}"
    );
    // Scalar metrics (gauges/counters/histogram) are present from
    // registration; label-vec families only appear once a child exists, so
    // they are pinned by the increment test in grpc_integration.rs instead.
    for name in [
        "synth_propose_latency_seconds",
        "synth_macro_packs_loaded",
        "synth_experiments_loaded",
        "synth_mine_runs_total",
        "synth_policy_fallback_total",
    ] {
        assert!(metrics.contains(name), "missing {name} in: {metrics}");
    }

    let not_found = get(server.addr, "/nope").await;
    assert!(
        not_found.starts_with("HTTP/1.1 404"),
        "unknown path status: {not_found}"
    );

    let _ = server.shutdown_tx.send(());
    server.task.await.expect("clean shutdown");
}
