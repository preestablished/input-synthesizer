# 07 — Consolidated Acceptance + Handback

## Acceptance checklist (from `02-requested-work.md` §Acceptance Criteria)

- [ ] Owner IMPLEMENTATION-PLAN Accept lists green for M0, M1, M2 (and M3 if
      reached) — see per-milestone files.
- [ ] **Phase 4 exit gate 4, all three clauses, each citing test names + CI
      run URLs:**
      - (a) identical bursts for identical (request, seed) across x86_64 and
        aarch64 → M1 golden-seed replay suite on both CI matrix arms.
      - (b) χ²/KS distribution tests green → M1 statistical suite test names.
      - (c) macro packs load and instantiate → M2 pack-load + instantiation
        golden test names.
- [ ] Proto audit table (`docs/proto-audit.md`), zero unresolved divergences.
- [ ] INTEGRATION.md seed rule matches `derive_synth_request_seed`; bead
      `exploration-orchestrator-isj` closed or commented ready-to-close.
- [ ] IMPLEMENTATION-PLAN header contradiction fixed; correction recorded.
- [ ] Goldens + statistical suite green on both arches, or aarch64 recorded
      as pending debt with a named blocker.
- [ ] v1 gate evidence: image digest, smoke transcript/counts, illegal-burst
      count source = hypervisor-side.
- [ ] All beads closed with `-r` evidence-linking reasons (M3 may stay open
      as a ready bead per `06-`).
- [ ] Burst fixtures offered to the hypervisor repo for their contract test
      (risk-table item): note in resolution which `testdata/` fixtures to use.

## Handback: write `.agents/requests/phase4-m0-m2-v1-generators/04-resolution.md`

Contents (per `03-verification-offer.md`):
- Git SHA per milestone (M0, M1, M2, v1 gate, M3-if-done).
- Bead IDs + final states.
- Proto audit table (or link to `docs/proto-audit.md` + control-plane SHA/tag).
- Seed-rule doc diff + `isj` coordination record.
- Owner-plan numbering doc fix (old → new text).
- Both-arch CI evidence (run URLs per suite).
- Exit-gate-4 evidence per clause (test names + runs).
- v1 gate evidence (image digest, smoke transcript, illegal-burst count
  source); if fallback mode, name the live-context rerun as the single open
  item and what unblocks it.
- M3 status (done with evidence / ready bead ID).

The phases track responds with `05-verification.md`; expect them to re-run
`cargo test` from a clean checkout at the recorded SHA, re-run golden +
statistical + pack suites, verify the CI golden↔version rule fails a synthetic
violation, and check the image digest reproduces from the SHA.
