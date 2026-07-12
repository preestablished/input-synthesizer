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

/// One applied mutation operator (API.md §2.4 `MutationOp`): the op name
/// plus every sampled argument, stringified deterministically. `args` is
/// sorted by key (never a proto `map<>` in decision paths — API.md §2.4's
/// wire `map<string,string>` is built from this sorted `Vec` at the server
/// boundary, same convention as `MacroProvenance.param_bindings`).
#[derive(Clone, Debug, PartialEq)]
pub struct MutationOpRec {
    pub op: String,
    /// Sorted by key.
    pub args: Vec<(String, String)>,
}

/// Domain form of API.md §2.4 `MutationProvenance` (ARCHITECTURE.md §5.2).
#[derive(Clone, Debug, PartialEq)]
pub struct MutationProvenance {
    /// The parent or sibling burst this mutant started from.
    pub base_burst_id: [u8; 32],
    /// The splice donor, if any op used one.
    pub donor_burst_id: Option<[u8; 32]>,
    pub base_was_sibling: bool,
    /// In application order; the forced retry pass (if triggered) is
    /// appended last with an extra `("retry", "1")` arg.
    pub ops: Vec<MutationOpRec>,
    /// Whether post-op length clamping changed anything.
    pub post_clamp: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Provenance {
    pub generator: GeneratorKind,
    pub slot: u32,
    /// Root stream label for the slot, e.g. `slot/7/wr/dir`.
    pub rng_stream: String,
    pub fallback_from: Option<GeneratorKind>,
    pub macro_: Option<MacroProvenance>,
    pub mutation: Option<MutationProvenance>,
    // PolicyProvenance lands with M6.
}

/// A generator whose weight was reallocated for a request, and why.
#[derive(Clone, Debug, PartialEq)]
pub struct Degraded {
    pub generator: GeneratorKind,
    pub reason: String,
}
