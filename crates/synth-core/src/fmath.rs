//! Pinned transcendental math for sampling paths.
//!
//! M0 decision (IMPLEMENTATION-PLAN risk table, float nondeterminism): every
//! transcendental in a sampling path routes through the pure-Rust `libm`
//! crate so x86_64 and aarch64 produce identical bits. Do NOT call
//! `f64::ln`/`exp`/`sin`/`cos` (std libm may differ per platform) in
//! `synth-core`, `synth-pad`, or `synth-gen` decision paths — use these.
//! (`f64::sqrt` is IEEE-correctly-rounded and therefore bit-stable
//! everywhere; the [`sqrt`] wrapper exists for stylistic consistency and is
//! deliberately absent from clippy.toml's disallowed-methods list.)

pub fn ln(x: f64) -> f64 {
    libm::log(x)
}

pub fn exp(x: f64) -> f64 {
    libm::exp(x)
}

pub fn sqrt(x: f64) -> f64 {
    libm::sqrt(x)
}

pub fn sin(x: f64) -> f64 {
    libm::sin(x)
}

pub fn cos(x: f64) -> f64 {
    libm::cos(x)
}

/// Logistic function σ(x) = 1 / (1 + e^(−x)).
pub fn sigmoid(x: f64) -> f64 {
    1.0 / (1.0 + exp(-x))
}

/// logit(x) = ln(x / (1 − x)), the inverse of [`sigmoid`].
pub fn logit(x: f64) -> f64 {
    ln(x / (1.0 - x))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sigmoid_logit_roundtrip() {
        for &p in &[0.01, 0.2, 0.5, 0.9, 0.99] {
            assert!((sigmoid(logit(p)) - p).abs() < 1e-12);
        }
    }

    #[test]
    fn ln_exp_basics() {
        assert_eq!(ln(1.0), 0.0);
        assert!((exp(0.0) - 1.0).abs() < 1e-15);
        assert!((ln(exp(2.5)) - 2.5).abs() < 1e-12);
    }
}
