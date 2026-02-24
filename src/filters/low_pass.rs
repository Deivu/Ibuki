use super::{AudioFilter, FilterError};

pub struct LowPassFilter {
    smoothing: f64,
    coefficient: f64,
    prev_left: f32,
    prev_right: f32,
}

impl LowPassFilter {
    pub fn new(smoothing: f64) -> Result<Self, FilterError> {
        if smoothing < 1.0 {
            return Err(FilterError::InvalidParameter(format!(
                "LowPass smoothing must be ≥ 1.0, got {}",
                smoothing
            )));
        }

        Ok(Self {
            smoothing,
            coefficient: 1.0 / smoothing,
            prev_left: 0.0,
            prev_right: 0.0,
        })
    }
}

impl AudioFilter for LowPassFilter {
    fn process(&mut self, samples: &mut [i16], _sample_rate: u32) -> Result<(), FilterError> {
        if samples.len() % 2 != 0 {
            return Err(FilterError::BufferSizeMismatch);
        }

        let coeff = self.coefficient as f32;

        for chunk in samples.chunks_exact_mut(2) {
            let left = chunk[0] as f32;
            let right = chunk[1] as f32;

            self.prev_left = self.prev_left + coeff * (left - self.prev_left);
            self.prev_right = self.prev_right + coeff * (right - self.prev_right);

            chunk[0] = self.prev_left.clamp(i16::MIN as f32, i16::MAX as f32) as i16;
            chunk[1] = self.prev_right.clamp(i16::MIN as f32, i16::MAX as f32) as i16;
        }

        Ok(())
    }

    fn is_active(&self) -> bool {
        (self.smoothing - 1.0).abs() > f64::EPSILON
    }

    fn name(&self) -> &'static str {
        "LowPass"
    }

    fn reset(&mut self) {
        self.prev_left = 0.0;
        self.prev_right = 0.0;
    }
}
