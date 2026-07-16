//! Pin guard on the frozen proto contract (tag `proto-v0.2.0`).
//!
//! The path dep on the sibling `control-plane` checkout can silently drift
//! past the frozen tag; these tests trip loudly when the version const or
//! the exact field set we build against moves. Schema corrections go
//! through control-plane's request flow — never adjust these assertions to
//! chase an untagged sibling HEAD (see `docs/proto-audit.md`).

use prost::Message;

#[test]
fn pinned_proto_version() {
    assert_eq!(synth_proto::PROTO_VERSION, "proto-v0.2.0");
    assert_eq!(synth_proto::v1::BURST_FORMAT_VERSION, 1);
}

/// Compile-time tripwire on the frozen message shapes: constructs one
/// `Burst` with a pad body and one `Provenance` field-by-field (no
/// `..Default::default()` — a removed or renamed field must fail the
/// build, not be papered over), then round-trips through prost.
#[test]
fn frozen_shapes_compile() {
    let burst = synth_proto::v1::Burst {
        format_version: synth_proto::v1::BURST_FORMAT_VERSION,
        burst_id: vec![0xAB; 32],
        body: Some(synth_proto::v1::burst::Body::Pad(
            synth_proto::v1::PadBurst {
                segments: vec![synth_proto::v1::PadSegment {
                    buttons: 0x0001,
                    hold_frames: 4,
                }],
                button_alphabet: "demo".to_owned(),
            },
        )),
    };
    let decoded = synth_proto::v1::Burst::decode(burst.encode_to_vec().as_slice())
        .expect("frozen Burst round-trips");
    assert_eq!(decoded, burst);

    let provenance = synth_proto::v1::Provenance {
        generator: synth_proto::v1::GeneratorKind::WeightedRandom as i32,
        slot: 0,
        rng_stream: "wr/0".to_owned(),
        config_fingerprint: vec![0xCD; 32],
        fallback_from: synth_proto::v1::GeneratorKind::Unspecified as i32,
        r#macro: None,
        mutation: None,
        policy: None,
    };
    let decoded = synth_proto::v1::Provenance::decode(provenance.encode_to_vec().as_slice())
        .expect("frozen Provenance round-trips");
    assert_eq!(decoded, provenance);
}
