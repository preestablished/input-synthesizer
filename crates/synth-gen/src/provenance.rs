//! Domain-side provenance (API.md §2.4). Ordered types only; the server
//! converts to proto at the boundary (proto `map<>` fields are built from
//! these sorted vectors).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeneratorKind {
    WeightedRandom,
    Macro,
    Mutation,
    Policy,
}

impl GeneratorKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::WeightedRandom => "weighted_random",
            Self::Macro => "macro",
            Self::Mutation => "mutation",
            Self::Policy => "policy",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MacroProvenance {
    pub pack_id: String,
    pub macro_name: String,
    /// Sorted by key.
    pub param_bindings: Vec<(String, String)>,
    pub macro_frames: u32,
    pub tail_frames: u32,
    pub chain_index: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Provenance {
    pub generator: GeneratorKind,
    pub slot: u32,
    /// Root stream label for the slot, e.g. `slot/7/wr/dir`.
    pub rng_stream: String,
    pub fallback_from: Option<GeneratorKind>,
    pub macro_: Option<MacroProvenance>,
    // MutationProvenance lands with M3; PolicyProvenance with M6.
}

/// A generator whose weight was reallocated for a request, and why.
#[derive(Clone, Debug, PartialEq)]
pub struct Degraded {
    pub generator: GeneratorKind,
    pub reason: String,
}
