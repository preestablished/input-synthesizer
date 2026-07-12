//! Macro pack loader, registry, resolution, and instantiation
//! (ARCHITECTURE.md §5.1, API.md §3).
//!
//! Pipeline: [`load_pack`] parses + validates one YAML document into a
//! [`MacroPack`] (atomic: any validation error loads nothing); a
//! [`PackRegistry`] holds loaded packs by insertion order; [`resolve`] binds
//! a registry + experiment config into a [`ResolvedMacros`] (button names ->
//! bits, against the *experiment's* alphabet — packs only validate
//! button/param *structure* at load time, never bit assignment, since the
//! alphabet isn't known until propose time); [`generate_macro_burst`]
//! instantiates one burst.
//!
//! Draw-order contract for a macro slot (part of the wire format; changing
//! it bumps `SYNTH_VERSION` and invalidates `testdata/golden/m2/*`):
//! - stream `slot/{s}/len`: identical to the weighted-random draw
//!   (ARCHITECTURE.md §4.5) — one length target for the whole slot.
//! - stream `slot/{s}/macro/pick`: created ONCE per slot; one `u` per chain
//!   element (`cfg.macro.chain_n`, 1..=4), each `u` picking that element's
//!   macro by weighted-categorical over the eligible set. Chain element
//!   `i`'s draw follows element `i-1`'s on this same stream.
//! - stream `slot/{s}/macro/params`: created ONCE per slot; for each chain
//!   element in order, one `u` per declared param of *that element's* picked
//!   macro, in the macro's declaration order. Chain element `i`'s draws
//!   follow element `i-1`'s on this same stream.
//! - stream `slot/{s}/macro/tail`: only drawn when the concatenated chain is
//!   shorter than the slot's target length and `cfg.macro.pad_to_length`;
//!   draws the entire weighted-random tail from this ONE stream (direction
//!   track, then each non-direction button in alphabet declaration order —
//!   `weighted_random::generate_single_stream`), never from per-label
//!   streams.
//!
//! Provenance: only the first chained macro's identity/bindings are
//! recorded (`chain_index: 0`); `macro_frames` covers the whole concatenated
//! chain (API.md §2.4 carries one `MacroProvenance` per burst — a known
//! upstream ambiguity for `chain_n > 1`, noted in the M2 plan).

use indexmap::IndexMap;
use serde::Deserialize;

use synth_core::config::{ExperimentConfig, Predicate};
use synth_core::rng::{next_unit_f64, stream};
use synth_core::types::{Burst, PadBurst, PadSegment};

use crate::context::{self, GenContext};
use crate::provenance::MacroProvenance;
use crate::weighted_random;

// ---------------------------------------------------------------------
// Wire-level document shapes (API.md §3)
// ---------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct IntRange {
    pub min: i64,
    pub max: i64,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ScaleRange {
    pub min: f64,
    pub max: f64,
}

/// A declared macro parameter's domain: `{ enum: [..] }` | `{ int:
/// {min,max} }` | `{ scale: {min,max} }`. Hand-rolled `Deserialize` (rather
/// than derived externally-tagged) because `serde_yaml` 0.9's enum support
/// does not accept a plain single-key mapping for non-unit variants (it
/// expects a `!Tag`-style YAML tag) — parsing via an all-optional shadow
/// struct sidesteps that entirely.
#[derive(Clone, Debug, PartialEq)]
pub enum ParamDomain {
    Enum(Vec<String>),
    Int(IntRange),
    Scale(ScaleRange),
}

impl<'de> Deserialize<'de> for ParamDomain {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawDomain {
            #[serde(rename = "enum", default)]
            enum_: Option<Vec<String>>,
            #[serde(default)]
            int: Option<IntRange>,
            #[serde(default)]
            scale: Option<ScaleRange>,
        }
        let raw = RawDomain::deserialize(deserializer)?;
        match (raw.enum_, raw.int, raw.scale) {
            (Some(v), None, None) => Ok(ParamDomain::Enum(v)),
            (None, Some(r), None) => Ok(ParamDomain::Int(r)),
            (None, None, Some(r)) => Ok(ParamDomain::Scale(r)),
            _ => Err(serde::de::Error::custom(
                "param domain must set exactly one of enum/int/scale",
            )),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ParamDef {
    pub name: String,
    pub domain: ParamDomain,
}

/// `frames: 24` or `frames: $runup` (the `$` is stripped; `Param` holds the
/// bare param name). Hand-rolled `Deserialize`: a plain untagged
/// `{Literal(u32), Param(String)}` would keep the `$`, and every lookup
/// site expects the bare name declared in `params`.
#[derive(Clone, Debug, PartialEq)]
pub enum FrameSpec {
    Literal(u32),
    Param(String),
}

impl<'de> Deserialize<'de> for FrameSpec {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Int(u32),
            Str(String),
        }
        match Raw::deserialize(deserializer)? {
            Raw::Int(n) => Ok(FrameSpec::Literal(n)),
            Raw::Str(s) => match s.strip_prefix('$') {
                Some(name) => Ok(FrameSpec::Param(name.to_owned())),
                None => Err(serde::de::Error::custom(format!(
                    "frames {s:?} must be a literal integer or a $param reference"
                ))),
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct StepSpec {
    pub hold: Vec<String>,
    pub frames: FrameSpec,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct TokenStepSpec {
    pub mask: Vec<String>,
    pub dur_bucket: u8,
}

/// A macro's instantiation recipe: `steps` XOR `token_steps` (API.md §3).
#[derive(Clone, Debug, PartialEq)]
pub enum MacroSteps {
    Steps(Vec<StepSpec>),
    TokenSteps(Vec<TokenStepSpec>),
}

fn default_weight() -> f64 {
    1.0
}

#[derive(Clone, Debug, Deserialize)]
struct RawMacro {
    name: String,
    #[serde(default = "default_weight")]
    weight: f64,
    #[serde(default)]
    tags: Vec<String>,
    /// Mined-macro stats block: preserved but not validated (M2 scope is
    /// handwritten packs; the miner lands with `synth-mine`).
    #[serde(default)]
    #[allow(dead_code)]
    stats: Option<serde_yaml::Value>,
    #[serde(default)]
    params: Vec<ParamDef>,
    /// Feature-comparison predicates only (API.md §3 example); history
    /// predicates are rejected at validation — packs don't get input-history
    /// eligibility in M2.
    #[serde(default)]
    eligibility: Vec<Predicate>,
    #[serde(default)]
    steps: Option<Vec<StepSpec>>,
    #[serde(default)]
    token_steps: Option<Vec<TokenStepSpec>>,
    #[serde(default)]
    mirror: IndexMap<String, IndexMap<String, String>>,
}

#[derive(Clone, Debug, Deserialize)]
struct RawPack {
    version: u32,
    kind: String,
    name: String,
    model: String,
    button_alphabet: String,
    source: String,
    #[serde(default)]
    #[allow(dead_code)]
    mined: Option<serde_yaml::Value>,
    macros: Vec<RawMacro>,
}

// ---------------------------------------------------------------------
// Loaded (validated, still name-based) types
// ---------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct MacroDef {
    pub name: String,
    pub weight: f64,
    pub tags: Vec<String>,
    pub params: Vec<ParamDef>,
    pub eligibility: Vec<Predicate>,
    pub kind: MacroSteps,
    pub mirror: IndexMap<String, IndexMap<String, String>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MacroPack {
    /// blake3-hex of the raw document bytes as received.
    pub pack_id: String,
    pub name: String,
    pub button_alphabet: String,
    pub macros: Vec<MacroDef>,
}

#[derive(Debug, PartialEq)]
pub enum MacroPackError {
    Parse {
        message: String,
        line: Option<usize>,
        column: Option<usize>,
    },
    /// Every validation failure found, atomic (API.md §2.2): a document with
    /// any error loads nothing.
    Invalid(Vec<String>),
}

impl std::fmt::Display for MacroPackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse {
                message,
                line,
                column,
            } => {
                write!(f, "parse error: {message}")?;
                if let (Some(l), Some(c)) = (line, column) {
                    write!(f, " (line {l}, column {c})")?;
                }
                Ok(())
            }
            Self::Invalid(errs) => write!(f, "{}", errs.join("; ")),
        }
    }
}

impl std::error::Error for MacroPackError {}

fn valid_name(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Parse + validate one macro-pack document (API.md §3), collecting ALL
/// validation errors before returning. `pack_id` is the blake3-hex of
/// exactly the bytes passed in — reloading identical bytes yields the same
/// id (API.md §2.2).
pub fn load_pack(bytes: &[u8]) -> Result<MacroPack, MacroPackError> {
    let doc: RawPack = serde_yaml::from_slice(bytes).map_err(|e| {
        let loc = e.location();
        MacroPackError::Parse {
            message: e.to_string(),
            line: loc.as_ref().map(|l| l.line()),
            column: loc.as_ref().map(|l| l.column()),
        }
    })?;
    let macros = validate_pack(&doc).map_err(MacroPackError::Invalid)?;
    let pack_id = blake3::hash(bytes).to_hex().to_string();
    Ok(MacroPack {
        pack_id,
        name: doc.name,
        button_alphabet: doc.button_alphabet,
        macros,
    })
}

fn validate_pack(doc: &RawPack) -> Result<Vec<MacroDef>, Vec<String>> {
    let mut errors: Vec<String> = Vec::new();

    if doc.version != 1 {
        errors.push(format!("version {} unsupported (expected 1)", doc.version));
    }
    if doc.kind != "macro_pack" {
        errors.push(format!("kind {:?} is not \"macro_pack\"", doc.kind));
    }
    if !valid_name(&doc.name) {
        errors.push(format!("name {:?} must match [a-z0-9-]+", doc.name));
    }
    if doc.model == "event_grammar" {
        errors.push(
            "model \"event_grammar\" is not supported until M5 (event_grammar packs use \
             event_steps, not the pad `steps`/`token_steps` machinery)"
                .to_owned(),
        );
    } else if doc.model != "pad" {
        errors.push(format!(
            "model {:?} unsupported (expected \"pad\")",
            doc.model
        ));
    }
    if doc.source != "handwritten" && doc.source != "mined" {
        errors.push(format!(
            "source {:?} must be \"handwritten\" or \"mined\"",
            doc.source
        ));
    }
    if doc.macros.is_empty() {
        errors.push("macros must be non-empty".to_owned());
    }

    let mut seen_names: Vec<&str> = Vec::new();
    let mut out = Vec::with_capacity(doc.macros.len());
    for m in &doc.macros {
        if seen_names.contains(&m.name.as_str()) {
            errors.push(format!("duplicate macro name {:?} within pack", m.name));
        }
        seen_names.push(&m.name);

        if !m.weight.is_finite() {
            errors.push(format!(
                "macro {:?}: weight {} must be finite",
                m.name, m.weight
            ));
        } else if m.weight <= 0.0 {
            errors.push(format!(
                "macro {:?}: weight {} must be > 0",
                m.name, m.weight
            ));
        }

        let mut param_names: Vec<&str> = Vec::new();
        for p in &m.params {
            if param_names.contains(&p.name.as_str()) {
                errors.push(format!(
                    "macro {:?}: duplicate param name {:?}",
                    m.name, p.name
                ));
            }
            param_names.push(&p.name);
            match &p.domain {
                ParamDomain::Enum(values) => {
                    if values.is_empty() {
                        errors.push(format!(
                            "macro {:?}: param {:?} enum domain must be non-empty",
                            m.name, p.name
                        ));
                    }
                }
                ParamDomain::Int(r) => {
                    if r.min > r.max {
                        errors.push(format!(
                            "macro {:?}: param {:?} int domain min {} > max {}",
                            m.name, p.name, r.min, r.max
                        ));
                    } else {
                        // Fix #7: reject domains wide enough that the span
                        // computation (`generate_macro_burst`'s int-param
                        // sampling) would need more than a `u32`'s worth of
                        // distinct values — an extreme pack-declared range
                        // has no legitimate use case and only invites
                        // overflow-adjacent arithmetic downstream.
                        let span = i128::from(r.max) - i128::from(r.min) + 1;
                        if span > (1i128 << 32) {
                            errors.push(format!(
                                "macro {:?}: param {:?} int domain span {} exceeds 2^32",
                                m.name, p.name, span
                            ));
                        }
                    }
                }
                ParamDomain::Scale(r) => {
                    if !r.min.is_finite() || !r.max.is_finite() {
                        errors.push(format!(
                            "macro {:?}: param {:?} scale domain must be finite (min {}, max {})",
                            m.name, p.name, r.min, r.max
                        ));
                    }
                    if r.min > r.max {
                        errors.push(format!(
                            "macro {:?}: param {:?} scale domain min {} > max {}",
                            m.name, p.name, r.min, r.max
                        ));
                    }
                    if r.min <= 0.0 {
                        errors.push(format!(
                            "macro {:?}: param {:?} scale domain min {} must be > 0",
                            m.name, p.name, r.min
                        ));
                    }
                }
            }
        }

        for pred in &m.eligibility {
            if matches!(pred, Predicate::History { .. }) {
                errors.push(format!(
                    "macro {:?}: eligibility predicates must be feature comparisons \
                     (history predicates are not supported in macro packs)",
                    m.name
                ));
            }
        }

        let kind = match (&m.steps, &m.token_steps) {
            (Some(steps), None) => {
                if steps.is_empty() {
                    errors.push(format!("macro {:?}: steps must be non-empty", m.name));
                }
                Some(MacroSteps::Steps(steps.clone()))
            }
            (None, Some(token_steps)) => {
                if token_steps.is_empty() {
                    errors.push(format!("macro {:?}: token_steps must be non-empty", m.name));
                }
                for ts in token_steps {
                    if ts.dur_bucket > 5 {
                        errors.push(format!(
                            "macro {:?}: token_steps dur_bucket {} must be in 0..=5",
                            m.name, ts.dur_bucket
                        ));
                    }
                }
                Some(MacroSteps::TokenSteps(token_steps.clone()))
            }
            (Some(_), Some(_)) => {
                errors.push(format!(
                    "macro {:?}: steps and token_steps are mutually exclusive",
                    m.name
                ));
                None
            }
            (None, None) => {
                errors.push(format!(
                    "macro {:?}: must declare steps or token_steps",
                    m.name
                ));
                None
            }
        };

        if let Some(MacroSteps::Steps(steps)) = &kind {
            for s in steps {
                if let FrameSpec::Param(pname) = &s.frames {
                    match m.params.iter().find(|p| &p.name == pname) {
                        Some(p) if matches!(p.domain, ParamDomain::Int(_)) => {}
                        Some(_) => errors.push(format!(
                            "macro {:?}: frames ${pname} references a non-int-domain param",
                            m.name
                        )),
                        None => errors.push(format!(
                            "macro {:?}: frames ${pname} references an undeclared param",
                            m.name
                        )),
                    }
                }
            }
        }

        let mut referenced_names: Vec<&str> = Vec::new();
        match &kind {
            Some(MacroSteps::Steps(steps)) => {
                for s in steps {
                    referenced_names.extend(s.hold.iter().map(String::as_str));
                }
            }
            Some(MacroSteps::TokenSteps(token_steps)) => {
                for t in token_steps {
                    referenced_names.extend(t.mask.iter().map(String::as_str));
                }
            }
            None => {}
        }
        for n in referenced_names {
            let Some(p) = m.params.iter().find(|p| p.name == n) else {
                // Not a declared param: presumed a literal button name,
                // validated against the actual alphabet at resolve time.
                continue;
            };
            match &p.domain {
                ParamDomain::Enum(values) => match m.mirror.get(n) {
                    None => errors.push(format!(
                        "macro {:?}: enum param {n:?} used in hold/mask has no mirror map",
                        m.name
                    )),
                    Some(map) => {
                        for v in values {
                            if !map.contains_key(v) {
                                errors.push(format!(
                                    "macro {:?}: mirror[{n:?}] missing enum value {v:?}",
                                    m.name
                                ));
                            }
                        }
                    }
                },
                _ => errors.push(format!(
                    "macro {:?}: param {n:?} used in hold/mask must be enum-domain",
                    m.name
                )),
            }
        }

        if let Some(kind) = kind {
            out.push(MacroDef {
                name: m.name.clone(),
                weight: m.weight,
                tags: m.tags.clone(),
                params: m.params.clone(),
                eligibility: m.eligibility.clone(),
                kind,
                mirror: m.mirror.clone(),
            });
        }
    }

    if errors.is_empty() {
        Ok(out)
    } else {
        Err(errors)
    }
}

// ---------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------

#[derive(Default)]
pub struct PackRegistry {
    /// Insertion order = load order. Used for `pack_ids()` and pack lookup
    /// only — cross-pack duplicate-macro-name selection does NOT follow this
    /// order (`resolve`'s doc comment): it follows `cfg.macro.packs`'
    /// document order instead, so the winner is a function of the
    /// fingerprinted config alone, never of incidental load order.
    packs: Vec<MacroPack>,
}

impl PackRegistry {
    pub fn new() -> Self {
        Self { packs: Vec::new() }
    }

    /// Load (or replace, if `pack.pack_id` is already present — no-op
    /// semantics for the RPC live at the server) a pack. Returns warnings
    /// for any macro name that now shadows a same-name macro in a
    /// *different* already-loaded pack, and for a same-name pack
    /// replacement.
    pub fn insert(&mut self, pack: MacroPack) -> Vec<String> {
        if let Some(idx) = self.packs.iter().position(|p| p.pack_id == pack.pack_id) {
            self.packs[idx] = pack;
            return Vec::new();
        }

        let mut warnings = Vec::new();
        // A pack NAME owns exactly one document: loading different content
        // under an existing name replaces the old pack. Without this,
        // `get(name)` would resolve an ambiguous name by registry LOAD
        // order, which is not part of the config fingerprint — two replicas
        // with identical fingerprints could resolve different packs (the
        // same cross-process determinism bug the `macro.packs` list-order
        // rule closes for macro names).
        if let Some(idx) = self.packs.iter().position(|p| p.name == pack.name) {
            warnings.push(format!(
                "pack '{}' (id {}) replaced by id {} — a pack name owns one document",
                pack.name, self.packs[idx].pack_id, pack.pack_id
            ));
            self.packs.remove(idx);
        }
        for m in &pack.macros {
            for other in &self.packs {
                if other.macros.iter().any(|om| om.name == m.name) {
                    warnings.push(format!(
                        "macro '{}' shadows same-name macro in pack {}",
                        m.name, other.name
                    ));
                }
            }
        }
        self.packs.push(pack);
        warnings
    }

    /// Look up a pack by declared name OR pack_id.
    pub fn get(&self, name_or_id: &str) -> Option<&MacroPack> {
        self.packs
            .iter()
            .find(|p| p.name == name_or_id || p.pack_id == name_or_id)
    }

    /// Sorted pack ids, for fingerprinting (API.md §2.1/§7).
    pub fn pack_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.packs.iter().map(|p| p.pack_id.clone()).collect();
        ids.sort_unstable();
        ids
    }
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ResolveError {
    #[error("macro pack '{0}' is not loaded")]
    PackNotLoaded(String),
    #[error(
        "pack '{pack}' alphabet '{pack_alphabet}' does not match experiment alphabet '{cfg_alphabet}'"
    )]
    AlphabetMismatch {
        pack: String,
        pack_alphabet: String,
        cfg_alphabet: String,
    },
    #[error("macro '{macro_name}' references unresolvable button '{button}'")]
    UnresolvableButton { macro_name: String, button: String },
}

// ---------------------------------------------------------------------
// Resolved (bit-level) types
// ---------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub enum FrameBinding {
    Literal(u32),
    /// Index into the macro's declared `params`.
    Param(usize),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedStep {
    pub static_mask: u16,
    /// Param indices (enum-domain) contributing a mirror-resolved bit.
    pub enum_contribs: Vec<usize>,
    pub frames: FrameBinding,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedTokenStep {
    pub static_mask: u16,
    pub enum_contribs: Vec<usize>,
    pub dur_bucket: u8,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ResolvedKind {
    Steps(Vec<ResolvedStep>),
    TokenSteps(Vec<ResolvedTokenStep>),
}

#[derive(Clone, Debug, PartialEq)]
struct ResolvedMirror {
    param_idx: usize,
    values: IndexMap<String, u16>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedMacro {
    pub name: String,
    pub weight: f64,
    pub pack_id: String,
    pub params: Vec<ParamDef>,
    pub eligibility: Vec<Predicate>,
    pub kind: ResolvedKind,
    mirrors: Vec<ResolvedMirror>,
}

impl ResolvedMacro {
    fn mirror_bit(&self, param_idx: usize, value: &str) -> Option<u16> {
        self.mirrors
            .iter()
            .find(|m| m.param_idx == param_idx)
            .and_then(|m| m.values.get(value).copied())
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ResolvedMacros {
    pub items: Vec<ResolvedMacro>,
}

/// Resolve `cfg.macro.packs` against a loaded registry into an eligible,
/// bit-level candidate list: packs in `cfg.macro.packs` order, macros in
/// pack order, cross-pack duplicate names deduped by `cfg.macro.packs`'
/// OWN order — the pack listed LATER in that list wins, regardless of load
/// order. This is deliberate (fix for a cross-process determinism bug):
/// the fingerprint hashes the *sorted* pack-id set, which is order-blind,
/// so if the winner depended on registry insertion order ("latest load
/// wins"), two processes that loaded the same packs in a different order
/// could produce an identical fingerprint for different resolved macro
/// sets. Keying the winner off `cfg.macro.packs`' own document order makes
/// the winner a pure function of the fingerprinted config, never of
/// incidental load order. Within one pack, macros keep pack order (no
/// duplicate names within a single pack — rejected at load time).
pub fn resolve(
    registry: &PackRegistry,
    cfg: &ExperimentConfig,
) -> Result<ResolvedMacros, ResolveError> {
    let mut winners: IndexMap<String, (&MacroDef, &MacroPack)> = IndexMap::new();
    for pack_name in &cfg.macro_.packs {
        let pack = registry
            .get(pack_name)
            .ok_or_else(|| ResolveError::PackNotLoaded(pack_name.clone()))?;
        if pack.button_alphabet != cfg.button_alphabet.name {
            return Err(ResolveError::AlphabetMismatch {
                pack: pack.name.clone(),
                pack_alphabet: pack.button_alphabet.clone(),
                cfg_alphabet: cfg.button_alphabet.name.clone(),
            });
        }
        for m in &pack.macros {
            // Forward iteration over `cfg.macro.packs`, unconditional
            // overwrite: the pack listed later in the config always wins.
            winners.insert(m.name.clone(), (m, pack));
        }
    }

    let mut items = Vec::with_capacity(winners.len());
    for (_, (m, pack)) in winners {
        items.push(resolve_macro(m, pack, cfg)?);
    }
    Ok(ResolvedMacros { items })
}

fn resolve_macro(
    m: &MacroDef,
    pack: &MacroPack,
    cfg: &ExperimentConfig,
) -> Result<ResolvedMacro, ResolveError> {
    let bit = |name: &str| -> Result<u16, ResolveError> {
        cfg.button_alphabet
            .bit(name)
            .map(|b| 1u16 << b)
            .ok_or_else(|| ResolveError::UnresolvableButton {
                macro_name: m.name.clone(),
                button: name.to_owned(),
            })
    };

    let mut mirrors = Vec::new();
    for (param_idx, p) in m.params.iter().enumerate() {
        if let ParamDomain::Enum(values) = &p.domain {
            if let Some(map) = m.mirror.get(&p.name) {
                let mut resolved_values = IndexMap::new();
                for v in values {
                    if let Some(button_name) = map.get(v) {
                        resolved_values.insert(v.clone(), bit(button_name)?);
                    }
                }
                mirrors.push(ResolvedMirror {
                    param_idx,
                    values: resolved_values,
                });
            }
        }
    }

    let resolve_names = |names: &[String]| -> Result<(u16, Vec<usize>), ResolveError> {
        let mut static_mask = 0u16;
        let mut enum_contribs = Vec::new();
        for n in names {
            if let Some(idx) = m.params.iter().position(|p| &p.name == n) {
                enum_contribs.push(idx);
            } else {
                static_mask |= bit(n)?;
            }
        }
        Ok((static_mask, enum_contribs))
    };

    let kind = match &m.kind {
        MacroSteps::Steps(steps) => {
            let mut out = Vec::with_capacity(steps.len());
            for s in steps {
                let (static_mask, enum_contribs) = resolve_names(&s.hold)?;
                let frames = match &s.frames {
                    FrameSpec::Literal(n) => FrameBinding::Literal(*n),
                    FrameSpec::Param(pname) => {
                        let idx = m
                            .params
                            .iter()
                            .position(|p| &p.name == pname)
                            .expect("validated: $param resolves to a declared int param");
                        FrameBinding::Param(idx)
                    }
                };
                out.push(ResolvedStep {
                    static_mask,
                    enum_contribs,
                    frames,
                });
            }
            ResolvedKind::Steps(out)
        }
        MacroSteps::TokenSteps(token_steps) => {
            let mut out = Vec::with_capacity(token_steps.len());
            for t in token_steps {
                let (static_mask, enum_contribs) = resolve_names(&t.mask)?;
                out.push(ResolvedTokenStep {
                    static_mask,
                    enum_contribs,
                    dur_bucket: t.dur_bucket,
                });
            }
            ResolvedKind::TokenSteps(out)
        }
    };

    Ok(ResolvedMacro {
        name: m.name.clone(),
        weight: m.weight,
        pack_id: pack.pack_id.clone(),
        params: m.params.clone(),
        eligibility: m.eligibility.clone(),
        kind,
        mirrors,
    })
}

// ---------------------------------------------------------------------
// Instantiation
// ---------------------------------------------------------------------

/// Duration-bucket midpoints, duplicated from `synth_pad`'s private
/// `BUCKET_MIDPOINTS` (ARCHITECTURE.md §6.2) — not exposed publicly there.
/// Kept in lockstep by `token_steps_midpoints_match_synth_pad_detokenize` in
/// `tests/macro_suite.rs`, which asserts equality against
/// `PadModel::detokenize` behavior.
const DUR_BUCKET_MIDPOINTS: [u32; 6] = [1, 2, 5, 11, 23, 48];

pub fn dur_bucket_midpoint(bucket: u8) -> u32 {
    DUR_BUCKET_MIDPOINTS[usize::from(bucket.min(5))]
}

fn round_clamp(v: f64) -> u32 {
    let r = v.round_ties_even();
    if r < 1.0 {
        1
    } else if r > f64::from(u32::MAX) {
        u32::MAX
    } else {
        r as u32
    }
}

#[derive(Clone, Debug)]
enum ParamBinding {
    Enum(String),
    Int(i64),
    Scale(f64),
}

fn stringify_binding(b: &ParamBinding) -> String {
    match b {
        ParamBinding::Enum(s) => s.clone(),
        ParamBinding::Int(n) => n.to_string(),
        // Shortest round-trip representation (Rust's `f64` `Display`).
        ParamBinding::Scale(v) => format!("{v}"),
    }
}

/// Weighted-categorical pick over `weights` (need not be normalized;
/// callers only ever pass all-positive weights of eligible macros).
fn pick_weighted(weights: &[f64], u: f64) -> usize {
    let total: f64 = weights.iter().sum();
    if total <= 0.0 {
        return weights.len().saturating_sub(1);
    }
    let mut acc = 0.0;
    for (i, w) in weights.iter().enumerate() {
        acc += w / total;
        if u < acc {
            return i;
        }
    }
    weights.len().saturating_sub(1)
}

/// Generate one macro-generator burst for slot `s` (ARCHITECTURE.md §5.1).
/// Caller supplies the fan-out root; this function derives its own labeled
/// streams (module doc: draw-order contract) and returns an un-legalized
/// burst — the pipeline legalizes.
///
/// Panics if `resolved` has zero eligible macros for `ctx`: the caller (the
/// mixer's availability check, `propose::generator_weights`) must never
/// assign a `Macro` slot unless at least one macro is eligible.
pub fn generate_macro_burst(
    cfg: &ExperimentConfig,
    ctx: &GenContext,
    root: &[u8; 32],
    slot: usize,
    length_hint: u32,
    resolved: &ResolvedMacros,
) -> (Burst, MacroProvenance) {
    let target = {
        let mut len_rng = stream(root, &format!("slot/{slot}/len"));
        u64::from(weighted_random::draw_length(cfg, length_hint, &mut len_rng))
    };

    let eligible: Vec<&ResolvedMacro> = resolved
        .items
        .iter()
        .filter(|m| {
            m.eligibility
                .iter()
                .all(|p| context::eval_predicate(p, ctx, cfg))
        })
        .collect();
    assert!(
        !eligible.is_empty(),
        "generate_macro_burst called with zero eligible macros for this context; the \
         mixer must never assign a Macro slot without at least one eligible macro"
    );

    let chain_n = cfg.macro_.chain_n.clamp(1, 4) as usize;
    let mut pick_rng = stream(root, &format!("slot/{slot}/macro/pick"));
    let mut params_rng = stream(root, &format!("slot/{slot}/macro/params"));

    // (pack_id, macro_name, sorted param_bindings) of chain element 0, the
    // identity/bindings recorded in `MacroProvenance` (module doc).
    type FirstMacroIdentity = (String, String, Vec<(String, String)>);

    let mut segments: Vec<PadSegment> = Vec::new();
    let mut first: Option<FirstMacroIdentity> = None;

    for chain_index in 0..chain_n {
        let weights: Vec<f64> = eligible.iter().map(|m| m.weight).collect();
        let u = next_unit_f64(&mut pick_rng);
        let picked = eligible[pick_weighted(&weights, u)];

        let mut bindings: Vec<(usize, ParamBinding)> = Vec::with_capacity(picked.params.len());
        for (idx, p) in picked.params.iter().enumerate() {
            let u = next_unit_f64(&mut params_rng);
            let val = match &p.domain {
                ParamDomain::Enum(values) => {
                    let vi = ((u * values.len() as f64) as usize).min(values.len() - 1);
                    ParamBinding::Enum(values[vi].clone())
                }
                ParamDomain::Int(r) => {
                    // Compute the span in i128 (fix #7): `r.max - r.min + 1`
                    // as plain i64 arithmetic can overflow for extreme
                    // pack-declared ranges (e.g. min = i64::MIN); i128 has
                    // ample headroom for any i64-bounded range.
                    let span = (i128::from(r.max) - i128::from(r.min) + 1) as f64;
                    let off = (u * span).floor() as i64;
                    let off = off.clamp(0, r.max - r.min);
                    ParamBinding::Int(r.min + off)
                }
                ParamDomain::Scale(r) => ParamBinding::Scale(r.min + u * (r.max - r.min)),
            };
            bindings.push((idx, val));
        }

        let scale_product: f64 = bindings
            .iter()
            .filter_map(|(_, v)| match v {
                ParamBinding::Scale(s) => Some(*s),
                _ => None,
            })
            .product();

        let mask_for = |idx: usize| -> u16 {
            match &bindings.iter().find(|(i, _)| *i == idx).map(|(_, v)| v) {
                Some(ParamBinding::Enum(v)) => picked.mirror_bit(idx, v).unwrap_or(0),
                _ => 0,
            }
        };
        let frame_val = |fb: &FrameBinding| -> f64 {
            match fb {
                FrameBinding::Literal(n) => f64::from(*n),
                FrameBinding::Param(idx) => {
                    match bindings.iter().find(|(i, _)| i == idx).map(|(_, v)| v) {
                        Some(ParamBinding::Int(n)) => *n as f64,
                        _ => unreachable!("frames $param must bind to an int param"),
                    }
                }
            }
        };

        let macro_segments: Vec<PadSegment> = match &picked.kind {
            ResolvedKind::Steps(steps) => steps
                .iter()
                .map(|s| {
                    let mut mask = s.static_mask;
                    for &idx in &s.enum_contribs {
                        mask |= mask_for(idx);
                    }
                    let frames = round_clamp(frame_val(&s.frames) * scale_product);
                    PadSegment {
                        buttons: mask,
                        hold_frames: frames,
                    }
                })
                .collect(),
            ResolvedKind::TokenSteps(token_steps) => token_steps
                .iter()
                .map(|t| {
                    let mut mask = t.static_mask;
                    for &idx in &t.enum_contribs {
                        mask |= mask_for(idx);
                    }
                    let base = f64::from(dur_bucket_midpoint(t.dur_bucket));
                    let frames = round_clamp(base * scale_product);
                    PadSegment {
                        buttons: mask,
                        hold_frames: frames,
                    }
                })
                .collect(),
        };

        if chain_index == 0 {
            let mut param_bindings: Vec<(String, String)> = bindings
                .iter()
                .map(|(idx, v)| (picked.params[*idx].name.clone(), stringify_binding(v)))
                .collect();
            param_bindings.sort_by(|a, b| a.0.cmp(&b.0));
            first = Some((picked.pack_id.clone(), picked.name.clone(), param_bindings));
        }

        segments.extend(macro_segments);
    }

    let macro_frames: u64 = segments.iter().map(|s| u64::from(s.hold_frames)).sum();
    // Fix #15: when the instantiated chain itself exceeds
    // `burst_len.max_frames`, the caller's `legalize` truncates the total to
    // `max_frames` — clamp the reported span to match, so
    // `macro_frames + tail_frames == <legalized total>` always holds. `target`
    // (the length draw) is itself already clamped to `max_frames`, so when
    // this clamp actually changes anything, `macro_frames >= target` already
    // held and `tail_frames` below stays 0 either way.
    let macro_frames_clamped = macro_frames.min(u64::from(cfg.burst_len.max_frames));
    let macro_frames_u32 = u32::try_from(macro_frames_clamped).unwrap_or(u32::MAX);

    let tail_frames = if macro_frames < target && cfg.macro_.pad_to_length {
        let tail_target = target - macro_frames;
        let mut tail_rng = stream(root, &format!("slot/{slot}/macro/tail"));
        let tail_segments =
            weighted_random::generate_single_stream(cfg, ctx, tail_target, &mut tail_rng);
        segments.extend(tail_segments);
        u32::try_from(tail_target).unwrap_or(u32::MAX)
    } else {
        0
    };

    let (pack_id, macro_name, param_bindings) =
        first.expect("chain_n >= 1 guarantees at least one chain iteration");

    (
        Burst::Pad(PadBurst { segments }),
        MacroProvenance {
            pack_id,
            macro_name,
            param_bindings,
            macro_frames: macro_frames_u32,
            tail_frames,
            chain_index: 0,
        },
    )
}
