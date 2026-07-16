# Plan: Phase 4 M1→M3 — Pad Model, Macro Packs, Mutation

Implementation plan for the `input-synthesizer` chain of Phase 4
(`~/.agents/projects/determinism/phases/phase-4-scoring-and-inputs.md`). The
synthesizer chain is **fully independent of the scorer chain** per the phase
doc — no cross-repo coordination is needed until the v1 first-boss gate smoke
(see `08-v1-gate-and-handback.md`). This plan is written for a coding agent
working in `~/git/preestablished/input-synthesizer` on branch `main` with no
prior conversation context.

## Normative sources (precedence order for conflicts)

1. Frozen proto: control-plane
   `crates/determinism-proto/proto/determinism/inputsynth/v1/synthesizer.proto`
   at tag `proto-v0.2.0` (commit `1a9fb94`, crate `determinism-proto` v0.2.0,
   feature `inputsynth`). **The generated crate is the wire truth.** Schema
   corrections go through control-plane's request flow — never fork locally.
2. `~/.agents/projects/determinism/docs/input-synthesizer/IMPLEMENTATION-PLAN.md`
   §M0–§M3 — the accept-when lists this plan must satisfy. That doc is the
   **owner** of acceptance criteria; this plan restates headlines only.
3. `~/.agents/projects/determinism/docs/input-synthesizer/API.md` — burst wire
   format (§1), RPC semantics + error codes (§2), macro pack YAML (§3),
   experiment config schema + validation rules (§5).
4. `~/.agents/projects/determinism/docs/input-synthesizer/ARCHITECTURE.md` —
   crate layout (§1), core types/trait (§2), pipeline (§3), weighted-random
   math (§4), macro/mutation generators (§5), seeding discipline (§7,
   normative), server shell (§8), perf targets (§9).
5. `~/.agents/projects/determinism/docs/input-synthesizer/INTEGRATION.md` —
   orchestrator-shaped flows (used by the v1 gate posture only).

## Current state (verified 2026-07-15)

- Repo: 2 commits (`59d681c` initial, `0aa6b34` Phase 0 skeleton), clean,
  pushed. Workspace crates: `synth-proto` (re-exports
  `determinism_proto::inputsynth::v1` as `synth_proto::v1`), `synth-core`,
  `synth-rng`, `synth-server`. No `synth-pad` / `synth-gen` yet. No `.beads/`.
- **BLOCKER — the workspace does not build.** `cargo build --workspace` fails
  with three E0560 errors in `crates/synth-core/src/lib.rs`: it was written
  against an older hand-stub proto and constructs
  `Burst { format_version, pad_segments: vec![PadSegment { start_frame,
  frames, buttons }] }`. Frozen v0.2.0 `Burst` has
  `{ format_version, burst_id, body: Option<burst::Body> }` and `PadSegment`
  has `{ buttons, hold_frames }` only. Package 01 fixes this first.
- `synth-rng` has a non-normative xorshift `FanoutRng::stream(root: u64,
  label: &str)` with one stability test. ARCHITECTURE §7.1 is **normative**
  (blake3 derive-key `fanout_root` + `blake3::keyed_hash` → `ChaCha8Rng`
  streams), so the internals must be brought to spec in package 03 — the spec
  demands it; keep the module shape and golden-vector test discipline.
- `synth-server` is a stub (`health() -> "input-synthesizer:m0"`). No tonic.
- CI (`.github/workflows/ci.yaml`) is x86_64-only: checkout of self +
  unpinned sibling `control-plane`, then fmt/build/test. No aarch64 leg, no
  clippy, no HashMap deny lint, no golden-version gate. Package 02 fixes this.
- `determinism-proto` dep is a path dep on `../control-plane` with
  `default-features = false, features = ["inputsynth"]` (pulls prost 0.14.4,
  tonic 0.14.6, tonic-prost 0.14.6). Tag `proto-v0.2.0` exists in
  control-plane and `determinism_proto::PROTO_VERSION == "proto-v0.2.0"`.

## Grounding notes

Confirmed against the frozen proto file AND the generated crate
(`determinism-proto` tests use these exact symbols):

- Module path `determinism_proto::inputsynth::v1`; const
  `BURST_FORMAT_VERSION: u32 = 1` lives in that module (facade-provided).
- Messages: `Burst{format_version, burst_id, body}` with
  `burst::Body::{Pad(PadBurst), Event(EventBurst)}`;
  `PadBurst{segments, button_alphabet}`; `PadSegment{buttons, hold_frames}`
  (both `u32` on the wire; domain type uses `u16` mask per ARCHITECTURE §2);
  `EventBurst`, `GrammarEvent`, `GrammarField`, `FieldValue` +
  `field_value::Value`; `ProposeBurstsRequest{experiment_id, node_context, k,
  length_hint, seed, model, config_overrides_yaml}`;
  `NodeContext{node_id, snapshot_ref, depth, node_score, novelty,
  ram_features, frame_embedding, recent_inputs, parent_burst,
  sibling_bursts}`; `ScoredBurst{burst, score_delta}`;
  `ProvenancedBurst{burst, provenance}`;
  `ProposeBurstsResponse{bursts, config_fingerprint, synth_version, seed,
  degraded}` (field is `degraded`, not the `degraded_generators` name
  ARCHITECTURE §3.1 uses — proto wins); `DegradedGenerator{generator, reason}`;
  `LoadMacroPackRequest{source(oneof document_yaml|artifact_ref), kind}` +
  `load_macro_pack_request::Source`; `LoadMacroPackResponse{document_id,
  items_loaded, warnings}`; `Provenance{generator, slot, rng_stream,
  config_fingerprint, fallback_from, macro, mutation, policy}` — **prost
  renames the `macro` field to `r#macro` in Rust**;
  `MacroProvenance{pack_id, macro_name, param_bindings, macro_frames,
  tail_frames, chain_index}`; `MutationProvenance{base_burst_id,
  donor_burst_id, base_was_sibling, ops, post_clamp}`; `MutationOp{op, args}`;
  `PolicyProvenance`; `HealthRequest`; `HealthResponse{status, synth_version,
  loaded_packs, loaded_experiments, policy_endpoint_up, policy_deterministic,
  mining_in_progress}` + `health_response::Status`; `MineMacrosRequest/
  Response`, `PathSample`, `MiningParams`, `MinedMacroStats`.
- Enums: `ModelKind::{Unspecified, Pad, EventGrammar}`,
  `GeneratorKind::{Unspecified, WeightedRandom, Macro, Mutation, Policy}`,
  `DocumentKind::{Unspecified, MacroPack, ExperimentConfig, EventGrammar}`.
- Service codegen: `input_synthesizer_server::{InputSynthesizer,
  InputSynthesizerServer}`, `input_synthesizer_client::InputSynthesizerClient`.
- `determinism_proto::PROTO_VERSION: &str = "proto-v0.2.0"` at crate root.

Unconfirmed / divergences an implementer must handle (do not build plans on
these without resolving):

- **`ubuntu-24.04-arm` runner**: this repo is **public** (verified via
  `gh repo view`), so rung 1 of Package 02's ladder — GitHub's hosted arm
  runner, free for public repos — is expected to work. The fallback ladder is
  contingency, not the expected path; do not silently drop cross-arch
  assertions.
- ARCHITECTURE §8 mentions a `LoadExperimentConfig` "sibling RPC" — **no such
  RPC exists** in the frozen proto. Use
  `LoadMacroPack(kind=DOCUMENT_KIND_EXPERIMENT_CONFIG)` (API.md §2.2 agrees).
- API.md §2.6 `determinism.policy.v1` `PolicyServing` service is **not**
  generated in determinism-proto v0.2.0; the `policy` feature only carries a
  handwritten `Token{token_id, logprob}` facade that diverges from API.md's
  `Token{mask, dur_bucket}`. Out of scope for M1–M3 (policy is M6). The
  canonical mining/dedup token is a **local** `synth-core` type
  `Token{mask: u16, dur_bucket: u8}` per ARCHITECTURE §6.2 — never import
  `policy::v1::Token` for it.
- reference-workload API.md §3.4 pad bit table was not re-verified in this
  planning pass; the demo alphabet is fully specified in input-synthesizer
  API.md §5.1 — copy that document verbatim for the demo experiment config
  fixture, and cross-check against reference-workload before the v1 gate.
- `statrs` (test-only dep suggested by ARCHITECTURE §1) version unpinned —
  implementer picks current, tests-only, never in a decision path.

## Work packages, sequence, and dependency graph

| File | Package | Milestone |
|---|---|---|
| `01-proto-drift-reconciliation.md` | WP1 — fix synth-core drift, workspace green, tracking setup | pre-M1 blocker |
| `02-ci-cross-arch-and-lints.md` | WP2 — aarch64 CI leg, clippy, HashMap deny, golden-version gate | M0/M1 gate infra |
| `03-m0-completion-core-foundations.md` | WP3 — normative RNG fan-out, config schema, domain types, synth-pad legalize | M0 completion |
| `04-m1-weighted-random-generator.md` | WP4 — weighted-random sampler + statistical suite | M1 |
| `05-m1-mixer-server-goldens.md` | WP5 — mixer, ProposeBursts end-to-end, Health/healthz/metrics/logs, 50 goldens, latency bench | M1 |
| `06-m2-macro-packs.md` | WP6 — pack loader, LoadMacroPack (3 kinds), macro generator, shipped pack | M2 |
| `07-m3-mutation-generator.md` | WP7 — seven operators, provenance replay, degradation | M3 |
| `08-v1-gate-and-handback.md` | WP8 — container image, orchestrator smoke posture, evidence bundle, handback | v1 gate |

```
WP1 ──► WP2 ──────────────┐
  │                        ▼
  └──► WP3 ──► WP4 ──► WP5 ──► WP6 ──► WP7
                            │      │
                            └──────┴──► WP8 (v1 gate = M1+M2+image; WP7 evidence
                                         joins WP8 if done, but does not block it)
```

- WP2 can run in parallel with WP3 once WP1 lands. The file split that makes
  this safe: WP2 owns `clippy.toml` + CI config only; the crate-level
  `#![deny(clippy::disallowed_types)]` attributes belong to WP3 (which
  rewrites those `lib.rs` files anyway). Everything else is sequential.
- WP2 is additionally a predecessor of WP3's and WP4's **acceptance-close**
  (their "both CI legs green" bullets need the aarch64 leg to exist), even
  though their implementation may proceed in parallel with WP2. Close those
  beads only after the first dual-leg run covering their tests.
- Milestones are strictly sequential M1 → M2 → M3 (phase doc). **M3 is
  stretch within Phase 4 but a hard requirement before Phase 5's gate run**
  (phase doc, synthesizer chain item 3) — if Phase 4 time runs out, WP8 ships
  the v1 gate on M1+M2 and WP7 becomes the first Phase 5 prerequisite.

## Ground rules

- Rust edition 2021. Keep `synth-core`, `synth-rng`, `synth-pad`, `synth-gen`
  pure (no tokio, no I/O, no `std::time` in sampling paths) per ARCHITECTURE
  §1; only `synth-server` gets tokio/tonic.
- Ordered collections only in decision paths (`Vec`, `BTreeMap`, `IndexMap`);
  `std::collections::HashMap` is deny-linted (WP2). All hashes blake3.
- ARCHITECTURE §7 seeding discipline is the project's most important
  contract: every stream label in §7.2 is normative; draw order within a
  stream is part of the format; changing either bumps `synth_version` and
  regenerates goldens in the same PR.
- Golden fixtures are recorded once, committed as literal bytes, and never
  regenerated by the code under test in CI. Cross-arch identity is asserted
  on recorded values, not self-consistency.
- Tracking: `bd init` in WP1; one bead per work package plus per-milestone
  acceptance beads; close with `-r` linking evidence (test names, bench
  output). Short titles, details in `-d`.
- Commit at each green package boundary (CI green on every commit); follow
  the review workflow (`/review` → reconcile → `/fix-review` → verify) before
  each milestone commit. Do not push a red main.
- **Branch discipline:** direct commits to `main` at green package boundaries
  are house practice in this project — most work never rides a PR. Any CI
  gate that only fires on `pull_request` is therefore vacuous; the WP2
  golden-version gate must fire on push as well.
- **Wrong-frozen-proto escalation:** on discovering a proto defect
  mid-implementation, file a request directory in
  `control-plane/.agents/requests/<name>/` following its observed structure
  (`00-overview` / `01-current-state` / `02-requested-work`), mark the
  affected bead blocked with a dep on a new "proto correction" bead, and
  continue non-dependent packages. The frozen tag remains wire truth until
  control-plane cuts a new tag — never build against untagged proto changes.
