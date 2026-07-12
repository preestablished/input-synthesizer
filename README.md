# input-synthesizer

synth those inputs

## Workspace layout

- `crates/synth-proto` — re-exports `determinism_proto::inputsynth::v1` (the
  gRPC/wire contract); no logic of its own.
- `crates/synth-core` — `InputModel` trait, `Burst`/`Token` domain types,
  provenance helpers, RNG fan-out (`fanout_root`/`stream`), experiment-config
  parsing/validation/deep-merge/fingerprint, and `SYNTH_VERSION`.
- `crates/synth-pad` — pad input model: button alphabet, legality filter,
  segment encoding, tokenize/detokenize.
- `crates/synth-gen` — generators and mixer (weighted-random, macros,
  mutation, mixer).
- `crates/synth-server` — the tonic gRPC service, `/healthz` + `/metrics`,
  and the `main.rs` binary.

Path dependency: `determinism-proto` is consumed from
`../control-plane/crates/determinism-proto` (workspace dep,
`default-features = false`, `features = ["inputsynth"]`). CI checks out
control-plane as a sibling directory pinned to tag `proto-v0.2.0`, so an
unrelated control-plane `HEAD` change can never silently change the proto
this repo builds against.

## Running tests

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace
cargo test --workspace
```

CI (`.github/workflows/ci.yaml`) runs the same steps on both `x86_64`
(`ubuntu-latest`) and `aarch64` (`ubuntu-24.04-arm`) — see the RNG-stream and
proto round-trip goldens under `testdata/` for what the cross-arch matrix is
there to catch.

## Determinism rules

This service's core contract is bit-identical output for a given seed, on
both supported architectures. The normative rules (RNG fan-out via blake3
derive-key + ChaCha8, `libm`-only transcendentals in sampling paths, ordered
collections instead of `HashMap`/`HashSet`, canonical `postcard` encoding for
hashed/fingerprinted values, etc.) are documented in:

- [`docs/proto-audit.md`](docs/proto-audit.md) — proto consumption pattern
- [`docs/evidence/`](docs/evidence/) — v1-gate deployment + 1,000-call smoke
  records (image digests, transcripts)
- [`testdata/contract/pad-burst-fixtures.yaml`](testdata/contract/pad-burst-fixtures.yaml)
  — raw-segment pad-burst fixtures offered to downstream repos for
  burst→input-log contract tests (self-contained decoding key in the header)
  and field-by-field audit against the API contract.
- `~/.agents/projects/determinism/docs/input-synthesizer/ARCHITECTURE.md`
  (owner-maintained architecture doc; §7.2 rule 4 is the ordered-collections
  rule enforced by `clippy.toml`'s `disallowed-types`).

`clippy.toml` denies `std::collections::HashMap`/`HashSet` workspace-wide
(`-D warnings` promotes clippy's warn-by-default `disallowed_types` lint to a
hard CI failure) — use `IndexMap`/`IndexSet`/`BTreeMap`/`BTreeSet` instead.
prost-generated `map<>` fields in `determinism-proto` are external and not
linted; convert them to ordered types at the proto boundary in `synth-core`
before any decision-path use.

## Golden ↔ version rule

Any change under `testdata/` (golden fixtures — RNG-stream vectors, config
goldens, etc.) must be accompanied by a bump of the workspace version (the
`version` key under `[workspace.package]` in the root `Cargo.toml`; every
crate inherits it via `version.workspace = true`, and `SYNTH_VERSION` tracks
that same key). CI enforces this on pull requests via
`ci/check-golden-version.sh` — see that script's header comment for the
one-liner to reproduce a synthetic violation locally, and for the documented
boundary (direct pushes to `main` and rebased stacks skip the gate).
