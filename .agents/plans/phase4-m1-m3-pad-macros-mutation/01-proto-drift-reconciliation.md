# WP1 — Proto Drift Reconciliation (build blocker) + Tracking Setup

**Goal:** the workspace builds and tests green against the frozen
`determinism-proto` v0.2.0 crate, with a pin test so drift can never land
silently again. Nothing else in this plan can start until this is done.

## Why this is first

`cargo build --workspace` fails today (verified 2026-07-15, exact output):

```
error[E0560]: struct `Burst` has no field named `pad_segments`
  --> crates/synth-core/src/lib.rs:22:9
error[E0560]: struct `PadSegment` has no field named `start_frame`
  --> crates/synth-core/src/lib.rs:23:13
error[E0560]: struct `PadSegment` has no field named `frames`
  --> crates/synth-core/src/lib.rs:24:13
```

`crates/synth-core/src/lib.rs` was written against an older hand-stub proto.
Frozen v0.2.0 shapes (confirmed in the proto file and generated crate):
`Burst{format_version, burst_id, body: Option<burst::Body>}` with
`burst::Body::Pad(PadBurst{segments, button_alphabet})`, and
`PadSegment{buttons, hold_frames}`.

## Steps

### 1. Tracking setup

- `bd init` at repo root (prefix `inputsynth` or whatever `bd init`
  defaults to for this repo name). Create one bead per work package WP1–WP8
  with `bd dep add` chains matching the overview graph; labels: WP1 `prep`,
  WP2 `prep`, WP3–WP7 `impl`, WP8 `docs`/`cleanup`. Priorities: WP1/WP2
  `-p 0` (blocker + gate infra), WP3–WP6 `-p 1`, WP7 `-p 2` (stretch), WP8
  `-p 1`. Use `--silent` and capture each ID into a shell variable for the
  `bd dep add` chains (`WP1=$(bd create "..." -p 0 -l prep --silent)`). Run
  `bd` commands serially, never in parallel batches.

### 2. Reconcile `synth-core` to the frozen proto

Files to modify:

- `crates/synth-core/src/lib.rs` — minimal, behavior-preserving fix (WP3
  rebuilds this crate properly; do not gold-plate here):
  - `neutral_burst(frames: u32) -> Burst` constructs
    `Burst { format_version: BURST_FORMAT_VERSION, burst_id: vec![],
    body: Some(burst::Body::Pad(PadBurst { segments: vec![PadSegment {
    buttons: 0, hold_frames: frames }], button_alphabet: String::new() })) }`.
    (`burst_id` stays empty until WP3 defines the canonical hash; that is
    acceptable for the skeleton.)
  - `PadModel::legalize` keeps only the `format_version` stamp for now.
  - Imports: `synth_proto::v1::{burst, Burst, PadBurst, PadSegment,
    BURST_FORMAT_VERSION}`.
- Sweep for other drift: `grep -rn "pad_segments\|start_frame" crates/` must
  return nothing after the fix. `synth-rng`, `synth-server`, `synth-proto`
  were verified drift-free (they don't touch proto types).

### 3. Proto pin guard

Files to create/modify:

- `crates/synth-proto/src/lib.rs` — add
  `pub use determinism_proto::PROTO_VERSION;` alongside the existing
  `pub use determinism_proto::inputsynth::v1;`.
- `crates/synth-proto/tests/proto_pin.rs` — new:
  - `fn pinned_proto_version()` asserts
    `synth_proto::PROTO_VERSION == "proto-v0.2.0"` and
    `synth_proto::v1::BURST_FORMAT_VERSION == 1`.
  - `fn frozen_shapes_compile()` constructs one `Burst` with a
    `burst::Body::Pad` body and one `Provenance { r#macro: None, .. }` and
    round-trips through `prost::Message::{encode_to_vec, decode}` — a
    compile-time tripwire on the exact field set (add `prost` as a
    dev-dependency of `synth-proto`, same 0.14 line as determinism-proto).

### 4. CI checkout pin (small, belongs with this fix)

- `.github/workflows/ci.yaml` — pin the sibling checkout:
  `repository: .../control-plane` gains `ref: proto-v0.2.0`. (The full CI
  overhaul is WP2; this one-line pin rides here because an unpinned sibling
  is the same class of drift this package exists to kill.)

## Acceptance criteria

Run each as a separate command and check each result (no `&&` chains):

- `cargo build --workspace` — exit 0.
- `cargo test --workspace` — exit 0; includes new
  `proto_pin::pinned_proto_version` and `proto_pin::frozen_shapes_compile`
  plus the pre-existing `synth-rng` `stream_is_stable`.
- `cargo fmt --all -- --check` — clean.
- `grep -rn "pad_segments\|start_frame" crates/` — no matches.
- Push and confirm the `ci` workflow is green on the commit (x86_64 leg;
  aarch64 arrives in WP2).
- Close the WP1 bead with `-r` citing the green CI run URL.

## Failure guidance

- If `determinism-proto` itself fails to build from the sibling path, verify
  `~/git/preestablished/control-plane` is checked out at (or contains) tag
  `proto-v0.2.0` and that `cargo metadata` resolves the path dep. Do NOT
  vendor or fork the proto to work around it — fix the checkout.
- If `PROTO_VERSION` asserts fail, control-plane moved past v0.2.0 locally;
  pin your local checkout to the tag for this work
  (`git -C ../control-plane switch --detach proto-v0.2.0` in a worktree if
  needed) and note it in the bead. The frozen tag is the contract.
- If more drifted symbols surface than the three above, fix them in this
  package (that is this package's charter) and list them in the bead close.
