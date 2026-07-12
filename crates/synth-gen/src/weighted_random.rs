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

use rand_chacha::ChaCha8Rng;
use synth_core::config::{ExperimentConfig, LengthDistribution};
use synth_core::fmath;
use synth_core::rng::{next_unit_f64, stream};
use synth_core::types::{Burst, PadBurst, PadSegment};

use crate::context::{effective_button_priors, effective_direction_priors, GenContext};

/// Inverse-CDF geometric draw on {1,2,…} with per-trial success `p`:
/// `d = max(1, ceil(ln(1−u)/ln(1−p)))`; `p = 1 ⇒ d = 1`.
fn geometric(rng: &mut ChaCha8Rng, p: f64) -> u64 {
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
struct DirSegment {
    mask: u16,
    frames: u64,
}

/// Direction name → bitmask (NEUTRAL = 0). Names are validated config.
fn dir_mask(cfg: &ExperimentConfig, name: &str) -> u16 {
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
/// back to the last entry on accumulated rounding).
fn categorical(entries: &[(String, f64)], u: f64) -> &str {
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
    let dir_cfg = &cfg.weighted_random.direction;
    let priors = effective_direction_priors(cfg, ctx);
    let r = 1.0 / dir_cfg.mean_hold_frames.max(1.0);
    let dir_group_mask = cfg
        .button_alphabet
        .mask(&cfg.button_alphabet.directions.group)
        .unwrap_or(0);

    // Initial direction: continue history's direction bits when configured
    // and present (temporal coherence); otherwise one categorical draw.
    let mut current: u16 = match ctx
        .last_frame_mask()
        .filter(|_| cfg.weighted_random.start_from_history.0)
    {
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

    // Materialize change points and compose the segment list.
    let mut cuts: Vec<u64> = vec![0, target];
    let mut acc = 0u64;
    for seg in &dir_segments {
        acc += seg.frames;
        if acc < target {
            cuts.push(acc);
        }
    }
    for (_, intervals) in &button_intervals {
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
        // Direction mask at `start`.
        let mut acc = 0u64;
        for seg in &dir_segments {
            let seg_end = acc + seg.frames;
            if start < seg_end {
                mask |= seg.mask;
                break;
            }
            acc = seg_end;
        }
        for (btn_mask, intervals) in &button_intervals {
            if intervals.iter().any(|&(s, e)| s <= start && start < e) {
                mask |= btn_mask;
            }
        }
        segments.push(PadSegment {
            buttons: mask,
            hold_frames: u32::try_from(end - start).unwrap_or(u32::MAX),
        });
    }

    Burst::Pad(PadBurst { segments })
}
