# WP6 — M2: Macro Packs, Loader, Macro Generator, Shipped Pack

**Goal:** M2 complete per IMPLEMENTATION-PLAN §M2 (owner of the acceptance
list): YAML macro-pack loader with content-hash `pack_id`, `LoadMacroPack`
handling all three document kinds, instantiation (params/mirror/scale/
`$param`/`token_steps`), tail padding, chaining, `MacroProvenance`, and the
shipped `packs/console16-movement-core.yaml`. M1 + M2 + the container image
(WP8) is the v1 first-boss gate.

Depends on: WP5. Blocks WP7.

## 1. Pack schema + loader (API.md §3)

Location: `crates/synth-gen/src/macros.rs` (ARCHITECTURE §1) with the serde
schema types in a `pack` submodule (or `synth-core::pack` if WP7 needs the
types without `synth-gen` — implementer's call; keep it in one place).

- Serde structs for the §3 document: `version`, `kind: macro_pack`, `name`
  (`[a-z0-9-]+`), `model`, `button_alphabet`, `source:
  handwritten|mined` (+ optional `mined` block), `macros[]` with `name`,
  `weight` (default 1.0), `tags`, `params[]` (domains `enum`, `int{min,max}`,
  `scale{min,max}`), optional `eligibility[]` predicates (same predicate
  language as WP4 §4.4), `steps[]` (`{hold: [names...], frames: int|$param}`)
  XOR `token_steps[]` (`{mask: [names...], dur_bucket}`), optional `mirror`
  maps, optional `stats`.
- `pack_id = blake3-hex(document bytes as loaded)` — no re-serialization.
- Validation, atomic (any error ⇒ nothing loads), errors carry line/column
  (serde_yaml `Location`): unknown button names vs the pack's alphabet,
  `$param` referencing undeclared/non-int params, mirror maps referencing
  undeclared enum params or unknown buttons, `steps` XOR `token_steps`,
  weight ≥ 0, `dur_bucket ≤ 5`.
- Duplicate macro names across loaded packs: latest load wins + warning
  string in `LoadMacroPackResponse.warnings` (API.md §3 rules).
- Reload of byte-identical doc: no-op, same `pack_id`.

## 2. `LoadMacroPack` — complete all three kinds (server)

- `DOCUMENT_KIND_MACRO_PACK` → loader above; `items_loaded` = macro count;
  Health `loaded_packs` gains the pack_id; `synth_macro_packs_loaded` gauge.
- `DOCUMENT_KIND_EXPERIMENT_CONFIG` → already done in WP5.
- `DOCUMENT_KIND_EVENT_GRAMMAR` → **scope decision, cite IMPLEMENTATION-PLAN
  §M2 as owner**: M2's headline says "LoadMacroPack for all three document
  kinds" while the grammar *sampler* is M5. Implement: parse + validate the
  §4 grammar schema, content-hash `grammar_id`, store it (usable by M5),
  `items_loaded` = event-type count — but no sampling anywhere. If the
  implementer finds the §4 validation surface disproportionate, the minimum
  honest reading is parse+hash+store with schema-shape validation only;
  record whichever reading was implemented in the bead and the WP8 evidence
  bundle.
- `FAILED_PRECONDITION` at ProposeBursts time (not load time) when the
  experiment config's `macro.packs` list names an unloaded pack (API.md §5
  validation note — load order is flexible). From M2 onward the same rule
  applies in parallel to `grammar_id`: an unloaded grammar at propose time
  is `FAILED_PRECONDITION` (WP3's M0 config validation keeps only the
  "present" check).
- Note also that `LoadMacroPack` doubling as the experiment-config/grammar
  loader papers over ARCHITECTURE §8's stale wording (a `LoadExperimentConfig`
  "sibling RPC" that does not exist in the frozen proto, and the
  `degraded_generators` field name vs the proto's `degraded`). That is a
  documentation defect, not an implementation choice: file a doc-issue note
  at
  `~/.agents/projects/determinism/reviews/doc-issues-inputsynth-architecture-s8-staleness.md`
  (per the `doc-issues-refwork-*.md` precedent there); the proto remains
  truth in the meantime. List the filed issue in the WP8 evidence bundle.

## 3. Macro generator (ARCHITECTURE §5.1)

`macros.rs` runtime path, streams exactly per §7.2:

1. Eligible set: model matches; `eligibility` predicate passes with context,
   or macro has no predicate. **With a predicate and no context ⇒
   ineligible** (API.md §3 comment — this asymmetry is an M2 accept bullet).
2. Pick by weight categorical, stream `"slot/{s}/macro/pick"`.
3. Bind params uniformly from domains, stream `"slot/{s}/macro/params"`.
4. Instantiate: name→bit via the experiment alphabet, `dir`-style enum
   params substituted through `mirror`, `$param` frames from bound int
   params, `scale` multiplies durations with round-half-to-even then clamp
   ≥ 1; `token_steps` via `PadModel::detokenize` (bucket midpoints, then
   scale).
5. If shorter than target L and `macro.pad_to_length` (default true): pad
   with the WP4 weighted-random tail, stream `"slot/{s}/macro/tail"`,
   conditioned on the macro's final mask as initial state.
6. `macro.chain_n` ∈ 1..=4: sample that many macros, concatenate, then pad;
   `chain_index` recorded per §2.4… note: one burst has ONE
   `MacroProvenance`; for chains record the FIRST macro's identity with
   `chain_index` semantics as the proto defines (`chain_index` = 0-based
   when `chain_n > 1`). The `chain_index` semantics for chains up to 4 are
   **spec-underdetermined** against API.md §2.4: pin the chosen reading
   locally in code docs + a golden as the interim implementation, AND file a
   documentation issue — write a note at
   `~/.agents/projects/determinism/reviews/doc-issues-inputsynth-macro-chain-index.md`
   following the `doc-issues-refwork-*.md` precedent in that directory; flag
   it in the bead and list it in the WP8 evidence bundle (the proto field
   set is the constraint: pack_id, macro_name, param_bindings, macro_frames,
   tail_frames, chain_index).
7. `MacroProvenance{pack_id, macro_name, param_bindings (stringified),
   macro_frames, tail_frames, chain_index}` on `Provenance.r#macro`;
   mixer availability: macro generator available iff ≥1 eligible macro
   (else weight reallocated + `degraded` reason `"no_macros_loaded"`).

## 4. Shipped pack — `packs/console16-movement-core.yaml`

~10 handwritten macros for the demo game against alphabet
`console16-12btn-v1` (API.md §5.1 demo table): long-jump (dir param
left/right, runup int, scale), charge-and-release (charge int), ladder-climb,
door-enter, menu-confirm, dash variants (≥2), plus enough movement primitives
to reach ~10. Use API.md §3's `long-jump` and `charge-and-release` examples
verbatim as the first two. Every macro must validate against the loader and
instantiate under the demo config.

## 5. Tests (M2 accept headlines → exact tests)

- `pack_valid_fixture_loads`, `pack_reload_identical_noop_same_id`.
- One fixture per schema violation under `testdata/packs/invalid/`, each
  asserting line/column presence + atomicity:
  `pack_unknown_button_rejected_with_location`,
  `pack_undeclared_param_rejected`, `pack_bad_mirror_rejected`,
  `pack_steps_and_token_steps_rejected`.
- Instantiation goldens (fixed seed ⇒ fixed bindings ⇒ fixed segments):
  `instantiate_steps_golden`, `instantiate_token_steps_golden`,
  `instantiate_mirror_golden`, `instantiate_scale_golden`.
- `eligibility_predicate_no_context_ineligible`,
  `eligibility_absent_always_eligible`,
  `eligibility_predicate_matching_context_eligible`.
- `provenance_frames_partition` — `macro_frames + tail_frames ==` total
  frames; `param_bindings` complete for every declared param.
- End-to-end `mix_half_wr_half_macro_k32_slots_16_16` — mix `{wr: 0.5,
  macro: 0.5}`, K=32 ⇒ 16/16 slot counts, every macro slot carries
  `MacroProvenance`, response goldens hold (add ≥5 macro-mix fixtures to
  `testdata/golden/m2/`, same replay harness as WP5).
- `shipped_pack_loads_and_all_macros_instantiate` — loads
  `packs/console16-movement-core.yaml`, instantiates every macro under the
  demo config with a fixed seed.
- Grammar-kind: `grammar_doc_loads_with_content_hash_id` (the §4 httpd
  fixture), `grammar_doc_invalid_rejected_atomically`.

## Acceptance criteria

- All tests above green on both CI legs (goldens cross-arch); clippy/fmt
  clean; golden-version gate satisfied (new goldens + version bump to
  `0.3.0` in the same PR).
- **Expected golden cascade:** the version bump invalidates ALL prior
  goldens (m1 included) because `config_fingerprint` embeds `synth_version`
  (ARCHITECTURE §7.2 rule 5) — this is NOT a determinism bug. Regenerate and
  hand-review every golden in the same commit as the bump; the `burst_id`
  lists inside each fixture must NOT change (only the fingerprint bytes).
- `Health` reports the loaded pack; metrics gauge moves.
- M2 bead closed with `-r`: test list + which grammar-kind reading shipped.

## Failure guidance

- Content-hash instability: hash the exact bytes received in
  `document_yaml`, never a re-serialization — if a test hashes differently
  on reload, something re-serialized.
- Round-half-to-even: use explicit banker's rounding
  (`f64::round_ties_even`), not `round()` — a golden will catch it; don't
  chase it blind.
- If 16/16 slot counts fail, the bug is availability ordering in the mixer
  (dropping/renormalizing before flooring) — re-read ARCHITECTURE §3.1 step
  order.
