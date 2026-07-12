#![forbid(unsafe_code)]

//! Generators + mixer (ARCHITECTURE.md §3–§5).
//!
//! Determinism discipline: every sampled value comes from a labeled ChaCha8
//! stream derived from the request seed (`synth_core::rng`); draw order
//! within each stream is part of the format — changing it is a breaking
//! change that bumps `SYNTH_VERSION` and regenerates goldens.

pub mod context;
pub mod macros;
pub mod mixer;
pub mod mutation;
pub mod propose;
pub mod provenance;
pub mod weighted_random;
