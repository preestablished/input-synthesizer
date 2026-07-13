//! Generator 1 — weighted random with temporal coherence
//! (ARCHITECTURE.md §4).
//!
//! Draw-order contract (part of the format; changing any order bumps
//! `SYNTH_VERSION`):
//! - stream `slot/{s}/len`: `u1, u2` (Box–Muller lognormal), unless the
//!   distribution is `fixed` (no draws) or `uniform` (one draw).
//! - stream `slot/{s}/wr/dir`: initial pick `u` (skipped when continuing
//!   history); then per segment: duration `u`; stickiness `u`; resample `u`
//!   (only when stickiness said resample); diagonal gate `u` + diagonal pick
//!   `u` (only when the segment is non-neutral, diagonals allowed, and a
//!   complementary direction exists).
//! - stream `slot/{s}/wr/btn/{bit}`: initial state `u` (skipped when
//!   continuing history); then alternating geometric gap/hold draws, one `u`
//!   each, starting with a gap when OFF and a hold when ON.
//!
//! [`generate_single_stream`] (used by the macro generator's tail padding,
//! `synth_gen::macros`) draws the *same* direction-then-buttons sequence
//! from one caller-supplied stream instead of per-label streams; it must
//! never change `generate`'s own per-label draw order (verified by
//! `golden_seed.rs`, which this refactor keeps byte-identical).

use rand_chacha::ChaCha8Rng;
use synth_core::config::{ExperimentConfig, LengthDistribution};
use synth_core::fmath;
use synth_core::rng::{next_unit_f64, stream};
use synth_core::types::{Burst, PadBurst, PadSegment};

use crate::context::{effective_button_priors, effective_direction_priors, GenContext};

/// Inverse-CDF geometric draw on {1,2,…} with per-trial success `p`:
/// `d = max(1, ceil(ln(1−u)/ln(1−p)))`; `p = 1 ⇒ d = 1`. `pub(crate)`: reused
/// by `synth_gen::mutation` (`extend`'s and `flip_button`'s repeat counts,
/// ARCHITECTURE.md §5.2) so both generators draw from the identical formula.
pub(crate) fn geometric(rng: &mut ChaCha8Rng, p: f64) -> u64 {
    debug_assert!(p > 0.0 && p <= 1.0);
    if p >= 1.0 {
        return 1;
    }
    let u = next_unit_f64(rng);
    let d = fmath::ln(1.0 - u) / fmath::ln(1.0 - p);
    let d = d.ceil();
    if d < 1.0 {
        1
    } else if d >= 1e18 {
        1_000_000_000_000_000_000
    } else {
        d as u64
    }
}

/// Burst target length (ARCHITECTURE.md §4.5), stream `slot/{s}/len`.
pub fn draw_length(cfg: &ExperimentConfig, length_hint: u32, rng: &mut ChaCha8Rng) -> u32 {
    let bl = &cfg.burst_len;
    let location = if length_hint > 0 {
        f64::from(length_hint)
    } else {
        f64::from(bl.mean_frames)
    };
    let l = match bl.distribution {
        LengthDistribution::Fixed => location,
        LengthDistribution::Uniform => {
            let u = next_unit_f64(rng);
            f64::from(bl.min_frames) + u * f64::from(bl.max_frames - bl.min_frames)
        }
        LengthDistribution::Lognormal => {
            // Box–Muller: z = sqrt(−2 ln u1) · cos(2π u2); draws u1 then u2.
            let u1 = next_unit_f64(rng);
            let u2 = next_unit_f64(rng);
            let z = fmath::sqrt(-2.0 * fmath::ln(1.0 - u1))
                * fmath::cos(2.0 * std::f64::consts::PI * u2);
            fmath::exp(fmath::ln(location) + bl.sigma * z)
        }
    };
    let l = l.round_ties_even();
    let l = if l < f64::from(bl.min_frames) {
        bl.min_frames
    } else if l > f64::from(bl.max_frames) {
        bl.max_frames
    } else {
        l as u32
    };
    l.max(1)
}

/// One direction segment: mask + duration.
#[cfg_attr(test, derive(Debug))]
struct DirSegment {
    mask: u16,
    frames: u64,
}

/// Direction name → bitmask (NEUTRAL = 0). Names are validated config.
/// `pub(crate)`: reused by `synth_gen::mutation`'s `flip_button` direction
/// re-roll (ARCHITECTURE.md §5.2).
pub(crate) fn dir_mask(cfg: &ExperimentConfig, name: &str) -> u16 {
    if name == "NEUTRAL" {
        0
    } else {
        cfg.button_alphabet
            .bit(name)
            .map(|b| 1u16 << b)
            .unwrap_or(0)
    }
}

/// Pick from a categorical by normalized weights (assumes sum ≈ 1; falls
/// back to the last entry on accumulated rounding). `pub(crate)`: reused by
/// `synth_gen::mutation` for op selection and `flip_button`'s direction
/// re-roll (ARCHITECTURE.md §5.2).
pub(crate) fn categorical(entries: &[(String, f64)], u: f64) -> &str {
    let mut acc = 0.0;
    for (name, w) in entries {
        acc += w;
        if u < acc {
            return name;
        }
    }
    entries.last().map(|(n, _)| n.as_str()).unwrap_or("NEUTRAL")
}

/// Directions legal to combine with `mask` for a diagonal: direction-group
/// bits not in `mask` and not exclusive with any bit of `mask`.
fn diagonal_candidates(cfg: &ExperimentConfig, mask: u16) -> Vec<u16> {
    let mut out = Vec::new();
    for name in &cfg.button_alphabet.directions.group {
        let Some(bit) = cfg.button_alphabet.bit(name) else {
            continue;
        };
        let candidate = 1u16 << bit;
        if candidate & mask != 0 {
            continue;
        }
        let combined = mask | candidate;
        let violates = cfg.button_alphabet.exclusive_groups.iter().any(|g| {
            match cfg.button_alphabet.mask(g) {
                Some(gm) => (combined & gm).count_ones() >= 2,
                None => false,
            }
        });
        if !violates {
            out.push(candidate);
        }
    }
    out
}

/// Sticky semi-Markov categorical direction process (§4.3), stream
/// `slot/{s}/wr/dir`. Generates segments covering ≥ `target` frames.
fn direction_track(
    cfg: &ExperimentConfig,
    ctx: &GenContext,
    target: u64,
    rng: &mut ChaCha8Rng,
) -> Vec<DirSegment> {
    let initial = ctx
        .last_frame_mask()
        .filter(|_| cfg.weighted_random.start_from_history.0);
    direction_track_with_initial(cfg, ctx, target, initial, rng)
}

/// [`direction_track`], parameterized by an explicit initial-direction-mask
/// override instead of always deriving it from `ctx`/`start_from_history`.
/// `direction_track` itself passes exactly the same expression it always
/// computed inline, so its behavior — and every M1/M2 golden depending on
/// it — is unchanged; [`generate_single_stream_from_mask`] (`synth_gen::
/// mutation`'s `extend` operator, ARCHITECTURE.md §5.2) is the only caller
/// that passes `Some(..)` unconditionally.
fn direction_track_with_initial(
    cfg: &ExperimentConfig,
    ctx: &GenContext,
    target: u64,
    initial: Option<u16>,
    rng: &mut ChaCha8Rng,
) -> Vec<DirSegment> {
    let dir_cfg = &cfg.weighted_random.direction;
    let priors = effective_direction_priors(cfg, ctx);
    let r = 1.0 / dir_cfg.mean_hold_frames.max(1.0);
    let dir_group_mask = cfg
        .button_alphabet
        .mask(&cfg.button_alphabet.directions.group)
        .unwrap_or(0);

    // Initial direction: continue the supplied mask when present (temporal
    // coherence, or the caller's explicit override); otherwise one
    // categorical draw.
    let mut current: u16 = match initial {
        Some(mask) => mask & dir_group_mask,
        None => dir_mask(cfg, categorical(&priors, next_unit_f64(rng))),
    };

    let mut segments = Vec::new();
    let mut covered: u64 = 0;
    while covered < target {
        let frames = geometric(rng, r);
        // Diagonal enrichment (§5.1 config `diagonal_factor`): applies to
        // the segment being emitted, non-neutral only.
        let mut mask = current;
        if mask != 0
            && cfg.button_alphabet.directions.allow_diagonals
            && dir_cfg.diagonal_factor > 0.0
        {
            let gate = next_unit_f64(rng);
            if gate < dir_cfg.diagonal_factor {
                let candidates = diagonal_candidates(cfg, mask);
                if !candidates.is_empty() {
                    let pick = next_unit_f64(rng);
                    let idx = ((pick * candidates.len() as f64) as usize).min(candidates.len() - 1);
                    mask |= candidates[idx];
                }
            }
        }
        segments.push(DirSegment { mask, frames });
        covered += frames;
        if covered >= target {
            break;
        }
        // Boundary: keep with stickiness κ, else resample (self-transitions
        // allowed).
        let keep = next_unit_f64(rng);
        if keep >= dir_cfg.stickiness {
            current = dir_mask(cfg, categorical(&priors, next_unit_f64(rng)));
        }
    }
    segments
}

/// One button's ON intervals within `[0, target)`, stream
/// `slot/{s}/wr/btn/{bit}`.
fn button_track(
    duty: f64,
    mu: f64,
    initial_on: Option<bool>,
    target: u64,
    rng: &mut ChaCha8Rng,
) -> Vec<(u64, u64)> {
    if duty <= 0.0 {
        return Vec::new();
    }
    let r = 1.0 / mu.max(1.0);
    let a = (duty / (mu.max(1.0) * (1.0 - duty))).min(1.0);
    if a <= 0.0 {
        // Fix #10: for a subnormal `duty`, `a` can underflow to exactly 0.0
        // (rather than a tiny positive value) — feeding that into
        // `geometric` as its per-trial success probability would either
        // trip its `p > 0.0` debug_assert or, in release, silently flip
        // semantics to "near-always-on" (since `geometric` treats `p <= 0`
        // pathologically). Treat an underflowed rate as a button that is
        // never pressed: a zero-rate chain draws nothing, consuming NO
        // `rng` draws — the same convention as `duty <= 0.0` above.
        return Vec::new();
    }
    let mut on = match initial_on {
        Some(state) => state,
        None => next_unit_f64(rng) < duty,
    };
    let mut intervals = Vec::new();
    let mut t: u64 = 0;
    while t < target {
        let d = geometric(rng, if on { r } else { a });
        if on {
            intervals.push((t, (t + d).min(target)));
        }
        t += d;
        on = !on;
    }
    intervals
}

/// Generate one weighted-random burst for slot `s` (ARCHITECTURE.md §4).
/// Caller supplies the fan-out root; this function derives its own labeled
/// streams and returns an un-legalized burst (the pipeline legalizes).
pub fn generate(
    cfg: &ExperimentConfig,
    ctx: &GenContext,
    root: &[u8; 32],
    slot: usize,
    length_hint: u32,
) -> Burst {
    let target = {
        let mut len_rng = stream(root, &format!("slot/{slot}/len"));
        u64::from(draw_length(cfg, length_hint, &mut len_rng))
    };

    let mut dir_rng = stream(root, &format!("slot/{slot}/wr/dir"));
    let dir_segments = direction_track(cfg, ctx, target, &mut dir_rng);

    let history_mask = ctx
        .last_frame_mask()
        .filter(|_| cfg.weighted_random.start_from_history.0);
    let priors = effective_button_priors(cfg, ctx);
    let dir_group_mask = cfg
        .button_alphabet
        .mask(&cfg.button_alphabet.directions.group)
        .unwrap_or(0);

    // Per-button ON intervals (direction-group bits are handled by the
    // direction process, not as independent chains).
    let mut button_intervals: Vec<(u16, Vec<(u64, u64)>)> = Vec::new();
    for &(bit, duty, mu) in &priors {
        let mask = 1u16 << bit;
        if mask & dir_group_mask != 0 {
            continue;
        }
        let initial_on = history_mask.map(|h| h & mask != 0);
        let mut btn_rng = stream(root, &format!("slot/{slot}/wr/btn/{bit}"));
        let intervals = button_track(duty, mu, initial_on, target, &mut btn_rng);
        if !intervals.is_empty() {
            button_intervals.push((mask, intervals));
        }
    }

    let segments = compose_segments(target, &dir_segments, &button_intervals);
    Burst::Pad(PadBurst { segments })
}

/// Tail-padding variant for the macro generator (ARCHITECTURE.md §5.1 step
/// 5, `synth_gen::macros` module doc, stream `slot/{s}/macro/tail`): draws
/// everything from the single caller-supplied stream instead of per-label
/// streams — direction track first, then each non-direction button in
/// alphabet declaration order, sequentially on the same `rng`. Composition
/// reuses [`compose_segments`], the same helper `generate` uses, so the only
/// difference from `generate`'s per-label streams is where the draws come
/// from, never the draw order or composition logic.
pub fn generate_single_stream(
    cfg: &ExperimentConfig,
    ctx: &GenContext,
    target: u64,
    rng: &mut ChaCha8Rng,
) -> Vec<PadSegment> {
    let dir_segments = direction_track(cfg, ctx, target, rng);

    let history_mask = ctx
        .last_frame_mask()
        .filter(|_| cfg.weighted_random.start_from_history.0);
    let priors = effective_button_priors(cfg, ctx);
    let dir_group_mask = cfg
        .button_alphabet
        .mask(&cfg.button_alphabet.directions.group)
        .unwrap_or(0);

    let mut button_intervals: Vec<(u16, Vec<(u64, u64)>)> = Vec::new();
    for &(bit, duty, mu) in &priors {
        let mask = 1u16 << bit;
        if mask & dir_group_mask != 0 {
            continue;
        }
        let initial_on = history_mask.map(|h| h & mask != 0);
        let intervals = button_track(duty, mu, initial_on, target, rng);
        if !intervals.is_empty() {
            button_intervals.push((mask, intervals));
        }
    }

    compose_segments(target, &dir_segments, &button_intervals)
}

/// Variant of [`generate_single_stream`] that forces the initial state (both
/// the direction track and every non-direction button chain) to `initial_mask`
/// instead of deriving it from `ctx`/`start_from_history` — used by
/// `synth_gen::mutation`'s `extend` operator (ARCHITECTURE.md §5.2) to
/// condition the appended segments on the burst's own final mask rather than
/// request-level history. A new function rather than a new parameter on
/// [`generate_single_stream`]/[`generate`]: those keep their existing
/// signatures and draw order byte-for-byte (M1/M2 goldens depend on it);
/// this one never runs on that path.
pub fn generate_single_stream_from_mask(
    cfg: &ExperimentConfig,
    ctx: &GenContext,
    target: u64,
    initial_mask: u16,
    rng: &mut ChaCha8Rng,
) -> Vec<PadSegment> {
    let dir_segments = direction_track_with_initial(cfg, ctx, target, Some(initial_mask), rng);

    let priors = effective_button_priors(cfg, ctx);
    let dir_group_mask = cfg
        .button_alphabet
        .mask(&cfg.button_alphabet.directions.group)
        .unwrap_or(0);

    let mut button_intervals: Vec<(u16, Vec<(u64, u64)>)> = Vec::new();
    for &(bit, duty, mu) in &priors {
        let mask = 1u16 << bit;
        if mask & dir_group_mask != 0 {
            continue;
        }
        let initial_on = Some(initial_mask & mask != 0);
        let intervals = button_track(duty, mu, initial_on, target, rng);
        if !intervals.is_empty() {
            button_intervals.push((mask, intervals));
        }
    }

    compose_segments(target, &dir_segments, &button_intervals)
}

/// Materialize change points across a direction track and per-button
/// intervals over `[0, target)` into a run-length segment list. Draws no
/// randomness itself — shared, order-preserving composition step for both
/// `generate` and `generate_single_stream`.
fn compose_segments(
    target: u64,
    dir_segments: &[DirSegment],
    button_intervals: &[(u16, Vec<(u64, u64)>)],
) -> Vec<PadSegment> {
    let mut cuts: Vec<u64> = vec![0, target];
    let mut acc = 0u64;
    for seg in dir_segments {
        acc += seg.frames;
        if acc < target {
            cuts.push(acc);
        }
    }
    for (_, intervals) in button_intervals {
        for &(start, end) in intervals {
            if start < target {
                cuts.push(start);
            }
            if end < target {
                cuts.push(end);
            }
        }
    }
    cuts.sort_unstable();
    cuts.dedup();

    // Merge-style single pass. Cut starts are strictly increasing, direction
    // segments are consecutive in time, and each button's intervals are
    // sorted and non-overlapping (button_track emits them forward in time),
    // so one monotone cursor per track replaces the per-cut rescans that
    // made this O(cuts x segments) — quadratic at long-burst configs (a
    // legal k=256 x 216000-frame request measured ~80 s; round-7 review).
    // Lookup semantics are identical: direction mask = first segment whose
    // cumulative end exceeds `start`; button held iff some interval
    // satisfies s <= start < e.
    let mut segments = Vec::with_capacity(cuts.len());
    let mut dir_idx = 0usize;
    let mut dir_cum_end = dir_segments.first().map(|s| s.frames).unwrap_or(0);
    let mut btn_cursors = vec![0usize; button_intervals.len()];
    for window in cuts.windows(2) {
        let (start, end) = (window[0], window[1]);
        if end <= start {
            continue;
        }
        let mut mask = 0u16;
        while dir_idx < dir_segments.len() && dir_cum_end <= start {
            dir_idx += 1;
            if let Some(seg) = dir_segments.get(dir_idx) {
                dir_cum_end += seg.frames;
            }
        }
        if let Some(seg) = dir_segments.get(dir_idx) {
            mask |= seg.mask;
        }
        for (cursor, (btn_mask, intervals)) in btn_cursors.iter_mut().zip(button_intervals) {
            while *cursor < intervals.len() && intervals[*cursor].1 <= start {
                *cursor += 1;
            }
            if let Some(&(s, _)) = intervals.get(*cursor) {
                if s <= start {
                    mask |= btn_mask;
                }
            }
        }
        segments.push(PadSegment {
            buttons: mask,
            hold_frames: u32::try_from(end - start).unwrap_or(u32::MAX),
        });
    }
    segments
}

#[cfg(test)]
mod tests {
    use super::*;
    use synth_core::rng::{fanout_root, stream};

    /// Executable form of the CLAUDE.md "categorical last-entry trap"
    /// guardrail: `categorical`'s cumulative scan falls back to
    /// `entries.last()` whenever `u` lands at or past the cumulative
    /// weight sum (the float-rounding guard) — so *appending* a
    /// zero-weight key changes that fallback (the trap), while
    /// *inserting* it before the final entry does not (the documented
    /// mitigation).
    #[test]
    fn categorical_last_entry_fallback_semantics() {
        let base: Vec<(String, f64)> = vec![
            ("alpha".to_string(), 0.1),
            ("beta".to_string(), 0.2),
            ("gamma".to_string(), 0.3),
        ];
        let sum: f64 = base.iter().map(|(_, w)| w).sum();
        assert!(sum < 1.0, "fixture weights must sum under 1.0, got {sum}");

        // u below the sum: an ordinary in-range draw, same answer
        // regardless of where (or whether) a zero-weight key sits.
        let u_below = 0.05;
        assert!(u_below < sum);

        // u above the sum (but < 1.0): only reachable via the fallback.
        let u_above = 0.8;
        assert!(u_above > sum && u_above < 1.0);

        // (a) No zero-weight key: fallback fires, returns the true last
        // entry — proves the fallback path is reachable at all.
        let no_zero = base.clone();
        assert_eq!(categorical(&no_zero, u_above), "gamma");

        // (b) THE TRAP: append a zero-weight key at the end. `0.0` is
        // additively neutral (acc unchanged), so `u_above` still exceeds
        // every cumulative sum and the loop still falls through — but
        // `entries.last()` is now the zero-weight key instead of "gamma".
        let mut appended_last = base.clone();
        appended_last.push(("zeta".to_string(), 0.0));
        assert_eq!(categorical(&appended_last, u_above), "zeta");

        // (c) THE MITIGATION: insert the same zero-weight key before the
        // final entry instead of after it. The fallback still fires (same
        // u_above, same reachable-fallback reasoning as (a)), but
        // `entries.last()` is once again "gamma" — inserting before the
        // final entry keeps the fallback semantics bit-neutral.
        let mut inserted_before_last = base.clone();
        inserted_before_last.insert(2, ("zeta".to_string(), 0.0));
        assert_eq!(categorical(&inserted_before_last, u_above), "gamma");

        // Normal-path neutrality: for a u below the sum, all three
        // configurations agree (the zero-weight key never changes a
        // draw that lands inside a real entry's range).
        assert_eq!(categorical(&no_zero, u_below), "alpha");
        assert_eq!(categorical(&appended_last, u_below), "alpha");
        assert_eq!(categorical(&inserted_before_last, u_below), "alpha");
    }

    /// Fix #10: a subnormal `duty` (smallest positive `f64`) underflows `a`
    /// to exactly 0.0; `button_track` must treat that as "never pressed"
    /// (empty intervals, no draws consumed) rather than panicking on
    /// `geometric`'s `p > 0.0` debug_assert or flipping to near-always-on.
    #[test]
    fn denormal_duty_never_presses_and_draws_nothing() {
        let root = fanout_root(1, "denormal-duty-test");
        let mut rng = stream(&root, "test/denormal-duty");
        let intervals = button_track(5e-324, 2.0, None, 1000, &mut rng);
        assert!(
            intervals.is_empty(),
            "an underflowed rate must never press the button"
        );
    }

    // -------------------------------------------------------------------
    // Round-8 differential check: the pre-498bc6c per-cut rescan
    // reimplemented verbatim below, proptested against the live
    // `compose_segments` monotone-cursor rewrite. Goldens prove
    // byte-identical output at the shapes they sample; this proves it
    // across arbitrary direction/button-interval/target combinations,
    // including the edge cases the rewrite's cursors depend on (dir
    // coverage short of or past `target`, touching/adjacent intervals,
    // interval boundaries landing exactly on `start`).
    // -------------------------------------------------------------------
    mod compose_segments_differential {
        use super::*;
        use proptest::prelude::*;

        /// One button mask paired with its ON intervals — same shape
        /// `compose_segments` takes, named here only to keep the proptest
        /// strategy signatures below under clippy's type-complexity limit.
        type ButtonIntervals = Vec<(u16, Vec<(u64, u64)>)>;

        /// Verbatim reimplementation of the removed per-cut linear-rescan
        /// body (see `git show 498bc6c` on this file) — not called by any
        /// non-test code, kept only as the reference oracle.
        fn compose_segments_old(
            target: u64,
            dir_segments: &[DirSegment],
            button_intervals: &[(u16, Vec<(u64, u64)>)],
        ) -> Vec<PadSegment> {
            let mut cuts: Vec<u64> = vec![0, target];
            let mut acc = 0u64;
            for seg in dir_segments {
                acc += seg.frames;
                if acc < target {
                    cuts.push(acc);
                }
            }
            for (_, intervals) in button_intervals {
                for &(start, end) in intervals {
                    if start < target {
                        cuts.push(start);
                    }
                    if end < target {
                        cuts.push(end);
                    }
                }
            }
            cuts.sort_unstable();
            cuts.dedup();

            let mut segments = Vec::with_capacity(cuts.len());
            for window in cuts.windows(2) {
                let (start, end) = (window[0], window[1]);
                if end <= start {
                    continue;
                }
                let mut mask = 0u16;
                let mut acc = 0u64;
                for seg in dir_segments {
                    let seg_end = acc + seg.frames;
                    if start < seg_end {
                        mask |= seg.mask;
                        break;
                    }
                    acc = seg_end;
                }
                for (btn_mask, intervals) in button_intervals {
                    if intervals.iter().any(|&(s, e)| s <= start && start < e) {
                        mask |= btn_mask;
                    }
                }
                segments.push(PadSegment {
                    buttons: mask,
                    hold_frames: u32::try_from(end - start).unwrap_or(u32::MAX),
                });
            }
            segments
        }

        fn dir_segments_strategy() -> impl Strategy<Value = Vec<DirSegment>> {
            prop::collection::vec((any::<u16>(), 1u64..5000), 0..50).prop_map(|v| {
                v.into_iter()
                    .map(|(mask, frames)| DirSegment { mask, frames })
                    .collect()
            })
        }

        /// Sorted, non-overlapping half-open intervals for one button,
        /// generated by walking a cursor forward with (gap, len) steps —
        /// `gap` may be 0 (touching the previous interval's end exactly,
        /// which only actually arises across different buttons/direction
        /// boundaries at runtime, but is exercised here regardless since
        /// `compose_segments` makes no same-button assumption).
        fn button_intervals_strategy(target: u64) -> impl Strategy<Value = ButtonIntervals> {
            let one_button =
                prop::collection::vec((0u64..50, 0u64..100), 0usize..100).prop_map(move |steps| {
                    let mut cursor = 0u64;
                    let mut out = Vec::new();
                    for (gap, len) in steps {
                        if cursor >= target {
                            break;
                        }
                        cursor = (cursor + gap).min(target);
                        if len == 0 || cursor >= target {
                            continue;
                        }
                        let end = (cursor + len).min(target);
                        if end > cursor {
                            out.push((cursor, end));
                        }
                        cursor = end;
                    }
                    out
                });
            prop::collection::vec((any::<u16>(), one_button), 0usize..12)
        }

        fn full_case_strategy() -> impl Strategy<Value = (u64, Vec<DirSegment>, ButtonIntervals)> {
            (1u64..20000).prop_flat_map(|target| {
                (
                    Just(target),
                    dir_segments_strategy(),
                    button_intervals_strategy(target),
                )
            })
        }

        fn assert_matches(
            target: u64,
            dir_segments: &[DirSegment],
            button_intervals: &[(u16, Vec<(u64, u64)>)],
        ) {
            let old = compose_segments_old(target, dir_segments, button_intervals);
            let new = compose_segments(target, dir_segments, button_intervals);
            assert_eq!(old, new, "target={target}");
        }

        proptest! {
            #[test]
            fn matches_old_per_cut_rescan(
                (target, dir_segments, button_intervals) in full_case_strategy()
            ) {
                assert_matches(target, &dir_segments, &button_intervals);
            }
        }

        #[test]
        fn dir_total_short_of_target_leaves_trailing_mask_zero() {
            // dir_segments cover only 300 of 1000 frames: old code's scan
            // finds nothing past frame 300 (mask stays 0); new code's
            // `dir_idx` runs past `dir_segments.len()` and `.get` returns
            // `None` (mask also stays 0) — same externally-observed result.
            assert_matches(
                1000,
                &[
                    DirSegment {
                        mask: 0b1,
                        frames: 100,
                    },
                    DirSegment {
                        mask: 0b10,
                        frames: 200,
                    },
                ],
                &[(0b100, vec![(500, 900)])],
            );
        }

        #[test]
        fn dir_total_exceeds_target() {
            assert_matches(
                100,
                &[
                    DirSegment {
                        mask: 0b1,
                        frames: 50,
                    },
                    DirSegment {
                        mask: 0b10,
                        frames: 100,
                    },
                ],
                &[],
            );
        }

        #[test]
        fn empty_dir_segments_and_empty_intervals() {
            assert_matches(100, &[], &[]);
        }

        #[test]
        fn interval_start_lands_exactly_on_a_cut_boundary() {
            assert_matches(
                100,
                &[DirSegment {
                    mask: 0b1,
                    frames: 100,
                }],
                &[(0b10, vec![(20, 40)])],
            );
        }

        #[test]
        fn adjacent_intervals_touch_at_shared_boundary() {
            assert_matches(100, &[], &[(0b1, vec![(0, 20), (20, 40), (40, 60)])]);
        }

        #[test]
        fn interval_ends_exactly_at_target() {
            assert_matches(100, &[], &[(0b1, vec![(50, 100)])]);
        }
    }
}
