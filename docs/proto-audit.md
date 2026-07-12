# Proto Audit — `determinism/inputsynth/v1/synthesizer.proto` vs API.md §1–§2

Audited 2026-07-12 for request `phase4-m0-m2-v1-generators` (bead
`input-synthesizer-16k`). Result: **zero divergences**.

## Provenance of the audited tree

| Item | Value |
|---|---|
| control-plane HEAD at audit | `66f0f9fd8e0e7bb39fb3331be20c549cde96b2e8` |
| Tag `proto-v0.2.0` | `1a9fb946b48f6bf5b328823a5e2004aa075ff79c` |
| Publishing commit `261141b` | ancestor of `proto-v0.2.0` (verified `git merge-base --is-ancestor`) |
| API doc audited against | `~/.agents/projects/determinism/docs/input-synthesizer/API.md` §1–§2 |

Byte-identity checks (both ran clean, empty diff):

```
cd ../control-plane
diff <(git show proto-v0.2.0:proto/determinism/inputsynth/v1/synthesizer.proto) \
     proto/determinism/inputsynth/v1/synthesizer.proto
diff proto/determinism/inputsynth/v1/synthesizer.proto \
     crates/determinism-proto/proto/determinism/inputsynth/v1/synthesizer.proto
```

So the copy this repo consumes (via the `determinism-proto` crate path
dependency) is byte-identical to the pinned tag's top-level proto tree.
`determinism-proto`'s `build.rs` additionally self-checks that its packaged
copy matches the workspace `proto/` tree at build time.

## Consumption pattern

- `synth-proto` re-exports `determinism_proto::inputsynth::v1`; workspace dep:
  `determinism-proto = { path = "../control-plane/crates/determinism-proto",
  default-features = false }` with feature `inputsynth` (pulls `common` +
  prost/tonic codegen only for this package).
- ARCHITECTURE §1's "proto/ symlink/submodule of shared protos" is satisfied
  in spirit by the crate dependency: the crate embeds the same files and the
  buf breaking-change gate in control-plane guards their evolution.
- CI checks out control-plane at `ref: proto-v0.2.0` so an unrelated
  control-plane HEAD change cannot alter or break this repo's builds.
- Divergence policy: any needed schema change is a buf-gated change request in
  control-plane — never a local fork.

## Field-by-field match table

Method: every message, field (name, type, number) and enum in API.md §1–§2's
protobuf blocks compared against the proto file. All match verbatim.

| API.md item | Proto item | Match | Note |
|---|---|---|---|
| §1 `PadSegment{buttons=1,hold_frames=2}` | same | ✓ | |
| §1 `PadBurst{segments=1,button_alphabet=2}` | same | ✓ | |
| §1 `FieldValue` oneof `{int_val=1,enum_val=2,dur_ns=3,bytes_val=4}` | same | ✓ | |
| §1 `GrammarField{name=1,value=2}` | same | ✓ | |
| §1 `GrammarEvent{event_type=1,at_offset_ns=2,fields=3,payload=4}` | same | ✓ | |
| §1 `EventBurst{events=1,grammar_id=2}` | same | ✓ | |
| §1 `Burst{format_version=1,burst_id=2,oneof body{pad=3,event=4}}` | same | ✓ | |
| §2 `service InputSynthesizer` 4 RPCs | same | ✓ | ProposeBursts, LoadMacroPack, MineMacros, Health |
| §2.1 `ProposeBurstsRequest` fields 1–7 | same | ✓ | incl. `fixed64 seed = 5` |
| §2.1 `ModelKind` 0–2 | same | ✓ | |
| §2.1 `NodeContext` fields 1–10 | same | ✓ | `map<string,double> ram_features = 6` |
| §2.1 `ScoredBurst{burst=1,score_delta=2}` | same | ✓ | |
| §2.1 `ProvenancedBurst{burst=1,provenance=2}` | same | ✓ | |
| §2.1 `ProposeBurstsResponse` fields 1–5 | same | ✓ | |
| §2.1 `DegradedGenerator{generator=1,reason=2}` | same | ✓ | |
| §2.2 `LoadMacroPackRequest` oneof source + `kind=3` | same | ✓ | |
| §2.2 `DocumentKind` 0–3 | same | ✓ | |
| §2.2 `LoadMacroPackResponse` fields 1–3 | same | ✓ | |
| §2.3 `MineMacrosRequest` fields 1–3 | same | ✓ | |
| §2.3 `PathSample{expansions=1,terminal_score=2}` | same | ✓ | |
| §2.3 `MiningParams` fields 1–6 | same | ✓ | |
| §2.3 `MineMacrosResponse` fields 1–5 | same | ✓ | |
| §2.3 `MinedMacroStats` fields 1–6 | same | ✓ | |
| §2.4 `GeneratorKind` 0–4 | same | ✓ | |
| §2.4 `Provenance` fields 1–8 | same | ✓ | |
| §2.4 `MacroProvenance` fields 1–6 | same | ✓ | `map<string,string> param_bindings = 3` |
| §2.4 `MutationProvenance` fields 1–5 | same | ✓ | |
| §2.4 `MutationOp{op=1,args=2}` | same | ✓ | `map<string,string> args = 2` |
| §2.4 `PolicyProvenance` fields 1–4 | same | ✓ | |
| §2.5 `HealthRequest{}` / `HealthResponse` fields 1–7 + `Status` | same | ✓ | |
| §2.6 `determinism.policy.v1 PolicyServing` | `proto/determinism/policy/v1/policy_serving.proto` | ✓ (exists) | Policy generator is M6 — not consumed in v1; not field-audited here |

## Notes for implementers

- Proto `map<>` fields (`ram_features`, `param_bindings`, `args`) generate
  `std::collections::HashMap` (prost default; `build.rs` sets no `btree_map`)
  and encode in iteration (hash) order — **never byte-compare or hash raw wire
  encodings of messages containing populated maps**; convert to ordered forms
  at the synth-core boundary first.
- `BURST_FORMAT_VERSION = 1` is a doc constant, not a proto constant; defined
  in `synth-core`.
