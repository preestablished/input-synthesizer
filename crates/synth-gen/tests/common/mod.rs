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
        ..Default::default()
    }
}

/// `base_yaml` with the generator mix set to pure macro (`{macro: 1.0}`) and
/// `macro.packs` set to `pack_names`, for the macro_suite/golden_macro test
/// binaries. `chain_n`/`pad_to_length` stay at their config defaults (1,
/// true) unless the caller `deep_merge`s further overrides onto the result.
pub fn macro_cfg(pack_names: &[&str]) -> ExperimentConfig {
    let base = parse_and_validate(&base_yaml(0.25));
    let list = format!(
        "[{}]",
        pack_names
            .iter()
            .map(|n| format!("{n:?}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let overrides = format!(
        r#"
generator_mix:
  weighted_random: 0.0
  macro: 1.0
  mutation: 0.0
  policy: 0.0
macro:
  packs: {list}
  pad_to_length: true
  chain_n: 1
"#
    );
    let merged = deep_merge(&base, overrides.as_bytes()).expect("deep_merge macro overrides");
    validate(&merged).expect("merged macro config must be valid");
    merged
}

// ---------------------------------------------------------------------
// M3: mutation
// ---------------------------------------------------------------------

/// `base_yaml` with the generator mix set to pure mutation (`{mutation: 1.0}`),
/// for the mutation_suite/golden_mutation test binaries.
pub fn mutation_cfg() -> ExperimentConfig {
    let base = parse_and_validate(&base_yaml(0.25));
    let overrides = br#"
generator_mix:
  weighted_random: 0.0
  macro: 0.0
  mutation: 1.0
  policy: 0.0
"#;
    let merged = deep_merge(&base, overrides).expect("deep_merge mutation overrides");
    validate(&merged).expect("merged mutation config must be valid");
    merged
}

/// `mutation_cfg` with `mutation.op_probs` overridden to put probability 1.0
/// on a single named op (validation requires the map to sum to 1) — forces
/// every sampled mutant to apply exactly that op, for per-operator unit
/// goldens (`06-m3-mutation-stretch.md` Accept list).
pub fn mutation_cfg_forced_op(op: &str) -> ExperimentConfig {
    let base = mutation_cfg();
    // `deep_merge` merges maps key-by-key (it never drops keys absent from
    // the override), so every other op must be zeroed explicitly — an
    // override naming only `op` would leave the base document's other six
    // probabilities in place and fail the "sums to 1" validation rule.
    const ALL_OPS: &[&str] = &[
        "perturb_timing",
        "extend",
        "flip_button",
        "splice",
        "truncate",
        "duplicate_segment",
        "swap_adjacent",
    ];
    let entries: String = ALL_OPS
        .iter()
        .map(|name| format!("    {name}: {}\n", if *name == op { 1.0 } else { 0.0 }))
        .collect();
    let overrides = format!(
        "mutation:\n  op_probs:\n{entries}  donor_bias: 0.5\n  timing_sigma: 0.25\n  \
         ops_binomial: {{ n: 3, p: 0.25 }}\n"
    );
    let merged = deep_merge(&base, overrides.as_bytes()).expect("deep_merge op_probs override");
    validate(&merged).expect("merged forced-op config must be valid");
    merged
}

/// A 6-segment hand-built pad burst, deliberately varied (neutral, single
/// button, direction, direction+button, neutral, button) so every operator
/// has something to act on.
pub fn mutation_base_pad() -> PadBurst {
    PadBurst {
        segments: vec![
            PadSegment {
                buttons: 0,
                hold_frames: 10,
            },
            PadSegment {
                buttons: 1 << 0, // A
                hold_frames: 8,
            },
            PadSegment {
                buttons: 1 << 9, // RIGHT
                hold_frames: 20,
            },
            PadSegment {
                buttons: (1 << 9) | (1 << 1), // RIGHT + B
                hold_frames: 5,
            },
            PadSegment {
                buttons: 0,
                hold_frames: 12,
            },
            PadSegment {
                buttons: 1 << 3, // Y
                hold_frames: 6,
            },
        ],
    }
}

/// A second hand-built pad burst, distinct in content (and therefore
/// `burst_id`) from `mutation_base_pad`, for tests that need a
/// donor/sibling that isn't identical to the base.
pub fn mutation_sibling_pad_a() -> PadBurst {
    PadBurst {
        segments: vec![
            PadSegment {
                buttons: 1 << 6, // UP
                hold_frames: 15,
            },
            PadSegment {
                buttons: 1 << 4, // L
                hold_frames: 9,
            },
            PadSegment {
                buttons: 0,
                hold_frames: 30,
            },
        ],
    }
}

/// A third distinct hand-built pad burst.
pub fn mutation_sibling_pad_b() -> PadBurst {
    PadBurst {
        segments: vec![
            PadSegment {
                buttons: (1 << 7) | (1 << 5), // DOWN + R
                hold_frames: 4,
            },
            PadSegment {
                buttons: 1 << 2, // X
                hold_frames: 40,
            },
        ],
    }
}

/// Wrap a `PadBurst` into a `ContextBurst` with its content-addressed id.
pub fn context_burst_from(pad: PadBurst) -> synth_gen::context::ContextBurst {
    let id = synth_core::types::burst_hash(&synth_core::types::Burst::Pad(pad.clone()));
    synth_gen::context::ContextBurst { pad, burst_id: id }
}

pub fn ctx_with_parent(node_id: &str, parent: PadBurst) -> GenContext {
    GenContext {
        node_id: node_id.to_owned(),
        parent_burst: Some(context_burst_from(parent)),
        ..Default::default()
    }
}

pub fn ctx_with_siblings_only(node_id: &str, siblings: Vec<(PadBurst, f64)>) -> GenContext {
    GenContext {
        node_id: node_id.to_owned(),
        sibling_bursts: siblings
            .into_iter()
            .map(|(p, sd)| synth_gen::context::ScoredContextBurst {
                burst: context_burst_from(p),
                score_delta: sd,
            })
            .collect(),
        ..Default::default()
    }
}

pub fn ctx_with_parent_and_siblings(
    node_id: &str,
    parent: PadBurst,
    siblings: Vec<(PadBurst, f64)>,
) -> GenContext {
    GenContext {
        node_id: node_id.to_owned(),
        parent_burst: Some(context_burst_from(parent)),
        sibling_bursts: siblings
            .into_iter()
            .map(|(p, sd)| synth_gen::context::ScoredContextBurst {
                burst: context_burst_from(p),
                score_delta: sd,
            })
            .collect(),
        ..Default::default()
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
