# 01 — Tracking and Doc Fixes

## 1. Beads bring-up

In the repo root:

```bash
bd init   # prefix: input-synthesizer (accept default)
```

Create one bead per unit below (short titles, details in `-d`, `--silent`,
capture IDs into shell vars), then wire dependencies. Suggested set:

| Var | Title | -l | -p |
|---|---|---|---|
| SEED | `Reconcile INTEGRATION.md synth seed rule with orchestrator (isj)` | docs | 1 |
| HDR | `Fix IMPLEMENTATION-PLAN header/v1-gate milestone contradiction` | docs | 2 |
| AUDIT | `Proto audit: consumed determinism-proto tree vs proto-v0.2.0 vs API.md §1-§2` | analysis | 1 |
| M0 | `M0: scaffolding & contracts (crates, RNG fan-out, config, dual-arch CI)` | impl | 0 |
| M1 | `M1: pad model + weighted-random generator + gRPC shell` | impl | 0 |
| M2 | `M2: macro packs + macro generator + demo pack` | impl | 0 |
| V1 | `v1 gate: container image + 1000-call live smoke vs orchestrator` | testing | 0 |
| M3 | `M3 (stretch): mutation generator + provenance replay` | impl | 1 |

Dependency edges (`bd dep add CHILD PARENT`):
`M0←AUDIT`? No — AUDIT is part of M0's session but track it separately:
`AUDIT` has no parents; `M0` depends on `AUDIT` and `SEED`; `M1` on `M0`;
`M2` on `M1`; `V1` on `M2`; `M3` on `V1`. `HDR` independent.

Close each bead with `-r` linking evidence (commit SHA, CI run URL, or file).

## 2. Doc fix A — INTEGRATION.md seed rule (bead SEED, blocks M0)

File: `~/.agents/projects/determinism/docs/input-synthesizer/INTEGRATION.md`,
"(2) Seed derivation" (line ~80). Replace the stale rule

> `seed = blake3(experiment_seed ‖ node_id ‖ expansion_counter)[..8]`

with the orchestrator's implemented rule:

> The orchestrator computes the request seed as
> `derive_synth_request_seed(experiment_seed, batch_seq)` — the first
> `next_u64()` draw from `DeterministicRng::synth(experiment_seed, batch_seq)`
> (exploration-orchestrator `orch-core/src/rng.rs`). Node ids are not part of
> the request-seed rule; the synthesizer separately mixes `node_id` into
> `fanout_root(seed, node_id)` as defense in depth (ARCHITECTURE.md §7.1).

Keep the surrounding sentence about logging/replaying the seed sequence.
Do not change ARCHITECTURE §7 — the fan-out is ours and correct.

Then coordinate closure of `exploration-orchestrator-isj`: from
`~/git/preestablished/exploration-orchestrator`, comment on the bead
(`bd comment isj "input-synthesizer INTEGRATION.md reconciled to
derive_synth_request_seed in <commit/date>; ready to close on your side"`).
**They close it, not us.** If `bd comment` is unavailable, `bd update isj`
with a note, or record the coordination in our resolution with the exact text.

## 3. Doc fix B — owner-plan header contradiction (bead HDR)

File: `~/.agents/projects/determinism/docs/input-synthesizer/IMPLEMENTATION-PLAN.md`,
header paragraph: "the first-boss platform milestone (MAP.md step 4) needs
M1–M3 of this plan (weighted random + macro packs + the serving shell) — that
is v1." This contradicts the plan's own v1-gate line under M2 ("M1 + M2 +
deployment artifact") and the phase doc ("M1+M2 is the documented v1").

Fix the header to match the v1-gate line, e.g.:

> the first-boss platform milestone (MAP.md step 4) needs **M1–M2** of this
> plan plus the deployment artifact (weighted random + macro packs + the
> serving shell) — that is v1. Mutation (M3) follows immediately and is
> required before Phase 5's gate run.

Record the correction (old text → new text, date, reason) in the resolution
(`07-`). Do not silently resolve — the request explicitly asks for the record.

## 4. Doc fix C — this repo's docs

`README.md` is two lines; extend it at v1 with run instructions (see `05-`).
No other in-repo doc debt known.
