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
        if !(0.0..1.0).contains(&duty) {
            fail!("button {name}: duty {duty} must be in [0, 1)");
            continue;
        }
        if mu < 1.0 {
            fail!("button {name}: mean_hold_frames {mu} must be >= 1");
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
    if !(0.0..1.0).contains(&dir.stickiness) {
        fail!("direction.stickiness {} must be in [0, 1)", dir.stickiness);
    }
    if dir.mean_hold_frames < 1.0 {
        fail!(
            "direction.mean_hold_frames {} must be >= 1",
            dir.mean_hold_frames
        );
    }
    for (name, p) in &dir.priors {
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
    if bl.max_frames < bl.min_frames {
        fail!(
            "burst_len.max_frames {} < min_frames {}",
            bl.max_frames,
            bl.min_frames
        );
    }
    if bl.sigma <= 0.0 {
        fail!("burst_len.sigma {} must be > 0", bl.sigma);
    }

    // ---- context rules / refractory reference declared buttons ----
    for rule in &cfg.context_rules {
        for name in rule.adjust_buttons.keys().filter(|n| !is_declared(n)) {
            fail!("context_rules.adjust_buttons references undeclared button {name}");
        }
        for name in rule.adjust_directions.keys() {
            if name != "NEUTRAL" && !alphabet.directions.group.contains(name) {
                fail!(
                    "context_rules.adjust_directions key {name} is neither NEUTRAL \
                     nor in directions.group"
                );
            }
        }
    }
    for r in &cfg.refractory {
        if !is_declared(&r.button) {
            fail!("refractory references undeclared button {}", r.button);
        }
    }

    // ---- macro / mutation ----
    if !(1..=4).contains(&cfg.macro_.chain_n) {
        fail!("macro.chain_n {} must be in 1..=4", cfg.macro_.chain_n);
    }
    let prob_sum: f64 = cfg.mutation.op_probs.values().sum();
    if (prob_sum - 1.0).abs() > 1e-9 {
        fail!("mutation.op_probs sums to {prob_sum} (must be 1 ± 1e-9)");
    }
    if !(0.0..=1.0).contains(&cfg.mutation.donor_bias) {
        fail!(
            "mutation.donor_bias {} must be in [0, 1]",
            cfg.mutation.donor_bias
        );
    }
    if cfg.mutation.timing_sigma <= 0.0 {
        fail!(
            "mutation.timing_sigma {} must be > 0",
            cfg.mutation.timing_sigma
        );
    }
    if !(0.0..=1.0).contains(&cfg.mutation.ops_binomial.p) {
        fail!(
            "mutation.ops_binomial.p {} must be in [0, 1]",
            cfg.mutation.ops_binomial.p
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
