# Verification And Handoff Shape

## Phases-Track Verification

On your resolution we will:

1. From a clean checkout at your recorded SHA: `cargo test` on x86_64;
   re-run the golden-seed fixture suite, the M1 statistical suite
   (χ²/KS — exit gate 4 clause b), and the M2 pack-load + instantiation
   goldens (clause c); confirm the CI rule (goldens changed ⇒
   `synth_version` bumped) actually fails a synthetic violation.
2. Check the proto audit table against `proto-v0.2.0`'s
   `determinism/inputsynth/v1/synthesizer.proto` and API.md §1–§2.
3. Read the reconciled INTEGRATION.md seed rule against
   exploration-orchestrator's documented `derive_synth_request_seed` and
   the state of their bead `isj`.
4. Verify v1 gate evidence: image digest reproducible from the recorded
   SHA, smoke counts, and that the illegal-burst count came from the
   hypervisor side.
5. Confirm bead states match the resolution.

## Choreography With Siblings

- **exploration-orchestrator** — your primary consumer and the v1-smoke
  counterpart. Shared beads: `isj` (seed rule — coordinate, they close)
  and `cww` (their async synth transport — their half of live wiring).
  Also note their open doc-drift habit works both ways: if your
  implementation diverges from *their* documented caller behavior,
  raise it on their repo rather than absorbing it.
- **state-scorer `phase4-m1-m4-first-boss-scoring/`** (filed today) —
  fully independent chain; no shared state; parallel by design. You
  only meet inside the orchestrator's loop.
- **reference-workload** — owns the pad contract you consume
  (`console16-12btn-v1`, pad-alphabet fulfillment) and the gated live
  context fixture (their corpus fast-follow). A burst→input-log
  semantics mismatch lands in the hypervisor's court per the plan's
  risk table — the plan asks for a contract test in the hypervisor repo
  against burst fixtures from your `testdata/`; offer the fixtures in
  your resolution.
- **control-plane** — proto tree + buf gate; all schema corrections
  route there.

## Handback Shape

Append `04-resolution.md` here: git SHAs per milestone, bead IDs +
states, proto audit table, seed-rule doc diff + `isj` coordination
record, the owner-plan numbering doc fix, both-arch CI evidence, the
exit-gate-4 evidence per clause, v1 gate evidence (image digest, smoke
transcript, illegal-burst count source), and M3 status (done / ready
bead). We respond with `05-verification.md`. If the live-context smoke
ran in fallback mode, name the rerun as the single open item and what
unblocks it.
