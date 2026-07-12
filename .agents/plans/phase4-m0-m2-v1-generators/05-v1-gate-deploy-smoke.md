# 05 — v1 Gate: Deployment Artifact + Live Smoke

Owner gate line (IMPLEMENTATION-PLAN, after §M2): M1 + M2 + container image
for either host + 1,000 consecutive `ProposeBursts` calls against the real
orchestrator dev loop, zero errors, zero illegal bursts (hypervisor-side
validation count = 0).

## 1. Container image

- `Dockerfile`: multi-stage (rust builder → distroless/debian-slim runtime),
  builds `synth-server` binary; build context must include the control-plane
  checkout (build arg or a pre-vendored proto crate — simplest: build from a
  parent context containing both repos, or `COPY` a control-plane checkout
  made by the build script). Provide `scripts/build-image.sh` that clones/uses
  `../control-plane` at `proto-v0.2.0` and runs
  `docker build` (and `docker buildx build --platform linux/amd64,linux/arm64`
  when buildx is available; single-arch local build is acceptable evidence for
  "buildable for either host" if CI or buildx covers the other — record what
  was actually built and its digest).
- Image runs with `--load` of a config/pack set or empty (documents pushed via
  `LoadMacroPack` by the orchestrator per INTEGRATION §3 (B)).
- Record: image digest(s), the exact build command, the git SHA baked in
  (embed `SYNTH_VERSION` + git SHA in `Health.synth_version` / a log line).

## 2. Smoke choreography (coordinate, don't assume)

Counterpart: exploration-orchestrator dev loop (M0–M5 complete on fakes).
Their bead `cww` (async transport adapter) is THEIR half of real-endpoint
wiring; our deliverable is the endpoint + fixtures.

Preconditions to verify in the scheduled window:
1. Deployed Phase 3 stack up (bridge systemd unit, dh-workerd, snapstore copy
   at `~/.rbo73/m4-regen-20260707/`) — worker/snapstore are user processes
   that die on reboot; check and (re)start per that repo's runbooks.
2. Orchestrator dev loop configured with our endpoint (`:7430`).

Run: 1,000 consecutive `ProposeBursts` calls driven by the orchestrator loop.
Capture: request/response counts, error count (must be 0), the orchestrator's
logs, and the **hypervisor-side illegal-burst/validation count (must be 0 and
must come from the hypervisor side, not our self-report)** — identify the
hypervisor's validation counter with their runbook and cite its source.

## 3. Fallback mode (expected)

Live `NodeContext` fixtures are corpus-gated (refwork-czi/5tk). If still gated
when everything else is green: run the smoke in **context-free mode**
(`NodeContext` carrying only `node_id`), driven through the orchestrator loop
with its existing fake context store. Record explicitly in the resolution:
- that the smoke ran in fallback mode,
- the single named open item: "live-context smoke rerun", unblocked by
  reference-workload's corpus fulfillment (`refwork-czi`/`refwork-5tk`).

If `cww` (their async adapter) blocks driving through the orchestrator loop in
the window, coordinate with that repo first; do NOT substitute a self-authored
bare gRPC bombardment silently — if the orchestrator loop truly cannot drive
it yet, record that as a deviation with the evidence you could produce
(e.g. 1,000-call client-driven smoke) and name the orchestrator-driven rerun
as an open item. The gate's wording is "the real orchestrator dev loop"; only
the phases track can accept a substitute.

## 4. Evidence bundle (goes into `04-resolution.md`)

- Image digest + build command + SHA.
- Smoke transcript/counts + where the illegal-burst count was read from.
- Which mode (live-context vs context-free fallback) + open-item statement.
- Bead V1 closed with `-r` linking the above.
