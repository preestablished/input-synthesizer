//! Config validation (API.md §5 tail): fail-fast at load with ALL errors
//! reported at once.

use super::{ConfigError, ExperimentConfig, ModelKindCfg};

pub fn validate(cfg: &ExperimentConfig) -> Result<(), Vec<ConfigError>> {
    let mut errors: Vec<ConfigError> = Vec::new();
    macro_rules! fail {
        ($($arg:tt)*) => {
            errors.push(ConfigError::Invalid(format!($($arg)*)))
        };
    }

    if cfg.version != 1 {
        fail!("version {} unsupported (expected 1)", cfg.version);
    }
    if cfg.kind != "experiment_config" {
        fail!("kind {:?} is not \"experiment_config\"", cfg.kind);
    }
    if cfg.experiment_id.is_empty() {
        fail!("experiment_id must be non-empty");
    }

    // ---- alphabet: bits unique, <= 16 buttons, groups reference declared ----
    let alphabet = &cfg.button_alphabet;
    if alphabet.buttons.is_empty() || alphabet.buttons.len() > 16 {
        fail!(
            "button_alphabet.buttons has {} entries (expected 1..=16)",
            alphabet.buttons.len()
        );
    }
    let mut seen_bits = 0u32;
    let mut declared: Vec<&str> = Vec::new();
    for b in &alphabet.buttons {
        if b.bit >= 16 {
            fail!("button {} has bit {} (must be < 16)", b.name, b.bit);
            continue;
        }
        if seen_bits & (1 << b.bit) != 0 {
            fail!("button {} reuses bit {}", b.name, b.bit);
        }
        seen_bits |= 1 << b.bit;
        if declared.contains(&b.name.as_str()) {
            fail!("button name {} declared twice", b.name);
        }
        declared.push(&b.name);
    }
    let is_declared = |name: &String| alphabet.buttons.iter().any(|b| &b.name == name);
    for g in &alphabet.exclusive_groups {
        for n in g.iter().filter(|n| !is_declared(n)) {
            fail!("exclusive_groups references undeclared button {n}");
        }
    }
    for f in &alphabet.forbidden_masks {
        for n in f.mask.iter().filter(|n| !is_declared(n)) {
            fail!("forbidden_masks.mask references undeclared button {n}");
        }
        for n in f.clear.iter().filter(|n| !is_declared(n)) {
            fail!("forbidden_masks.clear references undeclared button {n}");
        }
    }
    for n in alphabet.directions.group.iter().filter(|n| !is_declared(n)) {
        fail!("directions.group references undeclared button {n}");
    }

    // ---- weighted-random priors: the duty <= mu/(mu+1) inequality ----
    let mut priors: Vec<(&str, f64, f64)> = vec![(
        "default_button",
        cfg.weighted_random.default_button.duty,
        cfg.weighted_random.default_button.mean_hold_frames,
    )];
    for (name, prior) in &cfg.weighted_random.buttons {
        priors.push((name, prior.duty, prior.mean_hold_frames));
        if !is_declared(name) {
            fail!("weighted_random.buttons references undeclared button {name}");
        }
    }
    for (name, duty, mu) in priors {
        if !duty.is_finite() || !mu.is_finite() {
            fail!("button {name}: duty {duty} and mean_hold_frames {mu} must both be finite");
            continue;
        }
        if !(0.0..1.0).contains(&duty) {
            fail!("button {name}: duty {duty} must be in [0, 1)");
            continue;
        }
        if !(1.0..=216_000.0).contains(&mu) {
            fail!("button {name}: mean_hold_frames {mu} must be in [1, 216000]");
            continue;
        }
        let bound = mu / (mu + 1.0);
        // Reject only strictly-greater: duty == mu/(mu+1) (a == 1) is a
        // degenerate but valid chain (ARCHITECTURE.md §4.2).
        if duty > bound {
            fail!(
                "button {name}: duty {duty} > mean_hold/(mean_hold+1) = {bound} \
                 (requested duty cycle unreachable at mean hold {mu})"
            );
        }
    }
    let dir = &cfg.weighted_random.direction;
    if !dir.stickiness.is_finite() {
        fail!("direction.stickiness {} must be finite", dir.stickiness);
    } else if !(0.0..1.0).contains(&dir.stickiness) {
        fail!("direction.stickiness {} must be in [0, 1)", dir.stickiness);
    }
    if !dir.mean_hold_frames.is_finite() {
        fail!(
            "direction.mean_hold_frames {} must be finite",
            dir.mean_hold_frames
        );
    } else if !(1.0..=216_000.0).contains(&dir.mean_hold_frames) {
        fail!(
            "direction.mean_hold_frames {} must be in [1, 216000]",
            dir.mean_hold_frames
        );
    }
    if !dir.diagonal_factor.is_finite() {
        fail!(
            "direction.diagonal_factor {} must be finite",
            dir.diagonal_factor
        );
    }
    for (name, p) in &dir.priors {
        if !p.is_finite() {
            fail!("direction prior {name} value {p} must be finite");
            continue;
        }
        if *p < 0.0 {
            fail!("direction prior {name} is negative");
        }
        if name != "NEUTRAL" && !alphabet.directions.group.contains(name) {
            fail!("direction prior {name} is neither NEUTRAL nor in directions.group");
        }
    }

    // ---- generator mix ----
    let mix = &cfg.generator_mix;
    let weights = [mix.weighted_random, mix.macro_, mix.mutation, mix.policy];
    if weights.iter().any(|w| !w.is_finite()) {
        fail!("generator_mix values must be finite");
    }
    if weights.iter().any(|w| *w < 0.0) {
        fail!("generator_mix values must be >= 0");
    }
    if !weights.iter().any(|w| *w > 0.0) {
        fail!("generator_mix must have at least one weight > 0");
    }

    // ---- burst length ----
    let bl = &cfg.burst_len;
    if bl.min_frames == 0 {
        fail!("burst_len.min_frames must be >= 1");
    }
    if !(1..=216_000).contains(&bl.mean_frames) {
        fail!(
            "burst_len.mean_frames {} must be in [1, 216000]",
            bl.mean_frames
        );
    }
    if bl.max_frames < bl.min_frames {
        fail!(
            "burst_len.max_frames {} < min_frames {}",
            bl.max_frames,
            bl.min_frames
        );
    }
    // 216_000 frames = 1 hour at 60fps: an upper bound on the per-burst
    // resource footprint (frame buffers, legalize/tokenize work).
    if bl.max_frames > 216_000 {
        fail!(
            "burst_len.max_frames {} exceeds 216000 (1 hour at 60fps)",
            bl.max_frames
        );
    }
    if !bl.sigma.is_finite() {
        fail!("burst_len.sigma {} must be finite", bl.sigma);
    } else if bl.sigma <= 0.0 {
        fail!("burst_len.sigma {} must be > 0", bl.sigma);
    }

    // ---- context rules / refractory reference declared buttons ----
    for rule in &cfg.context_rules {
        for name in rule.adjust_buttons.keys().filter(|n| !is_declared(n)) {
            fail!("context_rules.adjust_buttons references undeclared button {name}");
        }
        for (name, v) in &rule.adjust_buttons {
            if !v.is_finite() {
                fail!("context_rules.adjust_buttons[{name}] value {v} must be finite");
            }
        }
        for name in rule.adjust_directions.keys() {
            if name != "NEUTRAL" && !alphabet.directions.group.contains(name) {
                fail!(
                    "context_rules.adjust_directions key {name} is neither NEUTRAL \
                     nor in directions.group"
                );
            }
        }
        for (name, v) in &rule.adjust_directions {
            if !v.is_finite() {
                fail!("context_rules.adjust_directions[{name}] value {v} must be finite");
            }
        }
    }
    for r in &cfg.refractory {
        if !is_declared(&r.button) {
            fail!("refractory references undeclared button {}", r.button);
        }
        if !r.logit_penalty.is_finite() {
            fail!(
                "refractory {}: logit_penalty {} must be finite",
                r.button,
                r.logit_penalty
            );
        }
    }

    // ---- macro / mutation ----
    if !(1..=4).contains(&cfg.macro_.chain_n) {
        fail!("macro.chain_n {} must be in 1..=4", cfg.macro_.chain_n);
    }
    const VALID_MUTATION_OPS: &[&str] = &[
        "perturb_timing",
        "extend",
        "flip_button",
        "splice",
        "truncate",
        "duplicate_segment",
        "swap_adjacent",
    ];
    for key in cfg.mutation.op_probs.keys() {
        if !VALID_MUTATION_OPS.contains(&key.as_str()) {
            fail!("mutation.op_probs has unknown key {key:?}");
        }
    }
    for (key, v) in &cfg.mutation.op_probs {
        if !v.is_finite() {
            fail!("mutation.op_probs[{key:?}] value {v} must be finite");
        }
        if *v < 0.0 {
            fail!("mutation.op_probs[{key:?}] value {v} is negative");
        }
    }
    let prob_sum: f64 = cfg.mutation.op_probs.values().sum();
    if (prob_sum - 1.0).abs() > 1e-9 {
        fail!("mutation.op_probs sums to {prob_sum} (must be 1 ± 1e-9)");
    }
    if !cfg.mutation.donor_bias.is_finite() {
        fail!(
            "mutation.donor_bias {} must be finite",
            cfg.mutation.donor_bias
        );
    } else if !(0.0..=1.0).contains(&cfg.mutation.donor_bias) {
        fail!(
            "mutation.donor_bias {} must be in [0, 1]",
            cfg.mutation.donor_bias
        );
    }
    if !cfg.mutation.timing_sigma.is_finite() {
        fail!(
            "mutation.timing_sigma {} must be finite",
            cfg.mutation.timing_sigma
        );
    } else if cfg.mutation.timing_sigma <= 0.0 {
        fail!(
            "mutation.timing_sigma {} must be > 0",
            cfg.mutation.timing_sigma
        );
    }
    if !cfg.mutation.ops_binomial.p.is_finite() {
        fail!(
            "mutation.ops_binomial.p {} must be finite",
            cfg.mutation.ops_binomial.p
        );
    } else if !(0.0..=1.0).contains(&cfg.mutation.ops_binomial.p) {
        fail!(
            "mutation.ops_binomial.p {} must be in [0, 1]",
            cfg.mutation.ops_binomial.p
        );
    }
    if cfg.mutation.ops_binomial.n > 64 {
        fail!(
            "mutation.ops_binomial.n {} must be <= 64",
            cfg.mutation.ops_binomial.n
        );
    }

    // ---- model / grammar ----
    if cfg.model == ModelKindCfg::EventGrammar && cfg.grammar_id.is_none() {
        fail!("model event_grammar requires grammar_id");
    }
    // (macro.packs loaded is checked at ProposeBursts time, not here, so load
    // order stays flexible — API.md §5.)

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}
