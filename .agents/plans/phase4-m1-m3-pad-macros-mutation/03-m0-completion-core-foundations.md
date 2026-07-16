# WP3 — M0 Completion: RNG Fan-out, Domain Types, Config, Pad Legalize

**Goal:** the M0 acceptance list (IMPLEMENTATION-PLAN §M0 — the owner of these
criteria) actually holds. The Phase 0 skeleton stubbed all of it. This package
delivers the pure foundations every generator builds on: normative RNG
fan-out, `synth-core` domain types + `InputModel` trait + canonical burst
hash, experiment-config parse/validate/deep-merge/fingerprint, and the
`synth-pad` model with `legalize`/`tokenize`/`detokenize`.

Depends on: WP1. Parallel with WP2 (WP2 owns `clippy.toml` + CI only; this
package owns the crate-level lint attributes, so the files stay disjoint).
Blocks WP4. WP2 is a predecessor of this package's **acceptance-close** —
see acceptance criteria.

Right-sizing: this is the largest package; land it as intermediate green
commits rather than one drop. Natural green-commit points, in order:
(1) `synth-rng` normative fan-out + goldens; (2) `synth-core` domain
types/trait/canonical hash; (3) `synth-core::config`
parse/validate/merge/fingerprint; (4) `synth-pad`
legalize/tokenize/detokenize. Each point leaves the workspace building and
its tests green, so a partial WP3 still lands green commits.

## 1. `synth-rng` — normative fan-out (ARCHITECTURE §7.1)

The existing xorshift `FanoutRng` does NOT match the normative §7.1 code
(blake3 derive-key root, `blake3::keyed_hash(root, label)` →
`ChaCha8Rng::from_seed`). The guidance "build on the existing fan-out, don't
replace it" yields here because the spec explicitly demands otherwise —
§7.1's Rust is normative. Keep the module, the API discipline, and the
golden-vector test style; replace the internals:

- `crates/synth-rng/Cargo.toml`: add `blake3`, `rand_chacha`, `rand_core`
  (pin current stable minor versions; `rand_chacha` must expose `ChaCha8Rng`).
- `crates/synth-rng/src/lib.rs`:
  - `pub fn fanout_root(seed: u64, node_id: &str) -> [u8; 32]` — blake3
    `new_derive_key("determinism.inputsynth.v1 2026 proposal root")`, update
    `seed.to_le_bytes()` then `node_id` bytes, finalize. Context string
    verbatim from §7.1.
  - `pub fn stream(root: &[u8; 32], label: &str) -> ChaCha8Rng` —
    `blake3::keyed_hash(root, label.as_bytes())` → `ChaCha8Rng::from_seed`.
  - Keep a deprecated shim or delete `FanoutRng` outright (nothing outside
    its own test uses it — verified); prefer delete, and replace
    `stream_is_stable` with the new golden test.
- Tests (`crates/synth-rng/src/lib.rs` or `tests/goldens.rs`):
  - `fanout_root_golden` — record the 32-byte root for
    `(seed=0x0123_4567_89AB_CDEF, node_id="node-a")` as a literal.
  - `stream_first_16_bytes_golden` — for labels `"mix"`, `"slot/0/len"`,
    `"slot/7/macro/params"`: record first 16 bytes drawn from each stream as
    literals (M0 accept: fixed seed + labels → fixed first-16-bytes vectors,
    identical on both architectures — ChaCha8 is integer-only, so any
    divergence is a build bug, which is exactly what the CI leg exists to
    catch).

## 2. `synth-core` — domain types, trait, canonical hash (ARCHITECTURE §2)

Rewrite `crates/synth-core/src/lib.rs` into modules (`types`, `config`,
`fingerprint`, `wire`):

- Domain types (serde-derived, NOT the prost types):
  `PadSegment{buttons: u16, hold_frames: u32}`, `PadBurst{segments:
  Vec<PadSegment>}`, `GrammarEvent`, `EventBurst`, `enum Burst{Pad(PadBurst),
  Event(EventBurst)}`, `Token{mask: u16, dur_bucket: u8}` (local type — see
  overview grounding note about the divergent `policy::v1::Token`). The
  facade divergence itself (handwritten `policy::v1::Token{token_id,
  logprob}` vs API.md §2.6's `Token{mask, dur_bucket}`) is a documentation
  defect that would otherwise be inherited by M6: file a doc-issue note at
  `~/.agents/projects/determinism/reviews/doc-issues-inputsynth-policy-token-facade.md`
  (per the `doc-issues-refwork-*.md` precedent there); the local type stays
  the interim implementation. List the filed issue in the WP8 evidence
  bundle.
- `trait InputModel: Send + Sync` per ARCHITECTURE §2: `type Unit`,
  `burst_len`, `legalize`, `tokenize`, `detokenize`, `burst_hash`, `kind()`.
  `kind()` returns the proto `ModelKind` (re-export from `synth_proto`).
  Only the pad implementation ships in Phase 4 (grammar is M5).
- Canonical `burst_id`: `burst_hash = blake3(postcard(body))` where the
  postcard encoding is of the **domain** body enum plus the
  `button_alphabet` string for pad (API.md §1: "BLAKE3-256 of the canonical
  postcard encoding of the oneof body" — pin the exact byte layout in a doc
  comment AND a golden test, because this hash is the provenance/lineage id
  everywhere). Add `postcard` + `serde` deps.
- `wire` module: lossless `domain ↔ synth_proto::v1` conversion
  (`to_proto_burst(&Burst, alphabet: &str) -> proto::Burst` sets
  `format_version = BURST_FORMAT_VERSION` and `burst_id = burst_hash`;
  `from_proto_burst` validates `buttons <= u16::MAX` etc.).
- `#![deny(clippy::disallowed_types)]` — this package owns the crate-level
  attributes for `synth-core`, `synth-rng`, `synth-pad` (WP2 owns only
  `clippy.toml` + CI, to keep the parallel packages off the same files); add
  the same attribute to `synth-rng`'s `lib.rs` in §1.

## 3. `synth-core::config` — experiment config (API.md §5)

- Serde structs mirroring §5.1–§5.9 with **all §5 defaults** encoded via
  `#[serde(default = ...)]`; ordered maps only (`IndexMap` for
  button/context maps — add `indexmap` with `serde` feature; `serde_yaml`
  for parsing; YAML superset means JSON is accepted for free).
- `parse(bytes) -> Result<ExperimentConfig, Vec<ConfigError>>` — fail-fast
  collection of ALL validation errors at once (API.md §5 validation list):
  - alphabet bits unique, ≤ 16 buttons; exclusive groups / forbidden masks /
    directions reference declared buttons;
  - per button: reject `duty > μ/(μ+1)` — error message must print the
    inequality with values (e.g. `duty 0.90 > mean_hold/(mean_hold+1) =
    0.857 for button A`); equality is valid and must load;
  - `generator_mix` values ≥ 0, at least one > 0;
  - `mutation.op_probs` sums to 1 ± 1e-9;
  - `model == event_grammar` ⇒ `grammar_id` present (grammar loading itself
    is M5; the rule still validates).
- `deep_merge(base, overrides_yaml) -> Result<ExperimentConfig, ...>` — maps
  merge, scalars/lists replace (API.md §5 last paragraph); re-validate the
  merged result with the same rules.
- `fingerprint(effective_config, sorted_pack_ids, synth_version) -> [u8; 32]`
  = `blake3(canonical-postcard(effective config) ‖ sorted pack_ids ‖
  synth_version)` (ARCHITECTURE §7.2 rule 5). Canonical-postcard requires the config
  struct's serde field order to be fixed — document that reordering fields is
  a fingerprint-breaking change.

## 4. `synth-pad` — new crate (ARCHITECTURE §2.1)

- `crates/synth-pad/{Cargo.toml, src/lib.rs}`; workspace member is picked up
  by the existing `members = ["crates/*"]` glob. Deps: `synth-core`,
  `synth-proto`. `#![forbid(unsafe_code)]`, `#![deny(clippy::disallowed_types)]`.
- `ButtonAlphabet` compiled from config §5.1: name→bit, exclusive-group
  bitmasks, forbidden masks with `clear` bits, direction-group bits.
- `PadModel: InputModel` with:
  - `legalize` (no RNG, deterministic, idempotent): clear ALL bits of any
    violated exclusive group; apply forbidden-mask `clear` bits; clamp
    `hold_frames ≥ 1`; merge adjacent equal masks; guarantee ≥ 1 segment
    (empty → single neutral segment of min length); clamp total frames to
    `[burst_len.min_frames, burst_len.max_frames]` (truncate tail /
    extend last segment).
  - `tokenize`/`detokenize` per ARCHITECTURE §6.2: `bucket(d) =
    min(floor(log2(d)), 5)` → ranges `[1],[2,3],[4,7],[8,15],[16,31],[32,∞)`;
    detokenize uses bucket midpoints. The midpoint of the open bucket 5
    `[32,∞)` is **spec-underdetermined**: pin it locally (suggest 48) in a
    doc comment + golden as the interim implementation, AND file a
    documentation issue — write a note at
    `~/.agents/projects/determinism/reviews/doc-issues-inputsynth-detokenize-bucket5.md`
    following the `doc-issues-refwork-*.md` precedent in that directory.
    Needed by M2 `token_steps`; list the filed issue in the WP8 evidence
    bundle.
  - `burst_len` = Σ `hold_frames`.

## 5. Tests (all pure, no network)

Exact names (module paths indicative):

- `synth-proto/tests/round_trip.rs::proto_round_trip_property` — proptest:
  arbitrary domain `PadBurst` → proto → encode → decode → domain, equal
  (M0 accept: prost encode/decode property test).
- `synth-core/tests/config.rs`:
  - `minimal_config_loads_with_all_defaults` — the §5 "empty config" document
    loads; assert a sample of defaulted values (`burst_len.mean_frames ==
    300`, `direction.stickiness == 0.55`, `mutation.ops_binomial == (3,
    0.25)`).
  - One failing-fixture test **per validation rule**, each asserting the
    exact error string, fixtures under `testdata/config/invalid/*.yaml`:
    `duty_exceeds_bound_rejected_with_inequality_message`,
    `duty_boundary_equality_loads`, `duplicate_bits_rejected`,
    `unknown_button_in_group_rejected`, `mix_all_zero_rejected`,
    `op_probs_sum_rejected`, `grammar_model_without_grammar_id_rejected`.
  - `deep_merge_maps_merge_scalars_replace`, `fingerprint_golden` (recorded
    32-byte literal for a fixed config + pack ids + version).
- `synth-rng` goldens per §1 above.
- `synth-pad/tests/legalize.rs`:
  - `legalize_idempotent_property` (proptest, arbitrary bursts),
  - `legalize_output_satisfies_api_invariants_property` — all API.md §1 pad
    invariants for arbitrary input,
  - `exclusive_group_clears_all_bits`, `forbidden_mask_clears_configured_bits`,
  - `tokenize_bucket_boundaries_golden` (d = 1,2,3,4,7,8,15,16,31,32,1000).

## Acceptance criteria

- `cargo test --workspace` green; `cargo clippy --workspace --all-targets --
  -D warnings` green; fmt clean.
- Both CI legs green (or WP2 fallback lane), proving the rng/hash goldens
  cross-arch. This bullet makes WP2 a predecessor of the **acceptance-close**
  (implementation may run in parallel): if WP3's code lands before WP2's
  aarch64 leg exists, the cross-arch bullet is satisfied retroactively by the
  first dual-leg run after WP2 lands — close the WP3 bead citing that run.
- Every M0 accept bullet in IMPLEMENTATION-PLAN §M0 maps to a named test
  above; note the mapping in the bead close.

## Failure guidance

- Postcard/serde canonicalization surprises (e.g. enum tag width): lock the
  encoding with byte-literal goldens FIRST, then treat any later golden
  change as a `synth_version`-bumping format change.
- If `IndexMap` ordering from YAML differs between serde_yaml versions, pin
  the serde_yaml version in the workspace and add a test asserting config
  file order is preserved.
- If proptest finds a legalize non-idempotence, fix legalize — never weaken
  the property. Shrunken cases become permanent regression tests.
