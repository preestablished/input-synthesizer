//! The `InputModel` trait (ARCHITECTURE.md §2).

use crate::types::{Burst, Token};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelKind {
    Pad,
    Grammar,
}

/// A family of inputs the platform can inject. Implementations: `PadModel`
/// (`synth-pad`), `GrammarModel` (M5). All methods are pure given
/// `(&self, args)` — `&self` holds only loaded config.
pub trait InputModel: Send + Sync {
    /// Atomic sampling unit. Pad: `PadSegment`. Grammar: `GrammarEvent`.
    type Unit: Clone;

    /// Burst length in model-native units of time (pad: frames; grammar:
    /// events).
    fn burst_len(&self, b: &Burst) -> u64;

    /// Enforce hard validity. Must be idempotent, deterministic, and take no
    /// RNG. Every generator output passes through this before leaving the
    /// service.
    fn legalize(&self, b: Burst) -> Burst;

    /// Canonical token stream for mining and dedup. Stable across versions
    /// of this crate for a given `tokenizer_version`.
    fn tokenize(&self, b: &Burst) -> Vec<Token>;

    /// Inverse of `tokenize` up to duration-bucket quantization.
    /// `detokenize(tokenize(b))` need not equal `b` exactly.
    fn detokenize(&self, t: &[Token]) -> Burst;

    /// Stable content hash (BLAKE3 over the canonical postcard encoding of
    /// the versioned wire form).
    fn burst_hash(&self, b: &Burst) -> [u8; 32] {
        crate::types::burst_hash(b)
    }

    fn kind(&self) -> ModelKind;
}
