#![forbid(unsafe_code)]

//! Core domain types, the `InputModel` trait, RNG fan-out, and experiment
//! configuration for the input synthesizer (ARCHITECTURE.md §2, §7; API.md §5).

pub mod config;
pub mod fmath;
pub mod model;
pub mod rng;
pub mod types;

/// Semver of the synthesizer build, echoed in every response and folded into
/// `config_fingerprint`. Tracks the workspace version: any golden change must
/// bump the root `[workspace.package] version` (CI-enforced).
pub const SYNTH_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Version of the burst wire format (API.md §1).
pub const BURST_FORMAT_VERSION: u32 = 1;
