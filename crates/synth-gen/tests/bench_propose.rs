//! M1 latency bench: `ProposeBursts(K=32, L=300)` p99 < 5 ms
//! (IMPLEMENTATION-PLAN.md §M1 Accept). `std::time::Instant` is fine here —
//! this is a test measuring wall-clock time, not a value in a sampling path
//! (which must stay bit-deterministic and never touch the clock).
//!
//! `#[ignore]`d like the plan's M1 doc says: CI runs this as a separate,
//! non-gating job (hosted shared runners have CPU steal that would make a
//! hard 5ms gate flake); locally it must pass.

#[path = "common/mod.rs"]
mod common;

use std::time::{Duration, Instant};

use synth_gen::propose::{propose, Availability};

const WARMUP: usize = 50;
const ITERS: usize = 1000;
const K: usize = 32;
const LENGTH_HINT: u32 = 300;

#[test]
#[ignore = "bench: run with `cargo test -p synth-gen --release bench_propose -- --ignored --nocapture`"]
fn bench_propose() {
    let cfg = common::parse_and_validate(&common::base_yaml(0.25));
    let ctx = common::ctx_free("bench-node");

    for i in 0..WARMUP {
        let _ = propose(
            &cfg,
            &ctx,
            K,
            LENGTH_HINT,
            0xB000_0000_0000_0000 ^ i as u64,
            Availability::default(),
            None,
        );
    }

    let mut samples = Vec::with_capacity(ITERS);
    for i in 0..ITERS {
        let seed = 0xB111_0000_0000_0000 ^ i as u64;
        let start = Instant::now();
        let (results, _) = propose(
            &cfg,
            &ctx,
            K,
            LENGTH_HINT,
            seed,
            Availability::default(),
            None,
        );
        let elapsed = start.elapsed();
        std::hint::black_box(&results);
        samples.push(elapsed);
    }

    samples.sort_unstable();
    let min = samples[0];
    let p50 = samples[samples.len() / 2];
    let p99 = samples[(samples.len() * 99) / 100];
    let max = samples[samples.len() - 1];

    println!(
        "propose(K={K}, L={LENGTH_HINT}) over {ITERS} calls (after {WARMUP} warmup): \
         min={min:?} p50={p50:?} p99={p99:?} max={max:?}"
    );

    assert!(
        p99 < Duration::from_millis(5),
        "p99 latency {p99:?} >= 5ms budget (min={min:?} p50={p50:?} max={max:?})"
    );
}
