#!/usr/bin/env bash
# ci/check-golden-version.sh
#
# Golden/version gate: if this PR touches testdata/ (golden fixtures) it
# must also bump the workspace version (the single `version = ...` line
# under [workspace.package] in the root Cargo.toml — all crates inherit it
# via `version.workspace = true`, and SYNTH_VERSION tracks it). This keeps
# golden vectors traceable to a specific published version.
#
# Intended for PR CI only. It diffs HEAD against the merge-base with
# origin/main; on a direct push to main (or a rebased stack where
# merge-base == HEAD) there is no range to check, so the gate is a no-op.
# That is an accepted boundary of this heuristic, not a bug: direct pushes
# to main and rebased stacks skip this gate.
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
# Note: verified against this repo's root Cargo.toml — the
# `version = "0.1.0"` line under [workspace.package] is the ONLY
# `^version = ` line in that file, so the grep below is unambiguous here.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

# Best-effort: make sure we actually have origin/main reachable. CI
# checkouts can be shallow, so try to deepen before falling back to a
# plain fetch.
git fetch --deepen=50 origin main >/dev/null 2>&1 || git fetch origin main >/dev/null 2>&1 || true

BASE="$(git merge-base HEAD origin/main 2>/dev/null || true)"
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
