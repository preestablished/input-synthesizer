//! Domain burst types and proto conversions (ARCHITECTURE.md §2, API.md §1).
//!
//! Domain types are the decision-path representation; proto types exist only
//! at the wire boundary. Proto `map<>` fields and any other unordered
//! structures never cross into decision paths — conversions normalize here.

use serde::{Deserialize, Serialize};

use crate::BURST_FORMAT_VERSION;

/// One run-length unit of a pad burst. Bit i set ⇔ button with `bit = i`
/// held for every frame of the segment; bit assignment comes entirely from
/// the experiment config's button alphabet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PadSegment {
    pub buttons: u16,
    /// ≥ 1 after `legalize`.
    pub hold_frames: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PadBurst {
    pub segments: Vec<PadSegment>,
}

impl PadBurst {
    pub fn total_frames(&self) -> u64 {
        self.segments.iter().map(|s| u64::from(s.hold_frames)).sum()
    }
}

/// A burst proposal. The event-grammar variant lands with M5; until then the
/// enum is single-variant but non-exhaustive so downstream matches stay
/// forward-compatible.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Burst {
    Pad(PadBurst),
}

/// Canonical mining/dedup token (ARCHITECTURE.md §6.2): legal mask + log2
/// duration bucket.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Token {
    pub mask: u16,
    pub dur_bucket: u8,
}

/// Stable content hash of a burst: BLAKE3-256 over the postcard encoding of
/// the versioned wire form `(BURST_FORMAT_VERSION, burst)`. Used for golden
/// tests, dedup, and provenance `burst_id`s. Postcard over these ordered
/// domain types is canonical (no maps involved).
pub fn burst_hash(burst: &Burst) -> [u8; 32] {
    let encoded = postcard::to_allocvec(&(BURST_FORMAT_VERSION, burst))
        .expect("postcard encoding of a burst cannot fail");
    *blake3::hash(&encoded).as_bytes()
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ConvertError {
    #[error("burst has no body")]
    MissingBody,
    #[error("event-grammar bursts are not supported until M5")]
    EventModelUnsupported,
    #[error("segment {index}: buttons mask {mask:#x} exceeds 16 bits")]
    ButtonsOutOfRange { index: usize, mask: u32 },
    #[error("burst format_version {got} unsupported (expected {expected})")]
    FormatVersion { got: u32, expected: u32 },
}

/// Convert a domain burst to its proto wire form, stamping `format_version`,
/// `burst_id`, and the alphabet name.
pub fn to_proto(burst: &Burst, button_alphabet: &str) -> synth_proto::v1::Burst {
    let id = burst_hash(burst);
    match burst {
        Burst::Pad(pad) => synth_proto::v1::Burst {
            format_version: BURST_FORMAT_VERSION,
            burst_id: id.to_vec(),
            body: Some(synth_proto::v1::burst::Body::Pad(
                synth_proto::v1::PadBurst {
                    segments: pad
                        .segments
                        .iter()
                        .map(|s| synth_proto::v1::PadSegment {
                            buttons: u32::from(s.buttons),
                            hold_frames: s.hold_frames,
                        })
                        .collect(),
                    button_alphabet: button_alphabet.to_owned(),
                },
            )),
        },
    }
}

/// Convert a proto burst into the domain form, rejecting (never truncating)
/// out-of-range values. Returns the burst and its declared alphabet name.
/// `format_version = 0` (unset) is accepted for inbound bursts; a different
/// explicit version is rejected.
pub fn from_proto(proto: &synth_proto::v1::Burst) -> Result<(Burst, String), ConvertError> {
    if proto.format_version != 0 && proto.format_version != BURST_FORMAT_VERSION {
        return Err(ConvertError::FormatVersion {
            got: proto.format_version,
            expected: BURST_FORMAT_VERSION,
        });
    }
    match proto.body.as_ref().ok_or(ConvertError::MissingBody)? {
        synth_proto::v1::burst::Body::Pad(pad) => {
            let mut segments = Vec::with_capacity(pad.segments.len());
            for (index, s) in pad.segments.iter().enumerate() {
                let buttons =
                    u16::try_from(s.buttons).map_err(|_| ConvertError::ButtonsOutOfRange {
                        index,
                        mask: s.buttons,
                    })?;
                segments.push(PadSegment {
                    buttons,
                    hold_frames: s.hold_frames,
                });
            }
            Ok((
                Burst::Pad(PadBurst { segments }),
                pad.button_alphabet.clone(),
            ))
        }
        synth_proto::v1::burst::Body::Event(_) => Err(ConvertError::EventModelUnsupported),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn arb_pad_burst() -> impl Strategy<Value = PadBurst> {
        proptest::collection::vec((any::<u16>(), 1u32..2000), 0..40).prop_map(|segs| PadBurst {
            segments: segs
                .into_iter()
                .map(|(buttons, hold_frames)| PadSegment {
                    buttons,
                    hold_frames,
                })
                .collect(),
        })
    }

    proptest! {
        /// M0 accept: protos round-trip (prost encode/decode property test),
        /// composed with the domain conversion so the whole boundary is
        /// covered.
        #[test]
        fn proto_roundtrip(pad in arb_pad_burst()) {
            use synth_proto::v1::Burst as ProtoBurst;
            let burst = Burst::Pad(pad);
            let proto = to_proto(&burst, "console16-12btn-v1");
            let bytes = prost::Message::encode_to_vec(&proto);
            let decoded = <ProtoBurst as prost::Message>::decode(&bytes[..]).unwrap();
            prop_assert_eq!(&decoded, &proto);
            let (back, alphabet) = from_proto(&decoded).unwrap();
            prop_assert_eq!(back, burst);
            prop_assert_eq!(alphabet, "console16-12btn-v1");
        }

        #[test]
        fn burst_hash_is_stable_and_input_sensitive(pad in arb_pad_burst()) {
            let b = Burst::Pad(pad.clone());
            prop_assert_eq!(burst_hash(&b), burst_hash(&b.clone()));
            let mut altered = pad;
            altered.segments.push(PadSegment { buttons: 1, hold_frames: 1 });
            prop_assert_ne!(burst_hash(&b), burst_hash(&Burst::Pad(altered)));
        }
    }

    #[test]
    fn from_proto_rejects_wide_masks() {
        let mut proto = to_proto(
            &Burst::Pad(PadBurst {
                segments: vec![PadSegment {
                    buttons: 1,
                    hold_frames: 1,
                }],
            }),
            "a",
        );
        if let Some(synth_proto::v1::burst::Body::Pad(pad)) = proto.body.as_mut() {
            pad.segments[0].buttons = 0x1_0000;
        }
        assert_eq!(
            from_proto(&proto),
            Err(ConvertError::ButtonsOutOfRange {
                index: 0,
                mask: 0x1_0000
            })
        );
    }
}
