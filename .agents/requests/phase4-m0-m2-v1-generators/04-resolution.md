# Resolution — Phase 4 M0–M2 v1 Generators (+M3)

Filed 2026-07-12 by the executing agent. Plan:
`.agents/plans/phase4-m0-m2-v1-generators/` (reviewed by two independent
subagent reviews before execution; review deltas in commit `fccfdbf`).

## Git SHAs per milestone (branch `phase4-v1-generators` = local `main`)

| Milestone | Commits | Landed |
|---|---|---|
| Plan + reviews | `466ab6d`, `fccfdbf` | plan files, review fixes |
| M0 | `b3447c3`, `e6487c6` | crate reshape, RNG fan-out, config, pad model, dual-arch CI |
| M1 | `de69324`, `ae5a3d4`, `f2f9eba` | weighted-random, mixer, gRPC shell, goldens, statistical suite |
| M2 | `83016c2`, `eaa7ea9` | macro engine, demo pack, server wiring |
| v1 gate | `1b374da` | Dockerfile/build script, smoke evidence, vendored harness |
| M3 (stretch) | `ab36bb4` | mutation generator + provenance replay |
| Post-implementation review round | `c9851c3` | dual code-review findings applied (see below); `SYNTH_VERSION` 0.1.0 → 0.2.0 |

## Beads (all closed with `-r` evidence)

`input-synthesizer-dax` (seed-rule doc fix), `-cvt` (owner-plan header fix),
`-16k` (proto audit), `-bh8` (M0), `-20a` (M1), `-hma` (M2), `-2g1` (v1
gate), `-1o6` (M3). `bd list` in-repo for details.

## Proto audit

`docs/proto-audit.md`: **zero divergences.** The consumed
`determinism-proto` crate copy is byte-identical to tag `proto-v0.2.0`
(= `1a9fb94`, publishing commit `261141b` is an ancestor) and matches API.md
§1–§2 field-for-field (full table in the doc). CI pins the control-plane
checkout at `ref: proto-v0.2.0`. No change requests to control-plane needed.

## Seed-rule reconciliation (bead `isj`)

INTEGRATION.md "(2) Seed derivation" replaced: the stale
`blake3(experiment_seed ‖ node_id ‖ expansion_counter)[..8]` is now the
orchestrator's implemented rule — `derive_synth_request_seed` = first
`next_u64()` from `DeterministicRng::synth(experiment_seed, batch_seq)`
(`orch-core/src/rng.rs:156`), with the note that node ids enter only our
`fanout_root` as defense in depth. Commented on
`exploration-orchestrator-isj` (2026-07-12) ready-to-close; **they close it.**
The boundary held: our `fanout_root`/`stream` fan-out (ARCHITECTURE §7.1) is
unchanged, and the v1 smoke derived its request seeds via their function.

## Owner-plan numbering doc fix

IMPLEMENTATION-PLAN.md header corrected from "first-boss … needs **M1–M3** …
that is v1" to "needs **M1–M2** plus the deployment artifact … matching the
v1-gate line under M2", with mutation (M3) noted as following immediately and
required before Phase 5's gate run. The correction and its rationale are
recorded as an HTML comment in the doc itself (dated 2026-07-12).

## Phase 4 exit gate 4 evidence

- **(a) identical bursts for identical (request, seed) across x86_64 and
  aarch64** — golden replay suites over canonical domain hashes:
  `synth-core rng::tests::stream_golden_vectors`,
  `synth-gen golden_seed` (50 M1 cases), `golden_macro` (8 M2 cases),
  `golden_mutation` (12 M3 cases), all with committed fixtures under
  `testdata/`. CI runs every suite on both matrix arms (`ubuntu-latest`
  x86_64 + `ubuntu-24.04-arm` aarch64): run
  https://github.com/preestablished/input-synthesizer/actions/runs/29181386490
  (branch `phase4-v1-generators`, commit `ab36bb4`) — **green on both arms**.
- **(b) χ²/KS distribution tests green** —
  `synth-gen tests/statistical_suite.rs`: per-button duty χ² (Markov
  variance-inflated), hold durations vs Geometric(1/μ) (χ² + mean ±10%),
  direction run lengths + back-solved κ ±0.05, context rule
  σ(logit(π)+1.2) ±10%, START refractory <10%, length median ±10%, zero
  illegal masks (hard assert). Fixed seeds; N=2000 (+300×8000-frame corpus
  for run-length tests).
- **(c) macro packs load and instantiate** —
  `macro_suite::demo_pack_loads_and_every_macro_instantiates` (all 10 demo
  macros), pack-load/violation/reload-noop tests, instantiation goldens,
  end-to-end 16/16 mix with `MacroProvenance` on every macro slot; plus the
  live smoke's 16,000 macro-slot bursts.

## Both-arch CI

Dual-arch matrix (hosted `ubuntu-24.04-arm` — the pattern the sibling repos
use), clippy `-D warnings` with `disallowed-types` HashMap/HashSet deny
(verified to fail a synthetic violation), and the golden↔version gate
(`ci/check-golden-version.sh`; both fail and pass legs exercised on a scratch
branch; reproduction one-liner in the script header). The work was pushed as
branch `phase4-v1-generators` (pushing `main` requires operator approval);
CI run 29181386490 on commit `ab36bb4` completed **success on both arches**
(`rust (x86_64)` ✓, `rust (aarch64)` ✓; bench jobs ✓) — no aarch64 pending
debt. Fast-forwarding `main` to the branch is left to the operator.

## v1 gate evidence

`docs/evidence/v1-smoke-2026-07-12.md` + transcript. Summary: image
`sha256:81400279ce70…` (from `eaa7ea9` + control-plane `proto-v0.2.0` via
`scripts/build-image.sh`); 1,000 consecutive `ProposeBursts` (k=32) against
the running container, driven through **exploration-orchestrator's own
driver layer** (`GeneratedInputSynthClient`, `SynthBringup`, their seed rule,
their per-batch fingerprint guard) in the acceptance-defined context-free
fallback mode: **0 errors, 32,000 bursts, 0 illegal bursts (client-side
validation), fingerprint stable across all calls**. Harness vendored at
`scripts/smoke-harness/`.

**Open items (named):**
1. **Live-context smoke rerun** — the single named open item per the
   request; unblocked by reference-workload's corpus fulfillment
   (`refwork-czi` / `refwork-5tk`).
2. The same rerun should be driven through the orchestrator's **served**
   dev loop with the **hypervisor-side** illegal-burst counter once their
   async transport adapter lands (`exploration-orchestrator-cww` — their
   half of this wiring). The Phase 3 stack was verified running on this
   host during the smoke.

## M3 status

**Done** (stretch landed after the v1 gate was recorded): all seven §5.2
operators, per-op goldens, operator-frequency χ² + mean 1.75±0.05 over
10,000 mutants, legality/bounds property tests, degradation path, and the
**provenance-replay test** (200 seeded cases: re-applying the recorded op
list with recorded args to the recorded base reproduces the mutant exactly).
One spec corner made explicit: ARCHITECTURE §5.2 accepts a still-identical
mutant after the single forced retry; the retry uses its own stream label
`slot/{s}/mut/retry` and is recorded in provenance.

## Offers / seams raised

- **Hypervisor contract-test fixtures** (risk-table item): burst fixtures
  under `testdata/golden/m1/goldens.yaml` (+ the recorder tests) are offered
  to determinism-hypervisor for their burst→input-log contract test; any
  case's `(request, burst_id)` pairs regenerate deterministically from the
  committed configs.
- **Pack-reference seam**: the orchestrator's bring-up validates
  `macro.packs` entries against `Health.loaded_packs`, which carries
  content-hash **ids** — configs consumed by the orchestrator must reference
  packs by id (API.md §5.6 allows names too; our resolver accepts both).
  Raised here rather than absorbed (per the choreography note about doc
  drift working both ways).

## Post-implementation code-review round (`c9851c3`)

Two independent subagent reviews of the full `0aa6b34..HEAD` delta — a
spec-fidelity audit against ARCHITECTURE/API/IMPLEMENTATION-PLAN and a
correctness bug hunt. 16 of 18 findings applied, notably: two
wire-reachable panics fixed (all-unavailable generator mix now degrades to
weighted-random per INTEGRATION §7; `mutation.op_probs` keys validated);
two fingerprint-integrity holes closed (single-guard state snapshot in
ProposeBursts; cross-pack macro shadowing resolved by the fingerprinted
`macro.packs` list order instead of unfingerprinted load order);
multi-splice provenance replay resolves each splice's own recorded donor;
resource bounds (`max_frames` ≤ 216000, `ops_binomial.n` ≤ 64), i128
int-param spans, finite-float validation, denormal-duty handling, mixer
floor-sum guard, HTTP read timeout, unconditional pack-presence
FAILED_PRECONDITION, k=256/257 boundary tests. Provenance `rng_stream` now
carries canonical §7.2 labels — a format change, so `SYNTH_VERSION` bumped
to 0.2.0 with goldens regenerated (m1 burst_ids verified byte-identical).
CI run 29195495676 on `c9851c3`: green on both arches.
ARCHITECTURE §7.2 gained the `slot/{s}/mut/retry` row (spec gap, dated
comment). Declined with rationale: `burst_hash` input composition differs
from API §1's literal wording (ids are opaque, internally consistent, and
pinned by goldens — changing would break every golden for doc literalism;
documented in `types.rs` instead); line/column on post-parse semantic
validation errors (structurally unavailable after deserialization — parse
errors do carry them).

## Review round 2 (`9353272`)

Two further subagent reviews: an adversarial verification of every fix in
`c9851c3` (9/10 confirmed outright; all gates re-run independently) and an
ops-artifacts + claims audit (every checkable claim in this document and the
evidence doc verified true against git/tests/docker/gh). Applied from their
findings: a pack **name** now owns exactly one document (loading different
content under an existing name replaces the old pack and removes its
`pack_id` from the fingerprint input — closing the last load-order-dependent
resolution path); API.md §3's "latest load wins" line corrected; bench CI
job no longer hides compile errors behind `|| true`; builder image pinned to
`rust:1.97-slim-bookworm`; `/opt/synth/packs` documented; golden-gate script
header updated. The version-skew the audit flagged (smoke ran against the
pre-review 0.1.0 image) was closed by rebuilding from `9353272` and
re-running the 1,000-call smoke: 0 errors, 0 illegal bursts,
`synth_version 0.2.0` (evidence doc, rerun section). Workspace at 106 tests
/ 21 suites green.

## Verification pointers

Clean checkout at `9353272` (final SHA incl. both review rounds): `cargo test
--workspace` (106 tests / 21
suites; needs the sibling `control-plane` checkout at `proto-v0.2.0`);
golden recorder modes are `--ignored` tests; the CI golden↔version rule's
synthetic-violation reproduction is documented in
`ci/check-golden-version.sh`'s header; image reproducible via
`scripts/build-image.sh` (SHAs printed at build time).
