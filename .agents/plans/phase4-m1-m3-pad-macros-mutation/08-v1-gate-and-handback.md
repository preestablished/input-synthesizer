# WP8 — v1 First-Boss Gate: Container Image, Smoke Posture, Evidence, Handback

**Goal:** assemble the v1 gate deliverables (IMPLEMENTATION-PLAN §M2 gate
note: **M1 + M2 + deployment artifact = v1**), record the Phase 4 exit-gate
evidence for the synthesizer chain (phase doc exit gate item 4), and leave a
clean handback.

Depends on: WP5 + WP6. WP7 evidence joins the bundle if M3 landed; it does
not block the v1 gate.

## 1. Container image

- `Dockerfile` at repo root: multi-stage (rust builder → distroless/debian
  slim runtime), builds `synth-server`, exposes 7430 (gRPC) + 7431 (HTTP),
  `ENTRYPOINT ["synth-server"]`. Build context needs the sibling
  `control-plane` path dep — handle it the same way CI does: a build stage
  that copies both checkouts (document the required layout:
  `docker build -f input-synthesizer/Dockerfile <parent-dir>` with the two
  repos as siblings), or a pre-`cargo vendor` step. Pick one, document it in
  the Dockerfile header.
- Image for "either host" (IMPLEMENTATION-PLAN v1 gate): build
  `linux/amd64`; `linux/arm64` via `docker buildx` where available. Mark:
  **special environment** — arm64 image build needs buildx+qemu or an arm
  host; if unavailable, record as deferred evidence alongside the WP2 CI
  debt (same lane, same rules).
- Smoke test (scripted, `ci/smoke-container.sh`): run the image, wait for
  `/healthz` 200, `grpcurl` (or a tiny client bin) `Health` → `SERVING`,
  `LoadMacroPack` the demo experiment config + shipped pack, one
  `ProposeBursts(K=8)` → 8 bursts with provenance. Runs in CI on the amd64
  leg.

## 2. Orchestrator smoke — posture, not execution

The v1 gate's "1 000 consecutive ProposeBursts calls with live
reference-workload contexts, zero errors, zero illegal bursts
(hypervisor-side validation count = 0)" requires the real orchestrator dev
loop and hypervisor — **externally gated, cross-repo**. This plan's posture
(mirroring state-scorer's joint-smoke handling):

- Deliver our half: the container image, the shipped pack, a demo experiment
  config fixture (`testdata/config/demo-experiment.yaml` copying API.md §5
  verbatim), and a `scripts/soak-propose.sh` (or small Rust bin) that fires
  1 000 sequential ProposeBursts with rotating seeds/contexts against a
  running instance and reports error/illegal counts (illegal = re-run
  `legalize` on each returned burst and diff — must be a no-op).
- Run the soak against our own instance with synthetic contexts and record
  the result (this is real evidence of the "zero errors" half; the "live
  reference-workload contexts + hypervisor validation" half stays pending
  until the orchestrator loop exists — Phase 5 wiring).
- Record the pending half explicitly in the bead and the evidence bundle;
  do not claim the v1 gate fully closed until the joint run happens.

## 3. Evidence bundle + handback

- `.agents/plans/phase4-m1-m3-pad-macros-mutation/09-evidence.md` (created
  at execution time, not now): per-milestone table — accept bullet (cite
  IMPLEMENTATION-PLAN section) → test name(s) → CI run URL per arch →
  deferred-evidence entries (aarch64 lane rung in effect, arm64 image,
  joint smoke, latency-on-Spark, and the `artifact_ref` load source —
  deferred for all of Phase 4 per WP5, `UNIMPLEMENTED` until control-plane
  registry integration).
- Documentation issues: list every doc-issue note filed during WP3/WP6/WP7
  (`~/.agents/projects/determinism/reviews/doc-issues-inputsynth-*.md` —
  detokenize bucket 5, policy Token facade, macro chain_index,
  ARCHITECTURE §8 staleness, mutation base-selection stream, donor ε) in
  the evidence bundle, each with its interim local pin.
- Beads: close WP/milestone beads with `-r` + evidence links; file
  follow-up beads for every deferred item and for M4 (mining), M5 (grammar),
  M6 (policy), and the WP6 grammar-kind scope decision if the minimal
  reading shipped.
- Final state: `git status` clean, pushed, CI green on `main` both legs (or
  documented lane), README updated with run/build/smoke commands and the
  port map.

## Acceptance criteria

- `docker build` succeeds; `ci/smoke-container.sh` green in CI (amd64).
- Soak script: 1 000 calls against local instance, zero errors, zero
  legalize diffs — output committed to the bead/evidence file.
- Evidence file exists and every M1/M2 (and M3 if done) accept bullet maps
  to named tests + run URLs; every deferred item has a bead.
- Handback summary — written as the **final section of `09-evidence.md`**
  in this plan directory (its home; there is no separate handback document),
  paired with the follow-up beads filed above — states plainly: what is
  verified, what is deferred and where that debt is recorded — no implied
  coverage that wasn't run.

## Failure guidance

- If the Docker build can't see the control-plane path dep, do NOT inline or
  fork the proto — fix the build context (sibling-checkout context or
  `cargo vendor`); the pin discipline from WP1 applies to images too.
- If the soak shows any legalize diff, that is a P0 determinism/validity bug,
  not a flake: capture the request fixture into `testdata/` as a regression
  golden before fixing.
- If buildx/qemu arm64 image builds OOM or crawl, defer with evidence rather
  than shipping an untested arm64 tag.
