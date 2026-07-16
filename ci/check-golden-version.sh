#!/usr/bin/env bash
# ci/check-golden-version.sh
#
# Golden/version gate: if a change touches testdata/ (golden fixtures) it
# must also bump the workspace version (the single `version = ...` line
# under [workspace.package] in the root Cargo.toml — all crates inherit it
# via `version.workspace = true`, and SYNTH_VERSION tracks it). This keeps
# golden vectors traceable to a specific published version.
#
# Runs on both PR and push CI (direct commits to main at green package
# boundaries are house practice here, so a PR-only gate would be vacuous).
# The base of the diff range is chosen as follows:
#   - If GOLDEN_GATE_BASE is set (CI passes the PR base SHA on
#     pull_request events and the push's parent, github.event.before, on
#     push events), diff HEAD against exactly that commit.
#   - Otherwise (local runs, or CI's fallback when the event SHA is
#     all-zeros/unknown, e.g. a brand-new branch or a force-push), diff
#     against the merge-base with origin/main. When that merge-base IS
#     HEAD there is no range to check and the gate is a no-op — the
#     accepted boundary of the fallback heuristic, not a bug.
#
# Reproduce a synthetic violation locally:
#   1. git checkout -b scratch-golden-check
#   2. touch testdata/some_new_file && git add testdata/some_new_file
#      git commit -m "scratch: touch testdata without version bump"
#   3. ci/check-golden-version.sh           # expect: exit 1, clear message
#   4. Edit the `version = "..."` line under [workspace.package] in the
#      root Cargo.toml, commit it, re-run                # expect: exit 0
#   5. Clean up: git checkout main && git branch -D scratch-golden-check
#
# Note: verified against this repo's root Cargo.toml — the `version = ...`
# line under [workspace.package] is the ONLY line in that file starting with
# `version = ` (workspace.dependencies entries are inline tables and never
# start a line with it), so the grep below is unambiguous here.
#
# Known boundary: `git fetch --deepen=50` only walks 50 commits past the
# current shallow boundary. A PR branched >50 commits behind main may fail
# to find a merge-base and fall into the "no range to check" no-op — rerun
# with a full fetch if the gate matters for such a branch.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

if [ -n "${GOLDEN_GATE_BASE:-}" ]; then
  # A bad explicit base must fail loudly, never degrade into the no-op path.
  BASE="$(git rev-parse --verify "${GOLDEN_GATE_BASE}^{commit}")"
else
  # Best-effort: make sure we actually have origin/main reachable. CI
  # checkouts can be shallow, so try to deepen before falling back to a
  # plain fetch.
  git fetch --deepen=50 origin main >/dev/null 2>&1 || git fetch origin main >/dev/null 2>&1 || true

  BASE="$(git merge-base HEAD origin/main 2>/dev/null || true)"
fi
HEAD_SHA="$(git rev-parse HEAD)"

if [ -z "$BASE" ] || [ "$BASE" = "$HEAD_SHA" ]; then
  echo "no range to check"
  exit 0
fi

CHANGED="$(git diff --name-only "$BASE"..HEAD)"

if ! printf '%s\n' "$CHANGED" | grep -qE '^testdata/'; then
  echo "no testdata/ changes in range; nothing to check"
  exit 0
fi

# Count distinct +/- `version = ` lines touched in the root Cargo.toml
# between BASE and HEAD. 0 lines = untouched; >=2 (a removed old line and
# an added new line, deduped by `sort -u`) = changed.
# `grep` exits 1 on no match; under `pipefail` that would otherwise trip
# `set -e` before we get to inspect the count, so neutralize it with `|| true`
# --- the assignment itself still captures whatever `wc -l` produced.
VERSION_LINE_COUNT="$(git diff "$BASE"..HEAD -- Cargo.toml | { grep -E '^[+-]version = ' || true; } | sort -u | wc -l)"

if [ "$VERSION_LINE_COUNT" -lt 2 ]; then
  echo "ERROR: testdata/ golden fixtures changed but the workspace version" >&2
  echo "       ([workspace.package] version in the root Cargo.toml) did not." >&2
  echo "       SYNTH_VERSION tracks that single version key — bump it" >&2
  echo "       alongside any golden/testdata change." >&2
  exit 1
fi

echo "testdata/ changed and workspace version was bumped: ok"
exit 0
