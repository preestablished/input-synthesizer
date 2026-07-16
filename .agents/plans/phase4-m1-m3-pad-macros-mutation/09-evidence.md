# Evidence bundle — phase4-m1-m3-pad-macros-mutation

Dated 2026-07-16. This plan was authored 2026-07-15 against the then-current
**remote** state (`origin/main` at the Phase 0 skeleton). The work it
describes had already been implemented locally under the predecessor plan
`.agents/plans/phase4-m0-m2-v1-generators/` (M0–M3 + v1 gate, shipped
2026-07-12, 35 review rounds) and was awaiting the operator push of `main`.
This bundle therefore records a **verification of the shipped implementation
against this plan, package by package**, plus the gap fixes that closed the
deltas — it does not restate the per-milestone play-by-play, which lives in:

- `.agents/requests/phase4-m0-m2-v1-generators/04-resolution.md` — the
  authoritative per-round record: CI run table, exit-gate evidence with
  named tests, review rounds 1–35, open items.
- `docs/evidence/v1-smoke-2026-07-12.md` (+ three transcripts alongside) —
  the v1-gate smoke: 3 runs × 1000 ProposeBursts calls, 32,000 bursts,
  0 errors / 0 illegal, stable fingerprint.

## Per-package verification (2026-07-16, four independent audit passes)

| Package | Verdict | Notes |
|---|---|---|
| 01 proto drift | Satisfied + gap fixed | Drift was fixed by the executed plan's WP3 rewrite. Gap closed this round: `synth-proto` now re-exports `PROTO_VERSION` and `crates/synth-proto/tests/proto_pin.rs` pins `proto-v0.2.0` + the frozen `Burst`/`Provenance` field sets (the local control-plane sibling had already drifted 8 commits past the tag — the guard has live value). |
| 02 CI cross-arch + lints | Satisfied + gap fixed | Dual-arch matrix, fmt/clippy/build/test order, `clippy.toml` (exceeded: fmath `disallowed-methods` too). Gap closed this round: the golden↔version gate now runs as its own `golden-version-gate` job on **push as well as pull_request** (`fetch-depth: 0`; base = PR base SHA / `github.event.before`, merge-base fallback; `ci/check-golden-version.sh` takes `GOLDEN_GATE_BASE`). Deliberate supersession kept: gate scope is ALL of `testdata/`, not `testdata/golden/` only (CLAUDE.md guardrail). |
| 03 M0 core foundations | Satisfied | RNG fan-out is normative blake3/ChaCha8 in `synth_core::rng` (no separate synth-rng crate — structural supersession); config schema/validation/merge/fingerprint complete with 37 invalid fixtures. Gaps closed this round: `fingerprint_golden` recorded-literal test (encoding drift was previously silent) + doc note that serde field reordering is fingerprint-breaking. |
| 04 M1 weighted-random | Satisfied | Generator + 8-part statistical suite. Gap closed this round: `history_continuation_holds_across_boundary` semantic test (was only golden-pinned). |
| 05 M1 mixer/server/goldens | Satisfied | 50 propose-level goldens (`testdata/golden/m1/`), transport-free replay path, bench on both arches. Gap closed this round: `http_shell_serves_healthz_and_metrics` + `generator_unavailable_metric_increments_on_degradation` tests. Deferred (beads): k=256 golden fixture; propose-level macro-mix goldens (testdata changes ⇒ version-bump-coupled). |
| 06 M2 macro packs | Satisfied | Loader/registry/generator/shipped pack (`packs/console16-movement-core.yaml`, 10 macros). EVENT_GRAMMAR load is an exact-message INVALID_ARGUMENT until M5 (executed-plan decision, test-pinned). Deferred (bead): `steps`+`token_steps`-both invalid fixture + table row. |
| 07 M3 mutation | Satisfied | Seven operators, exact provenance replay (`apply_ops`), degradation. Deviations are documented + golden-pinned (retry on `slot/{s}/mut/retry`; donor-list replay signature). Owner-doc pins added this round: ε = 1e-6 and base-selection draws on `slot/{s}/mut/ops` (ARCHITECTURE §5.2, dated 2026-07-16). |
| 08 v1 gate + handback | Satisfied with named open items | Dockerfile + `scripts/build-image.sh` (amd64 built; arm64 via `--multi-arch`, recorded in the evidence doc); smoke via `scripts/smoke-harness/` (three transcripted runs supersede a CI container-smoke job — a CI job remains open as a bead). README gained run/build/smoke commands + 7430/7431 port map this round. |

## Open items (external, non-blocking per this plan's §08)

1. **Live-context smoke rerun** — blocked on reference-workload's capture
   corpus (`refwork-5tk`, itself on `refwork-20v`/`refwork-d7t.1`; GO given
   2026-07-12).
2. **Served-dev-loop smoke** — blocked on exploration-orchestrator chain
   `kk2` → `cww` (their config fixtures vs. our schema; consumer-side
   illegal-burst counter does not exist anywhere yet).

## Deferred work (tracked as beads in this repo)

Follow-up beads cover: M4 (macro mining), M5 (event grammar), M6 (policy
generator), CI container-smoke job, arm64 image publication, the
version-bump-coupled testdata additions (k=256 golden, macro-mix goldens,
steps+token_steps invalid fixture), and the two external smoke items above.
`bd list` is the live view.

## Owner-doc amendments made this round (dated 2026-07-16 comments)

- API.md §3: pinned `token_steps` bucket midpoints `[1, 2, 5, 11, 23, 48]`
  (bucket 5 → 48).
- ARCHITECTURE.md §5.2: pinned ε = 1e-6 and the base-selection stream
  assignment.
- ARCHITECTURE.md §3.1-adjacent: noted the wire field is `degraded` (proto
  wins over the doc's `degraded_generators`).
- ARCHITECTURE.md §8: noted `LoadExperimentConfig` does not exist; loading
  goes through `LoadMacroPack(kind=DOCUMENT_KIND_EXPERIMENT_CONFIG)`.
