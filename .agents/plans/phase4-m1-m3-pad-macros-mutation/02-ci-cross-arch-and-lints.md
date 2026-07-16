# WP2 — CI: aarch64 Leg, Clippy, HashMap Deny Lint, Golden-Version Gate

**Goal:** CI enforces the cross-arch determinism contract from M0 onward:
fmt + clippy (`-D warnings`) + build + test on **both x86_64 and aarch64**,
`std::collections::HashMap` denied in decision-path crates, and a gate that
fails any push or PR changing golden fixtures (`testdata/golden/`) without a
`synth_version` bump.

Depends on: WP1 (green baseline). Can run in parallel with WP3.

## Why

IMPLEMENTATION-PLAN §M0 requires CI on both architectures from day one (the
cross-host float/PRNG guarantee), and §M1's golden-seed acceptance is "run on
both architectures". Current `ci.yaml` is `ubuntu-latest` only, no clippy.
Sibling precedent: `state-scorer/.github/workflows/ci.yaml` uses a matrix
`runner: [ubuntu-latest, ubuntu-24.04-arm]` with an explicit comment that the
arm leg is free for public repos only and, if unprovisionable, is recorded as
pending CI debt without dropping the cross-arch assertions. Mirror that
mechanism.

## Files to create/modify

### 1. `.github/workflows/ci.yaml` — rewrite

- `rust` job matrix: `runner: [ubuntu-latest, ubuntu-24.04-arm]` (keep the
  state-scorer style comment about arm availability and the debt rule).
- Keep the two-checkout layout (`repo` + sibling `control-plane`) with
  `ref: proto-v0.2.0` on the control-plane checkout (done in WP1).
- Steps per leg, in order (separate steps, so a mid-step failure is visible):
  1. `cargo fmt --all -- --check`
  2. `cargo clippy --workspace --all-targets -- -D warnings`
  3. `cargo build --workspace`
  4. `cargo test --workspace`
- New `golden-version-gate` job running on **both** `pull_request` and
  `push` — this project's house practice is direct commits to `main` at
  green package boundaries (overview ground rules), so a PR-only gate would
  be vacuous. Runs `ci/check-golden-version.sh` (below) against
  `${{ github.event.pull_request.base.sha }}` on PRs and against the push's
  parent (`${{ github.event.before }}`, falling back to the merge-base for
  force-pushes/new branches) on push events. Plumbing requirements: the gate
  job's checkout uses `fetch-depth: 0` (the default shallow clone cannot
  resolve `github.event.before` or `git show $BASE:Cargo.toml`); base SHA =
  `github.event.before` unless it is all-zeros (new branch) or unknown to
  the local repo, in which case fall back to
  `git merge-base origin/<default-branch> HEAD`. The script keeps
  `set -euo pipefail` — a bad base must fail loudly, never be suppressed.

### 2. `clippy.toml` (repo root) — new

```toml
disallowed-types = [
  { path = "std::collections::HashMap", reason = "iteration order nondeterminism; use BTreeMap/IndexMap (ARCHITECTURE §7.2 rule 4)" },
  { path = "std::collections::HashSet", reason = "same; use BTreeSet" },
]
```

### 3. Crate-level deny attributes (owned by WP3/WP4, not this package)

WP2 owns `clippy.toml` + CI config **only** — it must not touch any crate's
`lib.rs`, because WP3 rewrites `synth-core` and `synth-rng` in parallel and
the two packages would collide on those files. The
`#![deny(clippy::disallowed_types)]` attributes land with the crates' owners:
`synth-core`, `synth-rng`, `synth-pad` in WP3; `synth-gen` in WP4 (both
packages' checklists already include them; ARCHITECTURE §7.2 rule 4 names
`synth-core`, `synth-gen`, `synth-mine` — extend to all sampling crates).
Until WP3/WP4 land, the `clippy.toml` disallow list is already enforced
CI-wide by the clippy step's `-D warnings`; the crate-level attributes add
defense-in-depth for local builds.
`synth-server` stays at warn (its non-decision plumbing may use tokio types),
but must never let a `HashMap` feed anything sampled or ordered in a response.
Note: prost-generated `map<>` fields are `HashMap` on the wire structs — that
is outside our decision paths; convert at the boundary (WP3 domain types use
`BTreeMap`) and never iterate a prost map directly in a sampling or ordering
path.

### 4. `ci/check-golden-version.sh` — new (bash, `set -euo pipefail`)

Logic: given base SHA, if `git diff --name-only $BASE...HEAD --
testdata/golden/` is non-empty AND the workspace version in root `Cargo.toml`
(`[workspace.package] version`, which is what `synth_version` reports —
pinned in WP5) is unchanged between base and HEAD, exit 1 with a message
naming the changed fixtures. Scope is `testdata/golden/` **only** — the rest
of `testdata/` holds validation fixtures, not goldens (WP3 adds
`testdata/config/invalid/*.yaml`, WP6 adds `testdata/packs/invalid/`), and
watching those would false-positive on every new negative-test fixture.
Vacuously green until `testdata/golden/` exists (WP5).

## aarch64 fallback ladder (mark: special environment)

The `ubuntu-24.04-arm` leg needs GitHub's arm runners — free for public
repos, and **this repo is public (verified via `gh repo view`)**, so rung 1
is expected to work; the rest of the ladder is contingency. In order of
preference:

1. Hosted arm runner works → done; both legs required for milestone
   acceptance from here on.
2. Hosted arm unavailable → add a `qemu-cross` job on `ubuntu-latest`:
   install `gcc-aarch64-linux-gnu` + `qemu-user`, `rustup target add
   aarch64-unknown-linux-gnu`, then
   `CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_RUNNER="qemu-aarch64 -L
   /usr/aarch64-linux-gnu"
   CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc
   cargo test --workspace --target aarch64-unknown-linux-gnu` (the `-L`
   sysroot flag is required — test binaries are dynamically linked and qemu
   must find the aarch64 loader/libc). This executes real aarch64 code paths
   (including the aarch64 libm the goldens must survive), just slower. Mark
   the job `continue-on-error: false` — it is a real gate, not advisory.
3. Neither works in CI → run the aarch64 tests locally and attach the
   transcript to the milestone bead. On this project's macOS host (Apple
   Silicon), the actionable path is
   `docker run --platform linux/arm64 rust:<ver>` — natively aarch64-linux,
   no qemu — mounting **both** sibling checkouts (`input-synthesizer` +
   `control-plane`) so the path dep resolves, then `cargo test --workspace`
   inside the container. Alternative: a real aarch64-linux box (the DGX
   Spark from Phase 0 preflight) — access must be arranged with the
   operator. Either way, record **pending CI debt** in the bead +
   `08-v1-gate-and-handback.md` evidence bundle, exactly like state-scorer
   did. Never drop or weaken the cross-arch assertions themselves.

Whatever rung is in effect, any narrowed test scope must still run the
golden replay — see failure guidance below and WP5 §3.

## Acceptance criteria

- `cargo clippy --workspace --all-targets -- -D warnings` — clean locally.
- Negative lint check (local only, not committed): add
  `let _m: std::collections::HashMap<u8, u8> = Default::default();` to
  `synth-core`, confirm `cargo clippy -- -D warnings` fails with
  `disallowed_types` (via `clippy.toml`; the crate-level attributes arrive
  with WP3/WP4), revert.
- Push a branch touching a dummy `testdata/golden/` file without a version
  bump → `golden-version-gate` fails; bump version → passes; a dummy file
  elsewhere under `testdata/` does NOT trip the gate. (Do this on a throwaway
  PR or via `act`/local run of the script with a synthetic base; record which.)
- CI green on both matrix legs, or fallback lane active with the debt
  recorded in the WP2 bead.
- Close the WP2 bead with `-r` linking the run URLs and stating which rung of
  the fallback ladder is in effect.

## Failure guidance

- Arm leg queues forever / "runner not available" → that is rung 1 failing;
  move to rung 2 rather than waiting indefinitely.
- Qemu leg fails only in `tonic`/network-flavored tests → scope the qemu leg
  to the pure crates (`cargo test -p synth-core -p synth-rng -p synth-pad
  -p synth-gen --target aarch64-unknown-linux-gnu`); the determinism contract
  lives in those crates. **Exception that must survive any narrowing:** the
  WP5 golden replay (`golden_seed_replay_all_50`) is THE cross-arch gate,
  and it lives in `synth-server` tests — a naive pure-crate scope would
  silently drop it. WP5 therefore requires a transport-free replay path (a
  pure `handle_propose(request) -> response` function callable from a
  non-tokio crate, or replay via direct pipeline invocation in `synth-gen`)
  so the golden assertion runs in every narrowed arm leg. Note the narrowed
  scope in the bead.
- Clippy noise from generated/prost code → prefer targeted `#[allow]` at the
  offending item with a comment; never blanket-allow a determinism lint in a
  decision-path crate.
