//! Plain HTTP server: `GET /healthz` and `GET /metrics` (ARCHITECTURE.md
//! §8). No gRPC here — this is the conventional sidecar surface for
//! liveness probes and Prometheus scraping.

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;

use http_body_util::Full;
use hyper::body::Bytes;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use hyper_util::server::conn::auto::Builder as ConnBuilder;
use tokio::net::TcpListener;

use crate::SynthService;

async fn handle(
    req: Request<hyper::body::Incoming>,
    service: Arc<SynthService>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let response = match (req.method(), req.uri().path()) {
        (&Method::GET, "/healthz") => Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/plain")
            .body(Full::new(Bytes::from_static(b"ok")))
            .expect("valid response"),
        (&Method::GET, "/metrics") => {
            let body = service.metrics.encode();
            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "text/plain; version=0.0.4")
                .body(Full::new(Bytes::from(body)))
                .expect("valid response")
        }
        _ => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Full::new(Bytes::from_static(b"not found")))
            .expect("valid response"),
    };
    Ok(response)
}

/// Serve `/healthz` and `/metrics` on `addr` until `shutdown` resolves.
/// Returns the bound address (useful for tests that bind an ephemeral
/// port).
pub async fn serve(
    listener: TcpListener,
    service: Arc<SynthService>,
    mut shutdown: tokio::sync::oneshot::Receiver<()>,
) -> std::io::Result<()> {
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, _peer): (_, SocketAddr) = accepted?;
                let io = TokioIo::new(stream);
                let service = Arc::clone(&service);
                tokio::spawn(async move {
                    let svc = service_fn(move |req| handle(req, Arc::clone(&service)));
                    if let Err(err) = ConnBuilder::new(hyper_util::rt::TokioExecutor::new())
                        .serve_connection(io, svc)
                        .await
                    {
                        tracing::warn!(error = %err, "http connection error");
                    }
                });
            }
            _ = &mut shutdown => {
                return Ok(());
            }
        }
    }
}
