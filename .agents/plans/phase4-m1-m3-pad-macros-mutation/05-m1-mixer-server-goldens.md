# WP5 — M1 (part 2): Mixer, ProposeBursts End-to-End, Serving Shell, Goldens

**Goal:** M1 complete per IMPLEMENTATION-PLAN §M1 (owner of the acceptance
list): mixer with stratified allocation, `ProposeBursts` end-to-end for
`MODEL_KIND_PAD` with provenance + `config_fingerprint`, `Health`, `/healthz`,
`/metrics`, JSON logs, 50 golden-seed fixtures byte-identical cross-arch, and
the p99 < 5 ms bench.

Depends on: WP4 (generator) and WP2 (cross-arch CI for the golden gate).
Blocks WP6.

## 1. `synth-gen/src/mixer.rs` (ARCHITECTURE §3.1)

- `allocate_slots(k: u32, weights: &[(GeneratorKind, f64)], root: &[u8;32])
  -> SlotPlan`:
  1. drop unavailable generators (caller passes availability; in M1 only
     `WeightedRandom` is available — macro arrives WP6, mutation WP7),
     renormalize;
  2. floors `n_g = floor(w_g·K)`; remainder sampled without replacement ∝
     fractional parts, stream `"mix"`;
  3. seeded Fisher–Yates slot permutation, same `"mix"` stream (draw order:
     remainder draws first, then permutation — document it; it is part of
     the format).
- Emits `Vec<DegradedGenerator>` (proto type) for every dropped generator
  with the API.md §2.1 reason strings (`"no_macros_loaded"`,
  `"no_parent_burst"`, ...).
- Test `mixer_floors_exact_pm_one` — for K ∈ {8, 32, 64} and the default mix,
  realized per-generator counts equal stratified floors ± 1 (M1 accept).
- Test `mixer_deterministic_for_seed` — same inputs → identical plan.

## 2. `synth-server` — real service (ARCHITECTURE §3, §8)

Files: `crates/synth-server/src/{lib.rs, main.rs, service.rs, http.rs,
metrics.rs}`; `Cargo.toml` gains `tokio`, `tonic`, `tonic-prost`, `prost`,
`prometheus`, `tracing`, `tracing-subscriber` (json feature), `axum` or
`hyper` for the HTTP side (implementer's choice; keep it minimal).

- Binary serves gRPC on `:7430`, HTTP `/healthz` + `/metrics` on `:7431`
  (suggested ports per §8).
- `impl input_synthesizer_server::InputSynthesizer for SynthService`:
  - Structure requirement (cross-arch gate survival): the ProposeBursts
    pipeline must be a **transport-free pure function** — e.g.
    `handle_propose(state, request) -> Result<response, ...>` in a non-tokio
    module — with the tonic impl a thin wrapper. The WP2 fallback ladder may
    narrow the aarch64 leg to pure crates; the golden replay must remain
    runnable without tokio/tonic so that narrowing never drops it.
    **Placement decision (pinned):** the pure pipeline entry (config
    resolution + mixer + provenance assembly) lives at `synth-gen` level, so
    the narrowed leg's package list (`synth-core synth-rng synth-pad
    synth-gen`) always contains the replay test; `synth-server` only wraps
    it. (`synth-gen` cannot depend on `synth-server`, so putting the entry in
    the server would place it outside any narrowed leg.)
  - `propose_bursts` — pipeline per ARCHITECTURE §3: resolve effective
    config (loaded experiment ⊕ `config_overrides_yaml` deep-merge), validate
    (`INVALID_ARGUMENT`: k=0 or >256, unknown experiment_id, model mismatch,
    malformed overrides — message names the exact field; the call NEVER
    fails for missing optional context), `fanout_root(seed, node_id)`,
    mixer plan, per-slot generate → legalize → provenance
    (`Provenance{generator, slot, rng_stream, config_fingerprint,
    fallback_from: 0, r#macro: None, mutation: None, policy: None}`),
    respond with exactly-k slot-ordered bursts, `config_fingerprint`,
    `synth_version`, seed echo, `degraded`. Slot results written into a
    pre-sized `Vec` by index (rayon optional — output must be identical
    either way).
  - `load_macro_pack` — this package implements
    `DOCUMENT_KIND_EXPERIMENT_CONFIG` only (validate via WP3 config parser;
    `document_id` = blake3-hex of the document bytes; atomic; identical
    reload → same id, no-op). `MACRO_PACK`/`EVENT_GRAMMAR` return
    `UNIMPLEMENTED` "unsupported until M2" for now — these are well-formed
    requests for not-yet-supported kinds, not malformed arguments (WP6
    completes the three kinds). `artifact_ref` source likewise returns
    `UNIMPLEMENTED` "artifact_ref unsupported in Phase 4" (control-plane
    registry integration is out of scope for all of Phase 4; this deferral
    is a named deferred item in WP8's list — record in bead). Related error
    semantics for later packages: from M2 onward, an experiment config whose
    `grammar_id` names an unloaded grammar fails at **propose** time with
    `FAILED_PRECONDITION` (parallel to `macro.packs`, WP6 §2); WP3's M0
    config validation keeps only the cheaper "`grammar_id` present" check.
  - `mine_macros` — `UNIMPLEMENTED` (M4).
  - `health` — `HealthResponse{status: Serving, synth_version,
    loaded_packs, loaded_experiments, policy_endpoint_up: false,
    policy_deterministic: false, mining_in_progress: false}`.
- `synth_version`: `env!("CARGO_PKG_VERSION")` from the workspace version —
  the same value the WP2 golden gate watches. Bump workspace version to
  `0.2.0` at M1 completion.
- Metrics (§8 minimum list): `synth_propose_requests_total{model}`,
  `synth_bursts_total{generator}`, `synth_propose_latency_seconds` histogram,
  `synth_generator_unavailable_total{generator,reason}`,
  `synth_macro_packs_loaded`, `synth_mine_runs_total`,
  `synth_policy_fallback_total`.
- JSON logs: every ProposeBursts logs `{node_id, k, seed,
  config_fingerprint, per-generator slot counts, latency}`.
- State: loaded configs/packs behind an `RwLock<BTreeMap<String, ...>>`;
  nothing else persisted (§8 statelessness).

## 3. Golden-seed fixtures (M1 accept, the spine)

- `testdata/golden/m1/` — **50** recorded fixtures. Each fixture: a full
  `ProposeBurstsRequest` (YAML or binary prost, implementer picks ONE format
  and documents it) + expected `burst_id` list + blake3 of the full encoded
  `ProposeBurstsResponse.bursts` bytes. Vary across fixtures: k ∈ {1, 8, 32,
  64, 256}, length_hint ∈ {0, 60, 300, 1800}, seeds, context-free vs
  context-rich (ram_features, recent_inputs, refractory-triggering
  histories), override merges.
- Record once (a `cargo run --bin record-goldens` dev tool or ignored test
  is fine — but review its output by hand before committing; goldens are
  evidence, not code output to trust blindly).
- `crates/synth-server/tests/golden_replay.rs::golden_seed_replay_all_50` —
  in-process tonic server + `InputSynthesizerClient` over a duplex channel;
  replays every fixture; asserts byte-identical burst ids + response-bytes
  hash. Runs on both CI legs (this is THE cross-arch gate; goldens are
  recorded values, so both arches assert the same literals). **The
  cross-arch golden assertion must survive any WP2 fallback-ladder scope
  narrowing**: if the arm leg runs only pure crates, replay the same
  fixtures through the transport-free `handle_propose` path (§2) from a
  non-tokio test target that the narrowed leg includes.
- WP2's `golden-version-gate` now has teeth: golden changes require a
  workspace version bump in the same PR.

## 4. Latency bench (M1 accept)

- `crates/synth-server/benches/propose.rs` (criterion): in-process
  `ProposeBursts(K=32, length_hint=300)`, default demo config, non-policy
  mix. Assert/record p99 < 5 ms. Run on both hosts where available; CI runs
  it non-gating (hosted-runner jitter) via `cargo bench -p synth-server --
  --sample-size 200` in a non-required job or a locally-attached transcript
  in the bead. Mark: **special environment** — the "both hosts" figure wants
  real x86_64 + aarch64 hardware (DGX Spark); qemu numbers are meaningless
  for latency, so under the WP2 fallback lane record aarch64 latency as
  deferred evidence, not a fake pass.

## Acceptance criteria

- `cargo test --workspace` green on both legs, including
  `golden_seed_replay_all_50`, `mixer_floors_exact_pm_one`,
  `mixer_deterministic_for_seed`, and an RPC-error-path test per
  `INVALID_ARGUMENT` case (`propose_rejects_k_zero`, `propose_rejects_k_257`,
  `propose_rejects_unknown_experiment`, `propose_rejects_model_mismatch`,
  `propose_never_fails_on_missing_context`).
- `/healthz` returns 200 and `/metrics` exposes the §8 metric names —
  integration test `http_shell_serves_healthz_and_metrics`.
- Bench evidence recorded (x86_64 mandatory; aarch64 per environment note).
- Workspace version bumped; M1 bead closed with `-r` listing: golden run URLs
  for both legs, bench numbers, stat-suite names from WP4.

## Failure guidance

- Golden mismatch between legs = the P0 class this project exists to kill.
  Bisect: WP3 rng goldens → WP4 lognormal path (libm decision) → prost
  encoding (should be impossible — deterministic) → any `HashMap`/iteration
  order that slipped past the lint (check prost map fields at the boundary).
- If p99 misses 5 ms, profile before optimizing: serialization dominating is
  the documented trap (§9) — pre-size buffers; the sampling math itself is
  microseconds.
- If in-process duplex transport fights tonic versions, bind to
  `127.0.0.1:0` instead; never let transport plumbing block the golden gate.
