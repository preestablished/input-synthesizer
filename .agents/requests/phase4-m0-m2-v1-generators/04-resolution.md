# Resolution — Phase 4 M0–M2 v1 Generators (+M3)

**Final state (read this first):** branch `phase4-v1-generators` HEAD;
`cargo test --workspace` = 123 tests / 22 suites green; authoritative
dual-arch CI evidence = the newest green run on the branch (every push
triggers one; a documentation-only commit necessarily trails its own run
by one — check `gh run list` for the head's conclusion). Per-round map
(every run success on both arches); per-round test counts in the round
sections are scoped to their round's SHA, not the final tree:

| Round | SHA | CI run (id under https://github.com/preestablished/input-synthesizer/actions/runs/) | One-line outcome |
|---|---|---|---|
| — (v1 handback) | `ab36bb4` | 29181386490 | M0–M3 + v1 gate landed |
| 1 | `c9851c3` | 29195495676 | 2 wire panics + 2 fingerprint holes fixed; 0.2.0 |
| 2 | `9353272` | 29197027014 (on `a9197dd`) | pack-name determinism; ops hardening; smoke rerun |
| 3 | `6d41748` | 29198388598 | id-first pack lookup; clean verifier dry-run |
| 4 | `ab1e528` | 29199743592 | Health.loaded_packs names+ids (consumer blocker) |
| 5 | `ee17797` | 29200931736 | raw-segment contract fixtures; pad-table casing |
| 6 | `e096e47` | 29202385874 | handback coherence; fixture self-containment |
| 7 | `498bc6c` | 29204053262 | quadratic composition fixed; request budgets |
| 8 | `6ebf5a5` | 29205449380 | differential proof; final-code smoke |
| 9 | `b96f56b` | 29206606422 | sign-off simulation approved; citation fixes |
| 10 | `00efa09` | 29207905993 | cold-pass bug: flip_button prior normalization |
| 11 | `f9e2479` | 29209111619 | post_clamp retry re-clamp fix; spec pins |
| 12 | `8d88dbb` | 29210300455 | generator_mix zero-fill; §5 addendum pinned |
| 13 | `5b3f02d` | 29211515814 | schema-wide deny_unknown_fields; spec coverage complete |
| 14 | `7dc73d2` | 29212707663 | cross-repo config blocker raised (their `kk2`) |
| 15 | `7c96957` | 29213744050 | navigational fixes from artifact + reader audits |
| 16 | `5b931b5` | 29214780991 | closure audits clean; refwork-czi freshness folded in |
| 17 | `60a402f` | 29215956272 | recorder idempotency verified; reference-integrity nits |
| 18 | `387d2c1` | 29217243467 | merge preflight safe; release/flakiness runs clean |
| 19 | `20fb957` | 29218461581 | onboarding gaps fixed (CLAUDE.md, fmath lint, README) |
| 20 | `fc28e3a` | 29219744688 | onboarding fixes measured; trap knowledge captured |
| 21 | `7dcbe87` | 29220991338 | categorical trap pinned as test; sign-of-zero captured |
| 22 | `b5203c9` | 29222193998 | behavior-freeze certified; trap-test fairness settled |
| 23 | `e753677` | 29223298498 | table/heading currency restored; freshness sweep clean |
| 24 | `d851fc4` | 29224598305 | round-23 cells verified; session memory audited + rewritten |
| 25 | `ebb72ab` | 29225744970 | round-24 verified; freshness sweep quiet |
| 26 | `5ff7cc7` | 29226951334 | round-25 verified; sweep false-alarm corrected |
| 27 | this commit | (trails by one) | round-26 verified; sweep clean with corrected counts |

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

(Bead shorthand used below: `isj` = `exploration-orchestrator-isj`,
`cww` = `exploration-orchestrator-cww`, `kk2` =
`exploration-orchestrator-kk2` — all in the orchestrator's repo.)
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
  x86_64 + `ubuntu-24.04-arm` aarch64). Authoritative evidence = the newest
  green run on the branch (full per-round run list in the Final-state block
  up top; 29205449380 on `6ebf5a5` was the newest at round-9 sign-off, both
  arms green). The first green run (29181386490 on `ab36bb4`) predates the
  0.2.0 golden regeneration and is historical only.
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
branch `phase4-v1-generators` (pushing `main` requires operator approval).
Every push to the branch ran the matrix green (full run list in the
Final-state block; round 2's commit `9353272` is covered by the run on its
immediate descendant `a9197dd` — same tree plus a doc file). No aarch64
pending debt. Fast-forwarding `main` to the branch is left to the operator.

## v1 gate evidence

`docs/evidence/v1-smoke-2026-07-12.md` + transcript. (Digest note for
every image table in this document: digests are SHA-traceable, not
bit-identical across rebuilds — see Verification pointers.) Summary: image
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
   request; unblocked by reference-workload's corpus fulfillment.
   Freshness (2026-07-13): `refwork-czi` CLOSED 2026-07-12 (exporter +
   context verification landed at their `2827665`); `refwork-5tk` (corpus
   production/freeze) remains open, behind their `refwork-20v` AND
   `refwork-d7t.1` (dependency detail surfaced round 27) — their beads
   record a 2026-07-12 operator GO decision on the launch (previously
   no-go pending approval), so the chain is actively moving.
2. The same rerun should be driven through the orchestrator's **served**
   dev loop with a consumer-side illegal-burst counter (which does not
   exist yet anywhere — see round 5). Named preconditions on their side,
   in order: `exploration-orchestrator-kk2` (their experiment-config
   fixtures fail this service's real schema — see round 14), then
   `exploration-orchestrator-cww` (their async transport adapter). The
   Phase 3 stack was verified running on this host during the smoke.

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

- **Hypervisor contract-test fixtures** (risk-table item): round 5 found the
  original offer hollow — the golden files carry canonical hashes only.
  `testdata/contract/pad-burst-fixtures.yaml` now carries RAW
  `(buttons_mask, hold_frames)` segment lists per slot (with burst_ids and
  the generating seeds/config), verified against the sampler by
  `contract_fixtures.rs` — directly consumable by a burst→input-log
  translation test. Seam facts established by the round-5 review: the
  translation actually lives in exploration-orchestrator
  (`orch-sched/src/driver.rs` `burst_events`, run-length → edge-triggered
  PAD_SET — correctly implemented), determinism-hypervisor never talks to
  this service directly, and **no consumer-side illegal-burst validation
  counter exists yet anywhere** — open item 2 therefore requires that
  counter to be BUILT (orchestrator or hypervisor side), not merely a
  window scheduled. Bit assignments verified identical across
  reference-workload / API.md §5.1; button-name casing differs
  (reference-workload mixed-case, non-aliased) — API.md §5.1 wording
  corrected; cross-repo resolution is by bit, never name.
- **Pack-reference seam — resolved producer-side in round 4**: the
  orchestrator's bring-up validates `macro.packs` entries against
  `Health.loaded_packs` by verbatim membership. `loaded_packs` originally
  carried content-hash ids only, which would have failed bring-up for every
  name-based config (the documented default style). `Health.loaded_packs`
  now reports every identifier a loaded pack answers to — pack_ids AND
  declared names (API.md §2.5 updated with a dated note); the fingerprint
  still uses pack_ids only. Name- and id-based configs both bring up
  cleanly with the orchestrator's existing check, no change needed on
  their side.

## Review round 1 (`c9851c3`) — post-implementation code review

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
/ 21 suites green at that SHA. Scoping note (round 6): the rerun image was
built from `9353272` and therefore predates rounds 3–5's code changes
(`6d41748` id-first lookup, `ab1e528` Health.loaded_packs names+ids,
`ee17797` contract fixtures); those changes are additive or off the smoke's
pack-id-based path and are covered by the unit/integration suites and CI on
the branch head, not by a further live-container rerun.

## Review round 3 (`6d41748`)

An adversarial verification of round 2 plus a full dry-run of the
phases-track verification procedure from a clean checkout (steps 1-5 of
`03-verification-offer.md`: clean-clone workspace tests 106/21 green, named
suites re-run, golden-gate fail/pass legs reproduced verbatim from the
script header's recipe, proto byte-diffs clean, seed rule doc/code agree,
image rebuilt from the clone with correct SHA traceability, all 8 beads
confirmed closed — verdict: would sign off). One residual applied: a pack
NAME may legally collide with a different pack's pack_id (the name grammar
admits 64-char hex), and the combined name-or-id registry scan resolved
such collisions by load order (proven by PoC). `PackRegistry::get` is now
id-first (ids unique by construction, names unique by replacement), making
lookup a pure function of the loaded set; regression test
`pack_id_lookup_beats_name_collision_regardless_of_load_order`.

## Review round 4 (`ab1e528`)

Two further reviews: a consumer-contract audit reading the orchestrator's
client layer (orch-clients DTOs, orch-driver validators, grpc conversions)
end-to-end against our wire behavior, and an adversarial verification of
round 3 plus the first code review of the vendored smoke harness. The
contract audit verified 14 consumer checks compatible (burst_id/fingerprint
lengths and equality, k-length responses incl. the fallback path, slot
ordering, seed echo, fallback_from=UNSPECIFIED mapping, provenance payload
shapes, mutation id round-trips, experiment-id matching, their YAML pack
parser vs our configs, deadlines) and found one real blocker — the
`Health.loaded_packs` name/id mismatch fixed above. The round-3 fix
verified fully clean (id-first lookup total, request path protected via
resolve(), no other order-dependent scans). Harness review: added the
missing forbidden-mask (START|SELECT) invariant to the smoke's client-side
legality check and clearer arg-parse errors. Known non-issues recorded:
Health.status is constant SERVING in v1 (no policy tier to degrade on);
their tonic error mapper files Unimplemented under Internal (their side,
cosmetic).

## Review round 5 (`ee17797`)

A hypervisor-seam contract review and a mathematical audit of the
statistical suites (+ adversarial verification of round 4). The math audit
confirmed every derivation CORRECT — Markov variance inflation
(1+ρ)/(1−ρ) exact, hold-bucket telescoping and dof right, Wald's identity
grounds the direction/κ closed form, context-rule and median sizing sound,
mutation multinomial exact with ~6.7σ mean margin — with design notes only
(κ̂ ±0.05 nearly subsumed by the mean-length gate; min(B,3) provably dead
since B≤3; both match the spec text as written). Round 4 verified clean.
The seam review's findings and fixes are folded into the offers section
above (real raw-segment contract fixtures; casing correction; the
validation-counter gap sharpened in open item 2).

## Review round 6 (`e096e47`)

(This section was reconstructed after the fact; round 6's record
originally lived only in the Final-state block and evidence-doc edits.) A
fixture-consumer verification and a handback coherence audit. The consumer
check independently reproduced all 12 contract-fixture burst_id hashes with
a from-scratch binary, found zero invariant violations, and judged WR-only
coverage sufficient for a translation contract test (the translator sees
segments, never the generator kind). Applied: self-contained fixture header
(bit table + invariants), clamp-edge boundary cases (slot totals hit 16 and
1800 exactly), the Final-state block, branch-head verification pointers,
round-heading SHAs, evidence scoping note, README pointers.

## Review round 7 (`498bc6c`)

A resource-exhaustion security review (calibrated to the trusted-network,
runaway-buggy-caller threat model) and an empirical stress run against the
live release binary (~1,800 calls: baseline matched the published ~1 ms
p50; 8-way concurrency with interleaved reloads had zero errors and a
stable fingerprint; 300 distinct document loads and a 500-call malformed
flood left memory flat and Health sub-millisecond). Two real findings,
both fixed:

1. **Accidentally-quadratic burst composition.** `compose_segments`
   rescanned the direction track and every button's interval list from the
   start for each cut point — invisible at demo shapes (sub-ms) but ~80 s
   for a single legal k=256 × 216000-frame request (measured). Rewritten
   as a merge-style single pass with monotone cursors; outputs are
   byte-identical (all golden suites pass unmodified) and the cap-edge
   case measured ~18× faster (k=8: 2.29 s → 0.13 s, linear per slot).
2. **Uncapped k × max_frames product.** The same request emitted a 45 MB
   response — beyond tonic's default 4 MB client decode limit (the
   orchestrator could never consume it). New per-request budget:
   k × effective `burst_len.max_frames` ≤ 600,000 frames
   (INVALID_ARGUMENT naming both knobs; ~5× the largest legitimate
   shape and keeps worst-case responses under default client limits).

Proportionate hardening from the same review: lock-poison recovery on all
five state-guard sites (a panic under a guard previously wedged every
future RPC including Health until restart; `State` has no cross-field
invariant a lost insert could corrupt); runaway-context guards
(sibling_bursts ≤ 64, total context segments ≤ 100k, both
INVALID_ARGUMENT with clear messages); a `synth_experiments_loaded` gauge
(documents accumulate for the process lifetime by design — dashboards,
not caps, per the deployment model); README operational-notes section.
Reviewed and NOT changed: no load caps / rate limiting (wrong for the
one-orchestrator deployment shape), tonic's 4 MB decode default (adequate),
serde_yaml's built-in alias-expansion cap (billion-laughs already
defended upstream), the HTTP sidecar (30 s timeout suffices).

## Review round 8 (`6ebf5a5`)

Closed the loop on round 7 with the two strongest checks available:

1. **Differential verification of the composition rewrite.** The old
   `compose_segments` algorithm was reconstructed verbatim from the diff
   and property-tested against the new cursor version: 100,000 random
   track/interval/target shapes plus 12 forced edge cases (empty tracks,
   coverage shortfalls, exact-boundary interval starts, touching
   intervals, the legal-cap stress shape) — **zero mismatches**. The
   invariant the cursors rely on (per-button intervals sorted,
   non-overlapping, never touching) was proven from `geometric() >= 1`
   across all three sampler entry points. The differential proptest is now
   PERMANENT (`weighted_random.rs` test module, +7 tests → 117 total).
2. **Third live smoke, against the actual final code.** Image rebuilt from
   the round-7 SHA (`sha256:61beac68…`), 1,000-call smoke clean (0 errors,
   0 illegal bursts, fingerprint identical to the prior 0.2.0 run —
   independent confirmation the rewrite changed no bytes), and the
   round-7 frames budget verified over the wire (live k=256 × 216000
   request rejected with the documented INVALID_ARGUMENT). Evidence doc
   updated; the round-6 scoping note is superseded.

All round-7 claims verified: budget enforced post-merge (overrides cannot
evade), default and smoke shapes unaffected, five poison-recovery sites
with no cross-field write to corrupt, sibling cap checked pre-decode.
One cosmetic fix: stray whitespace in the budget error message.

## Review round 9 (`b96f56b`)

Two closing checks. (1) **Vacuity audit of the round-8 differential
test** (the sharpest remaining risk: an agent-written test verifying an
agent-written rewrite): the in-test reference implementation was diffed
against the genuinely-removed pre-round-7 code — verbatim identical — and
mutation-tested: all three semantically-live mutants (direction-cursor
boundary, button-interval boundary, cursor advance) are killed; the one
survivor mutates provably dead code shared by both implementations
(sort+dedup makes the guarded branch unreachable), unkillable by any
differential test by construction. The test is real evidence.
(2) **Final sign-off simulation** at the branch head from a fresh clone:
117/22 tests, all eight acceptance suites green by name, every checkable
resolution claim re-verified (CI conclusions at job level, image IDs,
transcripts to the microsecond, seed-rule parity, owner-doc fixes, beads,
golden-gate fail/pass legs) — verdict: **sign off, approved as written**,
bouncing only the stale round-7/8 self-citations fixed in this commit.

## Review round 10 (`00efa09`)

A findings-ledger reconciliation (all 57 findings from rounds 1–9 verified
against the tree: 55 fully in place, 2 audit-trail-only gaps fixed here —
this round-6 section restored, and the "round vs ties-to-even" note now
pinned in ARCHITECTURE §4.5 with a dated comment) and a COLD, unprimed
RedOwl pass (no knowledge of prior rounds). The cold pass found one real
bug nine primed rounds missed: `flip_button`'s direction re-roll fed RAW
config priors to a categorical that assumes sum 1 — a valid config with
scaled priors (weights, per §4.4) silently made late-declared directions
unreachable. Fixed by normalizing (bitwise no-op for sum-1 configs — all
goldens unchanged, proven by replay) + a scaled-priors regression test.
Same-family validation gaps closed: negative `op_probs` entries rejected;
`mean_hold_frames` and `burst_len.mean_frames` bounded to [1, 216000]
(closing a geometric-inversion corner at astronomical means and the
`ln(0)` lognormal location at mean 0); non-finite sibling `score_delta`
rejected (an inf delta deterministically inverted donor selection).
Deliberate scoping pinned in API.md §2.4: proto `map<>` fields make RAW
response frames non-byte-stable across processes; the reproducibility
contract is carried by burst contents, burst_ids, and the fingerprint —
consumers must not hash raw frames. Cosmetic: context-limit error
whitespace. The cold reviewer's overall verdict: "solid, unusually
well-hardened; the wire boundary is genuinely panic-free as far as I can
trace."

## Review round 11 (`f9e2479`)

An adversarial verification of round 10 and a COLD spec-conformance diff
of macros/mutation against ARCHITECTURE §5 + API §3/§2.4 (the reviewer saw
only spec and code). Round-10 verification: CONFIRMED throughout, including
the numeric heart of the normalization claim — the default priors sum to
bit-exact 1.0 (hex-checked), and the one golden config whose sum is one ULP
short (`variant_direction_priors`, 0.9999999999999999) provably never
reaches the fixed path; its version-bump analysis concluded no bump is
warranted (bug fix to contract-violating behavior; zero golden bytes
changed) — accepted. The spec-diff verified every quantitative requirement
exact (all seven operator probabilities, n_ops formula, U(0.1,0.5),
inclusive splice boundaries, donor weighting, rounding) and found two
deviations: **D2, fixed** — `post_clamp` was computed before the first
legalize and missed a re-clamp caused by the forced dedup-retry itself
(narrow window: identical-mutant path with the base at a length bound);
now OR-ed with the retry total's bounds check, with a directed
seed-scan test. **D1, pinned as v1 scope** — pack eligibility accepts
feature predicates only, narrower than §5.1's "same predicate language as
§4.4"; ARCHITECTURE §5.1 now states the v1 narrowing with a dated comment
(implementing history predicates in packs awaits a consumer). Ambiguities
the code had already documented are now pinned in API.md: chain_n>1
provenance records element 0 with macro_frames spanning the chain (§2.4);
`no_eligible_macros` added to the degraded-reason list (§2.1). The
spec-diff's fifteen SPEC-SILENT implementation choices are recorded in its
report; none contradicts the spec.

## Review round 12 (`8d88dbb`)

Cold spec-diffs of the remaining halves plus adversarial verification of
round 11. The sampler/mixer/conditioning half (ARCHITECTURE §3.1 + §4 in
full + the §7.2 label table) came back **zero deviations** — including the
two likeliest hiding spots, the a≤1 re-derivation under context
adjustment and the diagonal-factor semantics. Round 11's post_clamp fix
was verified empirically: the directed test was cherry-picked onto the
pre-fix code and failed as predicted (a genuine falsifier), and the fix
was proven golden-neutral by inspecting the fixtures (no golden case
records a retry op). The API §5 config-schema diff found **one real
deviation, fixed**: within a PRESENT `generator_mix` map the doc says
absent keys are 0, but serde's struct-default fill leaked 0.35/0.20 into
omitted keys — `GeneratorMix` now has asymmetric semantics (whole section
absent → documented defaults; present map → omitted keys 0.0) with a
regression test; every existing config specifies all four keys, so
nothing else changed. Doc pins: API §5.2's "normalized at load" corrected
to propose-time-over-available (dated), and the full hardening-validation
addendum (accumulated over rounds 7–11: finiteness, range bounds,
op-prob key/value rules, per-request budgets) is now pinned at the end of
API §5 so the doc's validation list matches the implementation.

## Review round 13 (`5b3f02d`)

The last uncovered spec slices (ARCHITECTURE §2/§2.1/§6.2-step-1/§8 server
shell + INTEGRATION §7's failure-mode table) were cold-diffed: **fully
conforming, zero deviations** — every metric series, log field, port,
statelessness test, and applicable failure-mode row matches; the policy
and mining rows are explicit v1 stubs (hardcoded unavailable reason /
unimplemented status), never faked success. Cold spec coverage of the
entire owner documentation is now complete. Adversarial verification of
round 12 reproduced the round-11 falsifier independently and confirmed
the zero-fill fix, surfacing three follow-ups, all applied: (1) the
`deny_unknown_fields` strictness I had added to one struct in passing is
now DELIBERATE and schema-wide (17 structs; a typo'd key errors with the
field named and the valid set listed, on fresh documents and overrides —
regression-tested per section; the untagged `Predicate` enum is the one
documented exception, serde cannot deny there); (2) the hardening
addendum overclaimed "every float finite" while `policy.*` was
unvalidated — `policy.temperature` finiteness now checked and the
addendum carries the parse-only-in-v1 carve-out; (3) API §5.2's
"absent key = 0" comment now states it applies to as-authored documents
only (overrides deep-merge onto a fully-populated base — siblings are
preserved, never zeroed).

## Review round 14 (`7dc73d2`)

Two audits of the round-13 strictness change and its blast radius. The
delta verification: exactly correct 17-struct application (untagged/
transparent exclusions right), the regression guard proven non-vacuous by
mutation (removing one attribute makes it fail), JSON documents still
accepted, merge keys confirmed unsupported but unused in all three repos
(decisively: no doc note warranted), and one pre-existing serde_yaml quirk
recorded for the radar — a bare `key:` (implicit null) on a defaulted
section silently uses the default rather than erroring, in mild tension
with the typo-protection framing (wrong NAMES error; empty VALUES do not).

The cross-repo compatibility audit found the most consequential seam since
round 4: **every experiment-config document exploration-orchestrator
constructs today fails the real synth-core parser** — `button_alphabet` is
written as a scalar name (`console16-12btn-v1`) where the schema (and
API.md §5's own example, which parses and validates cleanly) requires the
full mapping; one fixture omits the field entirely. It is fully masked by
`FakeSynth`'s hand-rolled line scanner (no real deserialization), so their
tests pass while real bring-up would fail INVALID_ARGUMENT on the first
document. Verified by parsing each of their literal documents with the
unmodified parser. Raised on their repo per the choreography rule:
**bead `exploration-orchestrator-kk2`** (all eight affected sites named;
fix = emit the mapping form or add registry indirection, plus make
FakeSynth schema-faithful), wired as a BLOCKER of their `cww` transport
bead with a comment. Open item 2 (served-loop smoke) therefore now has two
named preconditions on their side: `kk2` then `cww`. Nothing changes in
this repo — the schema matches its own specification.

## Review round 15 (`7c96957`)

Two closing audits: an artifact verification of round 14 (all nine `kk2`
sites, the FakeSynth scanner mechanism, and the bring-up wiring chain
verified in their repo; the dependency direction confirmed kk2-blocks-cww;
the parse failure independently reproduced with the exact error) and a
fresh-eyes reader pass over this document as its consumer. Both flagged
the same navigational debt, applied here: the per-round changelog table
above (replacing a run-id list that had silently gone stale at round 8),
the misleading "(final)" heading labels removed from rounds 9–14, open
item 2 updated in place with the kk2→cww precondition chain, the digest
caveat forward-referenced at the first image table, the round-6 note
rephrased, and bead shorthand glossed at first use. No code changes; the
reader pass's verdict: "solid working handback ... the failure mode is
purely navigational," with the proto-audit and exit-gate sections called
exemplary.

## Review round 16 (`5b931b5`)

Two closure audits. (1) The Final-state changelog table verified
cell-for-cell: all sixteen rows' SHAs exist with matching subjects, all
fifteen CI run ids resolve to the right head SHA with success on both
arches (including round 2's documented a9197dd exception), row order
matches commit order, and the three row-less commits are doc-only. The
round-15 commit's own trailing run (29213744050) confirmed green. (2) The
plan's acceptance checklist (07-acceptance-checklist.md) walked line by
line at HEAD: every item SATISFIED with independently verified evidence,
except the one disclosed gap — the v1-gate illegal-burst count is
client-side, not the hypervisor-side counter the plan demands, which is
exactly open item 2 (kk2→cww chain), "a handback honest about its one
shortfall rather than deficient in a hidden way" per the auditor. A
freshness sweep across the sibling repos found no invalidating movement:
zero inputsynth proto drift from the tag, the Phase 3 stack still
running, kk2/cww/isj in their recorded states — and one positive update
folded into open item 1: refwork-czi closed 2026-07-12.

## Review round 17 (`60a402f`)

Two final audits. (1) **Recorder idempotency** — a property untested in
sixteen rounds: all five `--ignored` golden recorders (rng streams, m1,
m2, m3, contract fixtures) were re-run in a clean clone and every one
regenerated its committed fixture **byte-for-byte** (`git status` empty
afterward) — the record/replay twins are in sync, with no accumulated
skew. (2) A mechanical reference-integrity check of this document: every
prose cross-reference resolves to an existing section, every named file
path exists, all seventeen named tests exist in the codebase, and every
bead id carries correct repo attribution — zero dangling references. Two
nits applied: round 1's section retitled to match the changelog table's
numbering, and a note added in Verification pointers that the owner docs
live outside this repository (the link-checker itself tripped on that).

## Review round 18 (`387d2c1`)

Two operator-facing final checks, both clean, no fixes needed. (1) **Merge
preflight**: the exact remaining operator action — fast-forwarding
`origin/main` to this branch — was simulated in a scratch clone: pure
linear history confirmed (`merge-base == origin/main`), `--ff-only` merges
cleanly (143 files, no conflicts), post-merge gates green (122/22, clippy,
fmt), and the golden-gate script's documented post-merge no-op ("no range
to check", exit 0) confirmed empirically. No committed file depends on the
branch name surviving deletion (the four references are all in this
historical document). **Verdict: safe to fast-forward.** (2) **Optimized
and repeated execution**: the full suite under `--release` (opt-level 3,
last verified four code-changing rounds earlier) — 122/22 green with
goldens replaying byte-identically under optimized codegen — and a triple
consecutive debug run with identical results each time (no flakiness).

## Review round 19 (`20fb957`)

A maintainer-onboarding dry run (a fresh agent planned a realistic config
field addition end-to-end — its plan was fully correct, including
correctly rejecting the GeneratorMix custom-deserialize precedent as
inapplicable) surfaced three onboarding gaps, all fixed:

1. **CLAUDE.md was placeholder boilerplate** ("worse than absent" — empty
   template sections imply content was considered and skipped). Now
   carries the proven-necessary content: build/test commands with the
   sibling control-plane requirement, the owner-docs location (the dry
   run's single longest search), and the determinism guardrails invisible
   to a passing test suite (fmath-only, the gate covering all of
   testdata/, the invalid-fixture table coupling, canonical-form goldens,
   the stream contract).
2. **The fmath-only rule had no lint teeth** — a `f64::exp` in a sampling
   path would compile, pass clippy, and only surface as a cross-arch
   golden mismatch. `clippy.toml` now carries `disallowed-methods` for
   the transcendental f64 methods (proven to fire on a synthetic
   violation, same discipline as the M0 HashMap deny); the untouched
   workspace passes clean, confirming the convention had in fact held
   everywhere.
3. **README's "Golden ↔ version rule" heading had lost its body** (a
   round-7 insertion landed between heading and paragraph); reunited, plus
   an explicit note that the gate covers all of `testdata/`, not only
   `testdata/golden/` (a naming trap the dry run flagged).

The companion sweep verified round 18's record-only commit and found no
sibling movement dating any claim; one freshness nuance folded into open
item 1: reference-workload's beads now record a 2026-07-12 operator GO
decision on the launch.

## Review round 20 (`fc28e3a`)

Round 19 closed out with a verification and a measurement. The
verification confirmed every CLAUDE.md claim accurate against the code
(including the control-plane tag naming and the empirically re-proven
lint), with three precision items applied: AGENTS.md now points at
CLAUDE.md instead of sitting as bare boilerplate (the round-19 leftover),
clippy.toml's spec citation softened to "this repo's enforcement of the
§7.2 float discipline" (rule 3 mandates the outcome, not the mechanism),
and fmath's sqrt inconsistency resolved in the doc comment (IEEE mandates
correctly-rounded sqrt — bit-stable everywhere, hence deliberately absent
from the lint; the wrapper is stylistic).

The measurement re-ran the onboarding dry run with a fresh agent and a
harder task (a new mutation operator whose "probability 0.0" default
hides a real determinism trap). Results: the round-19 fixes worked —
"Where the real docs are" was rated the single most useful sentence, the
gate-scope bullet was directly load-bearing — and the agent independently
FOUND and correctly RESOLVED the trap (a zero-weight key appended last
silently changes the categorical's rounding fallback, `entries.last()`).
Its residual friction became this round's fixes: the categorical
last-entry trap, the `VALID_MUTATION_OPS` allow-list location, the
feature-acceptance-docs path, and the owner-doc change process are now in
CLAUDE.md (with a matching pointer comment on `op_probs` itself), and the
operator-frequency χ² test is hardened against zero-probability bins
(expected = 0 previously produced a NaN chi2 with a misleading failure
message; zero bins are now hard-asserted at zero draws and excluded from
dof).

## Review round 21 (`7dcbe87`)

The round-20 trap knowledge was verified and then made executable. The
delta verification confirmed every fc28e3a claim (including that the
default op-prob sum lands on bit-exact 1.0, so the new zero-bin assert
can never false-positive today), settled the `-0.0` questions
decisively — a negative zero passes validation and is draw-neutral, but
postcard's bit-literal f64 serialization gives sign-distinct zeros
DIFFERENT config fingerprints, correct-by-design as document identity
and now recorded in CLAUDE.md — and tightened the trap bullet's wording
("bit-neutral" → "value-neutral"; the argument never needed the bit
claim). The companion agent empirically demonstrated the categorical
last-entry trap at the function level and pinned it as a permanent test
(`categorical_last_entry_fallback_semantics`: fallback reachable; a
zero-weight key appended last hijacks it; the documented before-last
insertion preserves it; normal draws unaffected in all three shapes).
Sibling sweep: kk2/cww unchanged; reference-workload is actively
executing the corpus GO decision (new gamepad/evdev capture commits).

## Review round 22 (`b5203c9`)

Two closing certifications, no fixes needed. (1) **Behavior-freeze
certificate**: every commit since round 13 (`5b3f02d`) was classified
hunk-by-hunk — all eight (rounds 14–21) touched only documentation, test
code, comments/doc-comments in production files, or additive lint config;
**zero executable production changes**; all five golden fixture files
byte-frozen across the range; and the workspace passed the newly-added
fmath lints without a single code edit (corroborating that no drift
existed to correct). The phases track may treat rounds 14–22 as a pure
verification overlay on the round-13 implementation. (2) The round-21
delta verified fully, including the trap test's fairness question settled
by a u-granularity computation: `next_unit_f64`'s maximum value
((2^53−1)/2^53 = 0.9999999999999999) equals the known one-ulp-short prior
sum bit-for-bit, so the realistic rounding-gap fallback IS reachable at
the boundary, and the test's exaggerated fixture exercises the identical
control flow — adequate coverage, no addition needed.

## Review round 23 (`e753677`)

A currency audit caught the round-15 failure mode recurring: the
changelog table (created in round 15 precisely because a run-id list had
silently gone stale) had itself gained no rows for seven rounds, and
rounds 15–22's headings still read "(this commit)". Fixed: rows 16–22
added with their verified SHAs and green run ids, and every settled
round heading now carries its SHA — only the newest round may say "this
commit". The audit's spot-checks independently re-confirmed the round-22
freeze certificate (zero testdata commits in the range; the three
production-file diffs are comments/test-module only; b5203c9 touched
only this document). The companion freshness sweep: reference-workload's
gamepad work does NOT touch pad_layout (bit table safe), zero proto
drift, ff-lineage still clean, stack running, kk2/cww/5tk unchanged.

## Review round 24 (`d851fc4`)

Two audits of the reviewer's own remaining artifacts, both clean. The
round-23 table addition was verified cell-by-cell (all eight SHAs, run
ids, and paraphrases exact; headings correct; round 23's own run green).
The session's persistent memory file — what future agent sessions load as
ground truth — was audited claim-by-claim: thirteen of thirteen
load-bearing facts verified against the repos and beads, with one path
correction (a missing `crates/` prefix on the orchestrator's driver
path), and rewritten from a 23-edit run-on into a 30-second read that
points at this document for history instead of duplicating it. No repo
changes beyond this record.

## Review round 25 (`ebb72ab`)

Maintenance round, both audits clean with zero findings. The round-24
delta and the rewritten session memory were verified in full (single-file
diff; run 29224598305 green; all thirteen memory facts re-confirmed
against the repos and live beads, including the corrected driver path).
The freshness sweep found no movement anywhere: all three sibling repos
at their anchors, zero proto drift, the fast-forward lineage clean, the
Phase 3 stack running, and every bead state exactly as recorded. This is
the first round in which neither agent produced a single finding of any
severity — the audit surface remains closed and external state is
static; only the operator merge and the sibling-repo chains (kk2→cww,
refwork-20v→5tk) can move the project from here.

## Review round 26 (`5ff7cc7`)

Maintenance round. The round-25 delta verified in full (single-file
diff, run green, prose consistent, gates 123/22). The freshness sweep
found all repos at their anchors with no bead movement — and produced
one false alarm worth recording as method: it reported snapstore-server
offline, but direct re-verification before recording showed three
instances plus dh-workerd and the bridge all running (the sweep agent's
`pgrep -c` invocation miscounted). Sweep findings are re-verified
before entering this record; that practice caught this one.

## Review round 27 (this commit)

Maintenance round, clean. The round-26 delta verified in full, including
independent re-confirmation of the false-alarm account (three
snapstore-server instances counted correctly with the fixed pgrep
invocation; bridge active). The sweep — now carrying the corrected
process-count guidance — found all repos at anchors, zero drift, stack
stable, and surfaced one dependency detail folded into open item 1:
`refwork-5tk` is additionally blocked on their `refwork-d7t.1` alongside
`refwork-20v`.

## Verification pointers

Owner docs (ARCHITECTURE.md, API.md, INTEGRATION.md, IMPLEMENTATION-PLAN.md)
live OUTSIDE this repository at
`~/.agents/projects/determinism/docs/input-synthesizer/` — every spec
reference in this document points there, not at a repo path.

Clean checkout at the HEAD of branch `phase4-v1-generators`: `cargo test
--workspace` (123 tests / 22
suites; needs the sibling `control-plane` checkout at `proto-v0.2.0`);
golden recorder modes are `--ignored` tests; the CI golden↔version rule's
synthetic-violation reproduction is documented in
`ci/check-golden-version.sh`'s header; image rebuildable via
`scripts/build-image.sh` (source SHAs printed at build time). Note on the
offer's step 4 wording: image *digests* are not bit-identical across
rebuilds (cargo/docker nondeterminism — confirmed empirically); "reproducible
from the recorded SHA" holds as SHA-traceable rebuildability, which is what
the evidence records.
