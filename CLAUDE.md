# Project Instructions for AI Agents

This file provides instructions and context for AI coding agents working on this project.

<!-- BEGIN BEADS INTEGRATION v:1 profile:minimal hash:ca08a54f -->
## Beads Issue Tracker

This project uses **bd (beads)** for issue tracking. Run `bd prime` to see full workflow context and commands.

### Quick Reference

```bash
bd ready              # Find available work
bd show <id>          # View issue details
bd update <id> --claim  # Claim work
bd close <id>         # Complete work
```

### Rules

- Use `bd` for ALL task tracking — do NOT use TodoWrite, TaskCreate, or markdown TODO lists
- Run `bd prime` for detailed command reference and session close protocol
- Use `bd remember` for persistent knowledge — do NOT use MEMORY.md files

## Session Completion

**When ending a work session**, you MUST complete ALL steps below. Work is NOT complete until `git push` succeeds.

**MANDATORY WORKFLOW:**

1. **File issues for remaining work** - Create issues for anything that needs follow-up
2. **Run quality gates** (if code changed) - Tests, linters, builds
3. **Update issue status** - Close finished work, update in-progress items
4. **PUSH TO REMOTE** - This is MANDATORY:
   ```bash
   git pull --rebase
   bd dolt push
   git push
   git status  # MUST show "up to date with origin"
   ```
5. **Clean up** - Clear stashes, prune remote branches
6. **Verify** - All changes committed AND pushed
7. **Hand off** - Provide context for next session

**CRITICAL RULES:**
- Work is NOT complete until `git push` succeeds
- NEVER stop before pushing - that leaves work stranded locally
- NEVER say "ready to push when you are" - YOU must push
- If push fails, resolve and retry until it succeeds
<!-- END BEADS INTEGRATION -->


## Build & Test

Requires a sibling checkout of `control-plane` at tag `proto-v0.2.0`
(the workspace path-depends on `../control-plane/crates/determinism-proto`).

```bash
cargo test --workspace                                  # full suite
cargo clippy --workspace --all-targets -- -D warnings   # lint gate (CI)
cargo fmt --all -- --check                              # format gate (CI)
```

## Where the real docs are

Architecture/API/integration docs are NOT in this repo. See
`~/.agents/projects/determinism/docs/input-synthesizer/{ARCHITECTURE,API,INTEGRATION,IMPLEMENTATION-PLAN}.md`
— those are the normative specs this service implements. In-repo:
`README.md` (layout, determinism rules), `docs/proto-audit.md`.

## Determinism guardrails you cannot see from tests passing

- **fmath-only in sampling paths.** Never call `f64::ln/exp/sin/cos/tan/
  powf` (or other transcendentals) in `synth-core`/`synth-pad`/`synth-gen`
  decision paths — route through `synth_core::fmath` (pinned pure-Rust
  libm), or x86_64 and aarch64 diverge bit-for-bit. Enforced by clippy
  `disallowed-methods` (see `clippy.toml`); test code may `#[allow]` at
  the smallest scope when computing statistics.
- **The golden↔version CI gate covers ALL of `testdata/`** — schema
  fixtures under `testdata/config/` included, not only `testdata/golden/`.
  Any testdata diff in a PR needs a `[workspace.package] version` bump in
  the root `Cargo.toml` (`SYNTH_VERSION` tracks it).
- **Invalid-fixture table coupling.** `crates/synth-core/tests/
  config_acceptance.rs` enforces that every file in
  `testdata/config/invalid/` has a matching expected-error row in its
  table (and vice versa) — new fixtures and rows land in the same commit.
- **Goldens hash canonical domain forms**, never prost wire bytes (proto
  `map<>` fields encode in per-process HashMap order). See the golden test
  files' headers.
- **RNG stream contract**: one label, one consumer, one pass; draw order
  within a stream is part of the format (`synth-core/src/rng.rs` header,
  ARCHITECTURE.md §7.2). Changing any draw order = regenerate goldens +
  version bump in the same PR.
