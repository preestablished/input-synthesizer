# 03 — M1: Pad Model + Weighted-Random Generator + gRPC Shell

Owner accept list: IMPLEMENTATION-PLAN §M1. Math: ARCHITECTURE §4 (normative,
including the derivations — implement the formulas as written). Stream labels:
§7.2 table, exactly.

## 1. `synth-pad` complete

- Button alphabet from config §5.1 (no hardcoded bits); direction group;
  exclusive groups; forbidden masks with configured `clear` bits.
- `legalize` (from M0) — deterministic, RNG-free: clear ALL bits of a violated
  exclusive group; apply forbidden-mask clears; `hold_frames ≥ 1`; merge
  adjacent equal masks; ≥ 1 segment; clamp total frames to
  `[min_frames, max_frames]` (extend a final neutral segment / truncate).
- `tokenize`/`detokenize` with log2 duration buckets
  (`bucket(d) = min(floor(log2(d)), 5)`; detokenize = bucket midpoint) —
  needed by M2 `token_steps`.

## 2. Weighted-random generator (`synth-gen/src/weighted_random.rs`)

Per §4.2–§4.5, all sampling run-length via geometric inverse-CDF
(`d = max(1, ceil(ln(1-u)/ln(1-p)))`, `p=1 ⇒ d=1`, through `synth-core::fmath`):

- Per-button two-state chains, `(π_b, μ_b)` → `(a_b, r_b)`; stationary
  Bernoulli(π) start, or continue `recent_inputs` last-frame state when
  `start_from_history` and history present.
- Direction: sticky semi-Markov categorical (prior `p`, `μ_dir`, stickiness
  `κ`, `diagonal_factor` adding a second legal direction).
- Context conditioning §4.4: logit adjustments from `context_rules` +
  `refractory` sugar; missing feature ⇒ predicate false; **a generator must
  never fail because context is missing** (context-free = zero adjustments).
- Length §4.5: lognormal around `length_hint` or config mean, clamped.
- Streams: `slot/{s}/len`, `slot/{s}/wr/btn/{bit}` (one per button),
  `slot/{s}/wr/dir`. Materialize change-points, cut at `L`, merge, `legalize`.

## 3. Mixer (`synth-gen/src/mixer.rs`)

§3.1 exactly: drop unavailable generators, renormalize, floors
`floor(w_g·K)`, remainder by sampling w/o replacement over fractional parts
(stream `"mix"`), then seeded Fisher–Yates slot permutation (same stream,
documented draw order). Emits `degraded[]` reasons
(`"no_macros_loaded"`, `"no_parent_burst"`, …).

## 4. Request pipeline + server (`synth-server`)

Pipeline per §3: effective config = loaded experiment config ⊕
`config_overrides_yaml` (validated); `fanout_root(seed, node_id)`; mixer plan;
per-slot generate→legalize→provenance; respond with bursts (slot order),
`config_fingerprint`, `SYNTH_VERSION`, seed echo, `degraded[]`. Slots may run
on rayon but results write into a pre-sized Vec by index (add rayon only if
trivial; sequential is acceptable at K≤256 — p99 budget is the test).

Server shell per §8: tonic on `:7430`; HTTP `:7431` `/healthz` + Prometheus
`/metrics` (the §8 minimum series); JSON logs via tracing (log
`{node_id, k, seed, config_fingerprint, per-generator slot counts, latency}`).
`Health` RPC per API §2.5. Errors per API §2.1 (`INVALID_ARGUMENT` naming the
exact field; `FAILED_PRECONDITION` for unloaded referenced packs; never fail on
missing optional context). Experiment configs load via `LoadMacroPack`
(`kind=EXPERIMENT_CONFIG`) — implement that arm of the RPC in M1 (macro-pack
arm lands in M2, grammar arm returns `INVALID_ARGUMENT "unsupported in v1"`...
no: return `UNIMPLEMENTED`-style `INVALID_ARGUMENT` with a clear message;
choose one and test it). `main.rs` binary with flags
`--grpc-addr`, `--http-addr`, optional `--load <path>[:kind]` repeatable for
standalone bring-up.

Statelessness test (§8): load config, propose, restart service in-process,
re-load same doc, propose again ⇒ identical bytes.

## 5. Acceptance (owner §M1 Accept, all CI-enforced on both arches)

1. **Golden-seed fixtures**: `testdata/golden/m1/*.json` — 50 recorded
   `(request, expected burst_id list)` pairs spanning: k ∈ {1,8,32,64},
   lengths, with/without context, with/without history, override merges.
   Generate once on x86_64 via a `cargo run -p synth-server --bin
   record-goldens` helper (or a test with `--ignored` record mode); replay
   test asserts byte-identical bursts (full burst bytes, not just ids — hash
   the full response). CI runs it on both arches.
2. **Statistical suite** (fixed seed, N=2000 bursts, `statrs` dev-dep), one
   test per owner bullet: per-button duty χ² (±10% rel, sized per plan
   testing-strategy note 2); hold-duration vs Geometric(1/μ) KS/χ² + mean
   ±10%; direction mean segment length ±10% + sticky-repeat rate κ ±0.05;
   zero illegal masks (hard assert); context rule test (boss_hp Y-duty shift
   `σ(logit(π)+1.2)` ±10%; START refractory <10% of unconditioned rate);
   length median ±10% of hint. Fixed seeds chosen once; if a tolerance
   marginally fails, resize N — never loosen beyond the owner numbers.
3. Context-free: same suite passes with `NodeContext{node_id}` only.
4. Bench: `ProposeBursts(K=32, L=300)` p99 < 5 ms — criterion bench or a
   simple 1000-iteration timing test that asserts the budget; run on both CI
   arches (hosted runners are noisy: use p99 over 1000 in-process calls, no
   network).
5. Mixer test: K ∈ {8,32,64}, default mix ⇒ per-generator counts = stratified
   floors ± 1.
6. In-process tonic integration test: real client↔server over a local socket
   exercising ProposeBursts happy path + each error path + Health.

Any cross-arch golden mismatch is **P0**: stop, diagnose (float path? map
order?), fix before proceeding.
