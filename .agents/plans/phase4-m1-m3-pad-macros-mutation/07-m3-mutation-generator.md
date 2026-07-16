# WP7 — M3: Mutation Generator

**Goal:** M3 complete per IMPLEMENTATION-PLAN §M3 (owner of the acceptance
list): the seven operators of ARCHITECTURE §5.2, base/donor selection,
op-count distribution, post-clamp, identical-mutant retry,
`MutationProvenance`, the mixer degradation path when context lacks
parent/siblings, the provenance-replay test, and the χ² operator-frequency
test.

Depends on: WP6. **Stretch within Phase 4; hard requirement before Phase 5's
gate run** (phase doc). If Phase 4 closes without it, WP8 ships the v1 gate
on M1+M2 and this package heads the Phase 5 queue.

## 1. `synth-gen/src/mutation.rs`

Inputs (from `NodeContext`): `parent_burst` and `sibling_bursts` (each a
`ProvenancedBurst`/`ScoredBurst` — convert to domain bursts at the boundary;
their `burst_id` bytes are the provenance base/donor ids). Neither present ⇒
generator **unavailable**: mixer reallocates weight, response `degraded`
carries `GeneratorKind::Mutation` with reason `"no_parent_burst"` (API.md
§2.1 reason string).

- Base selection, stream `"slot/{s}/mut/ops"` (shared with op count/choice —
  document exact draw order: base draw, then donor-bias draw, then n_ops,
  then op choices): with prob `mutation.donor_bias` (default 0.5, iff
  siblings exist) pick a sibling ∝ `max(score_delta, ε)` (pin ε = 1e-6 in
  code docs + config docs), else `parent_burst`. Record
  `base_was_sibling`. Two points here are **spec-underdetermined** and each
  needs a documentation issue filed alongside the local pin (write notes at
  `~/.agents/projects/determinism/reviews/doc-issues-inputsynth-mutation-base-selection-stream.md`
  and `...-inputsynth-mutation-donor-epsilon.md`, following the
  `doc-issues-refwork-*.md` precedent in that directory; list both in the
  WP8 evidence bundle):
  1. assigning the base/donor-selection RNG draws to `"slot/{s}/mut/ops"` —
     ARCHITECTURE §7.2's label table has no base-selection stream, so this
     is a local reading, pinned by the documented draw order + goldens;
  2. the ε = 1e-6 floor in donor weighting — a locally chosen constant, not
     a spec value.
- Op count: `n_ops = 1 + min(B, 3)`, `B ~ Binomial(3, 0.25)` — 1..=4 ops,
  mean 1.75. Ops drawn i.i.d. from `mutation.op_probs` (defaults §5.7).
- Seven operators, each a pure function
  `(burst, args_rng: stream("slot/{s}/mut/op/{i}")) -> burst` that ALSO
  emits its sampled arguments as the stringified `MutationOp.args` map —
  **args must fully determine the transformation** (replay requirement):
  - `perturb_timing` (0.25): per segment w.p. 0.5, `d' = max(1,
    round(d·exp(σ_t·z)))`, σ_t = `mutation.timing_sigma`; args record which
    segments and each z (or equivalently each d').
  - `extend` (0.20): append `m ~ 1+Geometric(0.25)` WR segments conditioned
    on final mask.
  - `flip_button` (0.15): `m ~ 1+Geometric(0.5)` toggles; uniform segment,
    uniform non-direction bit; direction bits re-roll the direction
    categorical for that segment.
  - `splice` (0.15): cut i in base, j in donor (uniform over segment
    boundaries), `base[..i] ++ donor[j..]`; donor sampled like base
    selection from siblings; **no donor ⇒ fall back to `extend`, recorded
    as `extend`** in provenance.
  - `truncate` (0.10): drop trailing `u ~ U(0.1, 0.5)` fraction of frames
    (shorten a mid-cut segment).
  - `duplicate_segment` (0.10): uniform run of 1–3 segments repeated
    k ∈ {1,2,3} uniform, in place.
  - `swap_adjacent` (0.05): swap segments at a uniform boundary.
- After ops: re-clamp to `[L_min, L_max]` — recorded ONLY as
  `post_clamp: true`, never as an extra op; `legalize`; if
  `burst_hash == base hash` force one extra `perturb_timing` pass (at most
  one retry, then accept). The retry pass IS recorded in `ops` (it is an
  applied op — replay must reproduce it; use the next `"slot/{s}/mut/op/{i}"`
  stream index).
- `MutationProvenance{base_burst_id, donor_burst_id (empty if none),
  base_was_sibling, ops, post_clamp}` on `Provenance.mutation`.
- Replay entry point (public, used by the replay test and later by mining/
  credit tooling): `apply_recorded_ops(base: &Burst, donor: Option<&Burst>,
  ops: &[MutationOp], cfg) -> Burst` — consumes NO RNG; every needed draw
  comes from parsed `args`.

## 2. Tests

- Per-operator unit goldens (fixed seed ⇒ exact output burst), one test per
  op: `op_perturb_timing_golden`, `op_extend_golden`, `op_flip_button_golden`,
  `op_splice_golden`, `op_truncate_golden`, `op_duplicate_segment_golden`,
  `op_swap_adjacent_golden`.
- `op_frequency_chi2_10000_mutants` — operator frequency over 10 000 mutants
  matches `op_probs` (χ² sized at p < 0.001); mean ops/mutant
  = 1.75 ± 0.05. Fixed seed.
- Property tests (proptest): `mutants_always_legal_and_length_bounded`,
  `mutant_hash_differs_from_base_after_retry`,
  `splice_without_donor_records_extend`.
- `provenance_replay_reproduces_mutant_exactly` — for a corpus of ≥100
  seeded mutants: re-applying the recorded op list with recorded args to the
  recorded base (+donor) via `apply_recorded_ops` reproduces the mutant
  byte-identically. This is the strongest acceptance item; if it fails, the
  args schema is incomplete — fix args capture, never the test.
- `degraded_without_parent_or_siblings` — request with mutation weight but
  bare context ⇒ exactly-k bursts still returned, mutation weight
  reallocated, `degraded` contains `(MUTATION, "no_parent_burst")`; metric
  `synth_generator_unavailable_total` increments.
- End-to-end goldens: ≥5 fixtures in `testdata/golden/m3/` with full
  three-generator mix and context carrying parent + siblings; replay harness
  from WP5.

## Acceptance criteria

- All tests above green on both CI legs; clippy/fmt clean; goldens + version
  bump (`0.4.0`) in the same PR per the WP2 gate. Same golden cascade as
  WP6: the bump regenerates ALL prior goldens via the fingerprint's
  `synth_version` component — regenerate + hand-review in the same commit;
  `burst_id` lists must not change.
- Stream-label audit extended to `"slot/{s}/mut/ops"` / `"slot/{s}/mut/op/{i}"`
  usage.
- M3 bead closed with `-r`: test list, χ² numbers, replay-corpus size.

## Failure guidance

- Replay mismatches almost always mean an op consulted its RNG for something
  not persisted in `args` (e.g. legalize-order effects or a resample hidden
  inside a helper). Make every draw explicit at the op boundary and
  stringify it; re-derive the args schema per op before touching goldens.
- χ² failure with correct-looking code: check that the op *chosen* is
  recorded even when the op was a no-op on a degenerate burst (e.g.
  swap_adjacent on a 1-segment burst) — define and document the degenerate
  behavior (no-op but still recorded) so frequency counts stay honest.
- Stringified float args: pin a formatting (e.g. Rust `{:?}` on f64, or hex
  bits) and round-trip-test it — `to_string`/parse drift is a silent replay
  killer.
