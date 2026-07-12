# Current State (Evidence-Based, Assessed 2026-07-12)

## This Repo

`main` at `0aa6b34` ("Phase 0 skeleton: workspace, crates, CI",
2026-06-09), clean, `origin` configured, `main == origin/main`. Contents:
cargo workspace with four crates (`synth-proto`, `synth-core`,
`synth-rng`, `synth-server`), `.github/workflows/ci.yaml`, a two-line
README. **No beads database** (`bd init` needed), no `.agents/`
directory before this packet.

The crates are *not* empty stubs — the skeleton already carries real M0
groundwork, so M0 starts as an audit-and-extend, not greenfield:

- `synth-proto` already **path-depends on
  `../control-plane/crates/determinism-proto`** and re-exports
  `determinism_proto::inputsynth::v1` — the proto consumption pattern is
  established.
- `synth-rng` already implements `FanoutRng::stream(root, label)` with a
  golden-value unit test (`stream_is_stable`) — the M0 fan-out primitive
  exists; what's missing is the plan's full accept list (both-arch
  golden vectors in CI, the libm decision).
- `synth-core` already defines an `InputModel` trait, a working
  `PadModel::legalize`, and a `neutral_burst` helper.

Audit what exists against ARCHITECTURE/API before writing anything —
keep, extend, or consciously replace; don't duplicate.

Crate-name note: the owner plan's M1 names `synth-pad`, which the
skeleton lacks; the skeleton's `synth-rng`/`synth-server` don't appear
verbatim in the plan text. Follow ARCHITECTURE §1's layout — reshaping
skeleton crates is expected M0 work.

## Owner Docs

`~/.agents/projects/determinism/docs/input-synthesizer/`: README,
ARCHITECTURE, API, INTEGRATION, IMPLEMENTATION-PLAN. The plan's M0–M3
carry the build lists and accept criteria this request adopts by
reference. M4 (mining), M5 (event grammar), M6 (policy), M7 (hardening)
are later phases' work.

**Known doc defect:** INTEGRATION.md's synth-request seed derivation is
stale relative to the orchestrator's implemented
`derive_synth_request_seed` (first draw from
`DeterministicRng::synth(experiment_seed, batch_seq)`) — tracked as
exploration-orchestrator bead `isj`, open since 2026-06-23. The caller's
implemented rule wins; the doc reconciliation is part of this request
(`02-`, item 1).

## Protos

`control-plane` `crates/determinism-proto/proto/determinism/inputsynth/v1/synthesizer.proto`
published in commit `261141b`, contained in tag `proto-v0.2.0`; four
RPCs: `ProposeBursts`, `LoadMacroPack`, `MineMacros`, `Health`. The
field-level match against this repo's API.md has not been audited —
that audit is M0 work. Control-plane's buf breaking-change gate is live.

## Inputs Available Today

- **Pad layout contract:** `console16-12btn-v1` is implemented and
  documented — the pad-alphabet half of
  `~/.agents/projects/reference-workload/requests/pad-alphabet-and-phase4-context-fixtures/`
  (FULFILLMENT.md: "partially fulfilled"; the *live Phase 4 context
  fixture* half remains gated on the real-capture evidence floor).
  M1/M2 can build against the pad contract now.
- **Demo-game macro knowledge:** plan M2's
  `packs/console16-movement-core.yaml` (~10 handwritten macros:
  long-jump l/r, charge-and-release, ladder-climb, door-enter,
  menu-confirm, dash variants) needs game knowledge, not captures —
  writable now.
- **Live `NodeContext` fixtures:** gated on reference-workload's corpus
  work (`refwork-czi` in progress; `refwork-5tk` open, with its
  2026-07-11 bead comment recording the launch decision as no-go pending
  operator approval). M1's acceptance explicitly includes
  context-free operation (`NodeContext` with only `node_id`), so this
  gates nothing before the v1 live smoke.

## The v1 Smoke's Counterpart

The v1 gate needs "the real orchestrator dev loop" with live
reference-workload contexts and a hypervisor-side illegal-burst count
of 0 over 1,000 calls. Status of the counterpart:

- exploration-orchestrator's loop is complete against fakes (M0–M5 done;
  6 open beads, all P2/P3). Two of its open beads touch you directly:
  `isj` (seed-rule doc drift, above) and `cww` (M6: replace its
  blocking `GeneratedInputSynthClient` with an async transport adapter —
  their side of wiring a *real* synth endpoint).
- The deployed Phase 3 stack (bridge systemd unit → dh-workerd
  `6e348e5` → durable snapstore copy at `~/.rbo73/m4-regen-20260707/`)
  is live but worker+snapstore are user processes that die on reboot —
  schedule the live smoke, don't assume the stack is up.

## Hosts

Cross-arch determinism (x86_64 Intel box + aarch64 DGX Spark) is an M0
acceptance item — golden `stream()` vectors identical on both. Same
caveat as the scorer sibling: if the aarch64 leg is unreachable, record
it as pending debt, don't drop the assertion.
