//! v1-gate smoke: 1,000 consecutive ProposeBursts against a real
//! input-synthesizer endpoint, driven through the exploration-orchestrator's
//! own client/driver layer (GeneratedInputSynthClient, SynthBringup,
//! derive_synth_request_seed, propose_bursts_with_fingerprint_guard) in the
//! documented context-free fallback mode (NodeContext carrying only ids).

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use orch_clients::input_synth::{
    Burst, BurstBody, LoadMacroPackSource, ModelKind, NodeContext, ProposeBurstsRequest,
};
use orch_core::rng::derive_synth_request_seed;
use orch_core::types::{
    CellKey, FrameCount, NodeId, Novelty, Score, SnapshotRef, Stage, StateHash,
};
use orch_driver::input_synth::{
    propose_bursts_with_fingerprint_guard, FingerprintRegistry, GeneratedInputSynthClient,
    GeneratedInputSynthConfig, SynthBringup,
};

// Repo root relative to this harness (scripts/smoke-harness/).
const REPO: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
// console16-12btn-v1 bit table (API.md §5.1).
const UP: u32 = 1 << 6;
const DOWN: u32 = 1 << 7;
const LEFT: u32 = 1 << 8;
const RIGHT: u32 = 1 << 9;

fn main() {
    let mut args = std::env::args().skip(1);
    let endpoint = args
        .next()
        .unwrap_or_else(|| "http://127.0.0.1:7430".to_owned());
    let calls: u64 = args.next().map(|s| s.parse().unwrap()).unwrap_or(1000);
    let k: u32 = args.next().map(|s| s.parse().unwrap()).unwrap_or(32);

    let pack_bytes =
        std::fs::read(format!("{REPO}/packs/console16-movement-core.yaml")).expect("read pack");
    let pack_id = blake3::hash(&pack_bytes).to_hex().to_string();

    // Experiment config: console16 alphabet, wr/macro 50/50 mix, demo pack
    // referenced BY PACK ID (the orchestrator bring-up checks macro.packs
    // entries against Health.loaded_packs, which carries content-hash ids).
    let experiment_id = "exp-v1-smoke";
    let config_yaml = format!(
        r#"version: 1
kind: experiment_config
experiment_id: {experiment_id}
model: pad
button_alphabet:
  name: console16-12btn-v1
  buttons:
    [ {{name: A, bit: 0}}, {{name: B, bit: 1}}, {{name: X, bit: 2}}, {{name: Y, bit: 3}},
      {{name: L, bit: 4}}, {{name: R, bit: 5}}, {{name: UP, bit: 6}},
      {{name: DOWN, bit: 7}}, {{name: LEFT, bit: 8}}, {{name: RIGHT, bit: 9}},
      {{name: START, bit: 10}}, {{name: SELECT, bit: 11}} ]
  exclusive_groups: [ [UP, DOWN], [LEFT, RIGHT] ]
  forbidden_masks: [ {{ mask: [START, SELECT], clear: [SELECT] }} ]
  directions: {{ group: [UP, DOWN, LEFT, RIGHT], allow_diagonals: true }}
generator_mix:
  weighted_random: 0.5
  macro: 0.5
  mutation: 0.0
  policy: 0.0
macro:
  packs: [ "{pack_id}" ]
"#
    );

    let bringup = SynthBringup::from_sources(
        experiment_id,
        LoadMacroPackSource::DocumentYaml(config_yaml.into_bytes()),
        vec![LoadMacroPackSource::DocumentYaml(pack_bytes)],
    )
    .expect("bringup sources");

    let mut client = GeneratedInputSynthClient::connect(
        GeneratedInputSynthConfig::new(endpoint.clone()).with_deadline(Duration::from_secs(10)),
    )
    .expect("connect");

    let report = bringup.run(&mut client).expect("bring-up");
    println!(
        "bring-up ok: experiment_config id {}, pack ids {:?}, synth_version {}",
        report.experiment_config.document_id,
        report
            .macro_pack_documents
            .iter()
            .map(|d| d.document_id.as_str())
            .collect::<Vec<_>>(),
        report.health.synth_version,
    );

    let mut registry = FingerprintRegistry::new();
    let experiment_seed: u64 = 0x5EED_F00D_2026_0712;
    let mut errors = 0u64;
    let mut illegal_bursts = 0u64;
    let mut total_bursts = 0u64;
    let mut macro_slots = 0u64;
    let mut latencies_us: Vec<u128> = Vec::with_capacity(calls as usize);
    let mut fingerprint_hex = String::new();

    for batch_seq in 0..calls {
        let seed = derive_synth_request_seed(experiment_seed, batch_seq);
        let request = ProposeBurstsRequest {
            experiment_id: experiment_id.to_owned(),
            node_context: NodeContext {
                node_id: NodeId::new(batch_seq + 1),
                parent_node_id: None,
                snapshot_ref: SnapshotRef::new([0; 32]),
                state_hash: StateHash::new([0; 32]),
                cell_key: CellKey::new(0),
                stage: Stage::new(0),
                depth: 0,
                frame_counter: FrameCount::new(0),
                node_score: Score::new(0.0).unwrap(),
                novelty: Novelty::new(0.0).unwrap(),
                ram_features: BTreeMap::new(),
                frame_embedding: Vec::new(),
                recent_inputs: None,
                parent_burst: None,
                sibling_bursts: Vec::new(),
            },
            k,
            length_hint: FrameCount::new(300),
            seed,
            model: ModelKind::Pad,
            config_overrides_yaml: Vec::new(),
        };

        let start = Instant::now();
        match propose_bursts_with_fingerprint_guard(&mut client, &bringup, &mut registry, request, 0)
        {
            Ok(response) => {
                latencies_us.push(start.elapsed().as_micros());
                fingerprint_hex = response
                    .config_fingerprint
                    .0
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect();
                for pb in &response.bursts {
                    total_bursts += 1;
                    if pb.provenance.macro_provenance.is_some() {
                        macro_slots += 1;
                    }
                    if !burst_is_legal(&pb.burst) {
                        illegal_bursts += 1;
                        eprintln!("ILLEGAL burst at batch {batch_seq} slot {}", pb.provenance.slot);
                    }
                }
            }
            Err(e) => {
                errors += 1;
                eprintln!("call {batch_seq} FAILED: {e}");
            }
        }
        if (batch_seq + 1) % 100 == 0 {
            println!("{}/{calls} calls done", batch_seq + 1);
        }
    }

    latencies_us.sort_unstable();
    let pct = |p: f64| -> u128 {
        if latencies_us.is_empty() {
            return 0;
        }
        latencies_us[((latencies_us.len() as f64 - 1.0) * p) as usize]
    };
    println!("--- v1 smoke summary ---");
    println!("endpoint: {endpoint}");
    println!("calls: {calls} (k={k}) errors: {errors}");
    println!("bursts: {total_bursts} (macro slots: {macro_slots})");
    println!("client-side illegal bursts: {illegal_bursts}");
    println!("config_fingerprint: {fingerprint_hex}");
    println!(
        "latency us: p50={} p99={} max={}",
        pct(0.50),
        pct(0.99),
        latencies_us.last().copied().unwrap_or(0)
    );
    if errors > 0 || illegal_bursts > 0 {
        std::process::exit(1);
    }
}

/// Client-side legality validation against console16-12btn-v1 + API.md §1
/// invariants. (Hypervisor-side validation is the gate's authoritative
/// counter; this is the fallback-mode proxy, recorded as such.)
fn burst_is_legal(burst: &Burst) -> bool {
    let BurstBody::Pad(pad) = &burst.body else {
        return false;
    };
    if pad.segments.is_empty() {
        return false;
    }
    let mut total: u64 = 0;
    let mut prev_mask: Option<u32> = None;
    for seg in &pad.segments {
        let mask = seg.buttons;
        if seg.hold_frames.get() == 0 {
            return false;
        }
        if mask & UP != 0 && mask & DOWN != 0 {
            return false;
        }
        if mask & LEFT != 0 && mask & RIGHT != 0 {
            return false;
        }
        if prev_mask == Some(mask) {
            return false;
        }
        prev_mask = Some(mask);
        total += u64::from(seg.hold_frames.get());
    }
    (16..=1800).contains(&total)
}
