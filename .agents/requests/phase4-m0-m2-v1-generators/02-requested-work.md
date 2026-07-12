# Requested Work

## Entry Conditions

None for M0–M2 — start immediately. The v1 live smoke (item 5) needs a
scheduled window with the orchestrator dev loop and the deployed stack
up; the context-free and golden/statistical acceptance never waits on
that.

## Work Items

0. **Tracking.** `bd init`; one bead per milestone (M0, M1, M2, v1 gate,
   M3) plus the proto audit, the seed-rule doc fix, and the owner-plan
   numbering doc fix (below); dependency edges per the chain.
   **Owner-plan doc fix:** the IMPLEMENTATION-PLAN's header ("first-boss
   … needs M1–M3") contradicts its own v1-gate line ("M1 + M2 +
   deployment artifact") and the phase doc ("M1+M2 is the documented
   v1"). Fix the header in
   `~/.agents/projects/determinism/docs/input-synthesizer/IMPLEMENTATION-PLAN.md`
   to match the v1-gate line, and record the correction in your
   resolution.

1. **Seed-rule reconciliation (early, cheap, blocking correctness).**
   Before implementing RNG fan-out: adopt the orchestrator's implemented
   rule — `derive_synth_request_seed` = first draw from
   `DeterministicRng::synth(experiment_seed, batch_seq)` — as the
   caller-side contract; fix this repo's INTEGRATION.md; coordinate
   closure of exploration-orchestrator bead `isj` (comment on it; they
   close it). Note the boundary: the *request seed* derivation is the
   orchestrator's; your `fanout_root`/`stream(root, label)` fan-out
   *from* that seed is yours per API.md and unaffected.

2. **M0 — scaffolding & contracts** (owner plan §M0). Two scope notes.
   First: do **not** author protos — the skeleton's `synth-proto`
   already consumes control-plane's `determinism-proto` crate via path
   dependency; keep that pattern, confirm the consumed tree matches tag
   `proto-v0.2.0`, and audit it field-by-field against API.md §1–§2,
   recording a match/diverge table; divergences are buf-gated change
   requests to control-plane, never local forks. Second: the skeleton
   already carries real groundwork (`FanoutRng::stream` + golden test,
   `InputModel`, `PadModel::legalize` — see `01-`); M0 is
   audit-and-extend against the plan's accept list, not greenfield.
   Everything else per the plan: `synth-core` types, config
   parse/validate/fingerprint, CI with the `HashMap` deny lint and
   **both-arch determinism from day one** (fixed-seed `stream()` golden
   vectors; decide the pinned-libm question here per the plan's
   float-nondeterminism risk row).

3. **M1 — pad model + weighted-random generator + gRPC shell** (plan
   §M1). The golden-seed reproducibility fixtures (50 recorded
   request→burst_id lists, byte-identical on both arches, CI-enforced
   `synth_version` bump on golden change) are the spine — treat any
   cross-arch mismatch as P0. Statistical suite fixed-seed per the
   plan's testing-strategy note (deterministic, never flaky).

4. **M2 — macro packs + macro generator** (plan §M2), including the
   handwritten `packs/console16-movement-core.yaml` demo pack (~10
   macros). Use the `console16-12btn-v1` pad contract from
   reference-workload's pad-alphabet fulfillment as the button alphabet.

5. **v1 gate — deployment artifact + live smoke.** Container image
   buildable for either host; then, in a scheduled window: 1,000
   consecutive `ProposeBursts` calls from the orchestrator dev loop with
   live reference-workload contexts — zero errors, zero illegal bursts
   (hypervisor-side validation count = 0). Coordinate with
   exploration-orchestrator (their bead `cww` is their async-transport
   half of this wiring) and confirm the deployed stack is up first
   (worker/snapstore are user processes; they die on reboot). If live
   contexts are still corpus-gated when everything else is green, run
   the smoke exercising M1's acceptance-defined context-free operation
   (`NodeContext` carrying only `node_id`), driven through the
   orchestrator loop with its existing fake context store — record that
   explicitly, and leave the live-context rerun as the single named open
   item.

6. **M3 — mutation generator (stretch here, hard before Phase 5).**
   Plan §M3 in full, including the provenance-replay test. Start only
   after the v1 gate is recorded; if this packet's execution window
   closes first, leave M3 as a ready bead with the plan reference — do
   not half-land it.

## Suggested Sequencing (Yours To Overrule)

1 → 2 → 3 → 4 → 5 → 6. Item 1 can fold into M0. The macro-pack YAML
authoring (item 4's content, not its loader) can be drafted any time.

## Acceptance Criteria

Each milestone's **Accept** list in the owner IMPLEMENTATION-PLAN
(§M0–§M3), adopted by reference, plus:

- **Phase 4 exit gate 4, explicitly, all three clauses:** (a) identical
  bursts for identical (request, seed) across x86_64 and aarch64;
  (b) χ²/KS distribution tests green; (c) macro packs load and
  instantiate. Cite the test names/CI runs proving each clause.
- Proto audit table recorded, zero unresolved divergences.
- INTEGRATION.md seed rule matches the orchestrator's implementation;
  bead `isj` closed or commented ready-to-close by their owner.
- Owner-plan header/v1-gate contradiction fixed and recorded (item 0).
- Golden fixtures + statistical suite green on both architectures (or
  aarch64 recorded as pending debt with a named blocker).
- v1 gate evidence: image digest, smoke transcript/counts, illegal-burst
  count source (hypervisor-side, not self-reported).
- Beads closed with `-r` evidence-linking reasons.

## Out Of Scope

- M4 macro mining, M5 event grammar, M6 policy generator, M7 hardening.
- Producing live context fixtures or captures (reference-workload's
  corpus request).
- The orchestrator's async transport adapter (`cww` — theirs; you
  provide the endpoint and fixtures).
- Authoring protos or bypassing control-plane's buf gate.
