#![forbid(unsafe_code)]

#[derive(Clone, Debug)]
pub struct FanoutRng {
    state: u64,
}

impl FanoutRng {
    pub fn stream(root: u64, label: &str) -> Self {
        let mut state = root ^ 0x9E37_79B9_7F4A_7C15;
        for byte in label.bytes() {
            state = state.rotate_left(5) ^ u64::from(byte);
        }
        Self { state }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state ^= self.state << 7;
        self.state ^= self.state >> 9;
        self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_is_stable() {
        let mut rng = FanoutRng::stream(123, "m0");
        assert_eq!(rng.next_u64(), 0x2E83_5164_65C2_D6C9);
    }
}
