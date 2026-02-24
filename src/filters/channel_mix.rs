use super::{AudioFilter, FilterError};

pub struct ChannelMixFilter {
    left_to_left: f64,
    left_to_right: f64,
    right_to_left: f64,
    right_to_right: f64,
}

impl ChannelMixFilter {
    pub fn new(
        left_to_left: f64,
        left_to_right: f64,
        right_to_left: f64,
        right_to_right: f64,
    ) -> Result<Self, FilterError> {
        for (name, val) in [
            ("leftToLeft", left_to_left),
            ("leftToRight", left_to_right),
            ("rightToLeft", right_to_left),
            ("rightToRight", right_to_right),
        ] {
            if !(0.0..=1.0).contains(&val) {
                return Err(FilterError::InvalidParameter(format!(
                    "{} must be 0.0–1.0, got {}",
                    name, val
                )));
            }
        }

        Ok(Self {
            left_to_left,
            left_to_right,
            right_to_left,
            right_to_right,
        })
    }
}

impl AudioFilter for ChannelMixFilter {
    fn process(&mut self, samples: &mut [i16], _sample_rate: u32) -> Result<(), FilterError> {
        if samples.len() % 2 != 0 {
            return Err(FilterError::BufferSizeMismatch);
        }

        for chunk in samples.chunks_exact_mut(2) {
            let left = chunk[0] as f64;
            let right = chunk[1] as f64;

            let new_left = left * self.left_to_left + right * self.right_to_left;
            let new_right = left * self.left_to_right + right * self.right_to_right;

            chunk[0] = new_left.clamp(i16::MIN as f64, i16::MAX as f64) as i16;
            chunk[1] = new_right.clamp(i16::MIN as f64, i16::MAX as f64) as i16;
        }

        Ok(())
    }

    fn is_active(&self) -> bool {
        (self.left_to_left - 1.0).abs() > f64::EPSILON
            || self.left_to_right.abs() > f64::EPSILON
            || self.right_to_left.abs() > f64::EPSILON
            || (self.right_to_right - 1.0).abs() > f64::EPSILON
    }

    fn name(&self) -> &'static str {
        "ChannelMix"
    }

    fn reset(&mut self) {
    }
}
