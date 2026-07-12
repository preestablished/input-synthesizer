# Resolution — Phase 4 M0–M2 v1 Generators (+M3)

**Final state (read this first):** branch `phase4-v1-generators` HEAD;
`cargo test --workspace` = 120 tests / 22 suites green; authoritative
dual-arch CI evidence = the newest green run on the branch (every push
triggers one; a documentation-only commit like the final sign-off edits
necessarily trails its own run by one — check `gh run list` for the
head's conclusion). Run ids: 29181386490 (`ab36bb4`), 29195495676
(`c9851c3`), 29197027014 (`a9197dd`, covers round 2), 29198388598
(`6d41748`), 29199743592 (`ab1e528`), 29200931736 (`ee17797`),
29202385874 (`e096e47`), 29204053262 (`498bc6c`), 29205449380
(`6ebf5a5`) — all success on both arches. Since the original handback
(`20c8e0d`), eleven review rounds landed fixes — all documented in the
round sections; per-round test counts in those sections are scoped to
their round's SHA, not the final tree.

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

(Restored as its own section in round 10 — this round's record had been
folded into the Final-state block and evidence-doc edits.) A
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

## Review round 9 (final)

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

## Review round 10 (final)

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

## Review round 11 (final)

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

## Verification pointers

Clean checkout at the HEAD of branch `phase4-v1-generators`: `cargo test
--workspace` (120 tests / 22
suites; needs the sibling `control-plane` checkout at `proto-v0.2.0`);
golden recorder modes are `--ignored` tests; the CI golden↔version rule's
synthetic-violation reproduction is documented in
`ci/check-golden-version.sh`'s header; image rebuildable via
`scripts/build-image.sh` (source SHAs printed at build time). Note on the
offer's step 4 wording: image *digests* are not bit-identical across
rebuilds (cargo/docker nondeterminism — confirmed empirically); "reproducible
from the recorded SHA" holds as SHA-traceable rebuildability, which is what
the evidence records.
