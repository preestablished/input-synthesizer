//! M1 golden-seed reproducibility (IMPLEMENTATION-PLAN.md §M1 Accept;
//! plan `03-m1-...md` §5.1): 50 recorded `(request, expected burst_id list)`
//! fixtures; replaying them must yield byte-identical bursts.
//!
//! Canonical-form rule (plan §5, non-negotiable): goldens hash the
//! **domain** form (`synth_core::types::burst_hash`, postcard over ordered
//! domain types), never raw prost wire bytes and never proto `map<>`
//! iteration order. Provenance is folded into the digest as sorted-by-slot
//! `(generator, slot, rng_stream)` line triples for the same reason.

#[path = "common/mod.rs"]
mod common;

use serde::{Deserialize, Serialize};
use synth_core::config::ExperimentConfig;
use synth_core::types::burst_hash;
use synth_gen::propose::{propose, Availability, SlotResult};

const CASE_COUNT: usize = 50;

fn goldens_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/golden/m1/goldens.yaml")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ConfigVariant {
    Base,
    DirectionVariant,
}

impl ConfigVariant {
    fn name(self) -> &'static str {
        match self {
            Self::Base => "base",
            Self::DirectionVariant => "direction_variant",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ContextVariant {
    None,
    Features,
    Full,
}

impl ContextVariant {
    fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Features => "features",
            Self::Full => "full",
        }
    }
}

struct CaseSpec {
    name: String,
    seed: u64,
    k: usize,
    length_hint: u32,
    config: ConfigVariant,
    context: ContextVariant,
}

/// Deterministic case list — pure Rust control flow, no maps, no RNG. Both
/// the recorder and the replay test call this and must therefore always
/// agree, as long as this function itself hasn't changed (a deliberate
/// change to it IS a golden-format change and must go through the version
/// bump + regeneration path like any other sampling change).
fn build_cases() -> Vec<CaseSpec> {
    let ks = [1usize, 8, 32, 64];
    let lengths = [0u32, 60, 300, 1500];
    let contexts = [
        ContextVariant::None,
        ContextVariant::Features,
        ContextVariant::Full,
    ];

    let mut cases = Vec::with_capacity(CASE_COUNT);
    let mut idx: u64 = 0;
    for &k in &ks {
        for &length_hint in &lengths {
            for &context in &contexts {
                let seed = 0x601D_0000_0000_0000u64 ^ idx;
                cases.push(CaseSpec {
                    name: format!("k{k}_len{length_hint}_{}", context.name()),
                    seed,
                    k,
                    length_hint,
                    config: ConfigVariant::Base,
                    context,
                });
                idx += 1;
            }
        }
    }
    debug_assert_eq!(cases.len(), 48);

    // Config-variant case: different direction priors via `deep_merge`, to
    // also exercise merge determinism (plan requirement).
    cases.push(CaseSpec {
        name: "variant_direction_priors".to_owned(),
        seed: 0x601D_0000_0000_0000u64 ^ idx,
        k: 32,
        length_hint: 300,
        config: ConfigVariant::DirectionVariant,
        context: ContextVariant::Full,
    });
    idx += 1;

    // One more case (seed varied, same shape as an existing combo) to reach
    // 50 while keeping the k×length×context grid intact.
    cases.push(CaseSpec {
        name: "k32_len300_none_reseeded".to_owned(),
        seed: 0x601D_0000_0000_0000u64 ^ idx ^ 0xFFFF_FFFF,
        k: 32,
        length_hint: 300,
        config: ConfigVariant::Base,
        context: ContextVariant::None,
    });

    assert_eq!(
        cases.len(),
        CASE_COUNT,
        "golden suite must have exactly 50 cases"
    );
    cases
}

fn config_for(variant: ConfigVariant) -> ExperimentConfig {
    match variant {
        // diagonal_factor stays at the API.md §5.4 default (0.25) here —
        // goldens pin whatever the sampler does, diagonals included.
        ConfigVariant::Base => common::parse_and_validate(&common::base_yaml(0.25)),
        ConfigVariant::DirectionVariant => common::variant_direction_yaml(0.25),
    }
}

fn ctx_for(variant: ContextVariant, node_id: &str) -> synth_gen::context::GenContext {
    match variant {
        ContextVariant::None => common::ctx_free(node_id),
        ContextVariant::Features => common::ctx_features(node_id),
        ContextVariant::Full => common::ctx_full(node_id),
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct GoldenCase {
    name: String,
    seed: u64,
    k: usize,
    length_hint: u32,
    config: String,
    context: String,
    burst_ids: Vec<String>,
    digest: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct GoldenFile {
    /// Bump alongside any intentional sampling/mixer/provenance change that
    /// would legitimately move these goldens (CI's golden-version gate
    /// checks the *workspace* version; this field additionally documents,
    /// inside the fixture itself, which synth build produced it).
    synth_version: String,
    cases: Vec<GoldenCase>,
}

/// Sorted-by-slot `(generator, slot, rng_stream)` triples, one per line —
/// the canonical, map-free provenance serialization the plan requires.
fn provenance_lines(results: &[SlotResult]) -> String {
    let mut lines: Vec<String> = results
        .iter()
        .map(|r| {
            format!(
                "{}|{}|{}\n",
                r.provenance.generator.name(),
                r.provenance.slot,
                r.provenance.rng_stream
            )
        })
        .collect();
    // Results are already slot-ordered (propose's contract), but sort
    // defensively so the digest never depends on incidental Vec order.
    lines.sort_unstable();
    lines.concat()
}

fn burst_ids_hex(results: &[SlotResult]) -> Vec<String> {
    results
        .iter()
        .map(|r| common::hex(&burst_hash(&r.burst)))
        .collect()
}

fn case_digest(burst_ids: &[String], results: &[SlotResult]) -> String {
    let mut hasher = blake3::Hasher::new();
    for id in burst_ids {
        hasher.update(id.as_bytes());
    }
    hasher.update(provenance_lines(results).as_bytes());
    common::hex(hasher.finalize().as_bytes())
}

fn run_case(spec: &CaseSpec) -> (Vec<String>, String) {
    let cfg = config_for(spec.config);
    let ctx = ctx_for(spec.context, "golden-node");
    let (results, degraded) = propose(
        &cfg,
        &ctx,
        spec.k,
        spec.length_hint,
        spec.seed,
        Availability::default(),
        None,
    );
    assert!(
        degraded.is_empty(),
        "case {}: pure-WR generator_mix should never report degraded generators",
        spec.name
    );
    let burst_ids = burst_ids_hex(&results);
    let digest = case_digest(&burst_ids, &results);
    (burst_ids, digest)
}

#[test]
#[ignore = "record mode: regenerates testdata/golden/m1/goldens.yaml"]
fn record_m1_goldens() {
    let cases: Vec<GoldenCase> = build_cases()
        .iter()
        .map(|spec| {
            let (burst_ids, digest) = run_case(spec);
            GoldenCase {
                name: spec.name.clone(),
                seed: spec.seed,
                k: spec.k,
                length_hint: spec.length_hint,
                config: spec.config.name().to_owned(),
                context: spec.context.name().to_owned(),
                burst_ids,
                digest,
            }
        })
        .collect();

    let file = GoldenFile {
        synth_version: synth_core::SYNTH_VERSION.to_owned(),
        cases,
    };
    let yaml = serde_yaml::to_string(&file).expect("serialize goldens");
    let path = goldens_path();
    std::fs::create_dir_all(path.parent().expect("parent dir"))
        .expect("mkdir -p testdata/golden/m1");
    std::fs::write(&path, &yaml).expect("write goldens.yaml");
    println!("wrote {} cases to {}", file.cases.len(), path.display());
}

#[test]
fn replay_m1_goldens_reproduce_byte_identical_bursts() {
    let yaml = std::fs::read_to_string(goldens_path()).unwrap_or_else(|e| {
        panic!(
            "read testdata/golden/m1/goldens.yaml: {e} — run \
             `cargo test -p synth-gen record_m1_goldens -- --ignored --nocapture` first"
        )
    });
    let file: GoldenFile = serde_yaml::from_str(&yaml).expect("parse goldens.yaml");
    let specs = build_cases();
    assert_eq!(
        file.cases.len(),
        specs.len(),
        "goldens.yaml has {} cases but build_cases() now produces {} — \
         a version bump + regeneration is required if this is an intentional \
         golden-suite change",
        file.cases.len(),
        specs.len()
    );

    for (spec, recorded) in specs.iter().zip(&file.cases) {
        assert_eq!(
            spec.name, recorded.name,
            "case order/name drift between build_cases() and goldens.yaml — \
             a version bump + regeneration is required if this is intentional"
        );
        assert_eq!(spec.seed, recorded.seed, "case {}: seed drift", spec.name);
        assert_eq!(spec.k, recorded.k, "case {}: k drift", spec.name);
        assert_eq!(
            spec.length_hint, recorded.length_hint,
            "case {}: length_hint drift",
            spec.name
        );
        assert_eq!(
            spec.config.name(),
            recorded.config,
            "case {}: config variant drift",
            spec.name
        );
        assert_eq!(
            spec.context.name(),
            recorded.context,
            "case {}: context variant drift",
            spec.name
        );

        let (burst_ids, digest) = run_case(spec);
        assert_eq!(
            burst_ids, recorded.burst_ids,
            "case {}: per-slot burst_id mismatch vs testdata/golden/m1/goldens.yaml — \
             if this change is intentional, bump SYNTH_VERSION (root Cargo.toml \
             [workspace.package] version) and regenerate goldens with \
             `cargo test -p synth-gen record_m1_goldens -- --ignored --nocapture`",
            spec.name
        );
        assert_eq!(
            digest, recorded.digest,
            "case {}: whole-case digest mismatch vs testdata/golden/m1/goldens.yaml — \
             if this change is intentional, bump SYNTH_VERSION (root Cargo.toml \
             [workspace.package] version) and regenerate goldens with \
             `cargo test -p synth-gen record_m1_goldens -- --ignored --nocapture`",
            spec.name
        );
    }
}
