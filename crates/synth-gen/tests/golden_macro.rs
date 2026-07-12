//! M2 macro-instantiation goldens (plan `04-m2-macro-packs.md` §4
//! "Acceptance"): fixed seed ⇒ fixed bindings ⇒ fixed segments, for `steps`,
//! `token_steps`, mirror (both branches), scale, `chain_n > 1`,
//! `pad_to_length: false`, and tail-padding cases.
//!
//! Canonical-form rule (00- global rule, restated from `golden_seed.rs`):
//! goldens hash the **domain** burst (`synth_core::types::burst_hash`),
//! never raw prost wire bytes; `MacroProvenance.param_bindings` is folded in
//! as its own sorted `Vec<(K,V)>` (never a proto `map<>`).

#[path = "common/mod.rs"]
mod common;

use serde::{Deserialize, Serialize};
use synth_core::config::{deep_merge, validate, ExperimentConfig};
use synth_core::model::InputModel;
use synth_core::rng::fanout_root;
use synth_core::types::burst_hash;
use synth_gen::context::GenContext;
use synth_gen::macros::{self, PackRegistry, ResolvedMacros};

const CASE_COUNT: usize = 8;

fn goldens_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/golden/m2/goldens.yaml")
}

fn small_pack_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/packs/valid/small.yaml")
}

struct CaseSpec {
    name: &'static str,
    seed: u64,
    length_hint: u32,
    chain_n: u32,
    pad_to_length: bool,
    macro_names: &'static [&'static str],
}

/// Deterministic case list — both the recorder and the replay test call
/// this and must therefore always agree, as long as this function itself
/// hasn't changed (a deliberate change to it IS a golden-format change and
/// must go through the version-bump + regeneration path, same as
/// `golden_seed.rs`'s `build_cases`).
fn build_cases() -> Vec<CaseSpec> {
    let cases = vec![
        CaseSpec {
            name: "steps_macro_fixed_seed",
            seed: 1,
            length_hint: 100,
            chain_n: 1,
            pad_to_length: true,
            macro_names: &["test-long-jump"],
        },
        CaseSpec {
            name: "token_steps_macro",
            seed: 2,
            length_hint: 100,
            chain_n: 1,
            pad_to_length: true,
            macro_names: &["test-mined-combo"],
        },
        // Seeds 4 and 2 were selected (offline, against this suite's exact
        // `cfg`/`ctx`) to land the `dir` enum param on "left" and "right"
        // respectively, so both mirror branches are exercised.
        CaseSpec {
            name: "mirror_left",
            seed: 4,
            length_hint: 100,
            chain_n: 1,
            pad_to_length: true,
            macro_names: &["test-long-jump"],
        },
        CaseSpec {
            name: "mirror_right",
            seed: 2,
            length_hint: 100,
            chain_n: 1,
            pad_to_length: true,
            macro_names: &["test-long-jump"],
        },
        CaseSpec {
            name: "scale_case",
            seed: 5,
            length_hint: 200,
            chain_n: 1,
            pad_to_length: true,
            macro_names: &["test-long-jump"],
        },
        CaseSpec {
            name: "chain_n2",
            seed: 7,
            length_hint: 300,
            chain_n: 2,
            pad_to_length: true,
            macro_names: &["test-long-jump", "test-mined-combo"],
        },
        CaseSpec {
            name: "pad_to_length_false",
            seed: 1,
            length_hint: 1000,
            chain_n: 1,
            pad_to_length: false,
            macro_names: &["test-long-jump"],
        },
        CaseSpec {
            name: "tail_case",
            seed: 2,
            length_hint: 1000,
            chain_n: 1,
            pad_to_length: true,
            macro_names: &["test-mined-combo"],
        },
    ];
    assert_eq!(
        cases.len(),
        CASE_COUNT,
        "golden suite must have exactly 8 cases"
    );
    cases
}

fn cfg_for(chain_n: u32, pad_to_length: bool) -> ExperimentConfig {
    let base = common::macro_cfg(&["small-test-pack"]);
    let overrides = format!(
        "macro: {{ packs: [small-test-pack], pad_to_length: {pad_to_length}, chain_n: {chain_n} }}"
    );
    let merged = deep_merge(&base, overrides.as_bytes()).expect("deep_merge macro overrides");
    validate(&merged).expect("merged macro config must be valid");
    merged
}

fn resolved_for(cfg: &ExperimentConfig, macro_names: &[&str]) -> ResolvedMacros {
    let bytes = std::fs::read(small_pack_path()).expect("read small.yaml fixture");
    let mut pack = macros::load_pack(&bytes).expect("load small.yaml fixture");
    pack.macros
        .retain(|m| macro_names.contains(&m.name.as_str()));
    let mut registry = PackRegistry::new();
    registry.insert(pack);
    macros::resolve(&registry, cfg).expect("resolve small.yaml fixture")
}

fn ctx() -> GenContext {
    GenContext {
        node_id: "golden-m2-node".to_owned(),
        // Satisfies `test-long-jump`'s `on_ground eq 1` eligibility;
        // harmless for the predicate-free `test-mined-combo`.
        ram_features: vec![("on_ground".to_owned(), 1.0)],
        ..Default::default()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct GoldenCase {
    name: String,
    seed: u64,
    length_hint: u32,
    chain_n: u32,
    pad_to_length: bool,
    burst_id: String,
    pack_id: String,
    macro_name: String,
    /// Sorted `(K, V)` pairs — never a proto `map<>`.
    param_bindings: Vec<(String, String)>,
    macro_frames: u32,
    tail_frames: u32,
    chain_index: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct GoldenFile {
    synth_version: String,
    cases: Vec<GoldenCase>,
}

fn run_case(spec: &CaseSpec) -> GoldenCase {
    let cfg = cfg_for(spec.chain_n, spec.pad_to_length);
    let resolved = resolved_for(&cfg, spec.macro_names);
    let ctx = ctx();
    let root = fanout_root(spec.seed, &ctx.node_id);

    let (burst, prov) =
        macros::generate_macro_burst(&cfg, &ctx, &root, 0, spec.length_hint, &resolved);

    let model = synth_pad::PadModel::new(
        &cfg.button_alphabet,
        cfg.burst_len.min_frames,
        cfg.burst_len.max_frames,
    );
    let legalized = model.legalize(burst);

    GoldenCase {
        name: spec.name.to_owned(),
        seed: spec.seed,
        length_hint: spec.length_hint,
        chain_n: spec.chain_n,
        pad_to_length: spec.pad_to_length,
        burst_id: common::hex(&burst_hash(&legalized)),
        pack_id: prov.pack_id,
        macro_name: prov.macro_name,
        param_bindings: prov.param_bindings,
        macro_frames: prov.macro_frames,
        tail_frames: prov.tail_frames,
        chain_index: prov.chain_index,
    }
}

#[test]
#[ignore = "record mode: regenerates testdata/golden/m2/goldens.yaml"]
fn record_m2_goldens() {
    let cases: Vec<GoldenCase> = build_cases().iter().map(run_case).collect();

    // Sanity (record-time only, not re-checked on replay): both mirror
    // branches actually appear, as the case names promise.
    let dir_of = |name: &str| -> String {
        cases
            .iter()
            .find(|c| c.name == name)
            .and_then(|c| c.param_bindings.iter().find(|(k, _)| k == "dir"))
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    };
    assert_eq!(dir_of("mirror_left"), "left");
    assert_eq!(dir_of("mirror_right"), "right");

    let file = GoldenFile {
        synth_version: synth_core::SYNTH_VERSION.to_owned(),
        cases,
    };
    let yaml = serde_yaml::to_string(&file).expect("serialize goldens");
    let path = goldens_path();
    std::fs::create_dir_all(path.parent().expect("parent dir"))
        .expect("mkdir -p testdata/golden/m2");
    std::fs::write(&path, &yaml).expect("write goldens.yaml");
    println!("wrote {} cases to {}", file.cases.len(), path.display());
}

#[test]
fn replay_m2_goldens_reproduce_byte_identical_bursts() {
    let yaml = std::fs::read_to_string(goldens_path()).unwrap_or_else(|e| {
        panic!(
            "read testdata/golden/m2/goldens.yaml: {e} — run \
             `cargo test -p synth-gen record_m2_goldens -- --ignored --nocapture` first"
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
            "case {}: burst_id mismatch vs testdata/golden/m2/goldens.yaml — if intentional, \
             bump SYNTH_VERSION and regenerate with `cargo test -p synth-gen record_m2_goldens \
             -- --ignored --nocapture`",
            spec.name
        );
        assert_eq!(
            got.pack_id, recorded.pack_id,
            "case {}: pack_id mismatch",
            spec.name
        );
        assert_eq!(
            got.macro_name, recorded.macro_name,
            "case {}: macro_name mismatch",
            spec.name
        );
        assert_eq!(
            got.param_bindings, recorded.param_bindings,
            "case {}: param_bindings mismatch",
            spec.name
        );
        assert_eq!(
            got.macro_frames, recorded.macro_frames,
            "case {}: macro_frames mismatch",
            spec.name
        );
        assert_eq!(
            got.tail_frames, recorded.tail_frames,
            "case {}: tail_frames mismatch",
            spec.name
        );
        assert_eq!(
            got.chain_index, recorded.chain_index,
            "case {}: chain_index mismatch",
            spec.name
        );
    }
}
