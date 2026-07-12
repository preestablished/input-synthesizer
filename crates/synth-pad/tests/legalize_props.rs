//! Property tests for `PadModel` (ARCHITECTURE.md §2.1, API.md §1 invariants).

use proptest::prelude::*;
use synth_core::config::{ButtonAlphabet, ButtonDef, DirectionsCfg, ForbiddenMask};
use synth_core::model::InputModel;
use synth_core::types::{Burst, PadBurst, PadSegment, Token};
use synth_pad::PadModel;

const MIN_FRAMES: u32 = 16;
const MAX_FRAMES: u32 = 1800;

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

fn model() -> PadModel {
    PadModel::new(&console16_alphabet(), MIN_FRAMES, MAX_FRAMES)
}

fn pad_segments(b: &Burst) -> &[PadSegment] {
    let Burst::Pad(pad) = b else { unreachable!() };
    &pad.segments
}

fn arb_burst() -> impl Strategy<Value = Burst> {
    proptest::collection::vec((any::<u16>(), 0u32..3000), 0..60).prop_map(|segs| {
        Burst::Pad(PadBurst {
            segments: segs
                .into_iter()
                .map(|(buttons, hold_frames)| PadSegment {
                    buttons,
                    hold_frames,
                })
                .collect(),
        })
    })
}

/// Every API.md §1 pad invariant, checked against a legalized burst.
fn assert_pad_invariants(m: &PadModel, b: &Burst) {
    let segments = pad_segments(b);

    // At least 1 segment.
    prop_assert_no_panic(!segments.is_empty(), "at least 1 segment");

    let mut total: u64 = 0;
    for (i, s) in segments.iter().enumerate() {
        // No hold_frames == 0.
        prop_assert_no_panic(s.hold_frames >= 1, "hold_frames >= 1");

        // No exclusive-group violation.
        for &(a, b_) in &[(UP, DOWN), (LEFT, RIGHT)] {
            let group = a | b_;
            prop_assert_no_panic(
                (s.buttons & group).count_ones() < 2,
                "no exclusive-group violation",
            );
        }

        // No forbidden mask fully set.
        let forbidden = START | SELECT;
        prop_assert_no_panic(
            s.buttons & forbidden != forbidden,
            "no forbidden mask fully set",
        );

        // No two adjacent equal masks.
        if i > 0 {
            prop_assert_no_panic(
                s.buttons != segments[i - 1].buttons,
                "no two adjacent equal masks",
            );
        }

        total += u64::from(s.hold_frames);
    }

    // Total frames within [min, max].
    prop_assert_no_panic(
        total >= u64::from(m.min_frames()) && total <= u64::from(m.max_frames()),
        "total frames within [min_frames, max_frames]",
    );
}

/// proptest's `prop_assert!` only works inside a `proptest! { #[test] fn }`
/// body; this thin wrapper lets the invariant checker above be a plain
/// function called from within such a body while still producing a useful
/// panic message on failure.
fn prop_assert_no_panic(cond: bool, what: &str) {
    assert!(cond, "invariant violated: {what}");
}

proptest! {
    /// M0 accept: legalize is idempotent.
    #[test]
    fn legalize_is_idempotent(b in arb_burst()) {
        let m = model();
        let once = m.legalize(b);
        let twice = m.legalize(once.clone());
        prop_assert_eq!(once, twice);
    }

    /// M0 accept: legalized output satisfies every API.md §1 pad invariant.
    #[test]
    fn legalized_output_satisfies_invariants(b in arb_burst()) {
        let m = model();
        let legalized = m.legalize(b);
        assert_pad_invariants(&m, &legalized);
    }

    /// detokenize(tokenize(b)) preserves masks and segment count for a
    /// legalized burst (durations only need to land in the same bucket).
    #[test]
    fn detokenize_tokenize_preserves_shape(b in arb_burst()) {
        let m = model();
        let legalized = m.legalize(b);
        let tokens = m.tokenize(&legalized);
        let back = m.detokenize(&tokens);
        let orig = pad_segments(&legalized);
        let back = pad_segments(&back);
        prop_assert_eq!(orig.len(), back.len());
        for (a, b) in orig.iter().zip(back.iter()) {
            prop_assert_eq!(a.buttons, b.buttons);
        }
    }

    /// tokenize(detokenize(tokens)) == tokens for arbitrary valid token
    /// sequences: the pinned bucket midpoints must be bucket-stable.
    #[test]
    fn tokenize_detokenize_is_identity_on_tokens(
        toks in proptest::collection::vec((any::<u16>(), 0u8..=5), 0..40)
    ) {
        let m = model();
        let tokens: Vec<Token> = toks
            .into_iter()
            .map(|(mask, dur_bucket)| Token { mask, dur_bucket })
            .collect();
        let burst = m.detokenize(&tokens);
        let round_tripped = m.tokenize(&burst);
        prop_assert_eq!(round_tripped, tokens);
    }
}
