#![forbid(unsafe_code)]

//! `synth-server`: the tonic gRPC service shell for the input synthesizer
//! (ARCHITECTURE.md §8). Transport only — every decision path (config
//! parsing/validation/merge/fingerprint, generation, legalization,
//! provenance) lives in `synth-core`/`synth-gen`/`synth-pad`; this crate
//! converts proto <-> domain types, holds loaded state, and reports metrics.
//!
//! Statelessness contract: nothing about a single RPC persists beyond the
//! call itself. The only state carried between calls is *explicitly loaded*
//! documents (experiment configs, and — from M2 — macro packs), held behind
//! an `RwLock` and keyed by their own declared id.

pub mod http;
pub mod metrics;

use std::sync::RwLock;

use indexmap::IndexMap;
use tonic::{Request, Response, Status};

use synth_core::config::ExperimentConfig;
use synth_core::SYNTH_VERSION;
use synth_gen::context::GenContext;
use synth_gen::propose::{propose, Availability};
use synth_gen::provenance::GeneratorKind as DomainGeneratorKind;
use synth_proto::v1::input_synthesizer_server::InputSynthesizer;
use synth_proto::v1::{
    self, DocumentKind, HealthRequest, HealthResponse, LoadMacroPackRequest, LoadMacroPackResponse,
    MineMacrosRequest, MineMacrosResponse, ModelKind, ProposeBurstsRequest, ProposeBurstsResponse,
    ProvenancedBurst,
};

/// Loaded state. Never touched outside a `LoadMacroPack`/`ProposeBursts`/
/// `Health` call; nothing request-scoped is stored here.
struct State {
    /// experiment_id -> (config, hex document hash of the raw bytes it was
    /// loaded from). Insertion order preserved for `Health.loaded_experiments`.
    experiments: IndexMap<String, (ExperimentConfig, String)>,
    /// Loaded macro pack ids (M2 placeholder: always empty in M1 — the
    /// `MACRO_PACK` document kind is rejected until M2 lands).
    loaded_pack_ids: Vec<String>,
}

pub struct SynthService {
    state: RwLock<State>,
    pub metrics: metrics::Metrics,
}

impl Default for SynthService {
    fn default() -> Self {
        Self::new()
    }
}

impl SynthService {
    pub fn new() -> Self {
        Self {
            state: RwLock::new(State {
                experiments: IndexMap::new(),
                loaded_pack_ids: Vec::new(),
            }),
            metrics: metrics::Metrics::new(),
        }
    }

    /// Convenience for standalone bring-up (`main.rs --load`): parse,
    /// validate, and load an experiment config document, returning its
    /// document id (hex blake3) or the joined validation error text.
    pub fn load_experiment_config(&self, bytes: &[u8]) -> Result<String, String> {
        let cfg = synth_core::config::parse(bytes).map_err(|e| e.to_string())?;
        synth_core::config::validate(&cfg).map_err(|errs| join_errors(&errs))?;
        let doc_id = blake3::hash(bytes).to_hex().to_string();
        let mut state = self.state.write().expect("state lock poisoned");
        state
            .experiments
            .insert(cfg.experiment_id.clone(), (cfg, doc_id.clone()));
        Ok(doc_id)
    }
}

fn join_errors<E: std::fmt::Display>(errs: &[E]) -> String {
    errs.iter()
        .map(std::string::ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

fn invalid_argument(msg: impl Into<String>) -> Status {
    Status::invalid_argument(msg.into())
}

fn domain_generator_kind_to_proto(kind: DomainGeneratorKind) -> v1::GeneratorKind {
    match kind {
        DomainGeneratorKind::WeightedRandom => v1::GeneratorKind::WeightedRandom,
        DomainGeneratorKind::Macro => v1::GeneratorKind::Macro,
        DomainGeneratorKind::Mutation => v1::GeneratorKind::Mutation,
        DomainGeneratorKind::Policy => v1::GeneratorKind::Policy,
    }
}

#[tonic::async_trait]
impl InputSynthesizer for SynthService {
    async fn propose_bursts(
        &self,
        request: Request<ProposeBurstsRequest>,
    ) -> Result<Response<ProposeBurstsResponse>, Status> {
        let start = std::time::Instant::now();
        let req = request.into_inner();

        if req.k == 0 || req.k > 256 {
            return Err(invalid_argument(format!(
                "k must be in 1..=256 (got {})",
                req.k
            )));
        }

        let requested_model = ModelKind::try_from(req.model).unwrap_or(ModelKind::Unspecified);
        if requested_model != ModelKind::Pad {
            return Err(invalid_argument(format!(
                "model {:?} is not supported in M1 (only MODEL_KIND_PAD is available)",
                requested_model
            )));
        }

        let (base_cfg, loaded_pack_ids) = {
            let state = self.state.read().expect("state lock poisoned");
            let Some((cfg, _doc_id)) = state.experiments.get(&req.experiment_id) else {
                return Err(invalid_argument(format!(
                    "unknown experiment_id {:?}",
                    req.experiment_id
                )));
            };
            (cfg.clone(), state.loaded_pack_ids.clone())
        };

        if base_cfg.model != synth_core::config::ModelKindCfg::Pad {
            return Err(invalid_argument(format!(
                "model mismatch: experiment_id {:?} is configured for {:?}, not pad",
                req.experiment_id, base_cfg.model
            )));
        }

        let effective_cfg = synth_core::config::deep_merge(&base_cfg, &req.config_overrides_yaml)
            .map_err(|e| invalid_argument(e.to_string()))?;
        synth_core::config::validate(&effective_cfg)
            .map_err(|errs| invalid_argument(join_errors(&errs)))?;

        // FAILED_PRECONDITION: config references macro packs that aren't
        // loaded and macro weight is actually in play. M1 never has any
        // packs loaded, so this fires whenever a config both names packs
        // and assigns them nonzero weight.
        if effective_cfg.generator_mix.macro_ > 0.0 {
            let missing: Vec<&String> = effective_cfg
                .macro_
                .packs
                .iter()
                .filter(|p| !loaded_pack_ids.contains(p))
                .collect();
            if !missing.is_empty() {
                let names = missing
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                return Err(Status::failed_precondition(format!(
                    "experiment config references macro pack(s) not loaded: {names}"
                )));
            }
        }

        let node_context = req
            .node_context
            .ok_or_else(|| invalid_argument("node_context.node_id is required"))?;
        if node_context.node_id.is_empty() {
            return Err(invalid_argument("node_context.node_id is required"));
        }

        // prost map boundary: `ram_features` is a `HashMap` on the wire;
        // sort by name immediately so decision paths never see it unordered.
        let mut ram_features: Vec<(String, f64)> = {
            #[allow(clippy::disallowed_types)] // prost map boundary
            let map: &std::collections::HashMap<String, f64> = &node_context.ram_features;
            map.iter().map(|(k, v)| (k.clone(), *v)).collect()
        };
        ram_features.sort_by(|a, b| a.0.cmp(&b.0));

        let recent_inputs = match &node_context.recent_inputs {
            None => None,
            Some(proto_burst) => {
                let (burst, _alphabet) = synth_core::types::from_proto(proto_burst)
                    .map_err(|e| invalid_argument(format!("node_context.recent_inputs: {e}")))?;
                match burst {
                    synth_core::types::Burst::Pad(pad) => Some(pad),
                    #[allow(unreachable_patterns)]
                    _ => unreachable!("from_proto only ever returns Burst::Pad in M1"),
                }
            }
        };

        let has_parent_burst = node_context.parent_burst.is_some();

        let gen_ctx = GenContext {
            node_id: node_context.node_id.clone(),
            ram_features,
            recent_inputs,
        };

        let availability = Availability { has_parent_burst };

        // Macro packs are not yet loadable through this server shell
        // (`LoadMacroPack` rejects `DOCUMENT_KIND_MACRO_PACK` below); a
        // follow-up wires a `PackRegistry` into `State` and resolves it here.
        let (results, degraded) = propose(
            &effective_cfg,
            &gen_ctx,
            req.k as usize,
            req.length_hint,
            req.seed,
            availability,
            None,
        );

        let fingerprint =
            synth_core::config::fingerprint(&effective_cfg, &loaded_pack_ids, SYNTH_VERSION);
        let alphabet_name = effective_cfg.button_alphabet.name.clone();

        let mut slot_counts: IndexMap<&'static str, usize> = IndexMap::new();
        let bursts: Vec<ProvenancedBurst> = results
            .iter()
            .map(|r| {
                *slot_counts
                    .entry(r.provenance.generator.name())
                    .or_insert(0) += 1;
                self.metrics
                    .bursts_total
                    .with_label_values(&[r.provenance.generator.name()])
                    .inc();

                let burst_proto = synth_core::types::to_proto(&r.burst, &alphabet_name);
                let provenance = v1::Provenance {
                    generator: domain_generator_kind_to_proto(r.provenance.generator) as i32,
                    slot: r.provenance.slot,
                    rng_stream: r.provenance.rng_stream.clone(),
                    config_fingerprint: fingerprint.to_vec(),
                    fallback_from: r
                        .provenance
                        .fallback_from
                        .map(domain_generator_kind_to_proto)
                        .unwrap_or(v1::GeneratorKind::Unspecified)
                        as i32,
                    r#macro: None,
                    mutation: None,
                    policy: None,
                };
                ProvenancedBurst {
                    burst: Some(burst_proto),
                    provenance: Some(provenance),
                }
            })
            .collect();

        for d in &degraded {
            self.metrics
                .generator_unavailable_total
                .with_label_values(&[d.generator.name(), d.reason.as_str()])
                .inc();
        }
        self.metrics
            .propose_requests_total
            .with_label_values(&["pad"])
            .inc();
        let elapsed = start.elapsed();
        self.metrics
            .propose_latency_seconds
            .observe(elapsed.as_secs_f64());

        tracing::info!(
            node_id = %node_context.node_id,
            k = req.k,
            seed = req.seed,
            config_fingerprint = %hex_encode(&fingerprint),
            slot_counts = ?slot_counts,
            latency_ms = elapsed.as_secs_f64() * 1000.0,
            "ProposeBursts"
        );

        let degraded_proto = degraded
            .into_iter()
            .map(|d| v1::DegradedGenerator {
                generator: domain_generator_kind_to_proto(d.generator) as i32,
                reason: d.reason,
            })
            .collect();

        Ok(Response::new(ProposeBurstsResponse {
            bursts,
            config_fingerprint: fingerprint.to_vec(),
            synth_version: SYNTH_VERSION.to_owned(),
            seed: req.seed,
            degraded: degraded_proto,
        }))
    }

    async fn load_macro_pack(
        &self,
        request: Request<LoadMacroPackRequest>,
    ) -> Result<Response<LoadMacroPackResponse>, Status> {
        let req = request.into_inner();
        let kind = DocumentKind::try_from(req.kind).unwrap_or(DocumentKind::Unspecified);

        match kind {
            DocumentKind::EventGrammar => {
                return Err(invalid_argument(
                    "event_grammar documents are not supported until M5",
                ));
            }
            DocumentKind::MacroPack => {
                return Err(invalid_argument("macro packs land with M2"));
            }
            DocumentKind::Unspecified => {
                return Err(invalid_argument("kind must be specified"));
            }
            DocumentKind::ExperimentConfig => {}
        }

        let bytes = match req.source {
            Some(v1::load_macro_pack_request::Source::ArtifactRef(_)) => {
                return Err(invalid_argument(
                    "artifact_ref sources are not supported in standalone mode; send document_yaml",
                ));
            }
            Some(v1::load_macro_pack_request::Source::DocumentYaml(bytes)) => bytes,
            None => {
                return Err(invalid_argument(
                    "artifact_ref sources are not supported in standalone mode; send document_yaml",
                ));
            }
        };

        let cfg = synth_core::config::parse(&bytes).map_err(|e| invalid_argument(e.to_string()))?;
        synth_core::config::validate(&cfg).map_err(|errs| invalid_argument(join_errors(&errs)))?;

        let doc_id = blake3::hash(&bytes).to_hex().to_string();

        let mut state = self.state.write().expect("state lock poisoned");
        if let Some((_, existing_id)) = state.experiments.get(&cfg.experiment_id) {
            if existing_id == &doc_id {
                // Reloading an identical document is a no-op.
                return Ok(Response::new(LoadMacroPackResponse {
                    document_id: doc_id,
                    items_loaded: 1,
                    warnings: Vec::new(),
                }));
            }
        }
        state
            .experiments
            .insert(cfg.experiment_id.clone(), (cfg, doc_id.clone()));

        Ok(Response::new(LoadMacroPackResponse {
            document_id: doc_id,
            items_loaded: 1,
            warnings: Vec::new(),
        }))
    }

    async fn mine_macros(
        &self,
        _request: Request<MineMacrosRequest>,
    ) -> Result<Response<MineMacrosResponse>, Status> {
        Err(Status::unimplemented("MineMacros lands with M4"))
    }

    async fn health(
        &self,
        _request: Request<HealthRequest>,
    ) -> Result<Response<HealthResponse>, Status> {
        let state = self.state.read().expect("state lock poisoned");
        Ok(Response::new(HealthResponse {
            status: v1::health_response::Status::Serving as i32,
            synth_version: SYNTH_VERSION.to_owned(),
            loaded_packs: state.loaded_pack_ids.clone(),
            loaded_experiments: state.experiments.keys().cloned().collect(),
            policy_endpoint_up: false,
            policy_deterministic: false,
            mining_in_progress: false,
        }))
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
