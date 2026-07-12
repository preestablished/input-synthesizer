//! Prometheus metrics (ARCHITECTURE.md §8 minimum series).

use prometheus::{Histogram, HistogramOpts, IntCounter, IntCounterVec, IntGauge, Opts, Registry};

/// All service metrics, registered once at construction. Exposed on the
/// HTTP `/metrics` endpoint in Prometheus text format.
pub struct Metrics {
    pub registry: Registry,
    pub propose_requests_total: IntCounterVec,
    pub bursts_total: IntCounterVec,
    pub propose_latency_seconds: Histogram,
    pub generator_unavailable_total: IntCounterVec,
    pub macro_packs_loaded: IntGauge,
    pub experiments_loaded: IntGauge,
    pub mine_runs_total: IntCounter,
    pub policy_fallback_total: IntCounter,
}

impl Metrics {
    pub fn new() -> Self {
        let registry = Registry::new();

        let propose_requests_total = IntCounterVec::new(
            Opts::new(
                "synth_propose_requests_total",
                "Total ProposeBursts requests handled, by model kind.",
            ),
            &["model"],
        )
        .expect("valid metric");
        let bursts_total = IntCounterVec::new(
            Opts::new("synth_bursts_total", "Total bursts emitted, by generator."),
            &["generator"],
        )
        .expect("valid metric");
        let propose_latency_seconds = Histogram::with_opts(HistogramOpts::new(
            "synth_propose_latency_seconds",
            "ProposeBursts handler latency in seconds.",
        ))
        .expect("valid metric");
        let generator_unavailable_total = IntCounterVec::new(
            Opts::new(
                "synth_generator_unavailable_total",
                "Total times a generator's weight was reallocated, by generator and reason.",
            ),
            &["generator", "reason"],
        )
        .expect("valid metric");
        let macro_packs_loaded = IntGauge::new(
            "synth_macro_packs_loaded",
            "Number of macro packs currently loaded.",
        )
        .expect("valid metric");
        let experiments_loaded = IntGauge::new(
            "synth_experiments_loaded",
            "Number of experiment configs currently loaded (round-7 review: \
             loads accumulate for the process lifetime; a runaway bring-up \
             loop shows up here before it becomes a memory problem).",
        )
        .expect("valid metric");
        let mine_runs_total =
            IntCounter::new("synth_mine_runs_total", "Total MineMacros runs completed.")
                .expect("valid metric");
        let policy_fallback_total = IntCounter::new(
            "synth_policy_fallback_total",
            "Total slots that fell back from the policy generator.",
        )
        .expect("valid metric");

        registry
            .register(Box::new(propose_requests_total.clone()))
            .expect("register metric");
        registry
            .register(Box::new(bursts_total.clone()))
            .expect("register metric");
        registry
            .register(Box::new(propose_latency_seconds.clone()))
            .expect("register metric");
        registry
            .register(Box::new(generator_unavailable_total.clone()))
            .expect("register metric");
        registry
            .register(Box::new(macro_packs_loaded.clone()))
            .expect("register metric");
        registry
            .register(Box::new(experiments_loaded.clone()))
            .expect("register metric");
        registry
            .register(Box::new(mine_runs_total.clone()))
            .expect("register metric");
        registry
            .register(Box::new(policy_fallback_total.clone()))
            .expect("register metric");

        Self {
            registry,
            propose_requests_total,
            bursts_total,
            propose_latency_seconds,
            generator_unavailable_total,
            macro_packs_loaded,
            experiments_loaded,
            mine_runs_total,
            policy_fallback_total,
        }
    }

    /// Render the registry in Prometheus text exposition format.
    pub fn encode(&self) -> String {
        use prometheus::Encoder;
        let encoder = prometheus::TextEncoder::new();
        let families = self.registry.gather();
        let mut buf = Vec::new();
        encoder.encode(&families, &mut buf).expect("encode metrics");
        String::from_utf8(buf).expect("prometheus text format is utf8")
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}
