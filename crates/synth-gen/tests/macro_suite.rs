//! M2 macro-pack acceptance suite (IMPLEMENTATION-PLAN.md §M2 Accept; plan
//! `04-m2-macro-packs.md` §4 "Acceptance"): pack loading/validation,
//! registry resolution, eligibility, provenance, end-to-end mixing, and the
//! demo pack.

#[path = "common/mod.rs"]
mod common;

use std::path::{Path, PathBuf};

use synth_core::types::Burst;
use synth_gen::context::GenContext;
use synth_gen::macros::{self, MacroPackError, PackRegistry};
use synth_gen::propose::{propose, Availability};

fn testdata_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/packs")
}

fn packs_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs")
}

fn read(path: PathBuf) -> Vec<u8> {
    std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

// ---------------------------------------------------------------------
// Loading / validation
// ---------------------------------------------------------------------

#[test]
fn valid_fixture_loads_ok() {
    let bytes = read(testdata_dir().join("valid/small.yaml"));
    let pack = macros::load_pack(&bytes).expect("valid fixture must load");
    assert_eq!(pack.name, "small-test-pack");
    assert_eq!(pack.macros.len(), 2);
    assert_eq!(pack.macros[0].name, "test-long-jump");
    assert_eq!(pack.macros[1].name, "test-mined-combo");
}

#[test]
fn demo_pack_loads_from_packs_dir() {
    let bytes = read(packs_dir().join("console16-movement-core.yaml"));
    let pack = macros::load_pack(&bytes).expect("demo pack must load");
    assert_eq!(pack.name, "console16-movement-core");
    assert_eq!(pack.macros.len(), 10);
}

/// One fixture per validation rule; each fails atomically (no partial
/// `MacroPack` — API.md §2.2) with a message substring naming the failure.
#[test]
fn invalid_fixtures_fail_with_expected_substrings() {
    let cases: &[(&str, &str)] = &[
        ("bad-name-regex.yaml", "[a-z0-9-]+"),
        ("bad-version.yaml", "version"),
        ("bad-kind.yaml", "macro_pack"),
        ("unknown-param.yaml", "undeclared param"),
        ("enum-hold-no-mirror.yaml", "no mirror map"),
        ("int-min-gt-max.yaml", "min"),
        ("scale-min-gt-max.yaml", "min"),
        ("weight-non-positive.yaml", "weight"),
        ("duplicate-macro-name.yaml", "duplicate macro name"),
        ("empty-steps.yaml", "steps must be non-empty"),
        ("event-grammar-model.yaml", "event_grammar"),
    ];
    for (file, expect_substr) in cases {
        let bytes = read(testdata_dir().join("invalid").join(file));
        let err = macros::load_pack(&bytes).expect_err(&format!("{file} must fail to load"));
        let msg = err.to_string();
        assert!(
            msg.contains(expect_substr),
            "{file}: error {msg:?} does not contain {expect_substr:?}"
        );
        assert!(
            matches!(err, MacroPackError::Invalid(_)),
            "{file}: expected a validation error, got {err:?}"
        );
    }
}

#[test]
fn parse_error_reports_line_and_column() {
    let bytes = read(testdata_dir().join("invalid/parse-error.yaml"));
    let err = macros::load_pack(&bytes).expect_err("malformed YAML must fail to parse");
    match err {
        MacroPackError::Parse { line, column, .. } => {
            assert!(line.is_some(), "parse error must carry a line number");
            assert!(column.is_some(), "parse error must carry a column number");
        }
        other => panic!("expected MacroPackError::Parse, got {other:?}"),
    }
}

#[test]
fn identical_bytes_load_to_the_same_pack_id() {
    let bytes = read(testdata_dir().join("valid/small.yaml"));
    let a = macros::load_pack(&bytes).expect("load a");
    let b = macros::load_pack(&bytes).expect("load b");
    assert_eq!(a.pack_id, b.pack_id);
    assert_eq!(a.pack_id.len(), 64, "blake3-256 hex must be 64 chars");
    assert!(a
        .pack_id
        .chars()
        .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
}

#[test]
fn different_bytes_load_to_different_pack_ids() {
    let small = read(testdata_dir().join("valid/small.yaml"));
    let demo = read(packs_dir().join("console16-movement-core.yaml"));
    let a = macros::load_pack(&small).expect("load small");
    let b = macros::load_pack(&demo).expect("load demo");
    assert_ne!(a.pack_id, b.pack_id);
}

// ---------------------------------------------------------------------
// Registry: shadow warnings, latest-load-wins
// ---------------------------------------------------------------------

/// Two packs, both declaring a macro named `shared-name`; the pack loaded
/// second must win at selection time, and `insert` must warn about it.
#[test]
fn cross_pack_duplicate_name_warns_and_latest_wins() {
    let pack_a_yaml = br#"
version: 1
kind: macro_pack
name: pack-a
model: pad
button_alphabet: console16-12btn-v1
source: handwritten
macros:
  - { name: shared-name, weight: 1.0, steps: [{ hold: [A], frames: 4 }] }
"#;
    let pack_b_yaml = br#"
version: 1
kind: macro_pack
name: pack-b
model: pad
button_alphabet: console16-12btn-v1
source: handwritten
macros:
  - { name: shared-name, weight: 1.0, steps: [{ hold: [B], frames: 4 }] }
"#;
    let pack_a = macros::load_pack(pack_a_yaml).expect("load pack-a");
    let pack_b = macros::load_pack(pack_b_yaml).expect("load pack-b");

    let mut registry = PackRegistry::new();
    let warnings_a = registry.insert(pack_a);
    assert!(warnings_a.is_empty(), "first pack never shadows anything");
    let warnings_b = registry.insert(pack_b);
    assert_eq!(warnings_b.len(), 1);
    assert!(warnings_b[0].contains("shared-name"));
    assert!(warnings_b[0].contains("shadows"));

    let cfg = common::macro_cfg(&["pack-a", "pack-b"]);
    let resolved = macros::resolve(&registry, &cfg).expect("resolve");
    let winner = resolved
        .items
        .iter()
        .find(|m| m.name == "shared-name")
        .expect("shared-name present exactly once");
    assert_eq!(winner.pack_id, registry.get("pack-b").unwrap().pack_id);
}

// ---------------------------------------------------------------------
// Eligibility
// ---------------------------------------------------------------------

#[test]
fn predicate_macro_skipped_without_matching_context_and_without_any_context() {
    let bytes = read(testdata_dir().join("valid/small.yaml"));
    let pack = macros::load_pack(&bytes).expect("load");
    let mut registry = PackRegistry::new();
    registry.insert(pack);
    let cfg = common::macro_cfg(&["small-test-pack"]);

    // No context at all: `test-long-jump`'s `on_ground eq 1` predicate must
    // be false (missing feature => false), leaving only test-mined-combo
    // eligible (which is predicate-free).
    let resolved = macros::resolve(&registry, &cfg).expect("resolve");
    let ctx_free = GenContext {
        node_id: "n".to_owned(),
        ..Default::default()
    };
    let eligible_free: Vec<&str> = resolved
        .items
        .iter()
        .filter(|m| {
            m.eligibility
                .iter()
                .all(|p| synth_gen::context::eval_predicate(p, &ctx_free, &cfg))
        })
        .map(|m| m.name.as_str())
        .collect();
    assert_eq!(eligible_free, vec!["test-mined-combo"]);

    // Non-matching context (on_ground = 0): still ineligible.
    let ctx_wrong = GenContext {
        node_id: "n".to_owned(),
        ram_features: vec![("on_ground".to_owned(), 0.0)],
        ..Default::default()
    };
    let eligible_wrong: Vec<&str> = resolved
        .items
        .iter()
        .filter(|m| {
            m.eligibility
                .iter()
                .all(|p| synth_gen::context::eval_predicate(p, &ctx_wrong, &cfg))
        })
        .map(|m| m.name.as_str())
        .collect();
    assert_eq!(eligible_wrong, vec!["test-mined-combo"]);

    // Matching context: both eligible.
    let ctx_right = GenContext {
        node_id: "n".to_owned(),
        ram_features: vec![("on_ground".to_owned(), 1.0)],
        ..Default::default()
    };
    let mut eligible_right: Vec<&str> = resolved
        .items
        .iter()
        .filter(|m| {
            m.eligibility
                .iter()
                .all(|p| synth_gen::context::eval_predicate(p, &ctx_right, &cfg))
        })
        .map(|m| m.name.as_str())
        .collect();
    eligible_right.sort_unstable();
    assert_eq!(eligible_right, vec!["test-long-jump", "test-mined-combo"]);
}

#[test]
fn predicate_free_macro_is_always_eligible() {
    let bytes = read(testdata_dir().join("valid/small.yaml"));
    let pack = macros::load_pack(&bytes).expect("load");
    let mined = pack
        .macros
        .iter()
        .find(|m| m.name == "test-mined-combo")
        .expect("test-mined-combo present");
    assert!(mined.eligibility.is_empty());
}

// ---------------------------------------------------------------------
// Provenance
// ---------------------------------------------------------------------

fn instantiated_total_frames(burst: &Burst) -> u64 {
    let Burst::Pad(pad) = burst else {
        unreachable!("pad model emits pad bursts")
    };
    pad.total_frames()
}

#[test]
fn provenance_frames_and_bindings_are_complete() {
    let bytes = read(testdata_dir().join("valid/small.yaml"));
    let pack = macros::load_pack(&bytes).expect("load");
    let mut registry = PackRegistry::new();
    registry.insert(pack);
    let cfg = common::macro_cfg(&["small-test-pack"]);
    let resolved = macros::resolve(&registry, &cfg).expect("resolve");

    let ctx = GenContext {
        node_id: "prov-test".to_owned(),
        ram_features: vec![("on_ground".to_owned(), 1.0)],
        ..Default::default()
    };

    for seed in 0u64..64 {
        let root = synth_core::rng::fanout_root(seed, &ctx.node_id);
        let (burst, prov) = macros::generate_macro_burst(&cfg, &ctx, &root, 0, 300, &resolved);
        let unlegalized_total = instantiated_total_frames(&burst);
        assert_eq!(
            u64::from(prov.macro_frames) + u64::from(prov.tail_frames),
            unlegalized_total,
            "seed {seed}: macro_frames + tail_frames must equal the un-legalized total"
        );

        let model = synth_pad::PadModel::new(
            &cfg.button_alphabet,
            cfg.burst_len.min_frames,
            cfg.burst_len.max_frames,
        );
        use synth_core::model::InputModel;
        let legalized = model.legalize(burst);
        let legalized_total = instantiated_total_frames(&legalized);
        assert!(legalized_total >= u64::from(cfg.burst_len.min_frames));
        assert!(legalized_total <= u64::from(cfg.burst_len.max_frames));
        if unlegalized_total >= u64::from(cfg.burst_len.min_frames)
            && unlegalized_total <= u64::from(cfg.burst_len.max_frames)
        {
            assert_eq!(legalized_total, unlegalized_total);
        }

        // param_bindings complete: every declared param of the recorded
        // macro is bound exactly once.
        let picked = resolved
            .items
            .iter()
            .find(|m| m.name == prov.macro_name)
            .expect("recorded macro exists in resolved set");
        assert_eq!(prov.param_bindings.len(), picked.params.len());
        let mut bound_names: Vec<&str> = prov
            .param_bindings
            .iter()
            .map(|(k, _)| k.as_str())
            .collect();
        bound_names.sort_unstable();
        let mut declared_names: Vec<&str> = picked.params.iter().map(|p| p.name.as_str()).collect();
        declared_names.sort_unstable();
        assert_eq!(bound_names, declared_names);
        // sorted-by-key, as documented.
        let mut sorted = prov.param_bindings.clone();
        sorted.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(prov.param_bindings, sorted);

        assert_eq!(prov.chain_index, 0);
        assert_eq!(
            prov.pack_id,
            registry.get("small-test-pack").unwrap().pack_id
        );
    }
}

/// Cross-check against `synth_pad::PadModel::detokenize`'s bucket
/// midpoints, since `macros::dur_bucket_midpoint` duplicates that private
/// constant (module doc: "not exposed publicly there").
#[test]
fn token_steps_midpoints_match_synth_pad_detokenize() {
    use synth_core::model::InputModel;
    use synth_core::types::Token;

    let cfg = common::macro_cfg(&["small-test-pack"]);
    let model = synth_pad::PadModel::new(
        &cfg.button_alphabet,
        cfg.burst_len.min_frames,
        cfg.burst_len.max_frames,
    );
    for bucket in 0u8..=5 {
        let via_detokenize = model.detokenize(&[Token {
            mask: 0,
            dur_bucket: bucket,
        }]);
        let Burst::Pad(pad) = via_detokenize else {
            unreachable!("pad model emits pad bursts")
        };
        let expected = pad.segments[0].hold_frames;
        assert_eq!(macros::dur_bucket_midpoint(bucket), expected);
    }
}

// ---------------------------------------------------------------------
// End-to-end mixing (M2 accept)
// ---------------------------------------------------------------------

#[test]
fn even_mix_of_weighted_random_and_macro_splits_exactly() {
    let bytes = read(testdata_dir().join("valid/small.yaml"));
    let pack = macros::load_pack(&bytes).expect("load");
    let mut registry = PackRegistry::new();
    registry.insert(pack);

    let base = common::macro_cfg(&["small-test-pack"]);
    let overrides = br#"
generator_mix: { weighted_random: 0.5, macro: 0.5, mutation: 0.0, policy: 0.0 }
"#;
    let cfg = synth_core::config::deep_merge(&base, overrides).expect("merge");
    synth_core::config::validate(&cfg).expect("valid");
    let resolved = macros::resolve(&registry, &cfg).expect("resolve");

    let ctx = GenContext {
        node_id: "e2e".to_owned(),
        ram_features: vec![("on_ground".to_owned(), 1.0)],
        ..Default::default()
    };

    let run = || {
        propose(
            &cfg,
            &ctx,
            32,
            300,
            0xE2E_5EED,
            Availability::default(),
            Some(&resolved),
        )
    };
    let (results_a, degraded_a) = run();
    let (results_b, degraded_b) = run();
    assert!(degraded_a.is_empty());

    let wr_count = results_a
        .iter()
        .filter(|r| r.provenance.generator == synth_gen::provenance::GeneratorKind::WeightedRandom)
        .count();
    let macro_count = results_a
        .iter()
        .filter(|r| r.provenance.generator == synth_gen::provenance::GeneratorKind::Macro)
        .count();
    assert_eq!(wr_count, 16);
    assert_eq!(macro_count, 16);

    for r in &results_a {
        if r.provenance.generator == synth_gen::provenance::GeneratorKind::Macro {
            assert!(
                r.provenance.macro_.is_some(),
                "every macro slot carries MacroProvenance"
            );
        } else {
            assert!(r.provenance.macro_.is_none());
        }
    }

    // Deterministic across two runs.
    for (a, b) in results_a.iter().zip(&results_b) {
        assert_eq!(a.burst, b.burst);
        assert_eq!(a.provenance, b.provenance);
    }
    assert_eq!(degraded_a, degraded_b);
}

// ---------------------------------------------------------------------
// Demo pack: exit-gate-4 clause (c) evidence
// ---------------------------------------------------------------------

#[test]
fn demo_pack_loads_and_every_macro_instantiates() {
    let bytes = read(packs_dir().join("console16-movement-core.yaml"));
    let pack = macros::load_pack(&bytes).expect("demo pack must load");
    assert_eq!(pack.name, "console16-movement-core");

    let names: Vec<String> = pack.macros.iter().map(|m| m.name.clone()).collect();
    assert_eq!(names.len(), 10, "demo pack must declare 10 macros");

    let ctx = GenContext {
        node_id: "demo-instantiate".to_owned(),
        // Satisfies long-jump's `on_ground eq 1` eligibility predicate;
        // harmless for every predicate-free macro.
        ram_features: vec![("on_ground".to_owned(), 1.0)],
        ..Default::default()
    };

    for name in &names {
        // Force-pick each macro by building a registry containing only a
        // single-macro copy of the pack (the simplest way to guarantee the
        // weighted-categorical pick lands on it every time).
        let mut solo = pack.clone();
        solo.macros.retain(|m| &m.name == name);
        assert_eq!(solo.macros.len(), 1, "macro {name} must be uniquely named");

        let mut registry = PackRegistry::new();
        registry.insert(solo);
        let cfg = common::macro_cfg(&["console16-movement-core"]);
        let resolved = macros::resolve(&registry, &cfg).expect("resolve solo pack");
        assert_eq!(resolved.items.len(), 1);

        let root = synth_core::rng::fanout_root(0xD3D0_0001, &ctx.node_id);
        let (burst, prov) = macros::generate_macro_burst(&cfg, &ctx, &root, 0, 300, &resolved);
        assert_eq!(&prov.macro_name, name);
        assert!(
            prov.macro_frames >= 1,
            "macro {name} must instantiate >= 1 frame"
        );
        let Burst::Pad(pad) = burst else {
            unreachable!("pad model emits pad bursts")
        };
        assert!(
            !pad.segments.is_empty(),
            "macro {name} must produce >= 1 segment"
        );
        for seg in &pad.segments {
            assert!(
                seg.hold_frames >= 1,
                "macro {name}: segment with 0 hold_frames"
            );
        }
    }
}
