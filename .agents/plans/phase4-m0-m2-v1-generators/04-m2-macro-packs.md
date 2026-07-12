# 04 — M2: Macro Packs + Macro Generator

Owner accept list: IMPLEMENTATION-PLAN §M2. Formats: API §3 (pack YAML),
runtime behavior: ARCHITECTURE §5.1.

## 1. Pack loader (`synth-gen/src/macros.rs` + synth-core config plumbing)

- Parse + validate API §3 documents: required fields, `[a-z0-9-]+` name,
  `model` match, `button_alphabet` must equal the experiment alphabet at
  propose time (load time can't know the experiment — validate pack-internal
  consistency at load; alphabet compatibility at propose), param domains
  (`enum`/`int{min,max}`/`scale{min,max}`), `steps` button names resolvable
  (against the pack's declared alphabet name via the loaded experiment config
  if present — else defer to propose-time; keep it simple: validate names
  against `$param` references and structure at load), `mirror` maps cover
  every enum param used in steps, `frames` literal-or-`$param` with int-domain
  param, `token_steps` alternative.
- Validation errors: `INVALID_ARGUMENT` with line/column (serde_yaml `Location`)
  + message per failure; **atomic** — any error loads nothing.
- `pack_id` = blake3-hex of the canonical UTF-8 document bytes (as received).
  Reload of identical doc = no-op, same id. Duplicate macro name across packs:
  latest wins + warning in response.
- `LoadMacroPack` handles all three kinds after M2: MACRO_PACK (here),
  EXPERIMENT_CONFIG (M1), EVENT_GRAMMAR → clear `INVALID_ARGUMENT`
  ("event_grammar documents are not supported until M5").

## 2. Instantiation (ARCHITECTURE §5.1)

Eligibility (predicate language of §4.4; no-predicate ⇒ always eligible;
predicate + no context ⇒ ineligible) → pick by weight (stream
`slot/{s}/macro/pick`) → bind params (stream `…/params`; uniform over domain)
→ instantiate `steps` (mirror substitution, `$param` frames, `scale` multiply
with round-half-to-even, clamp ≥1) or `token_steps` via `detokenize` → tail
padding with weighted-random (stream `…/tail`) when shorter than target L and
`pad_to_length` → `chain_n` concatenation (chain_index in provenance) →
legalize → `MacroProvenance { pack_id, macro_name, param_bindings,
macro_frames, tail_frames, chain_index }`.

## 3. Demo pack `packs/console16-movement-core.yaml`

Handwritten, `version: 1`, `kind: macro_pack`, `name: console16-movement-core`,
`model: pad`, `button_alphabet: console16-12btn-v1`, `source: handwritten`.
~10 macros using the API §5.1 demo alphabet names (A jump, B run/dash, Y
attack, d-pad, START/SELECT):

1. `long-jump` (dir param left/right, runup int, scale) — as the API §3 example
2. `charge-and-release` (charge int 60–240) — as the example
3. `ladder-climb` (dir up/down via mirror on UP/DOWN, duration param)
4. `door-enter` (UP tap + wait)
5. `menu-confirm` (START, wait, A tap — eligibility-free)
6. `dash-burst` (dir param, B hold short)
7. `dash-jump` (dir, B+A overlap)
8. `hop-chain` (3 short A taps with dir hold)
9. `turnaround-dash` (dir then mirrored dir, B held)
10. `duck-slide` (DOWN + B, duration param)

Weights: movement 1.5–2.0, menu 0.2. Give `long-jump` the eligibility example
(`on_ground eq 1`) and leave most predicate-free so context-free requests keep
a rich eligible set.

## 4. Acceptance (owner §M2 Accept)

- Valid fixtures load (incl. the demo pack itself — test loads
  `packs/console16-movement-core.yaml`); one fixture per schema violation
  fails atomically with line/column; identical reload no-op w/ same pack_id.
- Instantiation goldens (both arches): fixed seed ⇒ fixed bindings ⇒ fixed
  segments for `steps`, `token_steps`, mirror, scale cases —
  `testdata/golden/m2/*.json`.
- Eligibility tests: predicate macro skipped without matching context AND
  without any context; predicate-free always eligible.
- Provenance: `macro_frames + tail_frames == total frames`; `param_bindings`
  complete (every declared param bound).
- End-to-end: mix `{wr: 0.5, macro: 0.5}`, K=32 ⇒ 16/16 slot counts, every
  macro slot carries MacroProvenance, goldens hold.
- **Exit-gate-4 clause (c) evidence**: name the pack-load + instantiation
  tests in the resolution.
