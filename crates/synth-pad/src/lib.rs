#![forbid(unsafe_code)]

//! Pad input model (ARCHITECTURE.md §2.1): button alphabets, legality
//! filter, segment encoding, tokenize/detokenize.

use synth_core::config::ButtonAlphabet;
use synth_core::model::{InputModel, ModelKind};
use synth_core::types::{Burst, PadBurst, PadSegment, Token};

/// Pinned duration-bucket midpoints (frames) used by `detokenize`. Buckets
/// are `[1], [2,3], [4,7], [8,15], [16,31], [32,inf)` (ARCHITECTURE.md
/// §6.2); each midpoint is chosen so `tokenize(midpoint)` maps back to the
/// same bucket (bucket stability), which the `bucket_midpoints_are_bucket_stable`
/// test below enforces.
const BUCKET_MIDPOINTS: [u32; 6] = [1, 2, 5, 11, 23, 48];

/// Pad input model: precomputed legality masks derived from a
/// [`ButtonAlphabet`] plus burst-length bounds. `&self` holds only loaded
/// config, per the `InputModel` contract.
#[derive(Clone, Debug)]
pub struct PadModel {
    min_frames: u32,
    max_frames: u32,
    /// One mask per `exclusive_groups` entry (e.g. `UP|DOWN`, `LEFT|RIGHT`).
    exclusive_masks: Vec<u16>,
    /// `(mask, clear)` pairs from `forbidden_masks`: if all bits of `mask`
    /// are set on a segment, clear the `clear` bits.
    forbidden: Vec<(u16, u16)>,
    /// Bitmask over the alphabet's declared direction group
    /// (`directions.group`), for samplers that treat direction as one
    /// categorical process (ARCHITECTURE.md §2.1, §4.3).
    direction_mask: u16,
}

impl PadModel {
    /// Build a `PadModel` from a validated [`ButtonAlphabet`] and the
    /// experiment's `[min_frames, max_frames]` burst-length bounds. Names
    /// that fail to resolve to a bit are skipped (`debug_assert`s in debug
    /// builds) rather than panicking — the alphabet is expected to already
    /// be validated config (API.md §5 `validate`).
    pub fn new(alphabet: &ButtonAlphabet, min_frames: u32, max_frames: u32) -> Self {
        debug_assert!(
            min_frames >= 1 && min_frames <= max_frames,
            "PadModel requires 1 <= min_frames <= max_frames (got {min_frames}..={max_frames})"
        );

        let mut exclusive_masks = Vec::with_capacity(alphabet.exclusive_groups.len());
        for group in &alphabet.exclusive_groups {
            match alphabet.mask(group) {
                Some(mask) => exclusive_masks.push(mask),
                None => debug_assert!(
                    false,
                    "exclusive group references undeclared button(s): {group:?}"
                ),
            }
        }

        let mut forbidden = Vec::with_capacity(alphabet.forbidden_masks.len());
        for fm in &alphabet.forbidden_masks {
            match (alphabet.mask(&fm.mask), alphabet.mask(&fm.clear)) {
                (Some(mask), Some(clear)) => forbidden.push((mask, clear)),
                _ => debug_assert!(
                    false,
                    "forbidden_masks entry references undeclared button(s): {fm:?}"
                ),
            }
        }

        let direction_mask = alphabet.mask(&alphabet.directions.group).unwrap_or(0);

        Self {
            min_frames,
            max_frames,
            exclusive_masks,
            forbidden,
            direction_mask,
        }
    }

    /// Minimum legal total burst length in frames.
    pub fn min_frames(&self) -> u32 {
        self.min_frames
    }

    /// Maximum legal total burst length in frames.
    pub fn max_frames(&self) -> u32 {
        self.max_frames
    }

    /// Bitmask over the alphabet's direction group (d-pad/stick bits), for
    /// samplers that need to treat direction as one categorical process.
    pub fn direction_mask(&self) -> u16 {
        self.direction_mask
    }

    /// Per-segment legality pass: exclusive groups, then forbidden masks,
    /// then hold_frames clamp. Order matters (ARCHITECTURE.md §2.1): a
    /// forbidden mask can only fire on bits that survived the exclusive-group
    /// pass.
    fn legalize_segment(&self, seg: &mut PadSegment) {
        for &group_mask in &self.exclusive_masks {
            if (seg.buttons & group_mask).count_ones() >= 2 {
                seg.buttons &= !group_mask;
            }
        }
        for &(mask, clear) in &self.forbidden {
            if seg.buttons & mask == mask {
                seg.buttons &= !clear;
            }
        }
        if seg.hold_frames == 0 {
            seg.hold_frames = 1;
        }
    }

    /// Merge adjacent segments sharing an identical mask, summing
    /// `hold_frames` with saturation. Never changes total frames except on
    /// `u32` saturation (unreachable in practice: burst lengths are bounded
    /// by `max_frames`).
    fn merge_adjacent(segments: Vec<PadSegment>) -> Vec<PadSegment> {
        let mut merged: Vec<PadSegment> = Vec::with_capacity(segments.len());
        for seg in segments {
            if let Some(last) = merged.last_mut() {
                if last.buttons == seg.buttons {
                    last.hold_frames = last.hold_frames.saturating_add(seg.hold_frames);
                    continue;
                }
            }
            merged.push(seg);
        }
        merged
    }

    /// Clamp total frames to `[min_frames, max_frames]`. Over-length bursts
    /// are truncated from the end (the last surviving segment may be cut
    /// mid-way; fully-dropped trailing segments never appear in the
    /// output). Under-length bursts are extended by growing the final
    /// segment (segments is always non-empty by this point — see
    /// `legalize`). Both directions are no-ops when already in range, which
    /// is what makes `legalize` idempotent around this step.
    fn clamp_total_frames(&self, mut segments: Vec<PadSegment>) -> Vec<PadSegment> {
        let total: u64 = segments.iter().map(|s| u64::from(s.hold_frames)).sum();
        if total > u64::from(self.max_frames) {
            let mut remaining = u64::from(self.max_frames);
            let mut truncated = Vec::with_capacity(segments.len());
            for seg in segments {
                if remaining == 0 {
                    break;
                }
                let take = u64::from(seg.hold_frames).min(remaining);
                truncated.push(PadSegment {
                    buttons: seg.buttons,
                    hold_frames: take as u32,
                });
                remaining -= take;
            }
            segments = truncated;
        }

        let total: u64 = segments.iter().map(|s| u64::from(s.hold_frames)).sum();
        if total < u64::from(self.min_frames) {
            let deficit = u64::from(self.min_frames) - total;
            let deficit = u32::try_from(deficit).unwrap_or(u32::MAX);
            if let Some(last) = segments.last_mut() {
                last.hold_frames = last.hold_frames.saturating_add(deficit);
            } else {
                segments.push(PadSegment {
                    buttons: 0,
                    hold_frames: self.min_frames,
                });
            }
        }

        // Truncation/extension never rearranges segments, but re-merge
        // defensively so the invariant ("no adjacent equal masks") holds
        // unconditionally regardless of how the segment list was produced.
        Self::merge_adjacent(segments)
    }
}

impl InputModel for PadModel {
    type Unit = PadSegment;

    fn burst_len(&self, b: &Burst) -> u64 {
        let Burst::Pad(pad) = b else {
            panic!("PadModel::burst_len called with a non-pad burst");
        };
        pad.total_frames()
    }

    fn legalize(&self, b: Burst) -> Burst {
        let Burst::Pad(pad) = b else {
            panic!("PadModel::legalize called with a non-pad burst");
        };
        let mut segments = pad.segments;

        // (1)-(3): per-segment exclusive groups, forbidden masks, hold clamp.
        for seg in &mut segments {
            self.legalize_segment(seg);
        }

        // (4): merge adjacent equal-mask segments.
        segments = Self::merge_adjacent(segments);

        // (5): ensure at least 1 segment.
        if segments.is_empty() {
            segments.push(PadSegment {
                buttons: 0,
                hold_frames: self.min_frames,
            });
        }

        // (6): clamp total frames to [min_frames, max_frames].
        segments = self.clamp_total_frames(segments);

        Burst::Pad(PadBurst { segments })
    }

    fn tokenize(&self, b: &Burst) -> Vec<Token> {
        let Burst::Pad(pad) = b else {
            panic!("PadModel::tokenize called with a non-pad burst");
        };
        pad.segments
            .iter()
            .map(|seg| {
                let hold = seg.hold_frames.max(1);
                let bucket = hold.ilog2().min(5);
                Token {
                    mask: seg.buttons,
                    dur_bucket: bucket as u8,
                }
            })
            .collect()
    }

    fn detokenize(&self, t: &[Token]) -> Burst {
        let segments = t
            .iter()
            .map(|tok| PadSegment {
                buttons: tok.mask,
                hold_frames: BUCKET_MIDPOINTS[usize::from(tok.dur_bucket.min(5))],
            })
            .collect();
        Burst::Pad(PadBurst { segments })
    }

    fn kind(&self) -> ModelKind {
        ModelKind::Pad
    }
}

/// One all-released segment spanning `frames` (clamped to >= 1, matching the
/// `PadSegment::hold_frames` invariant).
pub fn neutral_burst(frames: u32) -> Burst {
    Burst::Pad(PadBurst {
        segments: vec![PadSegment {
            buttons: 0,
            hold_frames: frames.max(1),
        }],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use synth_core::config::{ButtonDef, DirectionsCfg, ForbiddenMask};

    /// console16-12btn-v1 (API.md §5.1).
    fn console16_alphabet() -> ButtonAlphabet {
        ButtonAlphabet {
            name: "console16-12btn-v1".to_owned(),
            buttons: vec![
                ButtonDef {
                    name: "A".into(),
                    bit: 0,
                },
                ButtonDef {
                    name: "B".into(),
                    bit: 1,
                },
                ButtonDef {
                    name: "X".into(),
                    bit: 2,
                },
                ButtonDef {
                    name: "Y".into(),
                    bit: 3,
                },
                ButtonDef {
                    name: "L".into(),
                    bit: 4,
                },
                ButtonDef {
                    name: "R".into(),
                    bit: 5,
                },
                ButtonDef {
                    name: "UP".into(),
                    bit: 6,
                },
                ButtonDef {
                    name: "DOWN".into(),
                    bit: 7,
                },
                ButtonDef {
                    name: "LEFT".into(),
                    bit: 8,
                },
                ButtonDef {
                    name: "RIGHT".into(),
                    bit: 9,
                },
                ButtonDef {
                    name: "START".into(),
                    bit: 10,
                },
                ButtonDef {
                    name: "SELECT".into(),
                    bit: 11,
                },
            ],
            exclusive_groups: vec![
                vec!["UP".to_owned(), "DOWN".to_owned()],
                vec!["LEFT".to_owned(), "RIGHT".to_owned()],
            ],
            forbidden_masks: vec![ForbiddenMask {
                mask: vec!["START".to_owned(), "SELECT".to_owned()],
                clear: vec!["SELECT".to_owned()],
            }],
            directions: DirectionsCfg {
                group: vec![
                    "UP".to_owned(),
                    "DOWN".to_owned(),
                    "LEFT".to_owned(),
                    "RIGHT".to_owned(),
                ],
                allow_diagonals: true,
            },
        }
    }

    const UP: u16 = 1 << 6;
    const DOWN: u16 = 1 << 7;
    const LEFT: u16 = 1 << 8;
    const RIGHT: u16 = 1 << 9;
    const START: u16 = 1 << 10;
    const SELECT: u16 = 1 << 11;
    const B: u16 = 1 << 1;

    fn model(min_frames: u32, max_frames: u32) -> PadModel {
        PadModel::new(&console16_alphabet(), min_frames, max_frames)
    }

    fn seg(buttons: u16, hold_frames: u32) -> PadSegment {
        PadSegment {
            buttons,
            hold_frames,
        }
    }

    fn burst(segments: Vec<PadSegment>) -> Burst {
        Burst::Pad(PadBurst { segments })
    }

    fn pad_segments(b: &Burst) -> &[PadSegment] {
        let Burst::Pad(pad) = b else { unreachable!() };
        &pad.segments
    }

    #[test]
    fn up_down_exclusive_clears_both() {
        let m = model(16, 1800);
        let out = m.legalize(burst(vec![seg(UP | DOWN, 30)]));
        assert_eq!(pad_segments(&out), &[seg(0, 30)]);
    }

    #[test]
    fn left_right_exclusive_clears_both() {
        let m = model(16, 1800);
        let out = m.legalize(burst(vec![seg(LEFT | RIGHT | B, 30)]));
        assert_eq!(pad_segments(&out), &[seg(B, 30)]);
    }

    #[test]
    fn start_select_forbidden_clears_select() {
        let m = model(16, 1800);
        let out = m.legalize(burst(vec![seg(START | SELECT, 30)]));
        assert_eq!(pad_segments(&out), &[seg(START, 30)]);
    }

    #[test]
    fn zero_hold_frames_clamped_to_one() {
        let m = model(1, 1800);
        let out = m.legalize(burst(vec![seg(B, 0), seg(0, 20)]));
        assert_eq!(pad_segments(&out), &[seg(B, 1), seg(0, 20)]);
    }

    #[test]
    fn adjacent_equal_masks_merged() {
        let m = model(1, 1800);
        let out = m.legalize(burst(vec![seg(B, 10), seg(B, 5), seg(0, 3)]));
        assert_eq!(pad_segments(&out), &[seg(B, 15), seg(0, 3)]);
    }

    #[test]
    fn empty_burst_becomes_one_neutral_segment_of_min_frames() {
        let m = model(16, 1800);
        let out = m.legalize(burst(vec![]));
        assert_eq!(pad_segments(&out), &[seg(0, 16)]);
    }

    #[test]
    fn over_max_truncated_to_max() {
        let m = model(16, 100);
        let out = m.legalize(burst(vec![seg(B, 60), seg(0, 60)]));
        assert_eq!(pad_segments(&out), &[seg(B, 60), seg(0, 40)]);
    }

    #[test]
    fn under_min_extended_to_min() {
        let m = model(100, 1800);
        let out = m.legalize(burst(vec![seg(B, 10)]));
        assert_eq!(pad_segments(&out), &[seg(B, 100)]);
    }

    #[test]
    fn legalize_is_idempotent_on_fixtures() {
        let m = model(16, 100);
        let inputs = vec![
            burst(vec![seg(UP | DOWN, 30)]),
            burst(vec![seg(START | SELECT, 5), seg(START | SELECT, 5)]),
            burst(vec![]),
            burst(vec![seg(B, 1000), seg(B, 1000)]),
        ];
        for b in inputs {
            let once = m.legalize(b);
            let twice = m.legalize(once.clone());
            assert_eq!(once, twice);
        }
    }

    #[test]
    fn tokenize_bucket_boundaries() {
        let m = model(1, 3000);
        let cases: [(u32, u8); 11] = [
            (1, 0),
            (2, 1),
            (3, 1),
            (4, 2),
            (7, 2),
            (8, 3),
            (15, 3),
            (16, 4),
            (31, 4),
            (32, 5),
            (1000, 5),
        ];
        for (hold, expected_bucket) in cases {
            let b = burst(vec![seg(B, hold)]);
            let tokens = m.tokenize(&b);
            assert_eq!(
                tokens,
                vec![Token {
                    mask: B,
                    dur_bucket: expected_bucket
                }]
            );
        }
    }

    #[test]
    fn detokenize_preserves_masks_and_segment_count_for_legalized_burst() {
        let m = model(16, 1800);
        let legalized = m.legalize(burst(vec![seg(B, 30), seg(UP, 5), seg(0, 200)]));
        let tokens = m.tokenize(&legalized);
        let back = m.detokenize(&tokens);
        let orig_segments = pad_segments(&legalized);
        let back_segments = pad_segments(&back);
        assert_eq!(orig_segments.len(), back_segments.len());
        for (a, b) in orig_segments.iter().zip(back_segments.iter()) {
            assert_eq!(a.buttons, b.buttons);
        }
    }

    #[test]
    fn bucket_midpoints_are_bucket_stable() {
        // Pinned convention: each midpoint must map back to its own bucket.
        for (bucket, &midpoint) in BUCKET_MIDPOINTS.iter().enumerate() {
            let computed = midpoint.ilog2().min(5) as usize;
            assert_eq!(computed, bucket, "midpoint {midpoint} for bucket {bucket}");
        }
    }
}
