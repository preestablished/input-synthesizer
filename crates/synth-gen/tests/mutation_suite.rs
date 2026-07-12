//! M3 mutation-generator acceptance suite (`06-m3-mutation-stretch.md`
//! Accept list, ARCHITECTURE.md §5.2, `synth_gen::mutation` module doc).

#[path = "common/mod.rs"]
mod common;

use proptest::prelude::*;
use statrs::distribution::{ChiSquared, ContinuousCDF};
use synth_core::model::InputModel;
use synth_core::rng::fanout_root;
use synth_core::types::{burst_hash, Burst, PadBurst, PadSegment};
use synth_gen::context::GenContext;
use synth_gen::mutation;
use synth_gen::propose::propose;
use synth_gen::provenance::GeneratorKind;
use synth_pad::PadModel;

fn model_for(cfg: &synth_core::config::ExperimentConfig) -> PadModel {
    PadModel::new(
        &cfg.button_alphabet,
        cfg.burst_len.min_frames,
        cfg.burst_len.max_frames,
    )
}

fn pad_of(burst: &Burst) -> &PadBurst {
    match burst {
        Burst::Pad(p) => p,
        #[allow(unreachable_patterns)]
        _ => unreachable!("M1..M3 scope: pad model only"),
    }
}

fn seg_tuples(pad: &PadBurst) -> Vec<(u16, u32)> {
    pad.segments
        .iter()
        .map(|s| (s.buttons, s.hold_frames))
        .collect()
}

// ---------------------------------------------------------------------
// Per-operator unit goldens (fixed seed => exact output burst per operator).
//
// Values below were captured by running exactly this fixture (mutation_cfg_
// forced_op(op), ctx_with_parent_and_siblings("dump-node", ...), the listed
// seed) once and are now pinned; a deliberate sampling change must update
// them in the same PR as a `SYNTH_VERSION` bump (00- global rule).
// ---------------------------------------------------------------------

struct OpCase {
    op: &'static str,
    seed: u64,
    expected_segments: &'static [(u16, u32)],
    expected_base_was_sibling: bool,
    expected_has_donor: bool,
}

const OP_CASES: &[OpCase] = &[
    OpCase {
        op: "perturb_timing",
        seed: 100,
        expected_segments: &[(0, 8), (1, 11), (512, 19), (514, 5), (0, 11), (8, 6)],
        expected_base_was_sibling: true,
        expected_has_donor: false,
    },
    OpCase {
        op: "extend",
        seed: 101,
        expected_segments: &[
            (0, 10),
            (1, 8),
            (512, 20),
            (514, 5),
            (0, 12),
            (8, 9),
            (12, 1),
            (28, 1),
            (20, 7),
            (0, 5),
            (2, 2),
            (10, 9),
            (2, 15),
        ],
        expected_base_was_sibling: false,
        expected_has_donor: false,
    },
    OpCase {
        op: "flip_button",
        seed: 102,
        expected_segments: &[(0, 10), (16, 8), (512, 20), (514, 5), (0, 12), (8, 6)],
        expected_base_was_sibling: true,
        expected_has_donor: false,
    },
    OpCase {
        op: "splice",
        seed: 103,
        expected_segments: &[
            (0, 10),
            (1, 8),
            (512, 20),
            (514, 5),
            (0, 22),
            (1, 8),
            (512, 20),
            (514, 5),
            (0, 12),
            (8, 6),
        ],
        expected_base_was_sibling: true,
        expected_has_donor: true,
    },
    OpCase {
        op: "truncate",
        seed: 104,
        expected_segments: &[(0, 10), (1, 8), (512, 20), (514, 5), (0, 3)],
        expected_base_was_sibling: false,
        expected_has_donor: false,
    },
    OpCase {
        op: "duplicate_segment",
        seed: 105,
        expected_segments: &[
            (0, 10),
            (1, 8),
            (512, 20),
            (514, 5),
            (0, 12),
            (8, 6),
            (0, 12),
            (8, 18),
        ],
        expected_base_was_sibling: false,
        expected_has_donor: false,
    },
    OpCase {
        op: "swap_adjacent",
        seed: 106,
        expected_segments: &[(0, 10), (1, 8), (512, 20), (514, 5), (8, 6), (0, 12)],
        expected_base_was_sibling: true,
        expected_has_donor: false,
    },
];

fn dump_ctx() -> (PadBurst, GenContext) {
    let parent = common::mutation_base_pad();
    let siblings = vec![
        (common::mutation_base_pad(), 1.0),
        (common::mutation_base_pad(), 2.0),
    ];
    let ctx = common::ctx_with_parent_and_siblings("dump-node", parent.clone(), siblings);
    (parent, ctx)
}

#[test]
fn per_operator_goldens_match_pinned_output() {
    for case in OP_CASES {
        let cfg = common::mutation_cfg_forced_op(case.op);
        let model = model_for(&cfg);
        let (_, ctx) = dump_ctx();
        let root = fanout_root(case.seed, &ctx.node_id);
        let (burst, prov) = mutation::generate(&cfg, &ctx, &root, 0, &model);
        let pad = pad_of(&burst);
        assert_eq!(
            seg_tuples(pad),
            case.expected_segments.to_vec(),
            "op {}: segment mismatch",
            case.op
        );
        assert_eq!(
            prov.base_was_sibling, case.expected_base_was_sibling,
            "op {}: base_was_sibling mismatch",
            case.op
        );
        assert_eq!(
            prov.donor_burst_id.is_some(),
            case.expected_has_donor,
            "op {}: donor presence mismatch",
            case.op
        );
    }
}

// ---------------------------------------------------------------------
// Operator frequency + mean ops/mutant.
// ---------------------------------------------------------------------

fn chi2_critical(dof: u64, alpha: f64) -> f64 {
    ChiSquared::new(dof as f64)
        .expect("valid dof")
        .inverse_cdf(1.0 - alpha)
}

/// Ops actually sampled for a mutant, excluding the forced retry pass (which
/// is not one of the `n_ops = 1 + min(B,3)` draws op_probs governs).
fn non_retry_ops(
    ops: &[synth_gen::provenance::MutationOpRec],
) -> &[synth_gen::provenance::MutationOpRec] {
    match ops.last() {
        Some(last) if last.args.iter().any(|(k, _)| k == "retry") => &ops[..ops.len() - 1],
        _ => ops,
    }
}

#[test]
fn operator_frequency_and_mean_ops_match_config() {
    const N: usize = 10_000;
    let cfg = common::mutation_cfg();
    let parent = common::mutation_base_pad();
    let siblings = vec![
        (common::mutation_sibling_pad_a(), 1.0),
        (common::mutation_sibling_pad_b(), 2.5),
    ];
    let ctx = common::ctx_with_parent_and_siblings("freq-node", parent, siblings);

    let (results, degraded) = propose(&cfg, &ctx, N, 0, 0x5EED_F0F0, None);
    assert!(
        degraded.is_empty(),
        "pure-mutation mix should never degrade here"
    );

    let op_names: Vec<&str> = cfg.mutation.op_probs.keys().map(String::as_str).collect();
    let mut counts = vec![0u64; op_names.len()];
    let mut total_ops = 0u64;
    let mut total_mutants = 0u64;

    for r in &results {
        let prov = r
            .provenance
            .mutation
            .as_ref()
            .expect("every slot in a pure-mutation mix carries MutationProvenance");
        let ops = non_retry_ops(&prov.ops);
        total_mutants += 1;
        total_ops += ops.len() as u64;
        for op in ops {
            let idx = op_names
                .iter()
                .position(|n| *n == op.op.as_str())
                .unwrap_or_else(|| panic!("unexpected op name {:?}", op.op));
            counts[idx] += 1;
        }
    }

    let mean_ops = total_ops as f64 / total_mutants as f64;
    assert!(
        (mean_ops - 1.75).abs() <= 0.05,
        "mean ops/mutant {mean_ops:.4} not within 1.75 +- 0.05"
    );

    let mut chi2 = 0.0f64;
    for (i, name) in op_names.iter().enumerate() {
        let expected_p = cfg.mutation.op_probs[*name];
        let expected = expected_p * total_ops as f64;
        let observed = counts[i] as f64;
        chi2 += (observed - expected).powi(2) / expected;
    }
    let dof = (op_names.len() - 1) as u64;
    let critical = chi2_critical(dof, 0.001);
    assert!(
        chi2 < critical,
        "operator-frequency chi2 = {chi2:.3} exceeds p<0.001 critical {critical:.3} (dof={dof})"
    );
}

// ---------------------------------------------------------------------
// Property tests.
// ---------------------------------------------------------------------

proptest! {
    /// Every mutant is legal, within length bounds, and differs from its
    /// base by `burst_hash` (the retry logic's whole purpose).
    #[test]
    fn mutant_is_legal_bounded_and_differs_from_base(
        seed in any::<u64>(),
    ) {
        let cfg = common::mutation_cfg();
        let model = model_for(&cfg);
        // Derive an arbitrary base burst deterministically from `seed`
        // itself (a small splitmix64-style construction) rather than a
        // second `proptest` strategy, so shrinking still varies exactly one
        // input.
        let n = 3 + (seed % 5) as usize;
        let mut raw = Vec::with_capacity(n);
        let mut s = seed;
        for _ in 0..n {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1);
            let buttons = (s >> 32) as u16 & 0x0FFF;
            let hold = 1 + ((s >> 16) as u32 % 60);
            raw.push(PadSegment { buttons, hold_frames: hold });
        }
        let base_burst = model.legalize(Burst::Pad(PadBurst { segments: raw }));
        let Burst::Pad(base_pad) = base_burst.clone() else { unreachable!() };

        let ctx = common::ctx_with_parent("prop-node", base_pad);
        let root = fanout_root(seed, &ctx.node_id);
        let (mutant, prov) = mutation::generate(&cfg, &ctx, &root, 0, &model);
        let pad = pad_of(&mutant);

        let up = 1u16 << common::bit(&cfg, "UP");
        let down = 1u16 << common::bit(&cfg, "DOWN");
        let left = 1u16 << common::bit(&cfg, "LEFT");
        let right = 1u16 << common::bit(&cfg, "RIGHT");
        let total = pad.total_frames();
        prop_assert!(total >= u64::from(cfg.burst_len.min_frames));
        prop_assert!(total <= u64::from(cfg.burst_len.max_frames));
        let mut prev: Option<u16> = None;
        for seg in &pad.segments {
            prop_assert!(seg.hold_frames >= 1);
            prop_assert!(seg.buttons & (up | down) != (up | down));
            prop_assert!(seg.buttons & (left | right) != (left | right));
            if let Some(p) = prev {
                prop_assert_ne!(p, seg.buttons);
            }
            prev = Some(seg.buttons);
        }

        // ARCHITECTURE §5.2: an identical mutant forces one perturb_timing
        // retry, THEN is accepted even if still identical (possible on
        // degenerate bases: the retry's per-segment 0.5 gates can all miss,
        // or the post-clamp can revert a length change at the bounds). So
        // the property is: differs from base, OR the recorded ops show the
        // accepted-after-retry path was taken.
        let retried = prov
            .ops
            .iter()
            .any(|op| op.args.iter().any(|(k, v)| k == "retry" && v == "1"));
        prop_assert!(
            burst_hash(&mutant) != burst_hash(&base_burst) || retried,
            "mutant identical to base without a recorded retry pass"
        );

        // splice-without-donor (no siblings here) is always recorded as
        // extend, never literally "splice".
        for op in &prov.ops {
            prop_assert_ne!(op.op.as_str(), "splice");
        }
    }
}

// ---------------------------------------------------------------------
// Provenance replay (the accept test).
// ---------------------------------------------------------------------

fn resolve_by_id(ctx: &GenContext, id: [u8; 32]) -> Option<PadBurst> {
    if let Some(p) = &ctx.parent_burst {
        if p.burst_id == id {
            return Some(p.pad.clone());
        }
    }
    for s in &ctx.sibling_bursts {
        if s.burst.burst_id == id {
            return Some(s.burst.pad.clone());
        }
    }
    None
}

#[test]
fn provenance_replay_reproduces_mutant_exactly() {
    let cfg = common::mutation_cfg();
    let model = model_for(&cfg);

    for seed in 0..200u64 {
        let ctx = if seed % 2 == 0 {
            common::ctx_with_parent("replay-node", common::mutation_base_pad())
        } else {
            common::ctx_with_parent_and_siblings(
                "replay-node",
                common::mutation_base_pad(),
                vec![
                    (common::mutation_sibling_pad_a(), 1.0),
                    (common::mutation_sibling_pad_b(), 2.0),
                ],
            )
        };
        let root = fanout_root(seed, &ctx.node_id);
        let (burst, prov) = mutation::generate(&cfg, &ctx, &root, 0, &model);

        let base_pad = PadBurst {
            segments: resolve_by_id(&ctx, prov.base_burst_id)
                .unwrap_or_else(|| panic!("seed {seed}: base_burst_id not resolvable from ctx"))
                .segments,
        };
        // Fix #5: pass every sibling burst as a potential donor (not just
        // the one `MutationProvenance.donor_burst_id` happens to record —
        // that field only reflects the LAST splice's donor), since replay
        // must resolve each splice op from its own recorded arg.
        let sibling_pads: Vec<([u8; 32], PadBurst)> = ctx
            .sibling_bursts
            .iter()
            .map(|s| (s.burst.burst_id, s.burst.pad.clone()))
            .collect();
        let donors: Vec<(&[u8; 32], &PadBurst)> =
            sibling_pads.iter().map(|(id, pad)| (id, pad)).collect();

        let replayed = mutation::apply_ops(&cfg, &model, &base_pad, &prov.ops, &donors);
        assert_eq!(
            replayed, burst,
            "seed {seed}: replay did not reproduce the mutant exactly"
        );
    }
}

/// Fix #5 directed regression: force exactly 2 sampled ops via a degenerate
/// `ops_binomial` (`n: 1, p: 1.0` ⇒ `B = 1` ⇒ `n_ops = 1 + min(1, 3) = 2`,
/// deterministically, no seed search needed for op COUNT) and pin
/// `op_probs` so both are `splice`. With >= 2 siblings of very different
/// weight, search a small seed range for a case where the two splice ops
/// actually pick DIFFERENT donors (the scenario the single-donor bug
/// silently mishandled), then assert replay reproduces the mutant exactly.
#[test]
fn multi_splice_replay_resolves_each_ops_own_donor() {
    let base = common::mutation_cfg_forced_op("splice");
    let overrides = br#"
mutation:
  ops_binomial: { n: 1, p: 1.0 }
"#;
    let cfg = synth_core::config::deep_merge(&base, overrides).expect("merge");
    synth_core::config::validate(&cfg).expect("valid");
    let model = model_for(&cfg);

    let parent = common::mutation_base_pad();
    let siblings = vec![
        (common::mutation_sibling_pad_a(), 1.0),
        (common::mutation_sibling_pad_b(), 50.0),
    ];

    let mut found = false;
    for seed in 0u64..500 {
        let ctx = common::ctx_with_parent_and_siblings(
            "multi-splice-node",
            parent.clone(),
            siblings.clone(),
        );
        let root = fanout_root(seed, &ctx.node_id);
        let (burst, prov) = mutation::generate(&cfg, &ctx, &root, 0, &model);

        assert!(
            prov.ops.len() == 2
                || (prov.ops.len() == 3 && prov.ops[2].args.iter().any(|(k, _)| k == "retry")),
            "seed {seed}: forced ops_binomial must yield exactly 2 sampled ops (plus an \
             optional forced retry), got {}",
            prov.ops.len()
        );
        let splice_ops: Vec<_> = prov.ops.iter().filter(|o| o.op == "splice").collect();
        if splice_ops.len() < 2 {
            continue;
        }
        let donor_of = |o: &synth_gen::provenance::MutationOpRec| {
            o.args
                .iter()
                .find(|(k, _)| k == "donor_burst_id")
                .map(|(_, v)| v.clone())
        };
        let d0 = donor_of(splice_ops[0]);
        let d1 = donor_of(splice_ops[1]);
        if d0 == d1 {
            continue;
        }

        let base_pad = PadBurst {
            segments: resolve_by_id(&ctx, prov.base_burst_id)
                .unwrap_or_else(|| panic!("seed {seed}: base_burst_id not resolvable"))
                .segments,
        };
        let sibling_pads: Vec<([u8; 32], PadBurst)> = ctx
            .sibling_bursts
            .iter()
            .map(|s| (s.burst.burst_id, s.burst.pad.clone()))
            .collect();
        let donors: Vec<(&[u8; 32], &PadBurst)> =
            sibling_pads.iter().map(|(id, pad)| (id, pad)).collect();
        let replayed = mutation::apply_ops(&cfg, &model, &base_pad, &prov.ops, &donors);
        assert_eq!(
            replayed, burst,
            "seed {seed}: replay with two distinct-donor splice ops did not reproduce the \
             mutant exactly"
        );
        found = true;
        break;
    }
    assert!(
        found,
        "no seed in 0..500 produced two splice ops with distinct donors — widen the search"
    );
}

// ---------------------------------------------------------------------
// Degradation.
// ---------------------------------------------------------------------

#[test]
fn degraded_without_parent_or_siblings_excludes_mutation_slots() {
    // `base_yaml` pins a pure-weighted-random mix; the API.md §5.2 *default*
    // `GeneratorMix` (mutation weight 0.20) is what `testdata/config/valid/
    // minimal.yaml` exercises (same fixture `propose.rs`'s own
    // `degraded_lists_unavailable_generators` unit test uses).
    let yaml = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/config/valid/minimal.yaml"),
    )
    .expect("read minimal config");
    let cfg = synth_core::config::parse(&yaml).expect("parse minimal config");
    synth_core::config::validate(&cfg).expect("minimal config is valid");
    assert!(cfg.generator_mix.mutation > 0.0);
    let ctx = common::ctx_free("no-parent-node");
    let (results, degraded) = propose(&cfg, &ctx, 16, 0, 0xABCDEF, None);
    assert_eq!(results.len(), 16);
    assert!(results
        .iter()
        .all(|r| r.provenance.generator != GeneratorKind::Mutation));
    let reasons: Vec<&str> = degraded.iter().map(|d| d.reason.as_str()).collect();
    assert!(reasons.contains(&"no_parent_burst"));
}
