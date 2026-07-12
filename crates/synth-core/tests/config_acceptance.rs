//! M0 config acceptance tests (API.md §5): every valid/invalid fixture under
//! `testdata/config/` round-trips through `parse` + `validate` (+ `deep_merge`)
//! exactly as documented.

use std::fs;
use std::path::{Path, PathBuf};

use synth_core::config::{deep_merge, parse, ConfigError, ExperimentConfig, LengthDistribution};

fn fixtures_dir() -> PathBuf {
    // CARGO_MANIFEST_DIR = crates/synth-core; fixtures live at repo_root/testdata/config.
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/config")
}

fn load_valid(name: &str) -> ExperimentConfig {
    let path = fixtures_dir().join("valid").join(name);
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
    let cfg = parse(&bytes).unwrap_or_else(|e| panic!("parse {path:?}: {e}"));
    if let Err(errs) = synth_core::config::validate(&cfg) {
        panic!("validate {path:?} unexpectedly failed: {errs:?}");
    }
    cfg
}

fn invalid_path(name: &str) -> PathBuf {
    fixtures_dir().join("invalid").join(name)
}

// ---- valid/minimal.yaml: parse + validate OK, and every documented default ----

#[test]
fn minimal_parses_validates_and_matches_documented_defaults() {
    let cfg = load_valid("minimal.yaml");

    assert_eq!(cfg.version, 1);
    assert_eq!(cfg.kind, "experiment_config");
    assert_eq!(cfg.experiment_id, "exp-test");
    assert_eq!(cfg.button_alphabet.buttons.len(), 12);

    // 5.2 generator_mix defaults
    assert_eq!(cfg.generator_mix.weighted_random, 0.45);
    assert_eq!(cfg.generator_mix.macro_, 0.35);
    assert_eq!(cfg.generator_mix.mutation, 0.20);
    assert_eq!(cfg.generator_mix.policy, 0.0);

    // 5.3 burst_len defaults
    assert_eq!(cfg.burst_len.distribution, LengthDistribution::Lognormal);
    assert_eq!(cfg.burst_len.mean_frames, 300);
    assert_eq!(cfg.burst_len.sigma, 0.35);
    assert_eq!(cfg.burst_len.min_frames, 16);
    assert_eq!(cfg.burst_len.max_frames, 1800);

    // 5.4 weighted_random defaults
    assert_eq!(cfg.weighted_random.default_button.duty, 0.04);
    assert_eq!(cfg.weighted_random.default_button.mean_hold_frames, 6.0);
    assert_eq!(
        cfg.weighted_random.direction.priors.get("NEUTRAL").copied(),
        Some(0.15)
    );
    assert_eq!(
        cfg.weighted_random.direction.priors.get("RIGHT").copied(),
        Some(0.55)
    );
    assert_eq!(cfg.weighted_random.direction.mean_hold_frames, 24.0);
    assert_eq!(cfg.weighted_random.direction.stickiness, 0.55);
    assert_eq!(cfg.weighted_random.direction.diagonal_factor, 0.25);
    assert!(cfg.weighted_random.start_from_history.0);

    // 5.6 macro defaults
    assert!(cfg.macro_.pad_to_length);
    assert_eq!(cfg.macro_.chain_n, 1);

    // 5.7 mutation defaults
    assert_eq!(cfg.mutation.donor_bias, 0.5);
    assert_eq!(cfg.mutation.timing_sigma, 0.25);
    let op_prob_sum: f64 = cfg.mutation.op_probs.values().sum();
    assert!(
        (op_prob_sum - 1.0).abs() < 1e-9,
        "op_probs sum = {op_prob_sum}"
    );

    // 5.8 policy defaults
    assert_eq!(cfg.policy.endpoint, "http://spark-policy:7480");
    assert_eq!(cfg.policy.model_id, "pad-policy");
    assert_eq!(cfg.policy.temperature, 1.0);
    assert_eq!(cfg.policy.horizon_segments, 48);
    assert_eq!(cfg.policy.call_deadline_ms, 150);
    assert!(cfg.policy.strict_reproducibility);

    // 5.9 global switch
    assert!(cfg.strict_reproducibility);
}

// ---- valid/demo-full.yaml: parse + validate OK, spot-check non-defaults ----

#[test]
fn demo_full_parses_validates_and_carries_its_overrides() {
    let cfg = load_valid("demo-full.yaml");

    assert_eq!(cfg.experiment_id, "exp-firstboss-04");
    assert_eq!(
        cfg.weighted_random
            .buttons
            .get("A")
            .map(|p| (p.duty, p.mean_hold_frames)),
        Some((0.20, 18.0))
    );
    assert_eq!(
        cfg.weighted_random.buttons.get("SELECT").map(|p| p.duty),
        Some(0.0)
    );
    assert_eq!(cfg.context_rules.len(), 2);
    assert_eq!(cfg.refractory.len(), 1);
    assert_eq!(cfg.refractory[0].button, "START");
    assert_eq!(
        cfg.macro_.packs,
        vec!["console16-movement-core".to_string()]
    );
    assert_eq!(cfg.mutation.ops_binomial.n, 3);
    assert_eq!(cfg.mutation.ops_binomial.p, 0.25);
    assert_eq!(cfg.policy.endpoint, "http://spark-policy:7480");
}

// ---- valid/duty-boundary.yaml: the a == 1 boundary MUST validate OK -------

#[test]
fn duty_boundary_equality_is_accepted() {
    let cfg = load_valid("duty-boundary.yaml");
    let a = cfg
        .weighted_random
        .buttons
        .get("A")
        .expect("button A prior");
    assert_eq!(a.duty, 0.8);
    assert_eq!(a.mean_hold_frames, 4.0);
    // bound = mean_hold / (mean_hold + 1) = 4 / 5 = 0.8 == duty, i.e. a == 1.
}

// ---- invalid/*.yaml: one row per validation rule --------------------------

/// (fixture filename, substring that MUST appear in one of the validate() errors)
const INVALID_FIXTURES: &[(&str, &str)] = &[
    ("bad-version.yaml", "version 2 unsupported (expected 1)"),
    (
        "bad-kind.yaml",
        "kind \"not_experiment_config\" is not \"experiment_config\"",
    ),
    (
        "empty-experiment-id.yaml",
        "experiment_id must be non-empty",
    ),
    ("too-many-buttons.yaml", "17 entries (expected 1..=16)"),
    (
        "bit-out-of-range.yaml",
        "button SELECT has bit 16 (must be < 16)",
    ),
    ("duplicate-bit.yaml", "button B reuses bit 0"),
    ("duplicate-button-name.yaml", "button name A declared twice"),
    (
        "exclusive-groups-undeclared.yaml",
        "exclusive_groups references undeclared button ZZZ",
    ),
    (
        "forbidden-masks-undeclared.yaml",
        "forbidden_masks.mask references undeclared button ZZZ",
    ),
    (
        "directions-group-undeclared.yaml",
        "directions.group references undeclared button ZZZ",
    ),
    (
        "duty-exceeds-bound.yaml",
        "duty 0.9 > mean_hold/(mean_hold+1) = 0.8",
    ),
    (
        "duty-out-of-range.yaml",
        "button A: duty -0.1 must be in [0, 1)",
    ),
    (
        "mean-hold-too-low.yaml",
        "button A: mean_hold_frames 0.5 must be in [1, 216000]",
    ),
    (
        "stickiness-out-of-range.yaml",
        "direction.stickiness 1.5 must be in [0, 1)",
    ),
    (
        "direction-prior-negative.yaml",
        "direction prior NEUTRAL is negative",
    ),
    (
        "direction-prior-bad-key.yaml",
        "direction prior BOGUS is neither NEUTRAL nor in directions.group",
    ),
    (
        "generator-mix-negative.yaml",
        "generator_mix values must be >= 0",
    ),
    (
        "generator-mix-all-zero.yaml",
        "generator_mix must have at least one weight > 0",
    ),
    ("min-frames-zero.yaml", "burst_len.min_frames must be >= 1"),
    (
        "max-less-than-min.yaml",
        "burst_len.max_frames 50 < min_frames 100",
    ),
    ("sigma-non-positive.yaml", "burst_len.sigma 0 must be > 0"),
    (
        "context-rule-undeclared-button.yaml",
        "context_rules.adjust_buttons references undeclared button ZZZ",
    ),
    (
        "refractory-undeclared-button.yaml",
        "refractory references undeclared button ZZZ",
    ),
    (
        "chain-n-out-of-range.yaml",
        "macro.chain_n 5 must be in 1..=4",
    ),
    ("op-probs-not-summing.yaml", "mutation.op_probs sums to 2"),
    (
        "donor-bias-out-of-range.yaml",
        "mutation.donor_bias 1.5 must be in [0, 1]",
    ),
    (
        "timing-sigma-non-positive.yaml",
        "mutation.timing_sigma 0 must be > 0",
    ),
    (
        "ops-binomial-p-out-of-range.yaml",
        "mutation.ops_binomial.p 1.5 must be in [0, 1]",
    ),
    (
        "event-grammar-missing-grammar-id.yaml",
        "model event_grammar requires grammar_id",
    ),
    (
        "op-probs-unknown-key.yaml",
        "mutation.op_probs has unknown key \"bogus_op\"",
    ),
    (
        "op-probs-negative.yaml",
        "mutation.op_probs[\"extend\"] value -0.2 is negative",
    ),
    ("mean-hold-too-large.yaml", "must be in [1, 216000]"),
    (
        "mean-frames-zero.yaml",
        "burst_len.mean_frames 0 must be in [1, 216000]",
    ),
    (
        "max-frames-too-large.yaml",
        "burst_len.max_frames 300000 exceeds 216000",
    ),
    (
        "ops-binomial-n-too-large.yaml",
        "mutation.ops_binomial.n 100 must be <= 64",
    ),
    (
        "generator-mix-nan.yaml",
        "generator_mix values must be finite",
    ),
];

#[test]
fn every_invalid_fixture_reports_its_expected_error_substring() {
    for (name, expected_substring) in INVALID_FIXTURES {
        let path = invalid_path(name);
        let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
        let cfg = parse(&bytes).unwrap_or_else(|e| {
            panic!("fixture {name} is expected to be schema-valid YAML but failed to parse: {e}")
        });
        let errs = synth_core::config::validate(&cfg)
            .expect_err(&format!("fixture {name} was expected to fail validation"));
        let joined: Vec<String> = errs.iter().map(ToString::to_string).collect();
        assert!(
            joined.iter().any(|m| m.contains(expected_substring)),
            "fixture {name}: expected an error containing {expected_substring:?}, got {joined:?}"
        );
    }
}

#[test]
fn all_invalid_fixture_files_are_covered_by_the_table() {
    let dir = fixtures_dir().join("invalid");
    let mut on_disk: Vec<String> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read_dir {dir:?}: {e}"))
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".yaml"))
        .collect();
    on_disk.sort();

    let mut covered: Vec<String> = INVALID_FIXTURES
        .iter()
        .map(|(name, _)| name.to_string())
        .chain(std::iter::once("multi-error.yaml".to_string()))
        .collect();
    covered.sort();
    covered.dedup();

    assert_eq!(
        on_disk, covered,
        "every file in testdata/config/invalid must be exercised by a test"
    );
}

#[test]
fn multi_error_fixture_reports_every_violation_at_once() {
    let path = invalid_path("multi-error.yaml");
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
    let cfg = parse(&bytes).expect("multi-error.yaml is schema-valid YAML");
    let errs = synth_core::config::validate(&cfg).expect_err("multi-error.yaml must fail");
    assert!(
        errs.len() >= 2,
        "expected >= 2 simultaneous errors, got {}: {errs:?}",
        errs.len()
    );
    let joined: Vec<String> = errs.iter().map(ToString::to_string).collect();
    assert!(joined.iter().any(|m| m.contains("version 2 unsupported")));
    assert!(joined
        .iter()
        .any(|m| m.contains("duty 0.9 > mean_hold/(mean_hold+1) = 0.8")));
    assert!(joined
        .iter()
        .any(|m| m.contains("macro.chain_n 5 must be in 1..=4")));
}

// ---- deep_merge + validate: override introduces a validation failure ------

#[test]
fn deep_merge_override_can_introduce_a_validation_failure() {
    let base_bytes = fs::read(fixtures_dir().join("valid").join("minimal.yaml")).unwrap();
    let base = parse(&base_bytes).unwrap();
    synth_core::config::validate(&base).expect("base minimal.yaml must validate OK");

    let overrides = b"weighted_random: { buttons: { A: { duty: 0.9, mean_hold_frames: 4 } } }";
    let merged = deep_merge(&base, overrides).expect("merge must succeed");

    let errs =
        synth_core::config::validate(&merged).expect_err("merged config must now fail validation");
    let joined: Vec<String> = errs.iter().map(ToString::to_string).collect();
    assert!(
        joined
            .iter()
            .any(|m| m.contains("duty 0.9 > mean_hold/(mean_hold+1) = 0.8")),
        "got {joined:?}"
    );
}

// sanity: ConfigError's Display carries through .to_string() used above.
#[allow(dead_code)]
fn _assert_error_display(e: &ConfigError) -> String {
    e.to_string()
}
