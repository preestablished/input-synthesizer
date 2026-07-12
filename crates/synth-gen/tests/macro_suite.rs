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
use synth_gen::propose::propose;

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
        ("int-domain-too-wide.yaml", "exceeds 2^32"),
        ("weight-nan.yaml", "must be finite"),
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

/// Two packs, both declaring a macro named `shared-name`, loaded pack-a THEN
/// pack-b (load order: a, b). Fix #4: cross-pack duplicate-name selection
/// must follow `cfg.macro.packs`' own list order, not load order — so listing
/// them `[pack-b, pack-a]` (the reverse of load order) must pick pack-a, and
/// `[pack-a, pack-b]` must pick pack-b. This is what keeps the winner a pure
/// function of the fingerprinted config: two processes that loaded the same
/// packs in different orders must still resolve identically for the same
/// `cfg.macro.packs`.
#[test]
fn cross_pack_duplicate_name_dedup_follows_macro_packs_list_order() {
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
    // Load order: pack-a first, pack-b second.
    let warnings_a = registry.insert(pack_a);
    assert!(warnings_a.is_empty(), "first pack never shadows anything");
    let warnings_b = registry.insert(pack_b);
    assert_eq!(warnings_b.len(), 1);
    assert!(warnings_b[0].contains("shared-name"));
    assert!(warnings_b[0].contains("shadows"));

    // cfg.macro.packs lists pack-b BEFORE pack-a — the OPPOSITE of load
    // order. The list-order winner must be pack-a (listed last), not pack-b
    // (loaded last).
    let cfg_b_then_a = common::macro_cfg(&["pack-b", "pack-a"]);
    let resolved = macros::resolve(&registry, &cfg_b_then_a).expect("resolve");
    let winner = resolved
        .items
        .iter()
        .find(|m| m.name == "shared-name")
        .expect("shared-name present exactly once");
    assert_eq!(
        winner.pack_id,
        registry.get("pack-a").unwrap().pack_id,
        "list order must decide (pack-a listed last), not load order (pack-a loaded first)"
    );

    // Reversing the list order must reverse the winner too.
    let cfg_a_then_b = common::macro_cfg(&["pack-a", "pack-b"]);
    let resolved2 = macros::resolve(&registry, &cfg_a_then_b).expect("resolve");
    let winner2 = resolved2
        .items
        .iter()
        .find(|m| m.name == "shared-name")
        .expect("shared-name present exactly once");
    assert_eq!(winner2.pack_id, registry.get("pack-b").unwrap().pack_id);
}

/// A pack NAME owns exactly one document: loading different content under an
/// existing pack name replaces the old pack (with a warning) instead of
/// leaving `get(name)` ambiguous and load-order-dependent. The replacement
/// also removes the old pack_id from `pack_ids()`, so two replicas that
/// loaded different same-name documents can never share a config
/// fingerprint (review round 2, residual of fix #4).
#[test]
fn same_pack_name_different_content_replaces_and_changes_pack_ids() {
    let v1_yaml = br#"
version: 1
kind: macro_pack
name: core
model: pad
button_alphabet: console16-12btn-v1
source: handwritten
macros:
  - { name: tap-a, weight: 1.0, steps: [{ hold: [A], frames: 4 }] }
"#;
    let v2_yaml = br#"
version: 1
kind: macro_pack
name: core
model: pad
button_alphabet: console16-12btn-v1
source: handwritten
macros:
  - { name: tap-b, weight: 1.0, steps: [{ hold: [B], frames: 4 }] }
"#;
    let v1 = macros::load_pack(v1_yaml).expect("load v1");
    let v2 = macros::load_pack(v2_yaml).expect("load v2");
    let (v1_id, v2_id) = (v1.pack_id.clone(), v2.pack_id.clone());
    assert_ne!(v1_id, v2_id);

    let mut registry = PackRegistry::new();
    assert!(registry.insert(v1).is_empty());
    let warnings = registry.insert(v2);
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("replaced"), "warning: {warnings:?}");

    // The name now resolves unambiguously to the latest document, the old
    // pack_id is gone from the fingerprint input, and only one pack remains.
    assert_eq!(registry.get("core").unwrap().pack_id, v2_id);
    assert_eq!(registry.pack_ids(), vec![v2_id.clone()]);
    assert!(registry.get(&v1_id).is_none());
    assert!(registry.get(&v2_id).is_some());
}

/// A pack NAME may legally collide with a different pack's pack_id (the name
/// regex admits 64-char lowercase hex). `get()` must resolve ids before
/// names so such a collision cannot make resolution load-order-dependent
/// (round-3 review PoC: two registries with identical pack_ids() sets
/// resolved the same key to different packs under the old combined scan).
#[test]
fn pack_id_lookup_beats_name_collision_regardless_of_load_order() {
    let pack_b_yaml = br#"
version: 1
kind: macro_pack
name: pack-b
model: pad
button_alphabet: console16-12btn-v1
source: handwritten
macros:
  - { name: tap-b, weight: 1.0, steps: [{ hold: [B], frames: 4 }] }
"#;
    let pack_b = macros::load_pack(pack_b_yaml).expect("load pack-b");
    let b_id = pack_b.pack_id.clone();

    // Pack A's declared NAME is literally pack B's pack_id.
    let pack_a_yaml = format!(
        r#"
version: 1
kind: macro_pack
name: {b_id}
model: pad
button_alphabet: console16-12btn-v1
source: handwritten
macros:
  - {{ name: tap-a, weight: 1.0, steps: [{{ hold: [A], frames: 4 }}] }}
"#
    );
    let pack_a = macros::load_pack(pack_a_yaml.as_bytes()).expect("load pack-a");
    let a_id = pack_a.pack_id.clone();
    assert_ne!(a_id, b_id);

    for order in [[&pack_a, &pack_b], [&pack_b, &pack_a]] {
        let mut registry = PackRegistry::new();
        registry.insert((*order[0]).clone());
        registry.insert((*order[1]).clone());
        // Same loaded set either way...
        let mut ids = vec![a_id.clone(), b_id.clone()];
        ids.sort_unstable();
        assert_eq!(registry.pack_ids(), ids);
        // ...and the collided key resolves to the ID owner in both orders.
        assert_eq!(
            registry.get(&b_id).unwrap().pack_id,
            b_id,
            "id lookup must beat a colliding pack name, independent of load order"
        );
        // The colliding pack stays reachable by its own id.
        assert_eq!(registry.get(&a_id).unwrap().pack_id, a_id);
    }
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

    let run = || propose(&cfg, &ctx, 32, 300, 0xE2E_5EED, Some(&resolved));
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
// Fix #15: macro_frames provenance clamp
// ---------------------------------------------------------------------

/// A tiny `max_frames` forces `test-long-jump`'s instantiated chain (always
/// well over 50 frames: `runup` alone is `int { min: 20, max: 80 }`, plus two
/// more fixed-frame steps) to exceed it, so `legalize` truncates the burst.
/// Provenance's `macro_frames + tail_frames` must equal the LEGALIZED total,
/// not the pre-truncation instantiated span.
#[test]
fn macro_frames_provenance_clamped_when_chain_exceeds_max_frames() {
    use synth_core::model::InputModel;

    let bytes = read(testdata_dir().join("valid/small.yaml"));
    let pack = macros::load_pack(&bytes).expect("load");
    let mut registry = PackRegistry::new();
    registry.insert(pack);

    let base = common::macro_cfg(&["small-test-pack"]);
    let overrides = br#"
burst_len: { min_frames: 1, max_frames: 10 }
"#;
    let cfg = synth_core::config::deep_merge(&base, overrides).expect("merge");
    synth_core::config::validate(&cfg).expect("valid");
    let resolved = macros::resolve(&registry, &cfg).expect("resolve");

    let ctx = GenContext {
        node_id: "clamp-test".to_owned(),
        ram_features: vec![("on_ground".to_owned(), 1.0)],
        ..Default::default()
    };
    let model = synth_pad::PadModel::new(
        &cfg.button_alphabet,
        cfg.burst_len.min_frames,
        cfg.burst_len.max_frames,
    );

    let mut found = false;
    for seed in 0u64..32 {
        let root = synth_core::rng::fanout_root(seed, &ctx.node_id);
        let (burst, prov) = macros::generate_macro_burst(&cfg, &ctx, &root, 0, 300, &resolved);
        let unlegalized_total = instantiated_total_frames(&burst);
        if unlegalized_total <= u64::from(cfg.burst_len.max_frames) {
            continue;
        }
        let legalized = model.legalize(burst);
        let legalized_total = instantiated_total_frames(&legalized);
        assert_eq!(legalized_total, u64::from(cfg.burst_len.max_frames));
        assert_eq!(
            u64::from(prov.macro_frames) + u64::from(prov.tail_frames),
            legalized_total,
            "seed {seed}: clamped macro_frames + tail_frames must equal the legalized total"
        );
        found = true;
        break;
    }
    assert!(
        found,
        "no seed in 0..32 produced a chain exceeding max_frames=10 — widen the search"
    );
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
