//! Request pipeline (ARCHITECTURE.md §3): mixer plan → per-slot generate →
//! legalize → provenance. Pure given (config, context, seed); the server
//! shell handles proto/transport concerns.

use synth_core::config::ExperimentConfig;
use synth_core::model::InputModel;
use synth_core::rng::{fanout_root, stream};
use synth_core::types::Burst;
use synth_pad::PadModel;

use crate::context::{self, GenContext};
use crate::macros;
use crate::macros::ResolvedMacros;
use crate::mixer::{allocate_slots, GeneratorWeight};
use crate::mutation;
use crate::provenance::{Degraded, GeneratorKind, Provenance};
use crate::weighted_random;

#[derive(Clone, Debug)]
pub struct SlotResult {
    pub burst: Burst,
    pub provenance: Provenance,
}

/// Generate `k` bursts. Returns slot-ordered results plus the degraded list.
///
/// `macros`: `None` if no packs are resolved/loaded for this request; else
/// `Some(&ResolvedMacros)` from `synth_gen::macros::resolve`. Mutation
/// availability (M3) is derived from `ctx.parent_burst`/`ctx.sibling_bursts`
/// directly (see `generator_weights`); policy (M6) weight is still always
/// reallocated in this milestone.
pub fn propose(
    cfg: &ExperimentConfig,
    ctx: &GenContext,
    k: usize,
    length_hint: u32,
    seed: u64,
    macros: Option<&ResolvedMacros>,
) -> (Vec<SlotResult>, Vec<Degraded>) {
    let root = fanout_root(seed, &ctx.node_id);
    let model = PadModel::new(
        &cfg.button_alphabet,
        cfg.burst_len.min_frames,
        cfg.burst_len.max_frames,
    );

    let eligible_macro_count = macros.map_or(0, |resolved| {
        resolved
            .items
            .iter()
            .filter(|m| {
                m.eligibility
                    .iter()
                    .all(|p| context::eval_predicate(p, ctx, cfg))
            })
            .count()
    });

    let weights = generator_weights(cfg, ctx, macros.is_some(), eligible_macro_count);
    let mut mix_rng = stream(&root, "mix");
    let (plan, degraded) = allocate_slots(k, &weights, &mut mix_rng);

    let mut results = Vec::with_capacity(k);
    for (slot, kind) in plan.into_iter().enumerate() {
        let (burst, rng_stream, macro_prov, mutation_prov) = match kind {
            GeneratorKind::WeightedRandom => (
                weighted_random::generate(cfg, ctx, &root, slot, length_hint),
                format!("slot/{slot}/wr/dir"),
                None,
                None,
            ),
            GeneratorKind::Macro => {
                let resolved = macros.expect(
                    "mixer only assigns Macro when generator_weights reported it \
                     available, which requires macros = Some(..) with >= 1 eligible macro",
                );
                let (burst, prov) =
                    macros::generate_macro_burst(cfg, ctx, &root, slot, length_hint, resolved);
                (burst, format!("slot/{slot}/macro/pick"), Some(prov), None)
            }
            GeneratorKind::Mutation => {
                let (burst, prov) = mutation::generate(cfg, ctx, &root, slot, &model);
                (burst, format!("slot/{slot}/mut/ops"), None, Some(prov))
            }
            // The mixer only assigns generators whose availability the
            // caller vouched for; policy lands with M6.
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
                macro_: macro_prov,
                mutation: mutation_prov,
            },
        });
    }
    (results, degraded)
}

/// Map the config mix + availability to mixer inputs. Weighted-random is
/// always available (it needs nothing beyond config).
fn generator_weights(
    cfg: &ExperimentConfig,
    ctx: &GenContext,
    macros_present: bool,
    eligible_macro_count: usize,
) -> Vec<GeneratorWeight> {
    let mix = &cfg.generator_mix;
    let macro_unavailable = if !macros_present {
        Some("no_macros_loaded".to_owned())
    } else if eligible_macro_count == 0 {
        Some("no_eligible_macros".to_owned())
    } else {
        None
    };
    let mutation_available = ctx.parent_burst.is_some() || !ctx.sibling_bursts.is_empty();
    let mut weights = vec![
        GeneratorWeight {
            kind: GeneratorKind::WeightedRandom,
            weight: mix.weighted_random,
            unavailable: None,
        },
        GeneratorWeight {
            kind: GeneratorKind::Macro,
            weight: mix.macro_,
            unavailable: macro_unavailable,
        },
        GeneratorWeight {
            kind: GeneratorKind::Mutation,
            weight: mix.mutation,
            unavailable: if mutation_available {
                None
            } else {
                Some("no_parent_burst".to_owned())
            },
        },
        GeneratorWeight {
            kind: GeneratorKind::Policy,
            weight: mix.policy,
            unavailable: Some("policy_endpoint_down".to_owned()),
        },
    ];

    // Wire-reachable fallback (INTEGRATION.md §7: unavailability must
    // degrade, never error): a config can validly declare weight only on
    // generators that end up unavailable for this context/pack state (e.g.
    // `{weighted_random: 0, macro: 0, mutation: 1}` with no parent/siblings).
    // If nothing is left available, force weighted-random on — it needs
    // nothing beyond config, so it is the universal fallback the mixer can
    // always allocate every slot to.
    if !weights
        .iter()
        .any(|w| w.unavailable.is_none() && w.weight > 0.0)
    {
        if let Some(wr) = weights
            .iter_mut()
            .find(|w| w.kind == GeneratorKind::WeightedRandom)
        {
            wr.weight = 1.0;
            wr.unavailable = None;
        }
    }

    weights
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
        let run = || propose(&cfg, &ctx(), 32, 0, 0xDEADBEEF, None);
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
        let (a, _) = propose(&cfg, &ctx(), 4, 0, 1, None);
        let (b, _) = propose(&cfg, &ctx(), 4, 0, 2, None);
        assert_ne!(
            a.iter().map(|r| &r.burst).collect::<Vec<_>>(),
            b.iter().map(|r| &r.burst).collect::<Vec<_>>()
        );
    }

    #[test]
    fn degraded_lists_unavailable_generators() {
        let cfg = cfg();
        let (_, degraded) = propose(&cfg, &ctx(), 8, 0, 7, None);
        // Default mix: macro 0.35 (no packs), mutation 0.20 (no parent),
        // policy 0.0 (zero weight — not reported).
        let reasons: Vec<&str> = degraded.iter().map(|d| d.reason.as_str()).collect();
        assert!(reasons.contains(&"no_macros_loaded"));
        assert!(reasons.contains(&"no_parent_burst"));
        assert_eq!(degraded.len(), 2);
    }

    /// Fix #1: a mutation-only mix with no parent/siblings must degrade to
    /// weighted-random for every slot instead of panicking in
    /// `allocate_slots`.
    #[test]
    fn all_unavailable_generators_fall_back_to_weighted_random() {
        let cfg = cfg();
        let overrides =
            b"generator_mix: { weighted_random: 0.0, macro: 0.0, mutation: 1.0, policy: 0.0 }";
        let merged = synth_core::config::deep_merge(&cfg, overrides).expect("merge overrides");
        synth_core::config::validate(&merged).expect("merged config must be valid");

        let (results, degraded) = propose(&merged, &ctx(), 8, 0, 123, None);
        assert_eq!(results.len(), 8);
        assert!(
            results
                .iter()
                .all(|r| r.provenance.generator == GeneratorKind::WeightedRandom),
            "every slot must fall back to weighted-random"
        );
        let reasons: Vec<&str> = degraded.iter().map(|d| d.reason.as_str()).collect();
        assert!(reasons.contains(&"no_parent_burst"));
    }

    #[test]
    fn bursts_are_legal_and_length_bounded() {
        let cfg = cfg();
        let (results, _) = propose(&cfg, &ctx(), 64, 300, 99, None);
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
