//! Generation context (domain form of `NodeContext`) and declarative
//! context conditioning (ARCHITECTURE.md §4.4).
//!
//! All conditioning is logit adjustment, so context-free operation is the
//! zero-adjustment special case. Missing feature ⇒ predicate false; a
//! generator must never fail because context is missing.

use synth_core::config::{CmpOp, ExperimentConfig, Predicate};
use synth_core::fmath;
use synth_core::types::PadBurst;

/// A previously-generated pad burst usable as a mutation base or donor
/// (ARCHITECTURE.md §5.2): the parent that spawned this node, or one of its
/// siblings. Carries the content-addressed `burst_id` alongside the pad body
/// so `MutationProvenance` can reference it without recomputing the hash
/// (the id is also what the orchestrator stored the burst under).
#[derive(Clone, Debug, PartialEq)]
pub struct ContextBurst {
    pub pad: PadBurst,
    pub burst_id: [u8; 32],
}

/// A sibling burst plus the score delta it earned (ARCHITECTURE.md §5.2
/// donor selection: sampled `∝ max(score_delta, ε)`).
#[derive(Clone, Debug, PartialEq)]
pub struct ScoredContextBurst {
    pub burst: ContextBurst,
    pub score_delta: f64,
}

/// Domain form of the request's `NodeContext`. Built at the proto boundary;
/// `ram_features` is normalized to a name-sorted `Vec` there (the proto field
/// is an unordered map and must not reach decision paths as one).
#[derive(Clone, Debug, Default)]
pub struct GenContext {
    pub node_id: String,
    /// Sorted by name, unique names.
    pub ram_features: Vec<(String, f64)>,
    /// Tail of the input history that led to this node (same model).
    pub recent_inputs: Option<PadBurst>,
    /// The burst that created this node (mutation base, ARCHITECTURE.md
    /// §5.2). `None` when this node has no known parent burst (e.g. root).
    pub parent_burst: Option<ContextBurst>,
    /// Bursts from the same parent whose children scored well, each with its
    /// `score_delta`. Empty when none are known. Mutation is unavailable iff
    /// both `parent_burst` and `sibling_bursts` are absent/empty.
    pub sibling_bursts: Vec<ScoredContextBurst>,
}

impl GenContext {
    pub fn feature(&self, name: &str) -> Option<f64> {
        self.ram_features
            .binary_search_by(|(n, _)| n.as_str().cmp(name))
            .ok()
            .map(|i| self.ram_features[i].1)
    }

    /// True iff `recent_inputs` shows a press (rising edge, or a burst-start
    /// hold) of `bit` within the last `frames` frames.
    pub fn pressed_within(&self, bit: u8, frames: u32) -> bool {
        let Some(pad) = &self.recent_inputs else {
            return false;
        };
        let mask = 1u16 << bit;
        let total: u64 = pad.total_frames();
        let window_start = total.saturating_sub(u64::from(frames));
        let mut frame: u64 = 0;
        let mut prev_set = false;
        for seg in &pad.segments {
            let set = seg.buttons & mask != 0;
            // A press occurs at `frame` when the bit turns on (or the burst
            // begins with it on). It counts if that instant is in-window.
            if set && !prev_set && frame >= window_start {
                return true;
            }
            // Also count a press that happened before the window but is
            // still held into the window? No: "a press within the last R
            // frames" is the edge, not the hold (refractory semantics —
            // ARCHITECTURE.md §4.4 example: stops START re-presses).
            prev_set = set;
            frame += u64::from(seg.hold_frames);
        }
        // Special case: bit held from frame 0 counts as a press at frame 0.
        if window_start == 0 {
            if let Some(first) = pad.segments.first() {
                if first.buttons & mask != 0 {
                    return true;
                }
            }
        }
        false
    }

    /// Mask held on the last frame of history (for hold continuation).
    pub fn last_frame_mask(&self) -> Option<u16> {
        self.recent_inputs
            .as_ref()
            .and_then(|p| p.segments.last())
            .map(|s| s.buttons)
    }
}

fn eval_cmp(op: CmpOp, lhs: f64, rhs: f64) -> bool {
    match op {
        CmpOp::Lt => lhs < rhs,
        CmpOp::Le => lhs <= rhs,
        CmpOp::Gt => lhs > rhs,
        CmpOp::Ge => lhs >= rhs,
        CmpOp::Eq => lhs == rhs,
        CmpOp::Ne => lhs != rhs,
    }
}

/// Evaluate a config predicate against the context. Missing feature ⇒ false.
pub fn eval_predicate(pred: &Predicate, ctx: &GenContext, cfg: &ExperimentConfig) -> bool {
    match pred {
        Predicate::Feature { feature, op, value } => ctx
            .feature(feature)
            .map(|v| eval_cmp(*op, v, *value))
            .unwrap_or(false),
        Predicate::History { history } => {
            let pw = &history.pressed_within;
            cfg.button_alphabet
                .bit(&pw.button)
                .map(|bit| ctx.pressed_within(bit, pw.frames))
                .unwrap_or(false)
        }
    }
}

/// Effective per-button duty cycles after context conditioning:
/// `π'_b = σ(logit(π_b) + Σ 1[pred] · w_b − refractory penalties)`.
/// Returns `(button_bit, duty', mean_hold)` for every declared button, in
/// alphabet declaration order. Buttons with duty 0 stay 0 (logit undefined).
pub fn effective_button_priors(cfg: &ExperimentConfig, ctx: &GenContext) -> Vec<(u8, f64, f64)> {
    let wr = &cfg.weighted_random;
    let mut out = Vec::with_capacity(cfg.button_alphabet.buttons.len());
    for b in &cfg.button_alphabet.buttons {
        let prior = wr.buttons.get(&b.name).unwrap_or(&wr.default_button);
        let (duty, mu) = (prior.duty, prior.mean_hold_frames);
        if duty <= 0.0 {
            out.push((b.bit, 0.0, mu));
            continue;
        }
        let mut logit = fmath::logit(duty);
        for rule in &cfg.context_rules {
            if let Some(w) = rule.adjust_buttons.get(&b.name) {
                if eval_predicate(&rule.when, ctx, cfg) {
                    logit += w;
                }
            }
        }
        for r in &cfg.refractory {
            if r.button == b.name && ctx.pressed_within(b.bit, r.frames) {
                logit -= r.logit_penalty;
            }
        }
        let mut duty = fmath::sigmoid(logit);
        // Keep the chain realizable: cap at the a=1 boundary for this mean
        // hold (ARCHITECTURE.md §4.2 — contextual boosts must not make the
        // requested duty unreachable).
        let bound = mu / (mu + 1.0);
        if duty > bound {
            duty = bound;
        }
        out.push((b.bit, duty, mu));
    }
    out
}

/// Effective direction priors after context conditioning:
/// `p'_i ∝ p_i · exp(Σ 1[pred] · w_i)`, renormalized. Returned in config
/// declaration order as `(name, weight)`; weights sum to 1 unless all are 0.
pub fn effective_direction_priors(cfg: &ExperimentConfig, ctx: &GenContext) -> Vec<(String, f64)> {
    let dir = &cfg.weighted_random.direction;
    let mut out: Vec<(String, f64)> = Vec::with_capacity(dir.priors.len());
    for (name, p) in &dir.priors {
        let mut logit_delta = 0.0;
        for rule in &cfg.context_rules {
            if let Some(w) = rule.adjust_directions.get(name) {
                if eval_predicate(&rule.when, ctx, cfg) {
                    logit_delta += w;
                }
            }
        }
        out.push((name.clone(), p * fmath::exp(logit_delta)));
    }
    let sum: f64 = out.iter().map(|(_, w)| w).sum();
    if sum > 0.0 {
        for (_, w) in &mut out {
            *w /= sum;
        }
    }
    out
}
