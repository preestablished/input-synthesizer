# WP4 — M1 (part 1): Weighted-Random Generator + Statistical Suite

**Goal:** the weighted-random pad generator per ARCHITECTURE §4, mathematically
correct and proven by the seeded statistical suite from IMPLEMENTATION-PLAN
§M1 (the owner of the acceptance list; headlines restated below).

Depends on: WP3. Blocks WP5.

## 1. `synth-gen` — new crate

- `crates/synth-gen/{Cargo.toml, src/lib.rs, src/weighted_random.rs}`
  (ARCHITECTURE §1 layout; `mixer.rs` arrives in WP5, `macros.rs` WP6,
  `mutation.rs` WP7). Deps: `synth-core`, `synth-pad`, `synth-rng`,
  `synth-proto`. Pure crate; `#![forbid(unsafe_code)]`,
  `#![deny(clippy::disallowed_types)]`. `statrs` as dev-dependency only.
- Public entry:
  `weighted_random::generate(cfg: &EffectiveConfig, ctx: &ContextView,
  slot: u32, target_len: u32, root: &[u8; 32]) -> synth_core::Burst`
  — pure function of its arguments; all randomness via
  `synth_rng::stream(root, label)` with the §7.2 labels. `EffectiveConfig`
  and `ContextView` are **new types this package introduces** (WP3 defines
  `ExperimentConfig`, not these): `EffectiveConfig` is the post-`deep_merge`,
  validated `ExperimentConfig` (a newtype or alias over WP3's type is fine),
  and `ContextView` is a borrowed, domain-typed view over the request's
  `NodeContext` (ram_features, recent_inputs, parent/siblings) built at the
  server boundary.

## 2. Sampler mechanics (ARCHITECTURE §4.2–§4.5 — normative math)

- Per-button two-state chain, run-length sampled (O(#segments)):
  `r_b = 1/μ_b`, `a_b = π_b/(μ_b(1−π_b))`; gap/hold durations via geometric
  inverse-CDF `d = max(1, ceil(ln(1−u)/ln(1−p)))`, `p=1 ⇒ d=1`; `f64` math
  only, canonical `U[0,1)` from the stream. One stream per button:
  `"slot/{s}/wr/btn/{bit}"` (adding a button never shifts another's draws).
- Initial state per button: Bernoulli(π_b) stationary start, UNLESS
  `ctx.recent_inputs` present and `start_from_history: true` → continue the
  last frame's state (temporal coherence).
- Direction process on `"slot/{s}/wr/dir"`: sticky semi-Markov categorical —
  prior from config, segment length `Geometric(1/μ_dir)`, keep current with
  probability κ else resample (self-transitions allowed); diagonals per
  `allow_diagonals` + `diagonal_factor`.
- Context conditioning (§4.4): logit adjustments only.
  `π'_b = σ(logit(π_b) + Σ 1[pred_j] w_{j,b})`, then re-derive `a_b`;
  direction prior `p'_i ∝ p_i·exp(Σ ...)` renormalized. Predicate language:
  `{feature, op ∈ {lt,le,gt,ge,eq,ne}, value}` over `ram_features` +
  history predicate `pressed_within{button, frames}`. Missing feature ⇒
  false. Refractory sugar: press of `button` within last `R` frames of
  `recent_inputs` ⇒ `−λ` on its duty logit. **A generator must never fail
  because context is missing** — absent context ⇒ zero adjustments.
- Length (§4.5) on `"slot/{s}/len"`: `L = clamp(round(exp(N(ln ℓ, σ_L²))),
  L_min, L_max)`, `ℓ = length_hint` if nonzero else `burst_len.mean_frames`.
- Assemble: run chains + direction to change-points, cut at `L`, merge equal
  adjacent masks, `PadModel::legalize`.
- Float discipline (§7.3): if the lognormal/`ln`/`exp` path produces
  different bits across arches in the WP5 cross-arch golden run, route
  transcendentals through the pure-Rust `libm` crate — that decision point is
  called out in IMPLEMENTATION-PLAN's risk table and belongs to this package.

## 3. Statistical test suite (fixed seeds — deterministic, never flaky)

`crates/synth-gen/tests/wr_stats.rs`, N = 2 000 bursts (≈600k frames), demo
config from API.md §5, one fixed master seed. Sizing note per
IMPLEMENTATION-PLAN testing strategy: thresholds are for one-time sizing;
the committed assertions are on the deterministic sample. Exact test names:

- `duty_cycle_within_10pct_chi2` — per-button empirical duty vs configured
  `π_b`, ±10% relative (χ² sized at p < 0.001).
- `hold_durations_geometric_ks` — per-button hold histogram vs
  `Geometric(1/μ_b)` (discrete KS/χ²); empirical mean hold within ±10% of μ_b.
- `direction_segments_mean_and_stickiness` — mean direction-segment length
  ±10% of μ_dir; sticky-repeat rate within κ ± 0.05 absolute.
- `zero_illegal_masks_hard_assert` — no exclusive-group violation anywhere in
  the corpus (hard assert, not statistical).
- `context_rule_shifts_y_duty` — with the §5.5 `boss_hp > 0` rule, Y-duty
  rises to `σ(logit(π_Y)+1.2)` ± 10%.
- `start_refractory_suppresses_start` — history containing START within 120
  frames ⇒ START press rate < 10% of its unconditioned rate.
- `length_median_tracks_hint` — empirical median within ±10% of
  `length_hint`.
- `context_free_suite_passes` — rerun of duty/hold/direction/length
  assertions with `NodeContext` = node_id only (M1 accept: context-free
  operation).
- `history_continuation_holds_across_boundary` — recent_inputs ending
  mid-hold of RIGHT ⇒ first frame continues RIGHT (unit, not statistical).

## Acceptance criteria

- All tests above green via `cargo test -p synth-gen`; suite runtime sane
  (< ~60 s — run-length sampling makes 600k frames cheap).
- Same suite green on the aarch64 leg (or WP2 fallback lane). As with WP3,
  WP2 is a predecessor of the **acceptance-close** only: if the arm leg does
  not exist yet, this bullet is satisfied by the first dual-leg run after
  WP2 lands, cited in the bead close.
- Stream-label audit: grep the crate for `stream(` call sites and check each
  label against ARCHITECTURE §7.2's table; record the audit in the bead.
- Clippy/fmt clean; CI green; bead closed with `-r` naming the test list.

## Failure guidance

- A statistical test failing marginally at the fixed seed means the sizing is
  wrong or the math is wrong — re-derive `a_b`/`r_b` by hand against §4.2
  before touching tolerances. Tolerances come from the owner doc; do not
  widen them to pass.
- Cross-arch mismatch in this package's outputs: bisect with the WP3 rng
  goldens first (integer path) — if those pass, the divergence is libm;
  switch to the `libm` crate for `ln`/`exp` and note the `synth_version`
  implication (goldens recorded in WP5 must be generated AFTER this
  decision).
- If duty and hold cannot both hit ±10% simultaneously, the duty→(a,r)
  derivation is inverted somewhere; the boundary case `a_b = 1` (gaps of
  exactly 1) is a good unit probe.
