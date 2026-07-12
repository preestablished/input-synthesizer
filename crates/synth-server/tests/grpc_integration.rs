//! In-process tonic client<->server integration tests (ARCHITECTURE.md §8,
//! plan `03-m1-pad-weighted-random-grpc.md` §5.6).

use std::net::SocketAddr;
use std::sync::Arc;

use synth_proto::v1::input_synthesizer_client::InputSynthesizerClient;
use synth_proto::v1::input_synthesizer_server::InputSynthesizerServer;
use synth_proto::v1::{
    load_macro_pack_request, DocumentKind, HealthRequest, LoadMacroPackRequest, MineMacrosRequest,
    ModelKind, NodeContext, ProposeBurstsRequest,
};
use synth_server::SynthService;
use tonic::transport::Channel;
use tonic::Code;

/// One running in-process server plus the means to shut it down. Dropping
/// this (or calling `shutdown`) tears the server down so a fresh one can be
/// started for the statelessness test.
struct TestServer {
    addr: SocketAddr,
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<()>,
}

impl TestServer {
    async fn start() -> Self {
        Self::start_with(Arc::new(SynthService::new())).await
    }

    async fn start_with(service: Arc<SynthService>) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind ephemeral port");
        let addr = listener.local_addr().expect("local_addr");
        let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let task = tokio::spawn(async move {
            tonic::transport::Server::builder()
                .add_service(InputSynthesizerServer::from_arc(service))
                .serve_with_incoming_shutdown(incoming, async {
                    let _ = shutdown_rx.await;
                })
                .await
                .expect("server ran without transport error");
        });
        // Give the listener a moment to actually start accepting.
        tokio::task::yield_now().await;
        Self {
            addr,
            shutdown_tx: Some(shutdown_tx),
            task,
        }
    }

    async fn client(&self) -> InputSynthesizerClient<Channel> {
        let url = format!("http://{}", self.addr);
        for _ in 0..50 {
            if let Ok(c) = InputSynthesizerClient::connect(url.clone()).await {
                return c;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        InputSynthesizerClient::connect(url)
            .await
            .expect("connect to in-process server")
    }

    async fn shutdown(mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        let _ = self.task.await;
    }
}

fn minimal_config_bytes() -> Vec<u8> {
    std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/config/valid/minimal.yaml"),
    )
    .expect("read minimal.yaml")
}

fn load_request(bytes: Vec<u8>) -> LoadMacroPackRequest {
    LoadMacroPackRequest {
        source: Some(load_macro_pack_request::Source::DocumentYaml(bytes)),
        kind: DocumentKind::ExperimentConfig as i32,
    }
}

fn propose_request(experiment_id: &str, k: u32, node_id: &str, seed: u64) -> ProposeBurstsRequest {
    ProposeBurstsRequest {
        experiment_id: experiment_id.to_owned(),
        node_context: Some(NodeContext {
            node_id: node_id.to_owned(),
            ..Default::default()
        }),
        k,
        length_hint: 0,
        seed,
        model: ModelKind::Pad as i32,
        config_overrides_yaml: Vec::new(),
    }
}

#[tokio::test]
async fn happy_path_propose_bursts() {
    let server = TestServer::start().await;
    let mut client = server.client().await;

    let load_resp = client
        .load_macro_pack(load_request(minimal_config_bytes()))
        .await
        .expect("load config")
        .into_inner();
    assert_eq!(load_resp.items_loaded, 1);
    assert_eq!(load_resp.document_id.len(), 64); // hex blake3-256

    let resp = client
        .propose_bursts(propose_request("exp-test", 8, "n", 42))
        .await
        .expect("propose bursts")
        .into_inner();

    assert_eq!(resp.bursts.len(), 8);
    for pb in &resp.bursts {
        assert!(pb.burst.is_some());
        let prov = pb.provenance.as_ref().expect("provenance present");
        assert!(!prov.rng_stream.is_empty());
    }
    let reasons: Vec<&str> = resp.degraded.iter().map(|d| d.reason.as_str()).collect();
    assert!(reasons.contains(&"no_macros_loaded"));
    assert!(reasons.contains(&"no_parent_burst"));
    assert_eq!(resp.config_fingerprint.len(), 32);
    assert_eq!(resp.seed, 42);
    assert!(!resp.synth_version.is_empty());

    server.shutdown().await;
}

#[tokio::test]
async fn context_free_request_never_fails() {
    let server = TestServer::start().await;
    let mut client = server.client().await;
    client
        .load_macro_pack(load_request(minimal_config_bytes()))
        .await
        .expect("load config");

    // NodeContext{node_id} only: no ram_features, no recent_inputs, no
    // parent_burst, no siblings — the zero-adjustment / zero-context path.
    let resp = client
        .propose_bursts(propose_request("exp-test", 4, "solo-node", 7))
        .await
        .expect("context-free propose must never fail");
    assert_eq!(resp.into_inner().bursts.len(), 4);

    server.shutdown().await;
}

#[tokio::test]
async fn determinism_same_request_same_bursts() {
    let server = TestServer::start().await;
    let mut client = server.client().await;
    client
        .load_macro_pack(load_request(minimal_config_bytes()))
        .await
        .expect("load config");

    let req = || propose_request("exp-test", 16, "n", 0xDEAD_BEEF);
    let a = client
        .propose_bursts(req())
        .await
        .expect("propose 1")
        .into_inner();
    let b = client
        .propose_bursts(req())
        .await
        .expect("propose 2")
        .into_inner();

    let ids_a: Vec<Vec<u8>> = a
        .bursts
        .iter()
        .map(|p| p.burst.as_ref().unwrap().burst_id.clone())
        .collect();
    let ids_b: Vec<Vec<u8>> = b
        .bursts
        .iter()
        .map(|p| p.burst.as_ref().unwrap().burst_id.clone())
        .collect();
    assert_eq!(ids_a, ids_b);

    for (pa, pb) in a.bursts.iter().zip(&b.bursts) {
        let ba = pa.burst.as_ref().unwrap();
        let bb = pb.burst.as_ref().unwrap();
        assert_eq!(ba.body, bb.body, "full segment lists must match");
    }
    assert_eq!(a.config_fingerprint, b.config_fingerprint);

    server.shutdown().await;
}

#[tokio::test]
async fn k_zero_is_invalid_argument() {
    let server = TestServer::start().await;
    let mut client = server.client().await;
    client
        .load_macro_pack(load_request(minimal_config_bytes()))
        .await
        .expect("load config");

    let status = client
        .propose_bursts(propose_request("exp-test", 0, "n", 1))
        .await
        .expect_err("k=0 must fail");
    assert_eq!(status.code(), Code::InvalidArgument);
    assert!(
        status.message().contains('k'),
        "message: {}",
        status.message()
    );

    server.shutdown().await;
}

#[tokio::test]
async fn k_over_256_is_invalid_argument() {
    let server = TestServer::start().await;
    let mut client = server.client().await;
    client
        .load_macro_pack(load_request(minimal_config_bytes()))
        .await
        .expect("load config");

    let status = client
        .propose_bursts(propose_request("exp-test", 300, "n", 1))
        .await
        .expect_err("k=300 must fail");
    assert_eq!(status.code(), Code::InvalidArgument);
    assert!(
        status.message().contains('k'),
        "message: {}",
        status.message()
    );

    server.shutdown().await;
}

#[tokio::test]
async fn unknown_experiment_id_is_invalid_argument() {
    let server = TestServer::start().await;
    let mut client = server.client().await;

    let status = client
        .propose_bursts(propose_request("does-not-exist", 4, "n", 1))
        .await
        .expect_err("unknown experiment_id must fail");
    assert_eq!(status.code(), Code::InvalidArgument);
    assert!(status.message().contains("experiment_id"));

    server.shutdown().await;
}

#[tokio::test]
async fn event_grammar_model_is_invalid_argument() {
    let server = TestServer::start().await;
    let mut client = server.client().await;
    client
        .load_macro_pack(load_request(minimal_config_bytes()))
        .await
        .expect("load config");

    let mut req = propose_request("exp-test", 4, "n", 1);
    req.model = ModelKind::EventGrammar as i32;
    let status = client
        .propose_bursts(req)
        .await
        .expect_err("event_grammar model must fail in M1");
    assert_eq!(status.code(), Code::InvalidArgument);

    server.shutdown().await;
}

#[tokio::test]
async fn malformed_overrides_yaml_is_invalid_argument() {
    let server = TestServer::start().await;
    let mut client = server.client().await;
    client
        .load_macro_pack(load_request(minimal_config_bytes()))
        .await
        .expect("load config");

    let mut req = propose_request("exp-test", 4, "n", 1);
    req.config_overrides_yaml = b"{ not yaml".to_vec();
    let status = client
        .propose_bursts(req)
        .await
        .expect_err("malformed overrides must fail");
    assert_eq!(status.code(), Code::InvalidArgument);
    assert!(
        status.message().contains("config_overrides_yaml"),
        "message: {}",
        status.message()
    );

    server.shutdown().await;
}

#[tokio::test]
async fn missing_node_id_is_invalid_argument() {
    let server = TestServer::start().await;
    let mut client = server.client().await;
    client
        .load_macro_pack(load_request(minimal_config_bytes()))
        .await
        .expect("load config");

    let mut req = propose_request("exp-test", 4, "n", 1);
    req.node_context = Some(NodeContext {
        node_id: String::new(),
        ..Default::default()
    });
    let status = client
        .propose_bursts(req)
        .await
        .expect_err("missing node_id must fail");
    assert_eq!(status.code(), Code::InvalidArgument);
    assert!(
        status.message().contains("node_id"),
        "message: {}",
        status.message()
    );

    server.shutdown().await;
}

#[tokio::test]
async fn event_grammar_document_load_is_exact_message() {
    let server = TestServer::start().await;
    let mut client = server.client().await;

    let req = LoadMacroPackRequest {
        source: Some(load_macro_pack_request::Source::DocumentYaml(
            b"whatever".to_vec(),
        )),
        kind: DocumentKind::EventGrammar as i32,
    };
    let status = client
        .load_macro_pack(req)
        .await
        .expect_err("event_grammar document load must fail");
    assert_eq!(status.code(), Code::InvalidArgument);
    assert_eq!(
        status.message(),
        "event_grammar documents are not supported until M5"
    );

    server.shutdown().await;
}

#[tokio::test]
async fn macro_pack_document_load_is_placeholder_error() {
    let server = TestServer::start().await;
    let mut client = server.client().await;

    let req = LoadMacroPackRequest {
        source: Some(load_macro_pack_request::Source::DocumentYaml(
            b"whatever".to_vec(),
        )),
        kind: DocumentKind::MacroPack as i32,
    };
    let status = client
        .load_macro_pack(req)
        .await
        .expect_err("macro pack document load must fail in M1");
    assert_eq!(status.code(), Code::InvalidArgument);
    assert_eq!(status.message(), "macro packs land with M2");

    server.shutdown().await;
}

#[tokio::test]
async fn artifact_ref_source_is_invalid_argument() {
    let server = TestServer::start().await;
    let mut client = server.client().await;

    let req = LoadMacroPackRequest {
        source: Some(load_macro_pack_request::Source::ArtifactRef(
            "some-ref".to_owned(),
        )),
        kind: DocumentKind::ExperimentConfig as i32,
    };
    let status = client
        .load_macro_pack(req)
        .await
        .expect_err("artifact_ref source must fail in standalone mode");
    assert_eq!(status.code(), Code::InvalidArgument);
    assert!(status.message().contains("document_yaml"));

    server.shutdown().await;
}

#[tokio::test]
async fn identical_config_reload_is_a_noop_with_same_id() {
    let server = TestServer::start().await;
    let mut client = server.client().await;

    let first = client
        .load_macro_pack(load_request(minimal_config_bytes()))
        .await
        .expect("first load")
        .into_inner();
    let second = client
        .load_macro_pack(load_request(minimal_config_bytes()))
        .await
        .expect("second load of identical document")
        .into_inner();

    assert_eq!(first.document_id, second.document_id);

    server.shutdown().await;
}

#[tokio::test]
async fn mine_macros_is_unimplemented() {
    let server = TestServer::start().await;
    let mut client = server.client().await;

    let status = client
        .mine_macros(MineMacrosRequest {
            experiment_id: "exp-test".to_owned(),
            paths: Vec::new(),
            params: None,
        })
        .await
        .expect_err("MineMacros must be unimplemented in M1");
    assert_eq!(status.code(), Code::Unimplemented);

    server.shutdown().await;
}

#[tokio::test]
async fn health_reflects_loaded_experiments() {
    let server = TestServer::start().await;
    let mut client = server.client().await;

    let before = client
        .health(HealthRequest {})
        .await
        .expect("health before load")
        .into_inner();
    assert!(before.loaded_experiments.is_empty());
    assert!(!before.synth_version.is_empty());

    client
        .load_macro_pack(load_request(minimal_config_bytes()))
        .await
        .expect("load config");

    let after = client
        .health(HealthRequest {})
        .await
        .expect("health after load")
        .into_inner();
    assert_eq!(after.loaded_experiments, vec!["exp-test".to_owned()]);
    assert!(after.loaded_packs.is_empty());
    assert!(!after.policy_endpoint_up);
    assert!(!after.policy_deterministic);
    assert!(!after.mining_in_progress);

    server.shutdown().await;
}

#[tokio::test]
async fn statelessness_restart_reproduces_identical_bursts() {
    let server1 = TestServer::start().await;
    let mut client1 = server1.client().await;
    client1
        .load_macro_pack(load_request(minimal_config_bytes()))
        .await
        .expect("load config on server 1");
    let resp1 = client1
        .propose_bursts(propose_request("exp-test", 8, "n", 123))
        .await
        .expect("propose on server 1")
        .into_inner();
    server1.shutdown().await;

    // A brand new in-process server, no shared state whatsoever.
    let server2 = TestServer::start().await;
    let mut client2 = server2.client().await;
    client2
        .load_macro_pack(load_request(minimal_config_bytes()))
        .await
        .expect("load config on server 2");
    let resp2 = client2
        .propose_bursts(propose_request("exp-test", 8, "n", 123))
        .await
        .expect("propose on server 2")
        .into_inner();
    server2.shutdown().await;

    let ids1: Vec<Vec<u8>> = resp1
        .bursts
        .iter()
        .map(|p| p.burst.as_ref().unwrap().burst_id.clone())
        .collect();
    let ids2: Vec<Vec<u8>> = resp2
        .bursts
        .iter()
        .map(|p| p.burst.as_ref().unwrap().burst_id.clone())
        .collect();
    assert_eq!(ids1, ids2);
    for (pa, pb) in resp1.bursts.iter().zip(&resp2.bursts) {
        assert_eq!(
            pa.burst.as_ref().unwrap().body,
            pb.burst.as_ref().unwrap().body
        );
    }
    assert_eq!(resp1.config_fingerprint, resp2.config_fingerprint);
}
