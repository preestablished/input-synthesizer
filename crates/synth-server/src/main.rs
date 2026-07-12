//! `synth-server` binary: tonic gRPC + plain HTTP (`/healthz`, `/metrics`)
//! shell for the input synthesizer (ARCHITECTURE.md §8).

use std::net::SocketAddr;
use std::sync::Arc;

use synth_proto::v1::input_synthesizer_server::InputSynthesizerServer;
use synth_server::SynthService;

struct Args {
    grpc_addr: SocketAddr,
    http_addr: SocketAddr,
    load_paths: Vec<String>,
}

fn parse_args() -> Args {
    let mut grpc_addr: SocketAddr = "0.0.0.0:7430".parse().expect("valid default addr");
    let mut http_addr: SocketAddr = "0.0.0.0:7431".parse().expect("valid default addr");
    let mut load_paths = Vec::new();

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--grpc-addr" => {
                let v = args.next().expect("--grpc-addr requires a value");
                grpc_addr = v.parse().unwrap_or_else(|e| {
                    eprintln!("invalid --grpc-addr {v:?}: {e}");
                    std::process::exit(2);
                });
            }
            "--http-addr" => {
                let v = args.next().expect("--http-addr requires a value");
                http_addr = v.parse().unwrap_or_else(|e| {
                    eprintln!("invalid --http-addr {v:?}: {e}");
                    std::process::exit(2);
                });
            }
            "--load" => {
                let v = args.next().expect("--load requires a value");
                load_paths.push(v);
            }
            other => {
                eprintln!("unrecognized argument: {other}");
                std::process::exit(2);
            }
        }
    }

    Args {
        grpc_addr,
        http_addr,
        load_paths,
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let args = parse_args();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    runtime.block_on(run(args))
}

/// One `--load` argument: either a plain path (kind sniffed from the
/// document's `kind:` field) or `<path>:<kind>` with `kind` one of
/// `experiment_config` / `macro_pack` (explicit, overrides sniffing).
fn split_load_arg(arg: &str) -> (&str, Option<&str>) {
    match arg.rsplit_once(':') {
        Some((path, kind @ ("experiment_config" | "macro_pack"))) => (path, Some(kind)),
        _ => (arg, None),
    }
}

/// Sniff a loaded document's kind from its top-level `kind:` field. Returns
/// the raw string (e.g. `"experiment_config"`, `"macro_pack"`) or empty if
/// unparseable/absent — the caller treats anything unrecognized as fatal.
fn sniff_kind(bytes: &[u8]) -> String {
    let value: serde_yaml::Value = match serde_yaml::from_slice(bytes) {
        Ok(v) => v,
        Err(_) => return String::new(),
    };
    value
        .get("kind")
        .and_then(serde_yaml::Value::as_str)
        .unwrap_or("")
        .to_owned()
}

async fn run(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    let service = Arc::new(SynthService::new());

    // Standalone bring-up: --load accepts experiment configs and macro
    // packs, either `<path>:<kind>` (explicit) or a plain path (kind sniffed
    // from the document's own `kind:` field).
    for raw in &args.load_paths {
        let (path, explicit_kind) = split_load_arg(raw);
        let bytes = std::fs::read(path)
            .unwrap_or_else(|e| panic!("failed to read --load document {path:?}: {e}"));
        let kind = match explicit_kind {
            Some(k) => k.to_owned(),
            None => sniff_kind(&bytes),
        };
        match kind.as_str() {
            "experiment_config" => match service.load_experiment_config(&bytes) {
                Ok(doc_id) => {
                    tracing::info!(path, document_id = %doc_id, "loaded experiment config");
                }
                Err(msg) => {
                    eprintln!("failed to load {path:?}: {msg}");
                    std::process::exit(1);
                }
            },
            "macro_pack" => match service.load_macro_pack_doc(&bytes) {
                Ok(pack_id) => tracing::info!(path, pack_id = %pack_id, "loaded macro pack"),
                Err(msg) => {
                    eprintln!("failed to load {path:?}: {msg}");
                    std::process::exit(1);
                }
            },
            other => {
                eprintln!(
                    "failed to load {path:?}: unrecognized or missing document kind {other:?} \
                     (expected \"experiment_config\" or \"macro_pack\"; use --load <path>:<kind> \
                     to specify explicitly)"
                );
                std::process::exit(1);
            }
        }
    }

    let http_listener = tokio::net::TcpListener::bind(args.http_addr).await?;
    tracing::info!(
        grpc_addr = %args.grpc_addr,
        http_addr = %args.http_addr,
        synth_version = synth_core::SYNTH_VERSION,
        "synth-server starting"
    );

    let (grpc_shutdown_tx, grpc_shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let (http_shutdown_tx, http_shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    let grpc_addr = args.grpc_addr;
    let grpc_service = Arc::clone(&service);
    let grpc_task = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(InputSynthesizerServer::from_arc(grpc_service))
            .serve_with_shutdown(grpc_addr, async {
                let _ = grpc_shutdown_rx.await;
            })
            .await
    });

    let http_service = Arc::clone(&service);
    let http_task = tokio::spawn(async move {
        synth_server::http::serve(http_listener, http_service, http_shutdown_rx).await
    });

    tokio::signal::ctrl_c().await?;
    tracing::info!("shutdown signal received");
    let _ = grpc_shutdown_tx.send(());
    let _ = http_shutdown_tx.send(());

    grpc_task.await??;
    http_task.await??;
    Ok(())
}
