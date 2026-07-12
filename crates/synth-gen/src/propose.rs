//! Request pipeline (ARCHITECTURE.md §3): mixer plan → per-slot generate →
//! legalize → provenance. Pure given (config, context, seed); the server
//! shell handles proto/transport concerns.

use synth_core::config::ExperimentConfig;
use synth_core::model::InputModel;
use synth_core::rng::{fanout_root, stream};
use synth_core::types::Burst;
use synth_pad::PadModel;

use crate::context::GenContext;
use crate::mixer::{allocate_slots, GeneratorWeight};
use crate::provenance::{Degraded, GeneratorKind, Provenance};
use crate::weighted_random;

/// Availability inputs the server resolves before calling [`propose`].
#[derive(Clone, Copy, Debug, Default)]
pub struct Availability {
    /// At least one pack named by `cfg.macro_.packs` is loaded (M2).
    pub macros_loaded: bool,
    /// The request carried a parent burst (mutation base, M3).
    pub has_parent_burst: bool,
}

#[derive(Clone, Debug)]
pub struct SlotResult {
    pub burst: Burst,
    pub provenance: Provenance,
}

/// Generate `k` bursts. Returns slot-ordered results plus the degraded list.
///
/// M1 scope: weighted-random slots only; the mixer reallocates macro (until
/// packs are loadable, M2), mutation (M3), and policy (M6) weight, reporting
/// each in `degraded[]`.
pub fn propose(
    cfg: &ExperimentConfig,
    ctx: &GenContext,
    k: usize,
    length_hint: u32,
    seed: u64,
    availability: Availability,
) -> (Vec<SlotResult>, Vec<Degraded>) {
    let root = fanout_root(seed, &ctx.node_id);
    let model = PadModel::new(
        &cfg.button_alphabet,
        cfg.burst_len.min_frames,
        cfg.burst_len.max_frames,
    );

    let weights = generator_weights(cfg, availability);
    let mut mix_rng = stream(&root, "mix");
    let (plan, degraded) = allocate_slots(k, &weights, &mut mix_rng);

    let mut results = Vec::with_capacity(k);
    for (slot, kind) in plan.into_iter().enumerate() {
        let (burst, rng_stream) = match kind {
            GeneratorKind::WeightedRandom => (
                weighted_random::generate(cfg, ctx, &root, slot, length_hint),
                format!("slot/{slot}/wr"),
            ),
            // The mixer only assigns generators whose availability the
            // caller vouched for; macro dispatch lands with M2, mutation
            // with M3, policy with M6.
            other => unreachable!("mixer assigned {other:?} but no such generator is wired yet"),
        };
        let burst = model.legalize(burst);
        results.push(SlotResult {
            burst,
            provenance: Provenance {
                generator: kind,
                slot: u32::try_from(slot).expect("k <= 256"),
                rng_stream,
                fallback_from: None,
                macro_: None,
            },
        });
    }
    (results, degraded)
}

/// Map the config mix + availability to mixer inputs. Weighted-random is
/// always available (it needs nothing beyond config).
fn generator_weights(cfg: &ExperimentConfig, availability: Availability) -> Vec<GeneratorWeight> {
    let mix = &cfg.generator_mix;
    vec![
        GeneratorWeight {
            kind: GeneratorKind::WeightedRandom,
            weight: mix.weighted_random,
            unavailable: None,
        },
        GeneratorWeight {
            kind: GeneratorKind::Macro,
            weight: mix.macro_,
            unavailable: (!availability.macros_loaded).then(|| "no_macros_loaded".to_owned()),
        },
        GeneratorWeight {
            kind: GeneratorKind::Mutation,
            weight: mix.mutation,
            // M3 wires the generator; until then the slot class is
            // unavailable regardless of context.
            unavailable: Some(if availability.has_parent_burst {
                "mutation_generator_unavailable".to_owned()
            } else {
                "no_parent_burst".to_owned()
            }),
        },
        GeneratorWeight {
            kind: GeneratorKind::Policy,
            weight: mix.policy,
            unavailable: Some("policy_endpoint_down".to_owned()),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use synth_core::config::parse;

    fn cfg() -> ExperimentConfig {
        let yaml = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../testdata/config/valid/minimal.yaml"),
        )
        .expect("read minimal config");
        let cfg = parse(&yaml).expect("parse");
        synth_core::config::validate(&cfg).expect("valid");
        cfg
    }

    fn ctx() -> GenContext {
        GenContext {
            node_id: "node-0".to_owned(),
            ..Default::default()
        }
    }

    #[test]
    fn propose_is_deterministic_and_slot_ordered() {
        let cfg = cfg();
        let run = || propose(&cfg, &ctx(), 32, 0, 0xDEADBEEF, Availability::default());
        let (a, da) = run();
        let (b, db) = run();
        assert_eq!(a.len(), 32);
        for (x, y) in a.iter().zip(&b) {
            assert_eq!(x.burst, y.burst);
            assert_eq!(x.provenance, y.provenance);
        }
        assert_eq!(da, db);
        for (i, r) in a.iter().enumerate() {
            assert_eq!(r.provenance.slot as usize, i);
        }
    }

    #[test]
    fn different_seeds_differ() {
        let cfg = cfg();
        let (a, _) = propose(&cfg, &ctx(), 4, 0, 1, Availability::default());
        let (b, _) = propose(&cfg, &ctx(), 4, 0, 2, Availability::default());
        assert_ne!(
            a.iter().map(|r| &r.burst).collect::<Vec<_>>(),
            b.iter().map(|r| &r.burst).collect::<Vec<_>>()
        );
    }

    #[test]
    fn degraded_lists_unavailable_generators() {
        let cfg = cfg();
        let (_, degraded) = propose(&cfg, &ctx(), 8, 0, 7, Availability::default());
        // Default mix: macro 0.35 (no packs), mutation 0.20 (no parent),
        // policy 0.0 (zero weight — not reported).
        let reasons: Vec<&str> = degraded.iter().map(|d| d.reason.as_str()).collect();
        assert!(reasons.contains(&"no_macros_loaded"));
        assert!(reasons.contains(&"no_parent_burst"));
        assert_eq!(degraded.len(), 2);
    }

    #[test]
    fn bursts_are_legal_and_length_bounded() {
        let cfg = cfg();
        let (results, _) = propose(&cfg, &ctx(), 64, 300, 99, Availability::default());
        for r in results {
            #[allow(irrefutable_let_patterns)]
            let Burst::Pad(pad) = &r.burst
            else {
                unreachable!("pad model emits pad bursts")
            };
            let total = pad.total_frames();
            assert!(total >= u64::from(cfg.burst_len.min_frames));
            assert!(total <= u64::from(cfg.burst_len.max_frames));
            for s in &pad.segments {
                assert!(s.hold_frames >= 1);
                // UP|DOWN and LEFT|RIGHT never co-held.
                assert!(s.buttons & 0b1100_0000 != 0b1100_0000);
                assert!(s.buttons & 0b11_0000_0000 != 0b11_0000_0000);
            }
        }
    }
}
