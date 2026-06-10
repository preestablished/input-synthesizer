#![forbid(unsafe_code)]

use synth_proto::v1::{Burst, PadSegment, BURST_FORMAT_VERSION};

pub trait InputModel {
    fn legalize(&self, burst: Burst) -> Burst;
}

#[derive(Default)]
pub struct PadModel;

impl InputModel for PadModel {
    fn legalize(&self, mut burst: Burst) -> Burst {
        burst.format_version = BURST_FORMAT_VERSION;
        burst
    }
}

pub fn neutral_burst(frames: u32) -> Burst {
    Burst {
        format_version: BURST_FORMAT_VERSION,
        pad_segments: vec![PadSegment {
            start_frame: 0,
            frames,
            buttons: 0,
        }],
    }
}
