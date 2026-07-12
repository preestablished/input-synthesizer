//! Mixer: deterministic stratified slot allocation (ARCHITECTURE.md §3.1).
//!
//! Draw-order contract for stream `"mix"`: first the remainder assignment
//! (one `u` per remaining slot, sampling without replacement over fractional
//! parts), then the Fisher–Yates slot permutation (one `u` per swap, indices
//! K−1 down to 1).

use rand_chacha::ChaCha8Rng;
use synth_core::rng::next_unit_f64;

use crate::provenance::{Degraded, GeneratorKind};

/// A generator's weight and availability for one request.
#[derive(Clone, Debug)]
pub struct GeneratorWeight {
    pub kind: GeneratorKind,
    pub weight: f64,
    /// `None` = available; `Some(reason)` = unavailable (weight reallocated,
    /// reported via `degraded[]`).
    pub unavailable: Option<String>,
}

/// Allocate `k` slots across the available generators. Returns the
/// per-slot generator assignment (permuted) and the degraded list.
pub fn allocate_slots(
    k: usize,
    weights: &[GeneratorWeight],
    rng: &mut ChaCha8Rng,
) -> (Vec<GeneratorKind>, Vec<Degraded>) {
    let mut degraded = Vec::new();
    let mut available: Vec<(GeneratorKind, f64)> = Vec::new();
    for gw in weights {
        match &gw.unavailable {
            Some(reason) if gw.weight > 0.0 => degraded.push(Degraded {
                generator: gw.kind,
                reason: reason.clone(),
            }),
            Some(_) => {}
            None if gw.weight > 0.0 => available.push((gw.kind, gw.weight)),
            None => {}
        }
    }
    assert!(
        !available.is_empty(),
        "mixer requires at least one available generator with weight > 0 \
         (config validation guarantees a positive weight; availability rules \
         must keep weighted-random always available)"
    );

    let total: f64 = available.iter().map(|(_, w)| w).sum();
    let norm: Vec<(GeneratorKind, f64)> = available
        .iter()
        .map(|&(kind, w)| (kind, w / total))
        .collect();

    // 1. Stratified floors.
    let mut counts: Vec<usize> = norm
        .iter()
        .map(|&(_, w)| (w * k as f64).floor() as usize)
        .collect();
    let assigned: usize = counts.iter().sum();

    // 2. Remainder by sampling WITHOUT replacement over fractional parts.
    let mut fractions: Vec<f64> = norm
        .iter()
        .zip(&counts)
        .map(|(&(_, w), &c)| w * k as f64 - c as f64)
        .collect();
    for _ in assigned..k {
        let fsum: f64 = fractions.iter().sum();
        let idx = if fsum <= 0.0 {
            // All fractional mass consumed (can happen when remainder count
            // exceeds nonzero fractions): fall back to the largest weight.
            // Still consumes one draw so the stream stays aligned.
            let _ = next_unit_f64(rng);
            norm.iter()
                .enumerate()
                .max_by(|a, b| a.1 .1.partial_cmp(&b.1 .1).expect("weights are finite"))
                .map(|(i, _)| i)
                .unwrap_or(0)
        } else {
            let u = next_unit_f64(rng) * fsum;
            let mut acc = 0.0;
            let mut pick = fractions.len() - 1;
            for (i, f) in fractions.iter().enumerate() {
                acc += f;
                if u < acc {
                    pick = i;
                    break;
                }
            }
            pick
        };
        counts[idx] += 1;
        fractions[idx] = 0.0;
    }

    // 3. Materialize in generator order, then a seeded Fisher–Yates
    //    permutation so generator identity isn't correlated with slot index.
    let mut slots: Vec<GeneratorKind> = Vec::with_capacity(k);
    for (&(kind, _), &count) in norm.iter().zip(&counts) {
        slots.extend(std::iter::repeat(kind).take(count));
    }
    debug_assert_eq!(slots.len(), k);
    for i in (1..slots.len()).rev() {
        let u = next_unit_f64(rng);
        let j = ((u * (i + 1) as f64) as usize).min(i);
        slots.swap(i, j);
    }

    (slots, degraded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use synth_core::rng::{fanout_root, stream};

    fn weights(wr: f64, mac: f64, mu: f64) -> Vec<GeneratorWeight> {
        vec![
            GeneratorWeight {
                kind: GeneratorKind::WeightedRandom,
                weight: wr,
                unavailable: None,
            },
            GeneratorWeight {
                kind: GeneratorKind::Macro,
                weight: mac,
                unavailable: None,
            },
            GeneratorWeight {
                kind: GeneratorKind::Mutation,
                weight: mu,
                unavailable: None,
            },
        ]
    }

    /// M1 accept: for K ∈ {8, 32, 64} and the default mix, realized
    /// per-generator counts equal the stratified floors ± 1.
    #[test]
    fn stratified_counts_within_one_of_floors() {
        let root = fanout_root(42, "mixer-test");
        for k in [8usize, 32, 64] {
            for seed_label in ["a", "b", "c", "d", "e"] {
                let mut rng = stream(&root, &format!("mix/{k}/{seed_label}"));
                let (slots, degraded) = allocate_slots(k, &weights(0.45, 0.35, 0.20), &mut rng);
                assert!(degraded.is_empty());
                assert_eq!(slots.len(), k);
                for (kind, w) in [
                    (GeneratorKind::WeightedRandom, 0.45),
                    (GeneratorKind::Macro, 0.35),
                    (GeneratorKind::Mutation, 0.20),
                ] {
                    let count = slots.iter().filter(|&&s| s == kind).count();
                    let floor = (w * k as f64).floor() as usize;
                    assert!(
                        count == floor || count == floor + 1,
                        "K={k} {kind:?}: count {count} not in {{{floor}, {}}}",
                        floor + 1
                    );
                }
            }
        }
    }

    #[test]
    fn unavailable_generator_reallocated_and_reported() {
        let root = fanout_root(43, "mixer-test");
        let mut rng = stream(&root, "mix");
        let mut w = weights(0.5, 0.0, 0.5);
        w[2].unavailable = Some("no_parent_burst".to_owned());
        let (slots, degraded) = allocate_slots(16, &w, &mut rng);
        assert_eq!(slots.len(), 16);
        assert!(slots.iter().all(|&s| s == GeneratorKind::WeightedRandom));
        assert_eq!(degraded.len(), 1);
        assert_eq!(degraded[0].generator, GeneratorKind::Mutation);
        assert_eq!(degraded[0].reason, "no_parent_burst");
    }

    #[test]
    fn allocation_is_deterministic() {
        let root = fanout_root(44, "mixer-test");
        let run = || {
            let mut rng = stream(&root, "mix");
            allocate_slots(32, &weights(0.45, 0.35, 0.20), &mut rng).0
        };
        assert_eq!(run(), run());
    }

    /// Exact-split sanity: {wr: 0.5, macro: 0.5}, K=32 ⇒ 16/16 (M2 accept
    /// uses this; the mixer half is provable now).
    #[test]
    fn even_split_is_exact() {
        let root = fanout_root(45, "mixer-test");
        let mut rng = stream(&root, "mix");
        let (slots, _) = allocate_slots(32, &weights(0.5, 0.5, 0.0), &mut rng);
        let wr = slots
            .iter()
            .filter(|&&s| s == GeneratorKind::WeightedRandom)
            .count();
        assert_eq!(wr, 16);
    }
}
