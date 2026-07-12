# Plan: Phase 4 M0–M2 v1 Generators (+M3 Stretch)

Plan for request `.agents/requests/phase4-m0-m2-v1-generators/`. Implementing
agent: read this file first, then execute `01-` … `07-` in order. Normative
sources (adopted by reference, do not duplicate — read them):

- `~/.agents/projects/determinism/docs/input-synthesizer/ARCHITECTURE.md` (crate
  layout §1, types §2, pipeline §3, weighted-random math §4, macros/mutation §5,
  seeding §7 — the most important section, server shell §8)
- `~/.agents/projects/determinism/docs/input-synthesizer/API.md` (proto §1–§2,
  macro pack YAML §3, experiment config schema + validation §5)
- `~/.agents/projects/determinism/docs/input-synthesizer/IMPLEMENTATION-PLAN.md`
  (per-milestone Accept lists — these are the acceptance criteria)
- `~/.agents/projects/determinism/docs/input-synthesizer/INTEGRATION.md`

## Scope and numbering rule

v1 = owner-plan **M0 + M1 + M2 + deployment artifact + 1,000-call live smoke**.
Owner-plan **M3 (mutation) is in-scope stretch**: start only after the v1 gate
is recorded; never half-land it. M4–M7 are out of scope. (The phase doc numbers
milestones differently; this plan uses the owner IMPLEMENTATION-PLAN's numbers
everywhere.)

This packet owns **Phase 4 exit gate 4**: (a) identical bursts for identical
(request, seed) across x86_64 and aarch64; (b) χ²/KS distribution tests green;
(c) macro packs load and instantiate. Every clause must map to named tests/CI
runs in the resolution.

## Ground truths verified 2026-07-12 (trust these, re-verify only if stale)

1. **Proto parity.** `determinism/inputsynth/v1/synthesizer.proto` in
   control-plane is byte-identical across tag `proto-v0.2.0`, HEAD's top-level
   `proto/` tree, and the copy embedded in `crates/determinism-proto` that this
   repo path-depends on. Field-by-field it matches API.md §1–§2. The M0 audit
   (03 of `02-m0…`) should therefore produce an all-match table; do not author
   or fork protos.
2. **The skeleton does not compile.** `synth-core/src/lib.rs` uses
   `Burst.pad_segments` / `PadSegment.start_frame` / `PadSegment.frames`, which
   no longer exist — current proto has `Burst.body` (oneof `pad`/`event`) and
   `PadSegment { buttons, hold_frames }`. `cargo check` fails with E0560.
   First M0 commit must restore a green build.
3. **Seed rule.** The orchestrator implements
   `derive_synth_request_seed(experiment_seed, batch_seq)` = first `next_u64()`
   draw from `DeterministicRng::synth(experiment_seed, batch_seq)`
   (`exploration-orchestrator/crates/orch-core/src/rng.rs:156`; node ids are NOT
   part of the rule). This repo's INTEGRATION.md:81 still shows the stale
   `blake3(experiment_seed ‖ node_id ‖ expansion_counter)[..8]` rule. The
   orchestrator's implemented rule is authoritative. Doc fix in `01-`.
   Boundary: how the *request seed* is derived is the orchestrator's business;
   our `fanout_root(seed, node_id)` + `stream(root, label)` fan-out FROM that
   seed (ARCHITECTURE §7.1) is ours and unaffected.
4. **Dual-arch CI pattern exists.** exploration-orchestrator CI runs a matrix of
   `ubuntu-latest` (x86_64) + `ubuntu-24.04-arm` (aarch64) hosted runners, with
   control-plane checked out as a sibling path. Copy that pattern. No SSH access
   to the DGX Spark is configured on this box; hosted arm runners satisfy the
   both-arch acceptance. If the arm runner leg is unavailable, record pending
   debt with a named blocker — never drop the assertion.
5. **Pad contract.** `console16-12btn-v1` is fulfilled and documented by
   reference-workload (`~/.agents/projects/reference-workload/requests/pad-alphabet-and-phase4-context-fixtures/FULFILLMENT.md`);
   API.md §5.1's demo alphabet copies it verbatim. Use it as-is.
6. **Live-context fixtures are corpus-gated** (reference-workload `refwork-czi`
   in progress / `refwork-5tk` no-go pending operator approval). Expect the v1
   smoke to run in the documented context-free fallback mode (`05-`).

## File map

| File | Contents |
|---|---|
| `01-tracking-and-doc-fixes.md` | `bd init`, beads + dependency edges, the three doc fixes, `isj` coordination |
| `02-m0-scaffolding-and-contracts.md` | Crate reshape, proto audit, core types, RNG fan-out, config, dual-arch CI, libm decision |
| `03-m1-pad-weighted-random-grpc.md` | Pad model, weighted-random generator, mixer, tonic server, goldens + statistical suite |
| `04-m2-macro-packs.md` | Pack loader, instantiation, demo pack YAML, LoadMacroPack |
| `05-v1-gate-deploy-smoke.md` | Container image, smoke choreography, fallback rules, evidence capture |
| `06-m3-mutation-stretch.md` | Mutation operators, provenance replay — stretch gate rules |
| `07-acceptance-checklist.md` | Consolidated acceptance + `04-resolution.md` handback shape |

## Sequencing

`01` → `02` → `03` → `04` → `05` → `06`. The seed-rule doc fix (in `01`) folds
into the same working session as M0 — it blocks nothing in code (the rule is
caller-side) but must land before the resolution. The demo-pack YAML content
(`04`) can be drafted any time. Commit at each milestone boundary at minimum;
each milestone's Accept list must be green (CI on both arches) before the next
starts. Close each bead with `bd close <id> -r "<evidence link>"`.

## Global engineering rules (bind every milestone)

- **Determinism discipline per ARCHITECTURE §7**: all proposal-path randomness
  derives from `req.seed` via `fanout_root`/`stream`; canonical stream labels
  per §7.2; one label, one consumer, one pass; draw order is part of the format.
- **No `std::collections::HashMap` in decision paths**: ARCHITECTURE §7.2
  rule 4 names `synth-core`, `synth-gen`, `synth-mine`; we enforce
  workspace-wide via clippy `disallowed-types` (stricter is fine). Use
  `Vec`/`BTreeMap`/`IndexMap`.
- **Never golden-compare raw prost wire bytes of messages containing proto
  `map<>` fields.** prost generates `std::collections::HashMap` for map fields
  (determinism-proto never sets `.btree_map`), and map encoding iterates in
  hash order — byte output varies per process even on one arch. Goldens hash
  canonical internal forms (postcard over domain types with `IndexMap`/sorted
  entries) or compare decoded structures with maps normalized to sorted
  `Vec<(K,V)>`. `burst_hash` (postcard over internal types) is already safe.
- **No `std::time` in sampling paths.** No platform SIMD math in sampling.
- All transcendentals in sampling paths route through the pinned pure-Rust
  `libm` crate (decision rationale in `02-`, §libm).
- Golden change ⇒ `SYNTH_VERSION` bump, CI-enforced (`02-`, §CI).
- Every emitted burst passes `legalize` before leaving the service; `legalize`
  is deterministic and takes no RNG.
- Workspace stays `#![forbid(unsafe_code)]`.
- Dependencies: `tonic`/`prost` (already via determinism-proto), `rand_chacha`,
  `rand_core`, `blake3`, `serde`/`serde_yaml`, `indexmap`, `postcard`, `libm`;
  `statrs` + `proptest` dev-only; `prometheus`, `tracing`,
  `tracing-subscriber` (server only).
