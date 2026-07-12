//! Seeded RNG fan-out (ARCHITECTURE.md §7.1, normative).
//!
//! All randomness in the proposal path derives from the request seed via
//! [`fanout_root`] and [`stream`]. One label, one consumer, one pass; draw
//! order within a stream is part of the format (changing it bumps
//! `SYNTH_VERSION` and invalidates goldens).

use rand_chacha::rand_core::SeedableRng;
use rand_chacha::ChaCha8Rng;

/// Derive-key context string — part of the wire-level contract; never change
/// without a `SYNTH_VERSION` bump.
const ROOT_CONTEXT: &str = "determinism.inputsynth.v1 2026 proposal root";

/// 32-byte root from the request. `node_id` is mixed in so identical seeds at
/// different nodes don't correlate (defense in depth; the orchestrator's
/// request-seed rule does not include node ids).
pub fn fanout_root(seed: u64, node_id: &str) -> [u8; 32] {
    let mut h = blake3::Hasher::new_derive_key(ROOT_CONTEXT);
    h.update(&seed.to_le_bytes());
    h.update(node_id.as_bytes());
    *h.finalize().as_bytes()
}

/// Every consumer of randomness gets its own stream by domain label
/// (canonical labels: ARCHITECTURE.md §7.2).
pub fn stream(root: &[u8; 32], label: &str) -> ChaCha8Rng {
    let key = blake3::keyed_hash(root, label.as_bytes());
    ChaCha8Rng::from_seed(*key.as_bytes())
}

/// Draw a canonical `U[0,1)` with 53 bits of mantissa — the only f64 uniform
/// used in sampling paths.
pub fn next_unit_f64(rng: &mut ChaCha8Rng) -> f64 {
    use rand_chacha::rand_core::RngCore;
    const SCALE: f64 = 1.0 / ((1u64 << 53) as f64);
    ((rng.next_u64() >> 11) as f64) * SCALE
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_chacha::rand_core::RngCore;

    /// Cross-arch golden vectors: recorded on x86_64; CI asserts equality on
    /// both x86_64 and aarch64 (M0 accept item). Regenerate (and bump the
    /// workspace version) with `cargo test -p synth-core record_rng_goldens
    /// -- --ignored --nocapture`.
    #[test]
    fn stream_golden_vectors() {
        let goldens: serde_yaml::Value = serde_yaml::from_str(
            &std::fs::read_to_string(testdata_path()).expect("read rng goldens"),
        )
        .expect("parse rng goldens");
        let seed = goldens["seed"].as_u64().expect("seed");
        let node_id = goldens["node_id"].as_str().expect("node_id");
        let root = fanout_root(seed, node_id);
        let labels = goldens["labels"].as_mapping().expect("labels");
        assert!(!labels.is_empty());
        for (label, expected) in labels {
            let label = label.as_str().expect("label");
            let expected = expected.as_str().expect("hex");
            let mut rng = stream(&root, label);
            let mut got = [0u8; 16];
            rng.fill_bytes(&mut got);
            assert_eq!(
                hex(&got),
                expected,
                "stream({label:?}) first-16-bytes mismatch — cross-arch or \
                 format drift; a deliberate change needs a version bump"
            );
        }
    }

    #[test]
    #[ignore = "record mode: regenerates testdata/rng_stream_goldens.yaml"]
    fn record_rng_goldens() {
        let seed = 0x0123_4567_89AB_CDEFu64;
        let node_id = "node-golden";
        let root = fanout_root(seed, node_id);
        let mut out = format!("seed: {seed}\nnode_id: {node_id}\nlabels:\n");
        for label in ["mix", "slot/0/len", "slot/0/wr/btn/0", "slot/0/wr/dir"] {
            let mut rng = stream(&root, label);
            let mut buf = [0u8; 16];
            rng.fill_bytes(&mut buf);
            out.push_str(&format!("  {label}: {}\n", hex(&buf)));
        }
        std::fs::write(testdata_path(), &out).expect("write goldens");
        println!("{out}");
    }

    fn testdata_path() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/rng_stream_goldens.yaml")
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn streams_are_independent_per_label() {
        let root = fanout_root(1, "n");
        let mut a = stream(&root, "slot/0/wr/btn/0");
        let mut b = stream(&root, "slot/0/wr/btn/1");
        assert_ne!(a.next_u64(), b.next_u64());
    }

    #[test]
    fn unit_f64_in_range() {
        let root = fanout_root(2, "n");
        let mut rng = stream(&root, "test");
        for _ in 0..1000 {
            let u = next_unit_f64(&mut rng);
            assert!((0.0..1.0).contains(&u));
        }
    }
}
