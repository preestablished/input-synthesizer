//! M1 statistical acceptance suite (IMPLEMENTATION-PLAN.md §M1 Accept,
//! ARCHITECTURE.md §4). Every test below is **fixed-seed and deterministic**
//! — it asserts properties of one specific, reproducible sample; nothing
//! here re-rolls dice at runtime, so nothing here can flake (testing
//! strategy note 2).
//!
//! Corpus generation choice (documented per the task): N = 2000 bursts are
//! produced by a **single** `propose(..., k=2000, ...)` call per scenario,
//! not 2000 separate seeded calls. `generator_mix` is pinned to pure
//! weighted-random (1.0/0/0/0) so the mixer assigns all 2000 slots to the
//! weighted-random generator in one deterministic Fisher-Yates-permuted
//! plan; each slot still draws from its own independent labeled RNG stream
//! (`slot/{s}/wr/...`), so the 2000 bursts are as statistically independent
//! as 2000 separately-seeded calls would be, at a fraction of the harness
//! overhead.
//!
//! Config: the API.md §5 demo document (console16-12btn-v1, §5.4 priors)
//! with `diagonal_factor: 0.0` (see `common::base_yaml` doc comment) so the
//! sticky-direction closed-form math in ARCHITECTURE.md §4.3 applies without
//! the diagonal-enrichment perturbation.

#[path = "common/mod.rs"]
mod common;

use std::sync::OnceLock;

use statrs::distribution::{ChiSquared, ContinuousCDF};
use synth_core::config::ExperimentConfig;
use synth_core::fmath;
use synth_core::types::{Burst, PadBurst};
use synth_gen::propose::{propose, Availability, SlotResult};

const N: usize = 2000;
const LENGTH_HINT: u32 = 300;
const BASE_SEED: u64 = 0x5EED_1000_0000_0001;

fn stats_cfg() -> &'static ExperimentConfig {
    static CFG: OnceLock<ExperimentConfig> = OnceLock::new();
    CFG.get_or_init(|| common::parse_and_validate(&common::base_yaml(0.0)))
}

/// A second config, identical to `stats_cfg()` but with much longer bursts
/// (fixed at 8000 frames instead of ~300). Needed for (b) and (c): both
/// measure *interior* runs (excluding any run that touches the burst
/// boundary, per the task's own measurement note, to avoid biasing the mean
/// down from truncation). But excluding on `start > 0 && start+len < total`
/// is itself a length-biased selection — a run is *less* likely to be
/// interior the longer it is, since a longer run is more likely to reach
/// the boundary — and that bias is only negligible when the burst is much
/// longer than the quantity being measured. At the ~300-frame default burst
/// length this bias is severe for direction runs (mean ~62-118 frames: a
/// third of the burst), and non-trivial even for the shorter per-button
/// holds (mean up to 60 frames). Using 8000-frame bursts drives the
/// boundary-exclusion fraction down to a few percent, which is what the
/// task's "either exclude or account for it (excluding is fine, document)"
/// note assumes. Documented deviation from a literal N=2000-bursts-at-300-
/// frames setup; N is reduced accordingly (300 bursts × 8000 frames ≈ 2.4M
/// frames — more raw frames than the 2000×300 default corpus, just shaped
/// differently) so runtime stays reasonable.
fn large_target_cfg() -> &'static ExperimentConfig {
    static CFG: OnceLock<ExperimentConfig> = OnceLock::new();
    CFG.get_or_init(|| {
        let base = common::parse_and_validate(&common::base_yaml(0.0));
        let overrides = br#"
burst_len:
  distribution: fixed
  mean_frames: 8000
  min_frames: 16
  max_frames: 20000
"#;
        let merged = synth_core::config::deep_merge(&base, overrides).expect("deep_merge");
        synth_core::config::validate(&merged).expect("valid");
        merged
    })
}

const LARGE_N: usize = 300;
const LARGE_LENGTH_HINT: u32 = 8000;
const LARGE_SEED: u64 = 0x5EED_2000_0000_0002;

fn large_corpus() -> &'static Vec<SlotResult> {
    static CORPUS: OnceLock<Vec<SlotResult>> = OnceLock::new();
    CORPUS.get_or_init(|| {
        let cfg = large_target_cfg();
        let ctx = common::ctx_free("stats-node-large");
        let (results, degraded) = propose(
            cfg,
            &ctx,
            LARGE_N,
            LARGE_LENGTH_HINT,
            LARGE_SEED,
            Availability::default(),
            None,
        );
        assert!(degraded.is_empty());
        results
    })
}

/// The N=2000-burst context-free corpus, generated once per test binary
/// invocation and shared by every sub-test below (a/b/c/d/f all operate on
/// it; only (e) needs its own context-conditioned corpora).
fn corpus() -> &'static Vec<SlotResult> {
    static CORPUS: OnceLock<Vec<SlotResult>> = OnceLock::new();
    CORPUS.get_or_init(|| {
        let cfg = stats_cfg();
        let ctx = common::ctx_free("stats-node");
        let (results, degraded) = propose(
            cfg,
            &ctx,
            N,
            LENGTH_HINT,
            BASE_SEED,
            Availability::default(),
            None,
        );
        assert!(
            degraded.is_empty(),
            "pure-WR generator_mix should never report degraded generators"
        );
        results
    })
}

fn pads(results: &[SlotResult]) -> Vec<&PadBurst> {
    results
        .iter()
        .map(|r| match &r.burst {
            Burst::Pad(p) => p,
            #[allow(unreachable_patterns)]
            _ => unreachable!("M1 scope: pad model only"),
        })
        .collect()
}

/// Contiguous runs of a derived value over frames, reconstructed by
/// scanning segments and merging wherever the *derived* value repeats
/// (which happens both within one segment and across segment boundaries
/// caused by unrelated bits changing) — this is the general "measure on
/// per-frame reconstruction" method the task calls for, used for both the
/// per-button hold-duration test and the direction-segment test.
/// Returns `(value, start_frame, length)` triples.
fn value_runs<K: PartialEq + Copy>(pad: &PadBurst, key: impl Fn(u16) -> K) -> Vec<(K, u64, u64)> {
    let mut out = Vec::new();
    let mut frame = 0u64;
    let mut cur: Option<(K, u64)> = None;
    for seg in &pad.segments {
        let v = key(seg.buttons);
        match cur {
            Some((cv, _)) if cv == v => {}
            Some((cv, start)) => {
                out.push((cv, start, frame - start));
                cur = Some((v, frame));
            }
            None => cur = Some((v, frame)),
        }
        frame += u64::from(seg.hold_frames);
    }
    if let Some((cv, start)) = cur {
        out.push((cv, start, frame - start));
    }
    out
}

/// `value_runs`, filtered to drop any run touching frame 0 or the burst's
/// last frame (a hold/segment truncated by the burst boundary is not a
/// complete draw and would bias the mean down — documented exclusion, per
/// the task's measurement note).
fn interior_runs<K: PartialEq + Copy>(pad: &PadBurst, key: impl Fn(u16) -> K) -> Vec<(K, u64)> {
    let total = pad.total_frames();
    value_runs(pad, key)
        .into_iter()
        .filter(|&(_, start, len)| start > 0 && start + len < total)
        .map(|(v, _, len)| (v, len))
        .collect()
}

fn mean(xs: &[u64]) -> f64 {
    xs.iter().sum::<u64>() as f64 / xs.len() as f64
}

fn rel_err(observed: f64, expected: f64) -> f64 {
    (observed - expected).abs() / expected
}

/// χ² statistic for observed geometric-hold durations against
/// `Geometric(1/mu)` on {1,2,...}, bucketed into `n_bins` bins of equal
/// width `w = ceil(3*mu/n_bins)` covering `[1, 3*mu]`, with the last bin a
/// survival-function tail. Returns `(chi2, dof)`.
fn geometric_chi_square(durations: &[u64], mu: f64, n_bins: usize) -> (f64, u64) {
    let r = 1.0 / mu;
    let n = durations.len() as f64;
    let w = ((3.0 * mu) / n_bins as f64).ceil().max(1.0) as u64;
    let bin_of = |d: u64| -> usize {
        let idx = ((d.saturating_sub(1)) / w) as usize;
        idx.min(n_bins - 1)
    };
    let mut obs = vec![0f64; n_bins];
    for &d in durations {
        obs[bin_of(d)] += 1.0;
    }
    let mut exp_p = vec![0f64; n_bins];
    for (i, e) in exp_p.iter_mut().enumerate() {
        let lo = i as u64 * w + 1;
        *e = if i + 1 < n_bins {
            let hi = lo + w - 1;
            (1.0 - r).powi((lo - 1) as i32) - (1.0 - r).powi(hi as i32)
        } else {
            (1.0 - r).powi((lo - 1) as i32)
        };
    }
    let exp: Vec<f64> = exp_p.iter().map(|p| p * n).collect();
    let chi2: f64 = obs.iter().zip(&exp).map(|(o, e)| (o - e).powi(2) / e).sum();
    (chi2, (n_bins - 1) as u64)
}

fn chi2_critical(dof: u64, alpha: f64) -> f64 {
    ChiSquared::new(dof as f64)
        .expect("valid dof")
        .inverse_cdf(1.0 - alpha)
}

/// Variance-inflation factor for the ON-frame count of a two-state Markov
/// chain (ARCHITECTURE.md §4.2) over many frames, relative to i.i.d.
/// Bernoulli(duty) frames: for a chain with attack/release rates `(a, r)`,
/// the second eigenvalue is `rho = 1 - a - r`, and for large `L`,
/// `Var(on_frames) ≈ L · duty · (1-duty) · (1+rho)/(1-rho)`. Frames within a
/// hold/gap are almost perfectly correlated (that's the whole point of the
/// generator — ARCHITECTURE.md §4.1), so treating on/off frames as
/// independent Bernoulli trials (naive chi-square over frame counts)
/// drastically *understates* variance and produces a chi-square statistic
/// that rejects a correctly-implemented generator outright. This scaling is
/// what makes a frame-count-based chi-square statistically meaningful.
fn markov_variance_inflation(duty: f64, mu: f64) -> f64 {
    let r = 1.0 / mu;
    let a = duty / (mu * (1.0 - duty));
    let rho = 1.0 - a - r;
    (1.0 + rho) / (1.0 - rho).max(1e-12)
}

// ---------------------------------------------------------------------
// (a) Per-button empirical duty.
// ---------------------------------------------------------------------

/// Per-button empirical duty within ±10% relative of configured π_b, for
/// A/B/Y; plus a formal χ² goodness-of-fit gate across A/B/Y/START at
/// p < 0.001 (huge margin at N=2000 bursts ≈ 600k frames).
///
/// Deviations (both documented, per the task):
/// - START's configured duty is 0.002, so ±10% relative would need an
///   enormous N to resolve against sampling noise. Instead we hard-assert
///   its empirical duty stays under 0.01 (5x the configured value) — the χ²
///   gate below is the formal statistical check that covers START too.
/// - The χ² statistic is computed with **Markov-variance-scaled** expected
///   counts (`markov_variance_inflation`), not the naive
///   `(obs-exp)^2/exp` GOF formula a category-count chi-square normally
///   uses. Reason: frames within one hold/gap are almost perfectly
///   correlated (ARCHITECTURE.md §4.1's whole premise — that's why runs are
///   sampled instead of frames), so on-frame counts do **not** have
///   binomial variance; using the naive formula rejects a correct generator
///   outright at this N (verified while writing this suite: the naive
///   statistic came out ~20-30x its properly-scaled value, tracking each
///   button's `(1+rho)/(1-rho)` inflation factor almost exactly).
#[test]
fn per_button_duty_within_tolerance() {
    let cfg = stats_cfg();
    let corpus = corpus();
    let pads = pads(corpus);
    let total_frames: u64 = pads.iter().map(|p| p.total_frames()).sum();

    let cases = [
        ("A", 0u8, 0.20_f64, 18.0_f64),
        ("B", 1u8, 0.35, 60.0),
        ("Y", 3u8, 0.15, 30.0),
    ];
    let mut chi2 = 0.0f64;
    for (name, bit, expected_duty, mu) in cases {
        let mask = 1u16 << bit;
        let on_frames: u64 = pads
            .iter()
            .map(|p| {
                p.segments
                    .iter()
                    .filter(|s| s.buttons & mask != 0)
                    .map(|s| u64::from(s.hold_frames))
                    .sum::<u64>()
            })
            .sum();
        let empirical = on_frames as f64 / total_frames as f64;
        let err = rel_err(empirical, expected_duty);
        assert!(
            err < 0.10,
            "{name}: empirical duty {empirical:.4} vs configured {expected_duty} \
             (rel err {err:.4} >= 0.10)"
        );
        let expected_on = expected_duty * total_frames as f64;
        let inflation = markov_variance_inflation(expected_duty, mu);
        let variance = total_frames as f64 * expected_duty * (1.0 - expected_duty) * inflation;
        let z = (on_frames as f64 - expected_on) / variance.sqrt();
        chi2 += z * z;
    }
    // START: assert empirical duty stays well under the ±10%-relative
    // resolution floor (see doc comment), and fold it into the χ² gate.
    let start_bit = common::bit(cfg, "START");
    let start_mask = 1u16 << start_bit;
    let start_on: u64 = pads
        .iter()
        .map(|p| {
            p.segments
                .iter()
                .filter(|s| s.buttons & start_mask != 0)
                .map(|s| u64::from(s.hold_frames))
                .sum::<u64>()
        })
        .sum();
    let start_duty = start_on as f64 / total_frames as f64;
    assert!(
        start_duty < 0.01,
        "START empirical duty {start_duty:.5} >= 0.01 (configured 0.002)"
    );
    let expected_start_on = 0.002 * total_frames as f64;
    let start_mu = 2.0;
    let start_inflation = markov_variance_inflation(0.002, start_mu);
    let start_variance = total_frames as f64 * 0.002 * 0.998 * start_inflation;
    let start_z = (start_on as f64 - expected_start_on) / start_variance.sqrt();
    chi2 += start_z * start_z;

    // Formal gate: χ² across {A,B,Y,START} at p < 0.001, dof = 4.
    let critical = chi2_critical(4, 0.001);
    assert!(
        chi2 < critical,
        "per-button duty χ² = {chi2:.3} exceeds the p<0.001 critical value \
         {critical:.3} (dof=4) — generator duty math is off"
    );
}

// ---------------------------------------------------------------------
// (b) Hold-duration distribution per button.
// ---------------------------------------------------------------------

/// Hold-duration histogram per button (A, B, Y) vs `Geometric(1/mu_b)`: χ²
/// fail at p < 0.001, plus empirical mean hold within ±10% of `mu_b`.
///
/// Measurement note (per the task): a button's hold is the count of
/// consecutive frames with the bit set, merging across segment boundaries
/// caused by *other* buttons changing — `interior_runs` reconstructs
/// exactly that by keying on `buttons & mask != 0` and merging adjacent
/// segments with the same key. Runs touching the burst boundary are
/// excluded (a hold truncated by burst end biases the mean down).
#[test]
fn hold_duration_matches_geometric() {
    let corpus = large_corpus();
    let pads = pads(corpus);

    for (name, bit, mu) in [("A", 0u8, 18.0_f64), ("B", 1u8, 60.0), ("Y", 3u8, 30.0)] {
        let mask = 1u16 << bit;
        let durations: Vec<u64> = pads
            .iter()
            .flat_map(|p| interior_runs(p, |b| b & mask != 0))
            .filter(|&(on, _)| on)
            .map(|(_, len)| len)
            .collect();
        assert!(
            durations.len() > 200,
            "{name}: too few interior hold runs ({}) to test — check N/duty",
            durations.len()
        );

        let observed_mean = mean(&durations);
        let err = rel_err(observed_mean, mu);
        assert!(
            err < 0.10,
            "{name}: empirical mean hold {observed_mean:.2} vs mu_b {mu} \
             (rel err {err:.4} >= 0.10)"
        );

        let (chi2, dof) = geometric_chi_square(&durations, mu, 16);
        let critical = chi2_critical(dof, 0.001);
        assert!(
            chi2 < critical,
            "{name}: hold-duration χ² = {chi2:.3} exceeds p<0.001 critical \
             {critical:.3} (dof={dof}) vs Geometric(1/{mu})"
        );
    }
}

// ---------------------------------------------------------------------
// (c) Direction segment length + sticky-repeat rate.
// ---------------------------------------------------------------------

/// Direction segment mean length + sticky-repeat rate (ARCHITECTURE.md
/// §4.3): stickiness merges consecutive same-direction *segments* into one
/// longer visible run, so the raw per-draw `Geometric(1/mu_dir)` parameter
/// (24 frames) is not directly observable from composed-burst
/// reconstruction — only the merged visible-run length is. This test:
///
/// 1. Measures the mean visible-run length (in frames) for RIGHT (p=0.55)
///    and NEUTRAL (p=0.15) via `interior_runs` keyed on direction bits, and
///    checks it against the closed-form ARCHITECTURE §4.3 expectation
///    `mu_dir / ((1-kappa)(1-p_i))` within ±10% (exactly the "frames
///    version" the task asks to assert).
/// 2. Solves the *same* closed-form relation for `kappa` given the
///    known `mu_dir=24` and measured mean run length, and asserts that
///    back-derived `kappa_hat` is within ±0.05 absolute of the configured
///    0.55 — this is the "sticky-repeat rate consistent with kappa" bullet,
///    and is mathematically the more direct read of "consistent with
///    kappa" than a bare mean-length check would be.
///
/// `diagonal_factor: 0.0` in `stats_cfg()` keeps direction values pure
/// single bits (no diagonal enrichment perturbing the categorical values).
#[test]
fn direction_segment_length_and_stickiness() {
    let cfg = large_target_cfg();
    let corpus = large_corpus();
    let pads = pads(corpus);

    let dir_mask = cfg
        .button_alphabet
        .mask(&cfg.button_alphabet.directions.group)
        .expect("direction group declared");
    let right_bit = common::bit(cfg, "RIGHT");
    let right_mask = 1u16 << right_bit;
    let mu_dir = 24.0_f64;
    let kappa = 0.55_f64;

    let all_runs: Vec<(u16, u64)> = pads
        .iter()
        .flat_map(|p| interior_runs(p, |b| b & dir_mask))
        .collect();
    assert!(
        all_runs.len() > 500,
        "too few interior direction runs ({}) to test",
        all_runs.len()
    );

    for (name, value, p_i) in [("RIGHT", right_mask, 0.55_f64), ("NEUTRAL", 0u16, 0.15_f64)] {
        let lens: Vec<u64> = all_runs
            .iter()
            .filter(|&&(v, _)| v == value)
            .map(|&(_, len)| len)
            .collect();
        assert!(
            lens.len() > 50,
            "{name}: too few visible runs ({}) to test",
            lens.len()
        );
        let observed_mean = mean(&lens);
        let expected_frames = mu_dir / ((1.0 - kappa) * (1.0 - p_i));
        let err = rel_err(observed_mean, expected_frames);
        assert!(
            err < 0.10,
            "{name}: mean visible-run length {observed_mean:.2} vs expected \
             mu_dir/((1-kappa)(1-p_i)) = {expected_frames:.2} (rel err {err:.4} >= 0.10)"
        );

        // kappa_hat solves observed_mean = mu_dir / ((1-kappa_hat)(1-p_i)).
        let kappa_hat = 1.0 - mu_dir / (observed_mean * (1.0 - p_i));
        let abs_err = (kappa_hat - kappa).abs();
        assert!(
            abs_err <= 0.05,
            "{name}: kappa_hat {kappa_hat:.4} vs configured kappa {kappa} \
             (abs err {abs_err:.4} > 0.05)"
        );
    }
}

// ---------------------------------------------------------------------
// (d) Zero illegal masks (hard assert).
// ---------------------------------------------------------------------

/// Zero illegal masks across the whole corpus: no UP+DOWN, no LEFT+RIGHT,
/// no `hold_frames == 0`, no adjacent equal masks, and totals within
/// `[min_frames, max_frames]`. Hard assert, not statistical.
#[test]
fn zero_illegal_masks_across_corpus() {
    let cfg = stats_cfg();
    let corpus = corpus();
    let pads = pads(corpus);

    let up = 1u16 << common::bit(cfg, "UP");
    let down = 1u16 << common::bit(cfg, "DOWN");
    let left = 1u16 << common::bit(cfg, "LEFT");
    let right = 1u16 << common::bit(cfg, "RIGHT");

    for (i, pad) in pads.iter().enumerate() {
        let total = pad.total_frames();
        assert!(
            total >= u64::from(cfg.burst_len.min_frames)
                && total <= u64::from(cfg.burst_len.max_frames),
            "slot {i}: total frames {total} out of [{}, {}]",
            cfg.burst_len.min_frames,
            cfg.burst_len.max_frames
        );
        let mut prev_mask: Option<u16> = None;
        for (j, seg) in pad.segments.iter().enumerate() {
            assert!(
                seg.buttons & (up | down) != (up | down),
                "slot {i} segment {j}: UP+DOWN both set ({:#x})",
                seg.buttons
            );
            assert!(
                seg.buttons & (left | right) != (left | right),
                "slot {i} segment {j}: LEFT+RIGHT both set ({:#x})",
                seg.buttons
            );
            assert!(
                seg.hold_frames != 0,
                "slot {i} segment {j}: hold_frames == 0"
            );
            if let Some(prev) = prev_mask {
                assert_ne!(
                    prev, seg.buttons,
                    "slot {i} segment {j}: adjacent equal masks ({:#x})",
                    seg.buttons
                );
            }
            prev_mask = Some(seg.buttons);
        }
    }
}

// ---------------------------------------------------------------------
// (e) Context rule test: boss_hp Y-duty shift + START refractory.
// ---------------------------------------------------------------------

/// `boss_hp > 0` rule (API.md §5.5, `adjust_buttons: {Y: +1.2}`): Y's
/// empirical duty WITH context should sit within ±10% of
/// `sigma(logit(0.15) + 1.2)`, and clearly above the context-free duty.
#[test]
fn context_rule_boss_hp_shifts_y_duty() {
    let cfg = common::context_rule_yaml(0.0);
    let n = N;
    let ctx_free = common::ctx_free("ctx-node");
    let ctx_boss = common::ctx_with_boss_hp("ctx-node", 100.0);

    let (free_results, _) = propose(
        &cfg,
        &ctx_free,
        n,
        LENGTH_HINT,
        BASE_SEED ^ 0xA1,
        Availability::default(),
        None,
    );
    let (boss_results, _) = propose(
        &cfg,
        &ctx_boss,
        n,
        LENGTH_HINT,
        BASE_SEED ^ 0xA1,
        Availability::default(),
        None,
    );

    let y_bit = common::bit(&cfg, "Y");
    let y_mask = 1u16 << y_bit;
    let y_duty = |results: &[SlotResult]| -> f64 {
        let pads = pads(results);
        let total: u64 = pads.iter().map(|p| p.total_frames()).sum();
        let on: u64 = pads
            .iter()
            .map(|p| {
                p.segments
                    .iter()
                    .filter(|s| s.buttons & y_mask != 0)
                    .map(|s| u64::from(s.hold_frames))
                    .sum::<u64>()
            })
            .sum();
        on as f64 / total as f64
    };

    let free_duty = y_duty(&free_results);
    let boss_duty = y_duty(&boss_results);
    let expected = fmath::sigmoid(fmath::logit(0.15) + 1.2);

    let err = rel_err(boss_duty, expected);
    assert!(
        err < 0.10,
        "Y duty with boss_hp context {boss_duty:.4} vs expected \
         sigma(logit(0.15)+1.2) = {expected:.4} (rel err {err:.4} >= 0.10)"
    );
    assert!(
        boss_duty > free_duty,
        "boss_hp context should raise Y duty: free={free_duty:.4} boss={boss_duty:.4}"
    );
}

/// START refractory (120 frames, logit_penalty 4.0): with a history showing
/// a START press, START's press rate must be < 10% of its unconditioned
/// rate. Deviation (documented, per the task): this sub-test uses START
/// `{duty: 0.05, mean_hold_frames: 2}` (see `common::context_rule_yaml`)
/// instead of the API.md default `{duty: 0.002}` so enough presses occur at
/// N=2000 to compare rates meaningfully; the refractory *mechanism* under
/// test is unaffected by the specific duty value.
#[test]
fn context_rule_start_refractory_suppresses_presses() {
    let cfg = common::context_rule_yaml(0.0);
    let n = N;
    let ctx_free = common::ctx_free("refractory-node");
    let ctx_start = common::ctx_with_start_press("refractory-node");

    let (free_results, _) = propose(
        &cfg,
        &ctx_free,
        n,
        LENGTH_HINT,
        BASE_SEED ^ 0xB2,
        Availability::default(),
        None,
    );
    let (cond_results, _) = propose(
        &cfg,
        &ctx_start,
        n,
        LENGTH_HINT,
        BASE_SEED ^ 0xB2,
        Availability::default(),
        None,
    );

    let start_bit = common::bit(&cfg, "START");
    let start_mask = 1u16 << start_bit;
    // Press count = number of ON runs for the bit (rising edges, including
    // a hold from frame 0, matching context.rs's own press semantics).
    let press_rate = |results: &[SlotResult]| -> f64 {
        let pads = pads(results);
        let total_frames: u64 = pads.iter().map(|p| p.total_frames()).sum();
        let presses: usize = pads
            .iter()
            .map(|p| {
                value_runs(p, |b| b & start_mask != 0)
                    .into_iter()
                    .filter(|&(on, _, _)| on)
                    .count()
            })
            .sum();
        presses as f64 / total_frames as f64
    };

    let free_rate = press_rate(&free_results);
    let cond_rate = press_rate(&cond_results);
    assert!(
        free_rate > 0.0,
        "unconditioned START press rate is zero — test can't measure suppression"
    );
    assert!(
        cond_rate < 0.10 * free_rate,
        "START press rate with refractory-triggering history ({cond_rate:.6}) \
         is not < 10% of unconditioned rate ({free_rate:.6})"
    );
}

// ---------------------------------------------------------------------
// (f) Length distribution.
// ---------------------------------------------------------------------

/// Empirical MEDIAN of total frames within ±10% of `length_hint=300`.
#[test]
fn length_median_within_tolerance() {
    let corpus = corpus();
    let mut totals: Vec<u64> = pads(corpus).iter().map(|p| p.total_frames()).collect();
    totals.sort_unstable();
    let mid = totals.len() / 2;
    let median = if totals.len().is_multiple_of(2) {
        (totals[mid - 1] + totals[mid]) as f64 / 2.0
    } else {
        totals[mid] as f64
    };
    let err = rel_err(median, f64::from(LENGTH_HINT));
    assert!(
        err < 0.10,
        "median total frames {median} vs length_hint {LENGTH_HINT} (rel err {err:.4} >= 0.10)"
    );
}

// ---------------------------------------------------------------------
// Context-free operation.
// ---------------------------------------------------------------------

/// M1 accept: "Context-free operation: same suite passes with `NodeContext`
/// containing only `node_id`." All of (a),(b),(c),(d),(f) above already run
/// against `corpus()`, which is generated with `common::ctx_free` (a
/// `GenContext` whose only populated field is `node_id`) — so context-free
/// operation is exercised by construction, not as a separate duplicate
/// suite. This test just pins that fact so it can't silently drift.
#[test]
fn base_corpus_is_context_free() {
    let ctx = common::ctx_free("stats-node");
    assert!(ctx.ram_features.is_empty());
    assert!(ctx.recent_inputs.is_none());
}
