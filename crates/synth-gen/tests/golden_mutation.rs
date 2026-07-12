//! M3 mutation-instantiation goldens (`06-m3-mutation-stretch.md` Accept
//! list: "Per-operator unit goldens ... both arches. Golden comparison over
//! canonical forms"). 12 cases: one forced-op case per operator (including a
//! `splice`-without-donor fallback case), plus 4 default-op-probs cases
//! across parent-only / siblings-only / parent-and-siblings contexts.
//!
//! Canonical-form rule (00- global rule, restated from `golden_seed.rs`/
//! `golden_macro.rs`): goldens hash the **domain** burst
//! (`synth_core::types::burst_hash`), never raw prost wire bytes;
//! `MutationOp.args` is folded in as its own sorted `Vec<(K,V)>` (never a
//! proto `map<>`).

#[path = "common/mod.rs"]
mod common;

use serde::{Deserialize, Serialize};
use synth_core::rng::fanout_root;
use synth_core::types::burst_hash;
use synth_gen::context::GenContext;
use synth_gen::mutation;
use synth_pad::PadModel;

const CASE_COUNT: usize = 12;

fn goldens_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/golden/m3/goldens.yaml")
}

#[derive(Clone, Copy)]
enum CtxKind {
    ParentOnly,
    SiblingsOnly,
    Both,
}

struct CaseSpec {
    name: &'static str,
    seed: u64,
    forced_op: Option<&'static str>,
    ctx_kind: CtxKind,
}

/// Deterministic case list — both the recorder and the replay test call
/// this and must therefore always agree, as long as this function itself
/// hasn't changed (a deliberate change to it IS a golden-format change and
/// must go through the version-bump + regeneration path, same as
/// `golden_seed.rs`'s `build_cases`).
fn build_cases() -> Vec<CaseSpec> {
    let cases = vec![
        CaseSpec {
            name: "perturb_timing_forced",
            seed: 200,
            forced_op: Some("perturb_timing"),
            ctx_kind: CtxKind::ParentOnly,
        },
        CaseSpec {
            name: "extend_forced",
            seed: 201,
            forced_op: Some("extend"),
            ctx_kind: CtxKind::ParentOnly,
        },
        CaseSpec {
            name: "flip_button_forced",
            seed: 202,
            forced_op: Some("flip_button"),
            ctx_kind: CtxKind::ParentOnly,
        },
        CaseSpec {
            name: "splice_forced_with_siblings",
            seed: 203,
            forced_op: Some("splice"),
            ctx_kind: CtxKind::Both,
        },
        CaseSpec {
            name: "splice_forced_fallback_to_extend",
            seed: 204,
            forced_op: Some("splice"),
            ctx_kind: CtxKind::ParentOnly,
        },
        CaseSpec {
            name: "truncate_forced",
            seed: 205,
            forced_op: Some("truncate"),
            ctx_kind: CtxKind::ParentOnly,
        },
        CaseSpec {
            name: "duplicate_segment_forced",
            seed: 206,
            forced_op: Some("duplicate_segment"),
            ctx_kind: CtxKind::ParentOnly,
        },
        CaseSpec {
            name: "swap_adjacent_forced",
            seed: 207,
            forced_op: Some("swap_adjacent"),
            ctx_kind: CtxKind::ParentOnly,
        },
        CaseSpec {
            name: "default_mix_parent_only",
            seed: 300,
            forced_op: None,
            ctx_kind: CtxKind::ParentOnly,
        },
        CaseSpec {
            name: "default_mix_siblings_only",
            seed: 301,
            forced_op: None,
            ctx_kind: CtxKind::SiblingsOnly,
        },
        CaseSpec {
            name: "default_mix_parent_and_siblings_a",
            seed: 302,
            forced_op: None,
            ctx_kind: CtxKind::Both,
        },
        CaseSpec {
            name: "default_mix_parent_and_siblings_b",
            seed: 303,
            forced_op: None,
            ctx_kind: CtxKind::Both,
        },
    ];
    assert_eq!(
        cases.len(),
        CASE_COUNT,
        "golden suite must have exactly 12 cases"
    );
    cases
}

fn ctx_for(kind: CtxKind) -> GenContext {
    match kind {
        CtxKind::ParentOnly => {
            common::ctx_with_parent("golden-m3-node", common::mutation_base_pad())
        }
        CtxKind::SiblingsOnly => common::ctx_with_siblings_only(
            "golden-m3-node",
            vec![
                (common::mutation_sibling_pad_a(), 1.0),
                (common::mutation_sibling_pad_b(), 2.0),
            ],
        ),
        CtxKind::Both => common::ctx_with_parent_and_siblings(
            "golden-m3-node",
            common::mutation_base_pad(),
            vec![
                (common::mutation_sibling_pad_a(), 1.0),
                (common::mutation_sibling_pad_b(), 2.0),
            ],
        ),
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct GoldenOp {
    op: String,
    /// Sorted `(K, V)` pairs — never a proto `map<>`.
    args: Vec<(String, String)>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct GoldenCase {
    name: String,
    seed: u64,
    burst_id: String,
    base_burst_id: String,
    donor_burst_id: String,
    base_was_sibling: bool,
    post_clamp: bool,
    ops: Vec<GoldenOp>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct GoldenFile {
    synth_version: String,
    cases: Vec<GoldenCase>,
}

fn run_case(spec: &CaseSpec) -> GoldenCase {
    let cfg = match spec.forced_op {
        Some(op) => common::mutation_cfg_forced_op(op),
        None => common::mutation_cfg(),
    };
    let model = PadModel::new(
        &cfg.button_alphabet,
        cfg.burst_len.min_frames,
        cfg.burst_len.max_frames,
    );
    let ctx = ctx_for(spec.ctx_kind);
    let root = fanout_root(spec.seed, &ctx.node_id);

    let (burst, prov) = mutation::generate(&cfg, &ctx, &root, 0, &model);

    GoldenCase {
        name: spec.name.to_owned(),
        seed: spec.seed,
        burst_id: common::hex(&burst_hash(&burst)),
        base_burst_id: common::hex(&prov.base_burst_id),
        donor_burst_id: prov
            .donor_burst_id
            .map(|d| common::hex(&d))
            .unwrap_or_default(),
        base_was_sibling: prov.base_was_sibling,
        post_clamp: prov.post_clamp,
        ops: prov
            .ops
            .into_iter()
            .map(|o| GoldenOp {
                op: o.op,
                args: o.args,
            })
            .collect(),
    }
}

#[test]
#[ignore = "record mode: regenerates testdata/golden/m3/goldens.yaml"]
fn record_m3_goldens() {
    let cases: Vec<GoldenCase> = build_cases().iter().map(run_case).collect();

    // Sanity (record-time only, not re-checked on replay): the fallback
    // case actually falls back, per the case name's promise.
    let fallback = cases
        .iter()
        .find(|c| c.name == "splice_forced_fallback_to_extend")
        .expect("fallback case present");
    assert!(
        fallback.ops.iter().any(|o| o.op == "extend"),
        "splice-without-donor must record as extend"
    );

    let file = GoldenFile {
        synth_version: synth_core::SYNTH_VERSION.to_owned(),
        cases,
    };
    let yaml = serde_yaml::to_string(&file).expect("serialize goldens");
    let path = goldens_path();
    std::fs::create_dir_all(path.parent().expect("parent dir"))
        .expect("mkdir -p testdata/golden/m3");
    std::fs::write(&path, &yaml).expect("write goldens.yaml");
    println!("wrote {} cases to {}", file.cases.len(), path.display());
}

#[test]
fn replay_m3_goldens_reproduce_byte_identical_bursts() {
    let yaml = std::fs::read_to_string(goldens_path()).unwrap_or_else(|e| {
        panic!(
            "read testdata/golden/m3/goldens.yaml: {e} — run \
             `cargo test -p synth-gen record_m3_goldens -- --ignored --nocapture` first"
        )
    });
    let file: GoldenFile = serde_yaml::from_str(&yaml).expect("parse goldens.yaml");
    let specs = build_cases();
    assert_eq!(
        file.cases.len(),
        specs.len(),
        "goldens.yaml has {} cases but build_cases() now produces {} — a version bump + \
         regeneration is required if this is an intentional golden-suite change",
        file.cases.len(),
        specs.len()
    );

    for (spec, recorded) in specs.iter().zip(&file.cases) {
        assert_eq!(
            spec.name, recorded.name,
            "case order/name drift vs goldens.yaml"
        );
        let got = run_case(spec);
        assert_eq!(
            got.burst_id, recorded.burst_id,
            "case {}: burst_id mismatch vs testdata/golden/m3/goldens.yaml — if intentional, \
             bump SYNTH_VERSION and regenerate with `cargo test -p synth-gen record_m3_goldens \
             -- --ignored --nocapture`",
            spec.name
        );
        assert_eq!(
            got.base_burst_id, recorded.base_burst_id,
            "case {}: base_burst_id mismatch",
            spec.name
        );
        assert_eq!(
            got.donor_burst_id, recorded.donor_burst_id,
            "case {}: donor_burst_id mismatch",
            spec.name
        );
        assert_eq!(
            got.base_was_sibling, recorded.base_was_sibling,
            "case {}: base_was_sibling mismatch",
            spec.name
        );
        assert_eq!(
            got.post_clamp, recorded.post_clamp,
            "case {}: post_clamp mismatch",
            spec.name
        );
        assert_eq!(
            got.ops.len(),
            recorded.ops.len(),
            "case {}: op count mismatch",
            spec.name
        );
        for (g, r) in got.ops.iter().zip(&recorded.ops) {
            assert_eq!(g.op, r.op, "case {}: op name mismatch", spec.name);
            assert_eq!(
                g.args, r.args,
                "case {}: op {} args mismatch",
                spec.name, g.op
            );
        }
    }
}
