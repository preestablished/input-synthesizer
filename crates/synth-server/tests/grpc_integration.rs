//! In-process tonic client<->server integration tests (ARCHITECTURE.md §8,
//! plan `03-m1-pad-weighted-random-grpc.md` §5.6).

use std::net::SocketAddr;
use std::sync::Arc;

use synth_core::types::{to_proto, Burst, PadBurst, PadSegment};
use synth_proto::v1::input_synthesizer_client::InputSynthesizerClient;
use synth_proto::v1::input_synthesizer_server::InputSynthesizerServer;
use synth_proto::v1::{
    load_macro_pack_request, DocumentKind, HealthRequest, LoadMacroPackRequest, MineMacrosRequest,
    ModelKind, NodeContext, ProposeBurstsRequest, ProvenancedBurst,
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

fn console16_pack_bytes() -> Vec<u8> {
    std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../packs/console16-movement-core.yaml"),
    )
    .expect("read console16-movement-core.yaml")
}

fn macro_pack_load_request(bytes: Vec<u8>) -> LoadMacroPackRequest {
    LoadMacroPackRequest {
        source: Some(load_macro_pack_request::Source::DocumentYaml(bytes)),
        kind: DocumentKind::MacroPack as i32,
    }
}

/// Build an experiment config document (string-edited from
/// `testdata/config/valid/minimal.yaml`, same `console16-12btn-v1` alphabet
/// as `packs/console16-movement-core.yaml`) with a given `experiment_id`,
/// `macro.packs` list, and `generator_mix` weighted_random/macro split
/// (mutation and policy pinned to 0 so the split is exact).
fn macro_mix_config_bytes(
    experiment_id: &str,
    pack_names: &[&str],
    weighted_random: f64,
    macro_weight: f64,
) -> Vec<u8> {
    let base = String::from_utf8(minimal_config_bytes()).expect("minimal.yaml is utf8");
    let base = base.replace(
        "experiment_id: exp-test",
        &format!("experiment_id: {experiment_id}"),
    );
    let packs_yaml: String = pack_names.iter().map(|p| format!("    - {p}\n")).collect();
    format!(
        "{base}\n\
         generator_mix:\n\
         \x20\x20weighted_random: {weighted_random}\n\
         \x20\x20macro: {macro_weight}\n\
         \x20\x20mutation: 0.0\n\
         \x20\x20policy: 0.0\n\
         macro:\n\
         \x20\x20packs:\n\
         {packs_yaml}"
    )
    .into_bytes()
}

fn pad_total_frames(burst: &synth_proto::v1::Burst) -> u64 {
    match &burst.body {
        Some(synth_proto::v1::burst::Body::Pad(pad)) => {
            pad.segments.iter().map(|s| u64::from(s.hold_frames)).sum()
        }
        _ => panic!("expected a pad burst body"),
    }
}

/// Macros in `console16-movement-core.yaml` with zero declared params.
const NO_PARAM_MACROS: &[&str] = &["door-enter", "menu-confirm"];

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
async fn k_256_is_success() {
    let server = TestServer::start().await;
    let mut client = server.client().await;
    client
        .load_macro_pack(load_request(minimal_config_bytes()))
        .await
        .expect("load config");

    let resp = client
        .propose_bursts(propose_request("exp-test", 256, "n", 1))
        .await
        .expect("k=256 must succeed")
        .into_inner();
    assert_eq!(resp.bursts.len(), 256);

    server.shutdown().await;
}

#[tokio::test]
async fn k_257_is_invalid_argument() {
    let server = TestServer::start().await;
    let mut client = server.client().await;
    client
        .load_macro_pack(load_request(minimal_config_bytes()))
        .await
        .expect("load config");

    let status = client
        .propose_bursts(propose_request("exp-test", 257, "n", 1))
        .await
        .expect_err("k=257 must fail");
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
async fn macro_pack_document_load_is_invalid_argument_for_broken_yaml() {
    let server = TestServer::start().await;
    let mut client = server.client().await;

    // Unterminated flow sequence: a parse error, not a validation error, so
    // the message carries a `(line L, column C)` location (API.md §2.2).
    let broken = b"version: 1\nkind: macro_pack\nname: bad-pack\nmodel: pad\n\
                   button_alphabet: console16-12btn-v1\nsource: handwritten\n\
                   macros:\n  - name: foo\n    steps: [\n"
        .to_vec();
    let req = LoadMacroPackRequest {
        source: Some(load_macro_pack_request::Source::DocumentYaml(broken)),
        kind: DocumentKind::MacroPack as i32,
    };
    let status = client
        .load_macro_pack(req)
        .await
        .expect_err("malformed macro pack document must fail");
    assert_eq!(status.code(), Code::InvalidArgument);
    assert!(
        status.message().to_lowercase().contains("line"),
        "message: {}",
        status.message()
    );

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

// ---------------------------------------------------------------------
// M2: macro packs
// ---------------------------------------------------------------------

#[tokio::test]
async fn load_console16_macro_pack_succeeds() {
    let server = TestServer::start().await;
    let mut client = server.client().await;

    let resp = client
        .load_macro_pack(macro_pack_load_request(console16_pack_bytes()))
        .await
        .expect("load console16-movement-core")
        .into_inner();

    assert_eq!(resp.items_loaded, 10);
    assert_eq!(resp.document_id.len(), 64, "hex blake3-256 pack_id");
    assert!(
        resp.document_id.chars().all(|c| c.is_ascii_hexdigit()),
        "document_id: {}",
        resp.document_id
    );
    assert!(resp.warnings.is_empty());

    server.shutdown().await;
}

#[tokio::test]
async fn identical_macro_pack_reload_is_a_noop_with_same_id() {
    let server = TestServer::start().await;
    let mut client = server.client().await;

    let first = client
        .load_macro_pack(macro_pack_load_request(console16_pack_bytes()))
        .await
        .expect("first load")
        .into_inner();
    let second = client
        .load_macro_pack(macro_pack_load_request(console16_pack_bytes()))
        .await
        .expect("second load of identical pack")
        .into_inner();

    assert_eq!(first.document_id, second.document_id);
    assert_eq!(second.items_loaded, 10);
    assert!(second.warnings.is_empty());

    server.shutdown().await;
}

#[tokio::test]
async fn propose_bursts_with_macro_mix_assigns_macro_slots() {
    let server = TestServer::start().await;
    let mut client = server.client().await;

    let pack_resp = client
        .load_macro_pack(macro_pack_load_request(console16_pack_bytes()))
        .await
        .expect("load console16-movement-core")
        .into_inner();
    let pack_id = pack_resp.document_id;

    client
        .load_macro_pack(load_request(macro_mix_config_bytes(
            "exp-macro-mix",
            &["console16-movement-core"],
            0.5,
            0.5,
        )))
        .await
        .expect("load macro-mix config");

    let resp = client
        .propose_bursts(propose_request("exp-macro-mix", 32, "n", 0xC0FFEE))
        .await
        .expect("propose bursts with macro mix")
        .into_inner();

    assert_eq!(resp.bursts.len(), 32);

    let reasons: Vec<&str> = resp.degraded.iter().map(|d| d.reason.as_str()).collect();
    assert!(
        !reasons.contains(&"no_macros_loaded"),
        "degraded: {reasons:?}"
    );

    let mut macro_slots = 0;
    for pb in &resp.bursts {
        let prov = pb.provenance.as_ref().expect("provenance present");
        let Some(mp) = prov.r#macro.as_ref() else {
            continue;
        };
        macro_slots += 1;

        assert_eq!(mp.pack_id, pack_id);
        assert_eq!(
            u64::from(mp.macro_frames) + u64::from(mp.tail_frames),
            pad_total_frames(pb.burst.as_ref().expect("burst present"))
        );
        if !NO_PARAM_MACROS.contains(&mp.macro_name.as_str()) {
            assert!(
                !mp.param_bindings.is_empty(),
                "macro {:?} declares params but bindings are empty",
                mp.macro_name
            );
        }
    }
    // Stratified floors over an exact 0.5/0.5 split of k=32 with no
    // remainder: exactly half the slots.
    assert_eq!(macro_slots, 16);

    server.shutdown().await;
}

#[tokio::test]
async fn propose_bursts_referencing_unloaded_pack_is_failed_precondition() {
    let server = TestServer::start().await;
    let mut client = server.client().await;

    client
        .load_macro_pack(load_request(macro_mix_config_bytes(
            "exp-missing-pack",
            &["never-loaded-pack"],
            0.5,
            0.5,
        )))
        .await
        .expect("load config referencing an unloaded pack");

    let status = client
        .propose_bursts(propose_request("exp-missing-pack", 4, "n", 1))
        .await
        .expect_err("unloaded pack + macro weight > 0 must fail");
    assert_eq!(status.code(), Code::FailedPrecondition);
    assert!(
        status.message().contains("never-loaded-pack"),
        "message: {}",
        status.message()
    );

    server.shutdown().await;
}

/// Fix #13 (API.md §5): pack presence must be checked unconditionally at
/// ProposeBursts time, even when the effective config's macro mix weight is
/// 0.0 — a previous bug only checked presence when macro weight > 0.
#[tokio::test]
async fn unloaded_pack_with_zero_macro_weight_is_still_failed_precondition() {
    let server = TestServer::start().await;
    let mut client = server.client().await;

    client
        .load_macro_pack(load_request(macro_mix_config_bytes(
            "exp-missing-pack-zero-weight",
            &["never-loaded-pack"],
            1.0,
            0.0,
        )))
        .await
        .expect("load config referencing an unloaded pack with macro weight 0");

    let status = client
        .propose_bursts(propose_request("exp-missing-pack-zero-weight", 4, "n", 1))
        .await
        .expect_err("unloaded pack must fail unconditionally, regardless of macro weight");
    assert_eq!(status.code(), Code::FailedPrecondition);
    assert!(
        status.message().contains("never-loaded-pack"),
        "message: {}",
        status.message()
    );

    server.shutdown().await;
}

#[tokio::test]
async fn config_fingerprint_changes_after_loading_an_additional_pack() {
    let server = TestServer::start().await;
    let mut client = server.client().await;

    client
        .load_macro_pack(load_request(minimal_config_bytes()))
        .await
        .expect("load config");

    let before = client
        .propose_bursts(propose_request("exp-test", 4, "n", 1))
        .await
        .expect("propose before pack load")
        .into_inner();

    client
        .load_macro_pack(macro_pack_load_request(console16_pack_bytes()))
        .await
        .expect("load console16-movement-core");

    let after = client
        .propose_bursts(propose_request("exp-test", 4, "n", 1))
        .await
        .expect("propose after pack load")
        .into_inner();

    // INTEGRATION §3 audit trail: the fingerprint reflects the loaded-pack
    // set, not just the packs a given config references.
    assert_ne!(before.config_fingerprint, after.config_fingerprint);

    server.shutdown().await;
}

#[tokio::test]
async fn macro_mix_determinism_same_request_same_bursts() {
    let server = TestServer::start().await;
    let mut client = server.client().await;

    client
        .load_macro_pack(macro_pack_load_request(console16_pack_bytes()))
        .await
        .expect("load console16-movement-core");
    client
        .load_macro_pack(load_request(macro_mix_config_bytes(
            "exp-macro-determinism",
            &["console16-movement-core"],
            0.5,
            0.5,
        )))
        .await
        .expect("load macro-mix config");

    let req = || propose_request("exp-macro-determinism", 16, "n", 0xDEAD_BEEF);
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

    server.shutdown().await;
}

#[tokio::test]
async fn health_loaded_packs_contains_loaded_pack_id() {
    let server = TestServer::start().await;
    let mut client = server.client().await;

    let pack_resp = client
        .load_macro_pack(macro_pack_load_request(console16_pack_bytes()))
        .await
        .expect("load console16-movement-core")
        .into_inner();

    let health = client
        .health(HealthRequest {})
        .await
        .expect("health after pack load")
        .into_inner();
    assert!(health.loaded_packs.contains(&pack_resp.document_id));
    // The declared pack NAME must appear too: the orchestrator's bring-up
    // (SynthBringup::run) requires the experiment config's macro.packs
    // entries — names or ids — verbatim in this list, and name-based
    // configs are the documented default style (API.md §5.6).
    assert!(
        health
            .loaded_packs
            .contains(&"console16-movement-core".to_owned()),
        "Health.loaded_packs must cover declared pack names, not only ids: {:?}",
        health.loaded_packs
    );

    server.shutdown().await;
}

// ---------------------------------------------------------------------
// M3: mutation (context decode, provenance wire-out, availability)
// ---------------------------------------------------------------------

/// `minimal.yaml` with the generator mix pinned to pure mutation, so every
/// slot is forced through `synth_gen::mutation` when a parent (or sibling)
/// burst is present.
fn mutation_mix_config_bytes(experiment_id: &str) -> Vec<u8> {
    let base = String::from_utf8(minimal_config_bytes()).expect("minimal.yaml is utf8");
    let base = base.replace(
        "experiment_id: exp-test",
        &format!("experiment_id: {experiment_id}"),
    );
    format!(
        "{base}\n\
         generator_mix:\n\
         \x20\x20weighted_random: 0.0\n\
         \x20\x20macro: 0.0\n\
         \x20\x20mutation: 1.0\n\
         \x20\x20policy: 0.0\n"
    )
    .into_bytes()
}

/// A tiny legal one-segment pad burst wrapped as a `ProvenancedBurst`, with
/// a valid content-addressed `burst_id` (`synth_core::types::to_proto`
/// stamps it), for use as `NodeContext.parent_burst`/`sibling_bursts[].burst`.
fn small_provenanced_burst() -> ProvenancedBurst {
    let burst = Burst::Pad(PadBurst {
        segments: vec![PadSegment {
            buttons: 1,
            hold_frames: 16,
        }],
    });
    let proto_burst = to_proto(&burst, "console16-12btn-v1");
    ProvenancedBurst {
        burst: Some(proto_burst),
        provenance: None,
    }
}

#[tokio::test]
async fn parent_burst_flows_through_to_mutation_provenance() {
    let server = TestServer::start().await;
    let mut client = server.client().await;

    client
        .load_macro_pack(load_request(mutation_mix_config_bytes("exp-mutation")))
        .await
        .expect("load mutation-mix config");

    let parent = small_provenanced_burst();
    let parent_burst_id = parent.burst.as_ref().unwrap().burst_id.clone();

    let mut req = propose_request("exp-mutation", 8, "n", 0xF00D);
    req.node_context = Some(NodeContext {
        node_id: "n".to_owned(),
        parent_burst: Some(parent),
        ..Default::default()
    });

    let resp = client
        .propose_bursts(req)
        .await
        .expect("propose with parent_burst")
        .into_inner();

    assert_eq!(resp.bursts.len(), 8);
    let reasons: Vec<&str> = resp.degraded.iter().map(|d| d.reason.as_str()).collect();
    assert!(
        !reasons.contains(&"no_parent_burst"),
        "degraded: {reasons:?}"
    );

    let mut mutation_slots = 0;
    for pb in &resp.bursts {
        let prov = pb.provenance.as_ref().expect("provenance present");
        let Some(mp) = prov.mutation.as_ref() else {
            continue;
        };
        mutation_slots += 1;
        assert_eq!(mp.base_burst_id, parent_burst_id);
    }
    // Pure-mutation mix, no siblings: every slot bases off the parent.
    assert_eq!(mutation_slots, 8);

    server.shutdown().await;
}

/// Fix #1 (wire-reachable panic): a pure-mutation mix with no parent/
/// siblings must degrade to weighted-random for every slot instead of
/// panicking inside the mixer.
#[tokio::test]
async fn mutation_only_mix_without_parent_falls_back_to_weighted_random() {
    let server = TestServer::start().await;
    let mut client = server.client().await;

    client
        .load_macro_pack(load_request(mutation_mix_config_bytes(
            "exp-mutation-fallback",
        )))
        .await
        .expect("load mutation-mix config");

    let resp = client
        .propose_bursts(propose_request("exp-mutation-fallback", 8, "n", 1))
        .await
        .expect("propose must succeed via the weighted-random fallback")
        .into_inner();
    assert_eq!(resp.bursts.len(), 8);

    let reasons: Vec<&str> = resp.degraded.iter().map(|d| d.reason.as_str()).collect();
    assert!(
        reasons.contains(&"no_parent_burst"),
        "degraded: {reasons:?}"
    );
    for pb in &resp.bursts {
        let prov = pb.provenance.as_ref().expect("provenance present");
        assert_eq!(
            prov.generator,
            synth_proto::v1::GeneratorKind::WeightedRandom as i32,
            "every slot must fall back to weighted_random"
        );
    }

    server.shutdown().await;
}

#[tokio::test]
async fn malformed_parent_burst_id_is_invalid_argument() {
    let server = TestServer::start().await;
    let mut client = server.client().await;

    client
        .load_macro_pack(load_request(mutation_mix_config_bytes(
            "exp-mutation-bad-id",
        )))
        .await
        .expect("load mutation-mix config");

    let mut parent = small_provenanced_burst();
    // 5 bytes instead of the required 32.
    parent.burst.as_mut().unwrap().burst_id = vec![1, 2, 3, 4, 5];

    let mut req = propose_request("exp-mutation-bad-id", 4, "n", 1);
    req.node_context = Some(NodeContext {
        node_id: "n".to_owned(),
        parent_burst: Some(parent),
        ..Default::default()
    });

    let status = client
        .propose_bursts(req)
        .await
        .expect_err("malformed parent_burst.burst_id must fail");
    assert_eq!(status.code(), Code::InvalidArgument);
    assert!(
        status.message().contains("burst_id") && status.message().contains("32 bytes"),
        "message: {}",
        status.message()
    );

    server.shutdown().await;
}
