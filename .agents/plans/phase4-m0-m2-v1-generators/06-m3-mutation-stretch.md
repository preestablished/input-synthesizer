# 06 — M3: Mutation Generator (Stretch)

Gate rules: start ONLY after the v1 gate is recorded. If the execution window
closes first, leave bead M3 open/ready with a pointer to IMPLEMENTATION-PLAN
§M3 and this file — do not half-land it. M3 is a hard requirement before
Phase 5's gate run.

Owner accept list: IMPLEMENTATION-PLAN §M3. Spec: ARCHITECTURE §5.2 exactly —
operator table with probabilities, base/donor selection (`donor_bias`,
`∝ max(score_delta, ε)`), `n_ops = 1 + min(B,3)`, `B~Binomial(3,0.25)`,
post-clamp (recorded as `post_clamp=true`, not an op), legalize,
identical-mutant retry (one forced `perturb_timing`, at most one retry),
`MutationProvenance` with ordered ops + stringified sampled args.

Streams: `slot/{s}/mut/ops` (count + selection + base/donor pick),
`slot/{s}/mut/op/{i}` (i-th operator's arguments), and
`slot/{s}/mut/retry` (the forced `perturb_timing` retry pass). The retry MUST
use its own label: re-deriving an already-used label replays the identical
sequence and reproduces the identical mutant — a silent no-op — and would
violate the one-label-one-pass rule. Add `slot/{s}/mut/retry` to the repo's
stream-label documentation next to the §7.2 table entries.

Unavailability: no parent AND no siblings ⇒ generator unavailable, mixer
reallocates, `degraded[]` gets `"no_parent_burst"` (M1's mixer already
handles the mechanism; M3 adds the generator).

## Acceptance (owner §M3 Accept)

- Per-operator unit goldens (fixed seed ⇒ exact output burst per operator),
  both arches. Golden comparison over canonical forms — `MutationOp.args` is a
  proto `map<>` (HashMap wire order is nondeterministic); compare as sorted
  `Vec<(K,V)>` per the `00-` global rule.
- Operator frequency over 10,000 mutants matches `op_probs` (χ², p<0.001,
  fixed seed); mean ops/mutant = 1.75 ± 0.05.
- Property tests: every mutant legal; within length bounds; `burst_hash` ≠
  base after retry logic; splice-without-donor recorded as `extend`.
- **Provenance-replay test**: re-applying the recorded op list with recorded
  args to the recorded base reproduces the mutant exactly. This requires args
  capture to be complete — design `MutationOp.args` serialization first
  (every sampled value stringified deterministically, e.g. indices, sigma
  z-draws as bit-exact f64 hex, toggled bits, repeat counts).
- Degradation test: request without parent/siblings reallocates weight and
  reports `degraded[]`.
