//! Experiment config schema, defaults, validation, deep-merge, and
//! fingerprinting (API.md §5).
//!
//! Ordered collections only: maps are `IndexMap` (insertion order = document
//! order), so parsing, iteration, and the fingerprint are deterministic for a
//! given document. YAML and JSON are both accepted (YAML is a superset).

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

// ---- 5.1 Button alphabet ---------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ButtonDef {
    pub name: String,
    pub bit: u8,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ForbiddenMask {
    pub mask: Vec<String>,
    pub clear: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct DirectionsCfg {
    #[serde(default)]
    pub group: Vec<String>,
    #[serde(default)]
    pub allow_diagonals: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ButtonAlphabet {
    pub name: String,
    pub buttons: Vec<ButtonDef>,
    #[serde(default)]
    pub exclusive_groups: Vec<Vec<String>>,
    #[serde(default)]
    pub forbidden_masks: Vec<ForbiddenMask>,
    #[serde(default)]
    pub directions: DirectionsCfg,
}

impl ButtonAlphabet {
    /// Bit position for a button name, if declared.
    pub fn bit(&self, name: &str) -> Option<u8> {
        self.buttons.iter().find(|b| b.name == name).map(|b| b.bit)
    }

    /// Bitmask over a list of button names; `None` if any name is undeclared.
    pub fn mask(&self, names: &[String]) -> Option<u16> {
        names
            .iter()
            .map(|n| self.bit(n).map(|b| 1u16 << b))
            .try_fold(0u16, |acc, m| m.map(|m| acc | m))
    }
}

// ---- 5.2 Generator mix -----------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct GeneratorMix {
    pub weighted_random: f64,
    #[serde(rename = "macro")]
    pub macro_: f64,
    pub mutation: f64,
    pub policy: f64,
}

/// API.md §5.2: "absent generator = 0" — within a PRESENT `generator_mix`
/// map, omitted keys are 0.0, not the struct defaults (a partially-written
/// map means "only these generators"). A wholly-absent section still gets
/// the documented §5.2 defaults via `ExperimentConfig`'s field default.
/// (Round-12 spec-diff: `#[serde(default)]` used to leak struct defaults
/// into omitted keys of a present map.)
impl<'de> serde::Deserialize<'de> for GeneratorMix {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Shadow {
            #[serde(default)]
            weighted_random: Option<f64>,
            #[serde(rename = "macro", default)]
            macro_: Option<f64>,
            #[serde(default)]
            mutation: Option<f64>,
            #[serde(default)]
            policy: Option<f64>,
        }
        let shadow = Shadow::deserialize(deserializer)?;
        Ok(Self {
            weighted_random: shadow.weighted_random.unwrap_or(0.0),
            macro_: shadow.macro_.unwrap_or(0.0),
            mutation: shadow.mutation.unwrap_or(0.0),
            policy: shadow.policy.unwrap_or(0.0),
        })
    }
}

impl Default for GeneratorMix {
    fn default() -> Self {
        Self {
            weighted_random: 0.45,
            macro_: 0.35,
            mutation: 0.20,
            policy: 0.0,
        }
    }
}

// ---- 5.3 Burst length ------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LengthDistribution {
    Lognormal,
    Fixed,
    Uniform,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct BurstLen {
    pub distribution: LengthDistribution,
    pub mean_frames: u32,
    pub sigma: f64,
    pub min_frames: u32,
    pub max_frames: u32,
}

impl Default for BurstLen {
    fn default() -> Self {
        Self {
            distribution: LengthDistribution::Lognormal,
            mean_frames: 300,
            sigma: 0.35,
            min_frames: 16,
            max_frames: 1800,
        }
    }
}

// ---- 5.4 Weighted-random priors ---------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ButtonPrior {
    pub duty: f64,
    pub mean_hold_frames: f64,
}

impl Default for ButtonPrior {
    fn default() -> Self {
        Self {
            duty: 0.04,
            mean_hold_frames: 6.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct DirectionPriors {
    pub priors: IndexMap<String, f64>,
    pub mean_hold_frames: f64,
    pub stickiness: f64,
    pub diagonal_factor: f64,
}

impl Default for DirectionPriors {
    fn default() -> Self {
        let mut priors = IndexMap::new();
        priors.insert("NEUTRAL".to_owned(), 0.15);
        priors.insert("LEFT".to_owned(), 0.10);
        priors.insert("RIGHT".to_owned(), 0.55);
        priors.insert("UP".to_owned(), 0.08);
        priors.insert("DOWN".to_owned(), 0.12);
        Self {
            priors,
            mean_hold_frames: 24.0,
            stickiness: 0.55,
            diagonal_factor: 0.25,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct WeightedRandomCfg {
    pub default_button: ButtonPrior,
    pub buttons: IndexMap<String, ButtonPrior>,
    pub direction: DirectionPriors,
    pub start_from_history: StartFromHistory,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StartFromHistory(pub bool);

impl Default for StartFromHistory {
    fn default() -> Self {
        Self(true)
    }
}

// ---- 5.5 Context rules -------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CmpOp {
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PressedWithin {
    pub button: String,
    pub frames: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryPredicate {
    pub pressed_within: PressedWithin,
}

/// `when:` predicate — a RAM-feature comparison or a history predicate.
/// Missing feature ⇒ predicate false (never an error).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Predicate {
    Feature {
        feature: String,
        op: CmpOp,
        value: f64,
    },
    History {
        history: HistoryPredicate,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextRule {
    pub when: Predicate,
    #[serde(default)]
    pub adjust_buttons: IndexMap<String, f64>,
    #[serde(default)]
    pub adjust_directions: IndexMap<String, f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Refractory {
    pub button: String,
    pub frames: u32,
    pub logit_penalty: f64,
}

// ---- 5.6 Macro generator -------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct MacroCfg {
    pub packs: Vec<String>,
    pub pad_to_length: bool,
    pub chain_n: u32,
}

impl Default for MacroCfg {
    fn default() -> Self {
        Self {
            packs: Vec::new(),
            pad_to_length: true,
            chain_n: 1,
        }
    }
}

// ---- 5.7 Mutation ----------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpsBinomial {
    pub n: u32,
    pub p: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct MutationCfg {
    pub donor_bias: f64,
    pub op_probs: IndexMap<String, f64>,
    pub timing_sigma: f64,
    pub ops_binomial: OpsBinomial,
}

impl Default for MutationCfg {
    fn default() -> Self {
        let mut op_probs = IndexMap::new();
        op_probs.insert("perturb_timing".to_owned(), 0.25);
        op_probs.insert("extend".to_owned(), 0.20);
        op_probs.insert("flip_button".to_owned(), 0.15);
        op_probs.insert("splice".to_owned(), 0.15);
        op_probs.insert("truncate".to_owned(), 0.10);
        op_probs.insert("duplicate_segment".to_owned(), 0.10);
        op_probs.insert("swap_adjacent".to_owned(), 0.05);
        Self {
            donor_bias: 0.5,
            op_probs,
            timing_sigma: 0.25,
            ops_binomial: OpsBinomial { n: 3, p: 0.25 },
        }
    }
}

// ---- 5.8 Policy (parse-only in v1) ---------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct PolicyCfg {
    pub endpoint: String,
    pub model_id: String,
    pub temperature: f64,
    pub horizon_segments: u32,
    pub call_deadline_ms: u32,
    pub strict_reproducibility: bool,
}

impl Default for PolicyCfg {
    fn default() -> Self {
        Self {
            endpoint: "http://spark-policy:7480".to_owned(),
            model_id: "pad-policy".to_owned(),
            temperature: 1.0,
            horizon_segments: 48,
            call_deadline_ms: 150,
            strict_reproducibility: true,
        }
    }
}

// ---- Top level -----------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelKindCfg {
    Pad,
    EventGrammar,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExperimentConfig {
    pub version: u32,
    pub kind: String,
    pub experiment_id: String,
    pub model: ModelKindCfg,
    #[serde(default)]
    pub grammar_id: Option<String>,
    pub button_alphabet: ButtonAlphabet,
    #[serde(default)]
    pub generator_mix: GeneratorMix,
    #[serde(default)]
    pub burst_len: BurstLen,
    #[serde(default)]
    pub weighted_random: WeightedRandomCfg,
    #[serde(default)]
    pub context_rules: Vec<ContextRule>,
    #[serde(default)]
    pub refractory: Vec<Refractory>,
    #[serde(rename = "macro", default)]
    pub macro_: MacroCfg,
    #[serde(default)]
    pub mutation: MutationCfg,
    #[serde(default)]
    pub policy: PolicyCfg,
    #[serde(default = "default_true")]
    pub strict_reproducibility: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ConfigError {
    /// serde_yaml messages already carry "at line L column C" when known;
    /// line/column are also exposed structurally for RPC error details.
    #[error("parse error: {message}")]
    Parse {
        message: String,
        line: Option<usize>,
        column: Option<usize>,
    },
    #[error("{0}")]
    Invalid(String),
}

/// Parse a YAML/JSON experiment config document (no validation).
pub fn parse(bytes: &[u8]) -> Result<ExperimentConfig, ConfigError> {
    serde_yaml::from_slice(bytes).map_err(|e| {
        let loc = e.location();
        ConfigError::Parse {
            message: e.to_string(),
            line: loc.as_ref().map(|l| l.line()),
            column: loc.as_ref().map(|l| l.column()),
        }
    })
}

/// Validate every rule in API.md §5, reporting ALL failures at once.
pub fn validate(cfg: &ExperimentConfig) -> Result<(), Vec<ConfigError>> {
    validation::validate(cfg)
}

/// Deep-merge sparse YAML/JSON overrides onto a base document: maps merge,
/// scalars and lists replace. Returns the merged, re-parsed config
/// (unvalidated — callers validate the result).
pub fn deep_merge(
    base: &ExperimentConfig,
    overrides_yaml: &[u8],
) -> Result<ExperimentConfig, ConfigError> {
    merge::deep_merge(base, overrides_yaml)
}

/// `config_fingerprint = blake3(canonical-postcard(effective config) ‖
/// sorted pack_ids ‖ synth_version)` (ARCHITECTURE.md §7.5). `pack_ids` must
/// arrive sorted by the caller-visible convention (we sort here defensively).
pub fn fingerprint(cfg: &ExperimentConfig, pack_ids: &[String], synth_version: &str) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    let encoded = postcard::to_allocvec(cfg).expect("postcard encoding of config cannot fail");
    hasher.update(&encoded);
    let mut sorted: Vec<&String> = pack_ids.iter().collect();
    sorted.sort();
    for id in sorted {
        hasher.update(id.as_bytes());
        hasher.update(&[0]);
    }
    hasher.update(synth_version.as_bytes());
    *hasher.finalize().as_bytes()
}

mod merge;
mod validation;
