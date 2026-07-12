# Request: Phase 4 Synthesizer v1 — M0→M2 (+M3 Stretch) to First-Boss-Ready

## Who Is Asking

The phases track, on behalf of Phase 4
(`~/.agents/projects/determinism/phases/phase-4-scoring-and-inputs.md`).
Filed 2026-07-12, the day Phase 3 closed. This is the **first request
packet in this repo** — the 2026-07-10 audit in
`~/git/preestablished/REQUEST-WORK-ORDER-2026-07-07.md` established that
no round-4 bootstrap packet ever existed here and Phase 0 (the skeleton,
committed 2026-06-09) is done, not to be re-filed.

## Why Now

Phase 3's exit gates went green 2026-07-12 (cutover record:
`~/git/preestablished/rom-operator-bridge/.agents/handoffs/2026-07-12-real-snapshot-cutover-confirmation.md`).
The phase doc runs this repo's chain **fully independent of the scorer
chain** — one agent each — so it is startable today with no external
gate. **This packet owns Phase 4 exit gate 4**: identical bursts for
identical (request, seed) across x86_64 and aarch64; χ²/KS distribution
tests green; macro packs load and instantiate.

## Milestone Numbering — Read This Before The Plan

The phase doc and the owner IMPLEMENTATION-PLAN
(`~/.agents/projects/determinism/docs/input-synthesizer/IMPLEMENTATION-PLAN.md`)
number milestones differently, and the plan disagrees with itself:

- Phase doc "M1 (pad model + weighted-random + gRPC shell)" = plan
  **M0 + M1**. Phase doc "M2 (macro packs)" = plan **M2**. Phase doc
  "M3 (mutation)" = plan **M3**.
- The plan's header says first-boss "needs M1–M3"; its own v1 gate line
  under M2 says "**M1 + M2** + deployment artifact", and the phase doc
  says "M1+M2 is the documented v1" with mutation a stretch. **Rule for
  this request: v1 = plan M0+M1+M2 + the deployment artifact + the
  1,000-call live smoke; plan M3 (mutation) is in-scope stretch here and
  a hard requirement before Phase 5's gate run.** Record the header
  contradiction as a doc fix rather than resolving it silently.

## Two Fresh Facts The Plan Predates

1. **Protos are published.** `determinism/inputsynth/v1/synthesizer.proto`
   exists in control-plane (commit `261141b`, inside tag `proto-v0.2.0`)
   with `ProposeBursts`, `LoadMacroPack`, `MineMacros`, `Health`. M0
   consumes and audits the pinned tag against API.md §1–§2; corrections
   go through control-plane's live buf breaking-change gate.
2. **The seed-derivation spec in INTEGRATION.md is stale.**
   exploration-orchestrator bead `exploration-orchestrator-isj`
   (2026-06-23, open) records that this repo's INTEGRATION.md still
   specifies `blake3(experiment_seed || node_id || expansion_counter)[..8]`
   while the orchestrator — the caller — now documents and implements
   `derive_synth_request_seed` as the first draw from
   `DeterministicRng::synth(experiment_seed, batch_seq)`. **The
   orchestrator's implemented rule is authoritative.** Reconcile the doc
   (coordinate via that bead) before hard-coding any seed assumption.

## The Ask In One Paragraph

Stand up issue tracking (`bd init`), then execute the owner plan's
M0→M2 to each milestone's accept list — scaffolding/contracts with
cross-arch determinism CI from day one (M0), the pad model +
weighted-random generator + serving shell with golden-seed and
statistical acceptance (M1), macro packs + the handwritten
`console16-movement-core.yaml` demo pack (M2) — then close the v1 gate:
a deployable container image for either host, smoke-tested with 1,000
consecutive `ProposeBursts` calls against the real orchestrator dev loop
with zero errors and zero illegal bursts. M3 (mutation operators) is the
in-scope stretch: start it once v1 is closed; it must exist before
Phase 5's gate run.

## Files In This Request

| File | Contents |
|---|---|
| `00-overview.md` | This file — who/why/ask, numbering rule |
| `01-current-state.md` | Evidence: repo, protos, pad contract, consumer status |
| `02-requested-work.md` | Work items, acceptance, out-of-scope |
| `03-verification-offer.md` | Verification, handback, sibling choreography |
