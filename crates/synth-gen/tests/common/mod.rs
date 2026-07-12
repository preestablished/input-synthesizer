//! Shared test fixtures for the M1 acceptance suites (statistical_suite,
//! golden_seed, bench_propose). Not every helper is used by every binary
//! that includes this module (integration tests each compile as a separate
//! crate), so unused-function warnings are allowed wholesale here rather
//! than peppering `#[allow(dead_code)]` per item.
#![allow(dead_code)]

use synth_core::config::{deep_merge, parse, validate, ExperimentConfig};
use synth_core::types::{PadBurst, PadSegment};
use synth_gen::context::GenContext;

/// console16-12btn-v1 alphabet + API.md §5.4 demo priors, pure
/// weighted-random mix (generator_mix: {weighted_random: 1.0, rest: 0}) so
/// `propose` never hits the M2/M3/M6 `unreachable!()` arms. `diagonal_factor`
/// is the one axis callers vary:
/// - statistical_suite sets it to 0.0 so the sticky-direction math in
///   ARCHITECTURE.md §4.3 (which does not model diagonal enrichment) applies
///   cleanly.
/// - golden_seed keeps the API.md §5.4 default 0.25 — goldens pin whatever
///   the sampler actually does, diagonals included.
pub fn base_yaml(diagonal_factor: f64) -> String {
    format!(
        r#"
version: 1
kind: experiment_config
experiment_id: exp-m1-test
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
  weighted_random: 1.0
  macro: 0.0
  mutation: 0.0
  policy: 0.0
burst_len:
  distribution: lognormal
  mean_frames: 300
  sigma: 0.35
  min_frames: 16
  max_frames: 1800
weighted_random:
  default_button: {{ duty: 0.04, mean_hold_frames: 6 }}
  buttons:
    A:      {{ duty: 0.20, mean_hold_frames: 18 }}
    B:      {{ duty: 0.35, mean_hold_frames: 60 }}
    Y:      {{ duty: 0.15, mean_hold_frames: 30 }}
    START:  {{ duty: 0.002, mean_hold_frames: 2 }}
    SELECT: {{ duty: 0.0,  mean_hold_frames: 1 }}
  direction:
    priors: {{ NEUTRAL: 0.15, LEFT: 0.10, RIGHT: 0.55, UP: 0.08, DOWN: 0.12 }}
    mean_hold_frames: 24
    stickiness: 0.55
    diagonal_factor: {diagonal_factor}
  start_from_history: true
"#
    )
}

/// Parse + validate, panicking with the validation errors on failure (test
/// fixtures are expected to always be valid; a failure here is a test bug).
pub fn parse_and_validate(yaml: &str) -> ExperimentConfig {
    let cfg = parse(yaml.as_bytes()).expect("parse fixture config");
    validate(&cfg).expect("fixture config must be valid");
    cfg
}

/// `base_yaml` plus API.md §5.5's `boss_hp` rule and START refractory, with
/// START's prior overridden to `{duty: 0.05, mean_hold_frames: 2}` so its
/// press rate is measurable at N=2000 (the API.md default `duty: 0.002`
/// needs a much larger corpus to see enough presses to compare rates
/// meaningfully — documented deviation, sub-test (e) only; the refractory
/// *mechanism* under test does not depend on the specific duty value).
pub fn context_rule_yaml(diagonal_factor: f64) -> ExperimentConfig {
    let base = parse_and_validate(&base_yaml(diagonal_factor));
    let overrides = br#"
context_rules:
  - when: { feature: boss_hp, op: gt, value: 0 }
    adjust_buttons: { Y: 1.2 }
    adjust_directions: { RIGHT: 0.5, LEFT: -0.5 }
  - when: { history: { pressed_within: { button: START, frames: 120 } } }
    adjust_buttons: { START: -4.0 }
refractory:
  - { button: START, frames: 120, logit_penalty: 4.0 }
weighted_random:
  buttons:
    START: { duty: 0.05, mean_hold_frames: 2 }
"#;
    let merged = deep_merge(&base, overrides).expect("deep_merge context-rule overrides");
    validate(&merged).expect("merged context-rule config must be valid");
    merged
}

/// A config variant with different direction priors (RIGHT/LEFT swapped in
/// emphasis), constructed via `deep_merge` so the golden suite also
/// exercises merge determinism, per the plan's golden-seed deliverable.
pub fn variant_direction_yaml(diagonal_factor: f64) -> ExperimentConfig {
    let base = parse_and_validate(&base_yaml(diagonal_factor));
    let overrides = br#"
weighted_random:
  direction:
    priors: { NEUTRAL: 0.30, LEFT: 0.40, RIGHT: 0.10, UP: 0.10, DOWN: 0.10 }
"#;
    let merged = deep_merge(&base, overrides).expect("deep_merge direction-variant overrides");
    validate(&merged).expect("merged variant config must be valid");
    merged
}

pub fn ctx_free(node_id: &str) -> GenContext {
    GenContext {
        node_id: node_id.to_owned(),
        ..Default::default()
    }
}

pub fn ctx_with_boss_hp(node_id: &str, boss_hp: f64) -> GenContext {
    GenContext {
        node_id: node_id.to_owned(),
        ram_features: vec![("boss_hp".to_owned(), boss_hp)],
        ..Default::default()
    }
}

/// `ram_features {boss_hp: 100., player_x: 12.}`, no `recent_inputs` — the
/// golden suite's "features only" context variant. Names must stay sorted
/// (`GenContext.ram_features` is documented as sorted-by-name).
pub fn ctx_features(node_id: &str) -> GenContext {
    GenContext {
        node_id: node_id.to_owned(),
        ram_features: vec![("boss_hp".to_owned(), 100.0), ("player_x".to_owned(), 12.0)],
        ..Default::default()
    }
}

/// Small history burst: START held for the first 5 of 10 frames, then
/// released — a rising edge at frame 0 (well within any refractory window),
/// used to trigger the START refractory rule. Deliberately released by the
/// *last* frame (unlike a single "held the whole time" segment) so
/// `last_frame_mask()` reads START as OFF: `start_from_history` continuation
/// (ARCHITECTURE.md §4.2) would otherwise force the generated burst's
/// initial START state to ON regardless of the refractory-suppressed duty,
/// injecting a spurious guaranteed "press" per burst that has nothing to do
/// with the refractory mechanism under test and would swamp the measured
/// suppression.
pub fn ctx_with_start_press(node_id: &str) -> GenContext {
    let bit_start = 10u8;
    GenContext {
        node_id: node_id.to_owned(),
        recent_inputs: Some(PadBurst {
            segments: vec![
                PadSegment {
                    buttons: 1u16 << bit_start,
                    hold_frames: 5,
                },
                PadSegment {
                    buttons: 0,
                    hold_frames: 5,
                },
            ],
        }),
        ..Default::default()
    }
}

/// Small history burst for the golden suite's "with context" variant: three
/// segments, ending with a held RIGHT (bit 9), so `start_from_history`
/// continuation has something concrete to continue.
pub fn ctx_full(node_id: &str) -> GenContext {
    let bit_right = 9u8;
    let bit_a = 0u8;
    GenContext {
        node_id: node_id.to_owned(),
        ram_features: vec![("boss_hp".to_owned(), 100.0), ("player_x".to_owned(), 12.0)],
        recent_inputs: Some(PadBurst {
            segments: vec![
                PadSegment {
                    buttons: 1u16 << bit_a,
                    hold_frames: 12,
                },
                PadSegment {
                    buttons: 0,
                    hold_frames: 8,
                },
                PadSegment {
                    buttons: 1u16 << bit_right,
                    hold_frames: 20,
                },
            ],
        }),
    }
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Bit position lookup against the console16 alphabet declared above, for
/// tests that need it symbolically instead of hardcoding numbers.
pub fn bit(cfg: &ExperimentConfig, name: &str) -> u8 {
    cfg.button_alphabet
        .bit(name)
        .unwrap_or_else(|| panic!("button {name} not declared in fixture alphabet"))
}
