//! Generator 3 — mutation (ARCHITECTURE.md §5.2).
//!
//! Draw-order contract (part of the format; changing any order bumps
//! `SYNTH_VERSION` and invalidates `testdata/golden/m3/*` — mirrors the
//! conventions in `weighted_random.rs`'s and `macros.rs`'s module docs):
//!
//! - stream `slot/{s}/len`: **not used** by mutation — length comes from the
//!   base burst plus whatever the applied ops do to it; only the post-op
//!   clamp (config `[min_frames, max_frames]`) bounds it, and that is not a
//!   stream draw.
//! - stream `slot/{s}/mut/ops`, in order:
//!   1. IF `sibling_bursts` is non-empty: one `u` — the donor-bias gate
//!      (`u < donor_bias` ⇒ base comes from a sibling). Drawn even when
//!      `parent_burst` is absent (base is then unconditionally a sibling, but
//!      the draw still happens so the stream stays aligned regardless of
//!      which inputs are present).
//!   2. IF the base is coming from a sibling: one `u` — the sibling
//!      categorical, weights `max(score_delta, ε)` with `ε = 1e-6`, in
//!      `sibling_bursts` order.
//!   3. `cfg.mutation.ops_binomial.n` Bernoulli draws (one `u` each, success
//!      iff `u < ops_binomial.p`), summed to `B`; `n_ops = 1 + min(B, 3)`.
//!   4. `n_ops` categorical draws (one `u` each) over `cfg.mutation.op_probs`
//!      in `IndexMap` (config document) order, selecting each applied op's
//!      name.
//! - stream `slot/{s}/mut/op/{i}` (`i` = 0-based application index): all
//!   sampled arguments of the i-th applied op, in that op's own internal
//!   order (documented per-op below). A `splice` that falls back to `extend`
//!   (no siblings available) draws exactly `extend`'s sequence on this same
//!   per-index stream — the fallback consumes the slot, it does not skip it.
//! - stream `slot/{s}/mut/retry`: the forced `perturb_timing` retry pass
//!   (ARCHITECTURE.md §7.2 addendum, `06-m3-mutation-stretch.md`), used at
//!   most once, only when the legalized mutant's `burst_hash` equals the
//!   base's. Its own label — never a replay of `mut/op/{i}` — because
//!   re-deriving an already-used label would replay the identical sequence
//!   and silently reproduce the identical (non-)mutant.
//!
//! Per-operator argument draws (each op's own stream, `slot/{s}/mut/op/{i}`
//! or `slot/{s}/mut/retry`):
//! - `perturb_timing`: for each segment index `0..len`, one gate `u` (fires
//!   iff `u < 0.5`); for each fired segment, two more draws `u1, u2`
//!   (Box–Muller, same formula as `weighted_random::draw_length`'s lognormal
//!   branch: `z = sqrt(-2 ln(1-u1)) * cos(2π u2)`), then
//!   `d' = max(1, round_ties_even(d * exp(σ_t · z)))`. Args: `fired_segments`
//!   (comma-joined ascending indices, `""` if none fired), `z_bits`
//!   (comma-joined lowercase hex of each fired `z`'s `f64::to_bits()`, same
//!   order as `fired_segments`).
//! - `extend`: one `u` for `m = 1 + Geometric(0.25)` (`weighted_random::
//!   geometric`, which itself draws exactly one `u`); then
//!   `weighted_random::generate_single_stream_from_mask` conditioned on the
//!   current burst's final segment mask, sampled over a fixed large frame
//!   budget ([`EXTEND_STREAM_FRAMES`]) so at least `m` segments are almost
//!   always available; the result is cut to the first `m` segments (count,
//!   not frames — clamped to however many were actually sampled, an
//!   astronomically unlikely shortfall documented at [`EXTEND_STREAM_FRAMES`]).
//!   Args: `m` (the target segment count), `appended` (semicolon-joined
//!   `mask:frames` pairs, decimal) — the segment *content* is recorded so
//!   replay (`apply_ops`) never needs to re-invoke the sampler.
//! - `flip_button`: one `u` for `m = 1 + Geometric(0.5)` repeats
//!   (`weighted_random::geometric`); per repeat: one `u` segment pick
//!   (uniform over `0..len`), one `u` button pick over ALL declared buttons
//!   in alphabet declaration order (uniform); if the picked button is in the
//!   direction group, one more `u` re-rolls the direction categorical from
//!   `cfg.weighted_random.direction.priors` (the raw config prior,
//!   unadjusted by context) and REPLACES the segment's direction-group bits;
//!   otherwise the picked bit is toggled (XOR) on that segment. Args:
//!   `repeats` — semicolon-joined `seg,btn_bit,action[,new_dir_mask]`
//!   (`action` is `toggle` or `redirect`; `new_dir_mask` only present for
//!   `redirect`).
//! - `splice`: requires a donor. If `sibling_bursts` is empty: falls back to
//!   `extend` (recorded as `extend`, drawing exactly `extend`'s sequence on
//!   this op's stream — see the stream-contract note above). Otherwise: one
//!   `u` picks the donor `∝ max(score_delta, ε)` over `sibling_bursts` (may
//!   coincide with the base sibling, if any — allowed); one `u` for cross
//!   point `i` uniform over `0..=base.len()` (boundary positions, inclusive
//!   of both ends); one `u` for cross point `j` uniform over
//!   `0..=donor.len()`. Result = `base[..i] ++ donor[j..]`. Args:
//!   `donor_burst_id` (lowercase hex), `i`, `j` (decimal) — or `extend`'s
//!   args when fallen back.
//! - `truncate`: one `u` for `f = 0.1 + 0.4u ~ U(0.1, 0.5)`;
//!   `frames_dropped = round_ties_even(f * total_frames)` trailing frames are
//!   dropped, shortening (never fully removing) the last surviving segment;
//!   if the drop would empty the burst, the first frame of the first segment
//!   is kept instead. Args: `u_bits` (hex of `f.to_bits()` — the *scaled*
//!   fraction, not the raw draw, per the pinned arg name), `frames_dropped`.
//! - `duplicate_segment`: one `u` for a uniform run start `0..len`, one `u`
//!   for a uniform run length `1..=3` (clamped to the segments remaining from
//!   `start`), one `u` for `k ∈ {1,2,3}` uniform; the run is repeated `k`
//!   additional times, inserted immediately after itself. Args: `run` —
//!   `start,len,k` (decimal).
//! - `swap_adjacent`: one `u` for a uniform boundary `0..len-1`; segments at
//!   that boundary are swapped. No-op (but the draw still happens) if
//!   `len < 2`. Args: `boundary` (decimal).
//!
//! Pipeline: [`generate`] selects a base (siblings/parent), draws `n_ops`
//! ops, applies each via [`apply_named_op`] (which samples fresh args on the
//! op's own stream and hands them straight to the shared `apply_*`
//! transform — the same transform [`apply_ops`] replays from recorded args,
//! so replay equality is structural, not incidental), post-clamps + legalizes
//! once via the caller's `PadModel`, dedups against the base's `burst_hash`
//! with a single forced `perturb_timing` retry, and returns the mutant burst
//! plus its [`MutationProvenance`].

use std::f64::consts::PI;

use rand_chacha::ChaCha8Rng;
use synth_core::config::ExperimentConfig;
use synth_core::fmath;
use synth_core::model::InputModel;
use synth_core::rng::{next_unit_f64, stream};
use synth_core::types::{burst_hash, Burst, PadBurst, PadSegment};
use synth_pad::PadModel;

use crate::context::{GenContext, ScoredContextBurst};
use crate::provenance::{MutationOpRec, MutationProvenance};
use crate::weighted_random;

/// Donor/sibling categorical floor (ARCHITECTURE.md §5.2: `∝ max(score_delta,
/// ε)`).
const EPS: f64 = 1e-6;

/// Frame budget `extend` samples its candidate segments over before cutting
/// to the target segment count `m`. Chosen far larger than any realistic
/// `m` (mean ~5) so the "at least `m` segments available" shortfall this
/// module documents is only a theoretical possibility, never observed in
/// practice; `sample_extend` clamps defensively regardless.
const EXTEND_STREAM_FRAMES: u64 = 20_000;

fn segments_total_frames(segments: &[PadSegment]) -> u64 {
    segments.iter().map(|s| u64::from(s.hold_frames)).sum()
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn sorted_args(mut args: Vec<(String, String)>) -> Vec<(String, String)> {
    args.sort_by(|a, b| a.0.cmp(&b.0));
    args
}

// ---------------------------------------------------------------------
// Base selection
// ---------------------------------------------------------------------

struct BaseSelection {
    segments: Vec<PadSegment>,
    base_burst_id: [u8; 32],
    base_was_sibling: bool,
}

/// Weighted pick over `sibs` `∝ max(score_delta, EPS)`, consuming exactly one
/// `u` from `rng`. Panics if `sibs` is empty (callers only invoke this after
/// checking non-emptiness).
fn pick_sibling<'a>(
    sibs: &'a [ScoredContextBurst],
    rng: &mut ChaCha8Rng,
) -> &'a ScoredContextBurst {
    assert!(!sibs.is_empty(), "pick_sibling requires a non-empty list");
    let weights: Vec<f64> = sibs.iter().map(|s| s.score_delta.max(EPS)).collect();
    let total: f64 = weights.iter().sum();
    let u = next_unit_f64(rng);
    let mut acc = 0.0;
    for (i, w) in weights.iter().enumerate() {
        acc += w / total;
        if u < acc {
            return &sibs[i];
        }
    }
    sibs.last().expect("checked non-empty above")
}

/// Base selection (module doc, stream `slot/{s}/mut/ops` steps 1-2).
fn select_base(cfg: &ExperimentConfig, ctx: &GenContext, rng: &mut ChaCha8Rng) -> BaseSelection {
    let has_parent = ctx.parent_burst.is_some();
    let has_siblings = !ctx.sibling_bursts.is_empty();
    assert!(
        has_parent || has_siblings,
        "mutation::generate requires parent_burst or sibling_bursts; the mixer must never \
         assign a Mutation slot without at least one (propose::generator_weights)"
    );

    if !has_siblings {
        let parent = ctx
            .parent_burst
            .as_ref()
            .expect("has_parent checked true when !has_siblings");
        return BaseSelection {
            segments: parent.pad.segments.clone(),
            base_burst_id: parent.burst_id,
            base_was_sibling: false,
        };
    }

    // Siblings are present. The donor-gate draw always happens (even when
    // there's no parent to gate against) so the stream position of every
    // subsequent draw never depends on which inputs happened to be present.
    let gate = next_unit_f64(rng);
    let use_sibling = !has_parent || gate < cfg.mutation.donor_bias;
    if use_sibling {
        let sib = pick_sibling(&ctx.sibling_bursts, rng);
        BaseSelection {
            segments: sib.burst.pad.segments.clone(),
            base_burst_id: sib.burst.burst_id,
            base_was_sibling: true,
        }
    } else {
        let parent = ctx.parent_burst.as_ref().expect("has_parent checked true");
        BaseSelection {
            segments: parent.pad.segments.clone(),
            base_burst_id: parent.burst_id,
            base_was_sibling: false,
        }
    }
}

// ---------------------------------------------------------------------
// n_ops / op selection (stream slot/{s}/mut/ops, steps 3-4)
// ---------------------------------------------------------------------

fn sample_n_ops(cfg: &ExperimentConfig, rng: &mut ChaCha8Rng) -> u32 {
    let ob = &cfg.mutation.ops_binomial;
    let mut b: u32 = 0;
    for _ in 0..ob.n {
        if next_unit_f64(rng) < ob.p {
            b += 1;
        }
    }
    1 + b.min(3)
}

fn sample_op_names(cfg: &ExperimentConfig, n_ops: u32, rng: &mut ChaCha8Rng) -> Vec<String> {
    let entries: Vec<(String, f64)> = cfg
        .mutation
        .op_probs
        .iter()
        .map(|(k, v)| (k.clone(), *v))
        .collect();
    (0..n_ops)
        .map(|_| weighted_random::categorical(&entries, next_unit_f64(rng)).to_owned())
        .collect()
}

// ---------------------------------------------------------------------
// perturb_timing
// ---------------------------------------------------------------------

fn apply_perturb_timing(
    segments: &[PadSegment],
    fired: &[usize],
    zs: &[f64],
    sigma_t: f64,
) -> Vec<PadSegment> {
    let mut out = segments.to_vec();
    for (&idx, &z) in fired.iter().zip(zs) {
        if let Some(seg) = out.get_mut(idx) {
            let d = f64::from(seg.hold_frames);
            let d2 = (d * fmath::exp(sigma_t * z)).round_ties_even();
            let d2 = if d2 < 1.0 {
                1.0
            } else {
                d2.min(f64::from(u32::MAX))
            };
            seg.hold_frames = d2 as u32;
        }
    }
    out
}

fn sample_perturb_timing(
    cfg: &ExperimentConfig,
    segments: &[PadSegment],
    rng: &mut ChaCha8Rng,
) -> (Vec<PadSegment>, MutationOpRec) {
    let sigma_t = cfg.mutation.timing_sigma;
    let mut fired = Vec::new();
    let mut zs = Vec::new();
    for i in 0..segments.len() {
        let gate = next_unit_f64(rng);
        if gate < 0.5 {
            let u1 = next_unit_f64(rng);
            let u2 = next_unit_f64(rng);
            let z = fmath::sqrt(-2.0 * fmath::ln(1.0 - u1)) * fmath::cos(2.0 * PI * u2);
            fired.push(i);
            zs.push(z);
        }
    }
    let out = apply_perturb_timing(segments, &fired, &zs, sigma_t);
    let args = sorted_args(vec![
        (
            "fired_segments".to_owned(),
            fired
                .iter()
                .map(usize::to_string)
                .collect::<Vec<_>>()
                .join(","),
        ),
        (
            "z_bits".to_owned(),
            zs.iter()
                .map(|z| format!("{:016x}", z.to_bits()))
                .collect::<Vec<_>>()
                .join(","),
        ),
    ]);
    (
        out,
        MutationOpRec {
            op: "perturb_timing".to_owned(),
            args,
        },
    )
}

fn parse_usize_csv(s: &str) -> Vec<usize> {
    if s.is_empty() {
        return Vec::new();
    }
    s.split(',')
        .map(|x| {
            x.parse()
                .expect("recorded perturb_timing fired_segments index")
        })
        .collect()
}

fn parse_f64_bits_csv(s: &str) -> Vec<f64> {
    if s.is_empty() {
        return Vec::new();
    }
    s.split(',')
        .map(|x| {
            f64::from_bits(u64::from_str_radix(x, 16).expect("recorded perturb_timing z_bits hex"))
        })
        .collect()
}

// ---------------------------------------------------------------------
// extend
// ---------------------------------------------------------------------

fn apply_extend(segments: &[PadSegment], appended: &[PadSegment]) -> Vec<PadSegment> {
    let mut out = segments.to_vec();
    out.extend(appended.iter().copied());
    out
}

fn sample_extend(
    cfg: &ExperimentConfig,
    ctx: &GenContext,
    segments: &[PadSegment],
    rng: &mut ChaCha8Rng,
) -> (Vec<PadSegment>, MutationOpRec) {
    let g = weighted_random::geometric(rng, 0.25);
    let m = 1 + g;
    let initial_mask = segments.last().map(|s| s.buttons).unwrap_or(0);
    let generated = weighted_random::generate_single_stream_from_mask(
        cfg,
        ctx,
        EXTEND_STREAM_FRAMES,
        initial_mask,
        rng,
    );
    let m_actual = usize::try_from(m)
        .unwrap_or(usize::MAX)
        .min(generated.len())
        .max(1);
    let appended: Vec<PadSegment> = generated.into_iter().take(m_actual).collect();
    let out = apply_extend(segments, &appended);
    let appended_str = appended
        .iter()
        .map(|s| format!("{}:{}", s.buttons, s.hold_frames))
        .collect::<Vec<_>>()
        .join(";");
    let args = sorted_args(vec![
        ("m".to_owned(), m.to_string()),
        ("appended".to_owned(), appended_str),
    ]);
    (
        out,
        MutationOpRec {
            op: "extend".to_owned(),
            args,
        },
    )
}

fn parse_appended(s: &str) -> Vec<PadSegment> {
    if s.is_empty() {
        return Vec::new();
    }
    s.split(';')
        .map(|pair| {
            let (mask, frames) = pair.split_once(':').expect("recorded extend appended pair");
            PadSegment {
                buttons: mask.parse().expect("recorded extend appended mask"),
                hold_frames: frames.parse().expect("recorded extend appended frames"),
            }
        })
        .collect()
}

// ---------------------------------------------------------------------
// flip_button
// ---------------------------------------------------------------------

struct FlipRepeat {
    seg: usize,
    btn_bit: u8,
    redirect_mask: Option<u16>,
}

fn apply_flip_button(
    segments: &[PadSegment],
    repeats: &[FlipRepeat],
    dir_group_mask: u16,
) -> Vec<PadSegment> {
    let mut out = segments.to_vec();
    for r in repeats {
        let Some(seg) = out.get_mut(r.seg) else {
            continue;
        };
        match r.redirect_mask {
            Some(new_mask) => seg.buttons = (seg.buttons & !dir_group_mask) | new_mask,
            None => seg.buttons ^= 1u16 << r.btn_bit,
        }
    }
    out
}

fn sample_flip_button(
    cfg: &ExperimentConfig,
    segments: &[PadSegment],
    rng: &mut ChaCha8Rng,
) -> (Vec<PadSegment>, MutationOpRec) {
    let g = weighted_random::geometric(rng, 0.5);
    let m = 1 + g;
    let dir_group_mask = cfg
        .button_alphabet
        .mask(&cfg.button_alphabet.directions.group)
        .unwrap_or(0);
    let dir_priors: Vec<(String, f64)> = cfg
        .weighted_random
        .direction
        .priors
        .iter()
        .map(|(k, v)| (k.clone(), *v))
        .collect();
    let buttons = &cfg.button_alphabet.buttons;
    let seg_count = segments.len();

    let mut repeats: Vec<FlipRepeat> = Vec::new();
    let mut rec_parts: Vec<String> = Vec::new();
    for _ in 0..m {
        if seg_count == 0 || buttons.is_empty() {
            break;
        }
        let u_seg = next_unit_f64(rng);
        let seg_idx = ((u_seg * seg_count as f64) as usize).min(seg_count - 1);
        let u_btn = next_unit_f64(rng);
        let btn_idx = ((u_btn * buttons.len() as f64) as usize).min(buttons.len() - 1);
        let btn = &buttons[btn_idx];
        let is_direction = cfg
            .button_alphabet
            .directions
            .group
            .iter()
            .any(|n| n == &btn.name);

        if is_direction {
            let u_dir = next_unit_f64(rng);
            let dir_name = weighted_random::categorical(&dir_priors, u_dir);
            let new_mask = weighted_random::dir_mask(cfg, dir_name);
            rec_parts.push(format!("{seg_idx},{},redirect,{new_mask}", btn.bit));
            repeats.push(FlipRepeat {
                seg: seg_idx,
                btn_bit: btn.bit,
                redirect_mask: Some(new_mask),
            });
        } else {
            rec_parts.push(format!("{seg_idx},{},toggle", btn.bit));
            repeats.push(FlipRepeat {
                seg: seg_idx,
                btn_bit: btn.bit,
                redirect_mask: None,
            });
        }
    }
    // The same transform `apply_ops` replays from recorded args (module doc:
    // "generate = sample args -> apply"), applied here to the freshly
    // sampled `repeats` — never a separately-hand-rolled mutation loop.
    let out = apply_flip_button(segments, &repeats, dir_group_mask);
    let args = sorted_args(vec![("repeats".to_owned(), rec_parts.join(";"))]);
    (
        out,
        MutationOpRec {
            op: "flip_button".to_owned(),
            args,
        },
    )
}

fn parse_repeats(s: &str) -> Vec<FlipRepeat> {
    if s.is_empty() {
        return Vec::new();
    }
    s.split(';')
        .map(|entry| {
            let parts: Vec<&str> = entry.split(',').collect();
            let seg: usize = parts[0].parse().expect("recorded flip_button seg");
            let btn_bit: u8 = parts[1].parse().expect("recorded flip_button btn_bit");
            let redirect_mask = if parts.len() > 3 && parts[2] == "redirect" {
                Some(parts[3].parse().expect("recorded flip_button new_dir_mask"))
            } else {
                None
            };
            FlipRepeat {
                seg,
                btn_bit,
                redirect_mask,
            }
        })
        .collect()
}

// ---------------------------------------------------------------------
// splice (+ extend fallback)
// ---------------------------------------------------------------------

fn apply_splice(base: &[PadSegment], donor: &[PadSegment], i: usize, j: usize) -> Vec<PadSegment> {
    let i = i.min(base.len());
    let j = j.min(donor.len());
    let mut out = base[..i].to_vec();
    out.extend(donor[j..].iter().copied());
    out
}

/// Uniform pick over segment boundaries `0..=len` (`len + 1` values), one
/// `u`.
fn pick_boundary_inclusive(len: usize, u: f64) -> usize {
    let n = len + 1;
    ((u * n as f64) as usize).min(n - 1)
}

fn sample_splice_or_fallback(
    cfg: &ExperimentConfig,
    ctx: &GenContext,
    segments: &[PadSegment],
    rng: &mut ChaCha8Rng,
) -> (Vec<PadSegment>, MutationOpRec, Option<[u8; 32]>) {
    if ctx.sibling_bursts.is_empty() {
        let (segs, rec) = sample_extend(cfg, ctx, segments, rng);
        return (segs, rec, None);
    }
    let donor = pick_sibling(&ctx.sibling_bursts, rng);
    let donor_segments = donor.burst.pad.segments.clone();
    let donor_id = donor.burst.burst_id;

    let u_i = next_unit_f64(rng);
    let i = pick_boundary_inclusive(segments.len(), u_i);
    let u_j = next_unit_f64(rng);
    let j = pick_boundary_inclusive(donor_segments.len(), u_j);

    let out = apply_splice(segments, &donor_segments, i, j);
    let args = sorted_args(vec![
        ("donor_burst_id".to_owned(), hex_encode(&donor_id)),
        ("i".to_owned(), i.to_string()),
        ("j".to_owned(), j.to_string()),
    ]);
    (
        out,
        MutationOpRec {
            op: "splice".to_owned(),
            args,
        },
        Some(donor_id),
    )
}

// ---------------------------------------------------------------------
// truncate
// ---------------------------------------------------------------------

fn apply_truncate(segments: &[PadSegment], frames_dropped: u64) -> Vec<PadSegment> {
    let total = segments_total_frames(segments);
    let keep = total.saturating_sub(frames_dropped).max(1);
    let mut out = Vec::with_capacity(segments.len());
    let mut remaining = keep;
    for seg in segments {
        if remaining == 0 {
            break;
        }
        let take = u64::from(seg.hold_frames).min(remaining);
        if take > 0 {
            out.push(PadSegment {
                buttons: seg.buttons,
                hold_frames: take as u32,
            });
        }
        remaining -= take;
    }
    if out.is_empty() {
        out.push(PadSegment {
            buttons: 0,
            hold_frames: 1,
        });
    }
    out
}

fn sample_truncate(
    segments: &[PadSegment],
    rng: &mut ChaCha8Rng,
) -> (Vec<PadSegment>, MutationOpRec) {
    let u = next_unit_f64(rng);
    let f = 0.1 + u * 0.4;
    let total = segments_total_frames(segments);
    let frames_dropped = (f * total as f64).round_ties_even() as u64;
    let out = apply_truncate(segments, frames_dropped);
    let args = sorted_args(vec![
        ("u_bits".to_owned(), format!("{:016x}", f.to_bits())),
        ("frames_dropped".to_owned(), frames_dropped.to_string()),
    ]);
    (
        out,
        MutationOpRec {
            op: "truncate".to_owned(),
            args,
        },
    )
}

// ---------------------------------------------------------------------
// duplicate_segment
// ---------------------------------------------------------------------

fn apply_duplicate_segment(
    segments: &[PadSegment],
    start: usize,
    run_len: usize,
    k: usize,
) -> Vec<PadSegment> {
    if segments.is_empty() {
        return Vec::new();
    }
    let start = start.min(segments.len() - 1);
    let end = (start + run_len.max(1)).min(segments.len());
    let run = segments[start..end].to_vec();
    let mut out = segments[..end].to_vec();
    for _ in 0..k {
        out.extend(run.iter().copied());
    }
    out.extend(segments[end..].iter().copied());
    out
}

fn sample_duplicate_segment(
    segments: &[PadSegment],
    rng: &mut ChaCha8Rng,
) -> (Vec<PadSegment>, MutationOpRec) {
    let len = segments.len().max(1);
    let u_start = next_unit_f64(rng);
    let start = ((u_start * len as f64) as usize).min(len - 1);
    let remaining = segments.len().saturating_sub(start).max(1);
    let u_len = next_unit_f64(rng);
    let run_len_raw = 1 + ((u_len * 3.0) as usize).min(2);
    let run_len = run_len_raw.min(remaining);
    let u_k = next_unit_f64(rng);
    let k = 1 + ((u_k * 3.0) as usize).min(2);
    let out = apply_duplicate_segment(segments, start, run_len, k);
    let args = sorted_args(vec![("run".to_owned(), format!("{start},{run_len},{k}"))]);
    (
        out,
        MutationOpRec {
            op: "duplicate_segment".to_owned(),
            args,
        },
    )
}

fn parse_run(s: &str) -> (usize, usize, usize) {
    let parts: Vec<&str> = s.split(',').collect();
    let get = |i: usize| parts.get(i).and_then(|p| p.parse().ok()).unwrap_or(0);
    (get(0), get(1), get(2))
}

// ---------------------------------------------------------------------
// swap_adjacent
// ---------------------------------------------------------------------

fn apply_swap_adjacent(segments: &[PadSegment], boundary: usize) -> Vec<PadSegment> {
    let mut out = segments.to_vec();
    if boundary + 1 < out.len() {
        out.swap(boundary, boundary + 1);
    }
    out
}

fn sample_swap_adjacent(
    segments: &[PadSegment],
    rng: &mut ChaCha8Rng,
) -> (Vec<PadSegment>, MutationOpRec) {
    let u = next_unit_f64(rng);
    let len = segments.len();
    let boundary = if len < 2 {
        0
    } else {
        ((u * (len - 1) as f64) as usize).min(len - 2)
    };
    let out = apply_swap_adjacent(segments, boundary);
    let args = sorted_args(vec![("boundary".to_owned(), boundary.to_string())]);
    (
        out,
        MutationOpRec {
            op: "swap_adjacent".to_owned(),
            args,
        },
    )
}

// ---------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------

/// Sample + apply one named op on its own stream, returning the new segment
/// list, its provenance record (op name may differ from `op_name` — the
/// `splice`-without-donor fallback records `extend`), and a splice donor id
/// if one was used.
fn apply_named_op(
    op_name: &str,
    cfg: &ExperimentConfig,
    ctx: &GenContext,
    segments: &[PadSegment],
    rng: &mut ChaCha8Rng,
) -> (Vec<PadSegment>, MutationOpRec, Option<[u8; 32]>) {
    match op_name {
        "perturb_timing" => {
            let (segs, rec) = sample_perturb_timing(cfg, segments, rng);
            (segs, rec, None)
        }
        "extend" => {
            let (segs, rec) = sample_extend(cfg, ctx, segments, rng);
            (segs, rec, None)
        }
        "flip_button" => {
            let (segs, rec) = sample_flip_button(cfg, segments, rng);
            (segs, rec, None)
        }
        "splice" => sample_splice_or_fallback(cfg, ctx, segments, rng),
        "truncate" => {
            let (segs, rec) = sample_truncate(segments, rng);
            (segs, rec, None)
        }
        "duplicate_segment" => {
            let (segs, rec) = sample_duplicate_segment(segments, rng);
            (segs, rec, None)
        }
        "swap_adjacent" => {
            let (segs, rec) = sample_swap_adjacent(segments, rng);
            (segs, rec, None)
        }
        other => panic!(
            "mutation::generate: unknown op {other:?} in cfg.mutation.op_probs \
             (validation should have rejected an unrecognized op name)"
        ),
    }
}

/// Parse a lowercase-hex `burst_id` (as recorded by `sample_splice_or_fallback`'s
/// `donor_burst_id` arg) back into its 32 raw bytes.
fn parse_hex_32(s: &str) -> [u8; 32] {
    assert_eq!(
        s.len(),
        64,
        "recorded donor_burst_id must be 64 hex chars (32 bytes), got {s:?}"
    );
    let mut out = [0u8; 32];
    for (i, chunk) in out.iter_mut().enumerate() {
        *chunk = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16)
            .expect("recorded donor_burst_id must be valid hex");
    }
    out
}

/// Resolve one recorded splice's own donor from `donors` (fix #5: each
/// splice op records its own `donor_burst_id`; two splices in one mutant can
/// legitimately pick different donors, so replay must look each one up
/// individually rather than reusing a single caller-supplied donor for
/// every splice). Panics with a clear message if the recorded id isn't
/// present — `apply_ops` is test-side replay-only API that must only ever
/// see args recorded by `generate` for the exact donor set it drew from
/// (see the doc note on `apply_ops` itself).
fn resolve_donor<'a>(
    donors: &[(&'a [u8; 32], &'a PadBurst)],
    donor_id_hex: &str,
) -> &'a [PadSegment] {
    let id = parse_hex_32(donor_id_hex);
    donors
        .iter()
        .find(|(donor_id, _)| **donor_id == id)
        .map(|(_, burst)| burst.segments.as_slice())
        .unwrap_or_else(|| {
            panic!(
                "apply_ops: recorded donor_burst_id {donor_id_hex} not found among the \
                 {} donor(s) supplied to replay — donors must include every sibling the \
                 original `generate` call had available",
                donors.len()
            )
        })
}

fn apply_recorded_op(
    cfg: &ExperimentConfig,
    op: &MutationOpRec,
    segments: &[PadSegment],
    donors: &[(&[u8; 32], &PadBurst)],
    dir_group_mask: u16,
) -> Vec<PadSegment> {
    let get = |key: &str| -> &str {
        op.args
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
            .unwrap_or("")
    };
    match op.op.as_str() {
        "perturb_timing" => {
            let fired = parse_usize_csv(get("fired_segments"));
            let zs = parse_f64_bits_csv(get("z_bits"));
            apply_perturb_timing(segments, &fired, &zs, cfg.mutation.timing_sigma)
        }
        "extend" => {
            let appended = parse_appended(get("appended"));
            apply_extend(segments, &appended)
        }
        "flip_button" => {
            let repeats = parse_repeats(get("repeats"));
            apply_flip_button(segments, &repeats, dir_group_mask)
        }
        "splice" => {
            let i: usize = get("i").parse().unwrap_or(0);
            let j: usize = get("j").parse().unwrap_or(0);
            // This op's OWN recorded donor (fix #5) — never a single donor
            // shared across every splice in the mutant.
            let donor_segments = resolve_donor(donors, get("donor_burst_id"));
            apply_splice(segments, donor_segments, i, j)
        }
        "truncate" => {
            let frames_dropped: u64 = get("frames_dropped").parse().unwrap_or(0);
            apply_truncate(segments, frames_dropped)
        }
        "duplicate_segment" => {
            let (start, run_len, k) = parse_run(get("run"));
            apply_duplicate_segment(segments, start, run_len, k)
        }
        "swap_adjacent" => {
            let boundary: usize = get("boundary").parse().unwrap_or(0);
            apply_swap_adjacent(segments, boundary)
        }
        other => panic!("apply_ops: unknown recorded op {other:?}"),
    }
}

/// Split a recorded ops list into (main ops, the forced retry op if present).
/// The retry op, when present, is always last and is identified by carrying
/// a `("retry", _)` arg (module doc: `generate` appends it that way).
fn split_retry(ops: &[MutationOpRec]) -> (&[MutationOpRec], Option<&MutationOpRec>) {
    if let Some(last) = ops.last() {
        if last.args.iter().any(|(k, _)| k == "retry") {
            return (&ops[..ops.len() - 1], Some(last));
        }
    }
    (ops, None)
}

// ---------------------------------------------------------------------
// Public pipeline
// ---------------------------------------------------------------------

/// Generate one mutation-generator burst for slot `s` (ARCHITECTURE.md
/// §5.2). Caller supplies the fan-out root; this function derives its own
/// labeled streams (module doc: draw-order contract) and returns a
/// legalized burst (mutation legalizes internally — see the module doc's
/// pipeline note — so the caller's own post-match `legalize` call is an
/// idempotent no-op for this generator, same as every other).
///
/// Panics if neither `ctx.parent_burst` nor `ctx.sibling_bursts` is present:
/// the caller (`propose::generator_weights`) must never assign a `Mutation`
/// slot otherwise.
pub fn generate(
    cfg: &ExperimentConfig,
    ctx: &GenContext,
    root: &[u8; 32],
    slot: usize,
    model: &PadModel,
) -> (Burst, MutationProvenance) {
    let mut ops_rng = stream(root, &format!("slot/{slot}/mut/ops"));
    let base = select_base(cfg, ctx, &mut ops_rng);
    let n_ops = sample_n_ops(cfg, &mut ops_rng);
    let op_names = sample_op_names(cfg, n_ops, &mut ops_rng);

    let mut segments = base.segments.clone();
    let mut ops: Vec<MutationOpRec> = Vec::with_capacity(op_names.len());
    let mut donor_burst_id: Option<[u8; 32]> = None;

    for (i, op_name) in op_names.iter().enumerate() {
        let mut op_rng = stream(root, &format!("slot/{slot}/mut/op/{i}"));
        let (new_segments, rec, donor_id) =
            apply_named_op(op_name, cfg, ctx, &segments, &mut op_rng);
        segments = new_segments;
        if donor_id.is_some() {
            donor_burst_id = donor_id;
        }
        ops.push(rec);
    }

    let pre_total = segments_total_frames(&segments);
    let post_clamp =
        pre_total < u64::from(model.min_frames()) || pre_total > u64::from(model.max_frames());
    let legalized = model.legalize(Burst::Pad(PadBurst { segments }));
    let Burst::Pad(PadBurst {
        segments: mut legal_segments,
    }) = legalized
    else {
        unreachable!("PadModel::legalize always returns Burst::Pad")
    };

    let base_hash = burst_hash(&Burst::Pad(PadBurst {
        segments: base.segments.clone(),
    }));
    let mutant_hash = burst_hash(&Burst::Pad(PadBurst {
        segments: legal_segments.clone(),
    }));
    if mutant_hash == base_hash {
        let mut retry_rng = stream(root, &format!("slot/{slot}/mut/retry"));
        let (retried_segments, mut rec) =
            sample_perturb_timing(cfg, &legal_segments, &mut retry_rng);
        rec.args.push(("retry".to_owned(), "1".to_owned()));
        rec.args = sorted_args(rec.args);
        ops.push(rec);
        let relegalized = model.legalize(Burst::Pad(PadBurst {
            segments: retried_segments,
        }));
        let Burst::Pad(PadBurst { segments }) = relegalized else {
            unreachable!("PadModel::legalize always returns Burst::Pad")
        };
        legal_segments = segments;
    }

    let prov = MutationProvenance {
        base_burst_id: base.base_burst_id,
        donor_burst_id,
        base_was_sibling: base.base_was_sibling,
        ops,
        post_clamp,
    };
    (
        Burst::Pad(PadBurst {
            segments: legal_segments,
        }),
        prov,
    )
}

/// Replay a recorded op list against `base` (and `donors`, for any splice
/// ops — fix #5: each splice records its OWN `donor_burst_id`, so `donors`
/// must include every sibling burst that was available to the original
/// `generate` call, not just the one referenced by
/// `MutationProvenance.donor_burst_id`, which only ever reflects the LAST
/// splice's donor: proto has one field for it, but per-op `args` are
/// authoritative and this function always resolves each splice from its own
/// recorded arg) with zero RNG use, reproducing the mutant `generate`
/// emitted byte-for-byte — this is what makes the provenance-replay accept
/// test meaningful (`06-m3-mutation-stretch.md` Accept list). Mirrors
/// `generate`'s own structure: apply the main ops, legalize once, then (iff
/// `ops` carries a trailing forced-retry `perturb_timing`) apply it and
/// legalize again.
///
/// Safety (fix #12): this function must only ever be called with `ops`/args
/// that were recorded by a `generate` call for the exact `base`/`donors`
/// pairing it drew from — every parser above (`parse_usize_csv`,
/// `parse_f64_bits_csv`, `parse_appended`, `parse_repeats`, `parse_run`,
/// `parse_hex_32`) panics on malformed input by design, and `resolve_donor`
/// panics if a recorded donor id isn't in `donors`. `apply_ops` is
/// replay-only, test-side API (its only caller today is
/// `mutation_suite.rs`'s provenance-replay test); it must never be fed
/// untrusted wire data directly.
pub fn apply_ops(
    cfg: &ExperimentConfig,
    model: &PadModel,
    base: &PadBurst,
    ops: &[MutationOpRec],
    donors: &[(&[u8; 32], &PadBurst)],
) -> Burst {
    let dir_group_mask = cfg
        .button_alphabet
        .mask(&cfg.button_alphabet.directions.group)
        .unwrap_or(0);
    let (main_ops, retry_op) = split_retry(ops);

    let mut segments = base.segments.clone();
    for op in main_ops {
        segments = apply_recorded_op(cfg, op, &segments, donors, dir_group_mask);
    }
    let legalized = model.legalize(Burst::Pad(PadBurst { segments }));
    let Burst::Pad(PadBurst { mut segments }) = legalized else {
        unreachable!("PadModel::legalize always returns Burst::Pad")
    };

    if let Some(retry) = retry_op {
        segments = apply_recorded_op(cfg, retry, &segments, donors, dir_group_mask);
        let relegalized = model.legalize(Burst::Pad(PadBurst { segments }));
        let Burst::Pad(PadBurst { segments: s2 }) = relegalized else {
            unreachable!("PadModel::legalize always returns Burst::Pad")
        };
        segments = s2;
    }

    Burst::Pad(PadBurst { segments })
}
