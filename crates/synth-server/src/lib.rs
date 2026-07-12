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
use synth_gen::context::{ContextBurst, GenContext, ScoredContextBurst};
use synth_gen::macros::{self, MacroPackError, PackRegistry};
use synth_gen::propose::propose;
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
    /// Loaded macro packs (M2): held behind the same lock as `experiments`.
    pack_registry: PackRegistry,
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
                pack_registry: PackRegistry::new(),
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
        let mut state = self
            .state
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state
            .experiments
            .insert(cfg.experiment_id.clone(), (cfg, doc_id.clone()));
        self.metrics
            .experiments_loaded
            .set(i64::try_from(state.experiments.len()).unwrap_or(i64::MAX));
        Ok(doc_id)
    }

    /// Convenience for standalone bring-up (`main.rs --load`): parse,
    /// validate, and load a macro pack document, returning its `pack_id` or
    /// the joined error text (parse errors carry line/column; validation
    /// errors are joined one per line, API.md §2.2).
    pub fn load_macro_pack_doc(&self, bytes: &[u8]) -> Result<String, String> {
        self.load_macro_pack_bytes(bytes)
            .map(|(pack_id, _, _)| pack_id)
    }

    /// Shared implementation behind the `LoadMacroPack` RPC (kind ==
    /// `MACRO_PACK`) and `load_macro_pack_doc`: parse + validate atomically
    /// (`synth_gen::macros::load_pack`), insert into the registry (identical
    /// bytes -> no-op, same `pack_id`, API.md §2.2), and update the
    /// `synth_macro_packs_loaded` gauge. Returns `(pack_id, items_loaded,
    /// warnings)`.
    fn load_macro_pack_bytes(&self, bytes: &[u8]) -> Result<(String, u32, Vec<String>), String> {
        let pack = macros::load_pack(bytes).map_err(format_pack_error)?;
        let pack_id = pack.pack_id.clone();
        let items_loaded = u32::try_from(pack.macros.len()).unwrap_or(u32::MAX);

        let mut state = self
            .state
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let warnings = state.pack_registry.insert(pack);
        self.metrics
            .macro_packs_loaded
            .set(i64::try_from(state.pack_registry.pack_ids().len()).unwrap_or(i64::MAX));

        Ok((pack_id, items_loaded, warnings))
    }
}

/// Parse errors keep their `Display` form (message + `(line L, column C)`);
/// validation errors are joined one per line rather than `Display`'s `"; "`
/// join, per the task's error-shape contract.
fn format_pack_error(e: MacroPackError) -> String {
    match e {
        MacroPackError::Parse { .. } => e.to_string(),
        MacroPackError::Invalid(errs) => errs.join("\n"),
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

/// Decode a `ProvenancedBurst` (API.md §2.4 `NodeContext.parent_burst` /
/// `sibling_bursts[].burst`) into the domain `ContextBurst` mutation needs
/// (synth_gen::mutation, ARCHITECTURE.md §5.2): the pad body via the shared
/// `synth_core::types::from_proto` boundary conversion, plus the burst's own
/// `burst_id` — validated to be exactly 32 bytes, since it becomes
/// `MutationProvenance.base_burst_id`/`donor_burst_id` verbatim.
fn decode_context_burst(pb: &v1::ProvenancedBurst, field: &str) -> Result<ContextBurst, Status> {
    let burst_proto = pb
        .burst
        .as_ref()
        .ok_or_else(|| invalid_argument(format!("{field}.burst is required")))?;
    let (burst, _alphabet) = synth_core::types::from_proto(burst_proto)
        .map_err(|e| invalid_argument(format!("{field}: {e}")))?;
    let synth_core::types::Burst::Pad(pad) = burst else {
        #[allow(unreachable_patterns)]
        {
            unreachable!("from_proto only ever returns Burst::Pad in M1..M3")
        }
    };
    let burst_id: [u8; 32] = burst_proto.burst_id.as_slice().try_into().map_err(|_| {
        invalid_argument(format!(
            "{field}.burst_id must be exactly 32 bytes (got {})",
            burst_proto.burst_id.len()
        ))
    })?;
    Ok(ContextBurst { pad, burst_id })
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

        // Single read-lock scope (fix #3): base config, loaded pack ids, and
        // macro resolution must all observe the SAME snapshot of `state`. A
        // second, separately-acquired lock (the previous shape: one read
        // guard for base_cfg+pack_ids, dropped, then a fresh read guard for
        // `macros::resolve`) leaves a window where a concurrent
        // `LoadMacroPack` lands between the two locks — the bursts this
        // request returns would then reflect a pack set the fingerprint
        // (computed from `loaded_pack_ids`, captured under the first lock)
        // doesn't describe. `deep_merge`+`validate` are pure and cheap, so
        // running them inside the guard costs nothing and keeps everything
        // — config, pack ids, and macro resolution — consistent with one
        // snapshot.
        let (effective_cfg, loaded_pack_ids, resolved_macros) = {
            let state = self
                .state
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let Some((base_cfg, _doc_id)) = state.experiments.get(&req.experiment_id) else {
                return Err(invalid_argument(format!(
                    "unknown experiment_id {:?}",
                    req.experiment_id
                )));
            };
            if base_cfg.model != synth_core::config::ModelKindCfg::Pad {
                return Err(invalid_argument(format!(
                    "model mismatch: experiment_id {:?} is configured for {:?}, not pad",
                    req.experiment_id, base_cfg.model
                )));
            }

            let effective_cfg =
                synth_core::config::deep_merge(base_cfg, &req.config_overrides_yaml)
                    .map_err(|e| invalid_argument(e.to_string()))?;
            synth_core::config::validate(&effective_cfg)
                .map_err(|errs| invalid_argument(join_errors(&errs)))?;

            let loaded_pack_ids = state.pack_registry.pack_ids();

            // Fix #13 (API.md §5): pack presence/resolvability is checked
            // UNCONDITIONALLY whenever the effective config names any packs
            // at all — not only when the macro generator's mix weight is
            // > 0. `PackNotLoaded` / `AlphabetMismatch` / `UnresolvableButton`
            // all become FAILED_PRECONDITION naming the pack/button, since
            // these are config-vs-server-state mismatches, not malformed
            // requests. Only the *availability passed to `propose`* is
            // still gated on macro mix weight > 0 (weight 0 keeps the
            // existing `no_macros_loaded`-style degraded behavior moot,
            // since the macro generator is never picked with weight 0
            // anyway).
            let resolved = if !effective_cfg.macro_.packs.is_empty() {
                Some(
                    macros::resolve(&state.pack_registry, &effective_cfg)
                        .map_err(|e| Status::failed_precondition(e.to_string()))?,
                )
            } else {
                None
            };
            let resolved_macros = if effective_cfg.generator_mix.macro_ > 0.0 {
                resolved
            } else {
                None
            };

            (effective_cfg, loaded_pack_ids, resolved_macros)
        };

        // Per-request frames budget (round-7 review): k and
        // burst_len.max_frames are individually capped, but their PRODUCT
        // bounds both compute and response size (a k=256 x 216000-frame
        // request measured a 45 MB response — beyond tonic's default 4 MB
        // client decode limit). 600k frames is ~5x the largest legitimate
        // shape (k=64 x max 1800) and keeps the worst-case RLE response
        // under the default client limit.
        let frames_budget = u64::from(req.k) * u64::from(effective_cfg.burst_len.max_frames);
        if frames_budget > 600_000 {
            return Err(invalid_argument(format!(
                "k ({}) x burst_len.max_frames ({}) = {frames_budget} exceeds the 600000-frame per-request budget; lower k or max_frames",
                req.k, effective_cfg.burst_len.max_frames
            )));
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

        let parent_burst = node_context
            .parent_burst
            .as_ref()
            .map(|pb| decode_context_burst(pb, "node_context.parent_burst"))
            .transpose()?;
        // Runaway-caller guard (round-7 review): a buggy accumulator that
        // concatenates a node's whole history into siblings should fail
        // loudly, not allocate. Generous vs. real use (siblings ~ k's
        // ballpark).
        if node_context.sibling_bursts.len() > 64 {
            return Err(invalid_argument(format!(
                "node_context.sibling_bursts has {} entries (limit 64)",
                node_context.sibling_bursts.len()
            )));
        }
        let sibling_bursts = node_context
            .sibling_bursts
            .iter()
            .enumerate()
            .map(|(i, sb)| {
                let burst = sb
                    .burst
                    .as_ref()
                    .ok_or_else(|| {
                        invalid_argument(format!(
                            "node_context.sibling_bursts[{i}].burst is required"
                        ))
                    })
                    .and_then(|pb| {
                        decode_context_burst(pb, &format!("node_context.sibling_bursts[{i}].burst"))
                    })?;
                Ok(ScoredContextBurst {
                    burst,
                    score_delta: sb.score_delta,
                })
            })
            .collect::<Result<Vec<_>, Status>>()?;

        let context_segments = recent_inputs.as_ref().map_or(0, |p| p.segments.len())
            + parent_burst.as_ref().map_or(0, |b| b.pad.segments.len())
            + sibling_bursts
                .iter()
                .map(|s| s.burst.pad.segments.len())
                .sum::<usize>();
        if context_segments > 100_000 {
            return Err(invalid_argument(format!(
                "node_context carries {context_segments} pad segments across                  recent_inputs/parent_burst/sibling_bursts (limit 100000)"
            )));
        }

        let gen_ctx = GenContext {
            node_id: node_context.node_id.clone(),
            ram_features,
            recent_inputs,
            parent_burst,
            sibling_bursts,
        };

        let (results, degraded) = propose(
            &effective_cfg,
            &gen_ctx,
            req.k as usize,
            req.length_hint,
            req.seed,
            resolved_macros.as_ref(),
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
                let macro_proto = r.provenance.macro_.as_ref().map(|mp| {
                    // prost map boundary: `param_bindings` is a `HashMap` on
                    // the wire; built here from the sorted `Vec<(K,V)>` the
                    // domain type carries (never compared/golden-tested in
                    // its raw wire form, per the macro-pack determinism
                    // rule).
                    #[allow(clippy::disallowed_types)]
                    let param_bindings: std::collections::HashMap<
                        String,
                        String,
                    > = mp.param_bindings.iter().cloned().collect();
                    v1::MacroProvenance {
                        pack_id: mp.pack_id.clone(),
                        macro_name: mp.macro_name.clone(),
                        param_bindings,
                        macro_frames: mp.macro_frames,
                        tail_frames: mp.tail_frames,
                        chain_index: mp.chain_index,
                    }
                });
                let mutation_proto = r.provenance.mutation.as_ref().map(|mp| {
                    let ops: Vec<v1::MutationOp> =
                        mp.ops
                            .iter()
                            .map(|op| {
                                // prost map boundary: `args` is a `HashMap` on
                                // the wire; built here from the sorted
                                // `Vec<(K,V)>` the domain type carries (same
                                // convention as `MacroProvenance.param_bindings`
                                // above).
                                #[allow(clippy::disallowed_types)]
                            let args: std::collections::HashMap<String, String> =
                                op.args.iter().cloned().collect();
                                v1::MutationOp {
                                    op: op.op.clone(),
                                    args,
                                }
                            })
                            .collect();
                    v1::MutationProvenance {
                        base_burst_id: mp.base_burst_id.to_vec(),
                        donor_burst_id: mp.donor_burst_id.map(|d| d.to_vec()).unwrap_or_default(),
                        base_was_sibling: mp.base_was_sibling,
                        ops,
                        post_clamp: mp.post_clamp,
                    }
                });
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
                    r#macro: macro_proto,
                    mutation: mutation_proto,
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
            DocumentKind::Unspecified => {
                return Err(invalid_argument("kind must be specified"));
            }
            DocumentKind::MacroPack | DocumentKind::ExperimentConfig => {}
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

        if kind == DocumentKind::MacroPack {
            let (pack_id, items_loaded, warnings) = self
                .load_macro_pack_bytes(&bytes)
                .map_err(invalid_argument)?;
            return Ok(Response::new(LoadMacroPackResponse {
                document_id: pack_id,
                items_loaded,
                warnings,
            }));
        }

        let cfg = synth_core::config::parse(&bytes).map_err(|e| invalid_argument(e.to_string()))?;
        synth_core::config::validate(&cfg).map_err(|errs| invalid_argument(join_errors(&errs)))?;

        let doc_id = blake3::hash(&bytes).to_hex().to_string();

        let mut state = self
            .state
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
        self.metrics
            .experiments_loaded
            .set(i64::try_from(state.experiments.len()).unwrap_or(i64::MAX));

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
        let state = self
            .state
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Ok(Response::new(HealthResponse {
            status: v1::health_response::Status::Serving as i32,
            synth_version: SYNTH_VERSION.to_owned(),
            // ids AND declared names: the orchestrator's SynthBringup checks
            // config macro.packs entries (names or ids) for verbatim
            // membership here; ids alone would fail every name-based config.
            loaded_packs: state.pack_registry.loaded_pack_identifiers(),
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
