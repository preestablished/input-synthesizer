# 02 — M0: Scaffolding & Contracts

Owner accept list: IMPLEMENTATION-PLAN §M0. M0 is **audit-and-extend**: the
skeleton already carries a proto-consumption pattern, an `InputModel` trait, a
`PadModel::legalize` stub, and a `FanoutRng` — keep, extend, or *consciously
replace*; don't duplicate.

## 1. Crate reshape (ARCHITECTURE §1, v1 subset)

Target workspace:

```
crates/
├── synth-proto/    # KEEP as-is: re-exports determinism_proto::inputsynth::v1
├── synth-core/     # InputModel trait, Burst/Token domain types, Provenance
│                   # helpers, RNG fan-out, experiment-config types +
│                   # validation + deep-merge + fingerprint, SYNTH_VERSION
├── synth-pad/      # NEW: pad model — alphabet, legality, segments, tokenize
├── synth-gen/      # NEW: generators + mixer (weighted_random, macros(M2),
│                   # mutation(M3), mixer)
└── synth-server/   # tonic service, /healthz + /metrics, main.rs binary
```

- **Delete `synth-rng`** (fold into `synth-core::rng`): ARCHITECTURE §1 places
  fan-out in synth-core, and the skeleton's xorshift `FanoutRng` is a
  placeholder that does NOT match the normative §7.1 (blake3 derive-key +
  ChaCha8). This is a *conscious replacement* — note it in the resolution. The
  golden-test *pattern* (`stream_is_stable`) survives with new vectors.
- `synth-grammar`, `synth-mine`, `packs/`, `testdata/` per §1; create `packs/`
  and `testdata/` now (empty `.gitkeep` ok), grammar/mine crates NOT now.
- Keep the path-dep proto pattern (workspace dep `determinism-proto`,
  feature `inputsynth`); ARCHITECTURE §1's "proto/ symlink" is satisfied in
  spirit by the crate dep — record that in the audit note.

## 2. Fix the build break first

`synth-core/src/lib.rs` must compile against the real proto shape:
`Burst { format_version, burst_id, body: Option<burst::Body> }`,
`PadBurst { segments, button_alphabet }`,
`PadSegment { buttons, hold_frames }`. Rewrite `neutral_burst` accordingly.
First commit = green `cargo check --workspace` + existing tests.

## 3. Proto audit (bead AUDIT)

Produce `docs/proto-audit.md` in this repo:

- Record control-plane SHA + confirmation that
  `crates/determinism-proto/proto/determinism/inputsynth/v1/synthesizer.proto`
  is byte-identical to `git show proto-v0.2.0:proto/determinism/inputsynth/v1/synthesizer.proto`
  (verified 2026-07-12: identical — re-run and cite the diff command).
- Field-by-field table: every message/field/enum in API.md §1–§2 vs the proto —
  columns: `API.md item | proto item | match? | note`. Expect all-match.
- Note the consumption pattern (path dep, `default-features=false`,
  `features=["inputsynth"]`) and that the CI checks out control-plane as a
  sibling — pin the checkout: **CI must check out control-plane at tag
  `proto-v0.2.0`** (add `ref: proto-v0.2.0` to the checkout step) so an
  unrelated control-plane HEAD change can't break or silently alter us.
- Any divergence found: file a buf-gated change request in control-plane;
  NEVER fork locally. Zero unresolved divergences is an acceptance criterion.

## 4. `synth-core` contents

### 4.1 Domain types (§2)

Internal (non-proto) types: `PadSegment { buttons: u16, hold_frames: u32 }`,
`PadBurst { segments: Vec<PadSegment> }`, `Burst::Pad(PadBurst)` (Event variant
deferred to M5 — define the enum with `#[non_exhaustive]` or a single variant;
choose the simplest that compiles), `Token { mask: u16, dur_bucket: u8 }`.
Conversions to/from proto types live in synth-core (alphabet name is supplied
at conversion time). `burst_hash` = BLAKE3-256 of the canonical `postcard`
encoding of the versioned wire form (§2 trait doc); `burst_id` = that hash.

`InputModel` trait per §2 exactly: `burst_len`, `legalize`, `tokenize`,
`detokenize`, `burst_hash`, `kind`. The skeleton's 2-method trait is replaced.
`SYNTH_VERSION: &str` constant (start `"0.1.0"`, keep = crate version) and
`BURST_FORMAT_VERSION: u32 = 1`.

### 4.2 RNG fan-out (§7.1, normative — copy verbatim semantics)

```rust
pub fn fanout_root(seed: u64, node_id: &str) -> [u8; 32]  // blake3 derive-key
    // context string EXACTLY: "determinism.inputsynth.v1 2026 proposal root"
pub fn stream(root: &[u8; 32], label: &str) -> ChaCha8Rng // blake3 keyed_hash
```

Golden test: fixed `(seed=0x0123456789ABCDEF, node_id="node-golden")` + labels
`["mix", "slot/0/len", "slot/0/wr/btn/0", "slot/0/wr/dir"]` → assert the first
16 bytes of each stream against recorded vectors in
`testdata/rng_stream_goldens.json` (committed values generated on x86_64, CI
asserts equality on both arches — that IS the cross-arch test).

### 4.3 Config (API §5)

`serde` structs mirroring §5 with all defaults; accept YAML and JSON
(serde_yaml handles both). Ordered maps = `IndexMap` (`serde` feature,
preserving file order). Implement:

- `parse(bytes) -> Result<ExperimentConfig, ConfigError>`
- `validate(&cfg) -> Result<(), Vec<ConfigError>>` — ALL errors at once,
  each rule from API §5's validation list, incl. the duty inequality with the
  exact message format `duty {duty} > mean_hold/(mean_hold+1) = {bound} for
  button {name}` (reject strictly-greater only; equality valid and must load).
- `deep_merge(base, overrides)` — maps merge, scalars/lists replace.
- `fingerprint(effective_cfg, sorted_pack_ids, synth_version) -> [u8; 32]` =
  blake3 over canonical postcard of the effective config ‖ pack_ids ‖ version.
  Canonicalization: derive a deterministic serialization (postcard over the
  fully-defaulted struct; IndexMap order = config order — document that
  fingerprint depends on document order, which is fine: identical documents ⇒
  identical fingerprints).

Tests (M0 accept): minimal valid document loads with all §5 defaults
(assert several defaults explicitly); one failing fixture per validation rule
under `testdata/config/invalid/*.yaml`, each test asserting the exact error;
boundary fixture `duty == μ/(μ+1)` loads. Proto round-trip property test
(prost encode/decode) with `proptest`. `legalize` property tests: idempotent;
output satisfies every API §1 pad invariant for arbitrary inputs
(the pad `legalize` itself is implemented in `synth-pad`, M0 provides the
trait + tests may land with M1 if legalize is still a stub — but the M0 accept
list includes them, so implement real pad legalize in M0: move/extend the
skeleton's `PadModel` into `synth-pad` with the alphabet-driven rules of §2.1).

## 5. libm decision (risk-table item, decide at M0)

**Decision: route ALL transcendentals in sampling paths (`ln`, `exp`, lognormal
via `exp(N(...))`, logistic/logit) through the pure-Rust `libm` crate from day
one.** Rationale: removes the cross-arch float risk class instead of measuring
it; cost is negligible at our scale (µs of math per burst). Enforce by
convention + a grep-based CI guard is overkill — instead put the only allowed
wrappers in `synth-core::fmath` (`fn ln(f64)->f64`, `exp`, etc. delegating to
`libm`) and use them everywhere in synth-core/pad/gen. Record the decision in
`docs/proto-audit.md` or a short `docs/decisions.md`.

## 6. CI (`.github/workflows/ci.yaml`)

Copy exploration-orchestrator's matrix pattern:

- Matrix: `ubuntu-latest`/x86_64 + `ubuntu-24.04-arm`/aarch64; checkout self at
  `repo/`, control-plane at `control-plane/` **with `ref: proto-v0.2.0`**.
- Steps: `cargo fmt --check`; `cargo clippy --workspace --all-targets -- -D
  warnings`; `cargo build --workspace`; `cargo test --workspace`.
- **HashMap deny**: add `clippy.toml` with
  `disallowed-types = ["std::collections::HashMap", "std::collections::HashSet"]`
  — workspace-wide is simplest; `synth-server` may allow via
  `#[allow(clippy::disallowed_types)]` at use sites if genuinely needed
  (prefer not). Note: prost-generated `map<>` fields ARE `HashMap` — they live
  in determinism-proto (external, not linted); convert to ordered types at the
  proto boundary in synth-core before any decision-path use.
- **Golden↔version gate**: script `ci/check-golden-version.sh` — if
  `git diff --name-only $(git merge-base HEAD origin/main)..HEAD` touches
  `testdata/` goldens but no `SYNTH_VERSION` change is in the diff of
  `crates/synth-core` version file, fail with a message. Runs as a CI step on
  PRs (skip when merge-base == HEAD). Add a self-test: the verification offer
  says the phases track will "confirm the CI rule actually fails a synthetic
  violation" — document in the script header the one-liner to reproduce a
  synthetic violation locally.

## 7. M0 exit checklist (map to owner Accept list)

- [ ] Workspace per §1 (v1 subset), builds green both arches in CI.
- [ ] Proto round-trip property test green.
- [ ] Config: defaults test + per-rule failing fixtures + duty boundary test.
- [ ] `stream()` golden vectors identical on both arches (CI links recorded).
- [ ] Pad `legalize` property tests: idempotent + §1 invariants.
- [ ] `docs/proto-audit.md` committed, zero unresolved divergences.
- [ ] libm decision recorded.
- [ ] Beads AUDIT + M0 closed with evidence; SEED closed (doc fix + isj comment).
