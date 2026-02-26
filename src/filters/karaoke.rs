use super::{AudioFilter, FilterError};

pub struct KaraokeFilter {
    level: f64,
    mono_level: f64,
    filter_band: f64,
    filter_width: f64,
}

impl KaraokeFilter {
    pub fn new(
        level: f64,
        mono_level: f64,
        filter_band: f64,
        filter_width: f64,
    ) -> Result<Self, FilterError> {
        if !(0.0..=1.0).contains(&level) {
            return Err(FilterError::InvalidParameter(format!(
                "Karaoke level must be 0.0–1.0, got {}",
                level
            )));
        }
        if !(0.0..=1.0).contains(&mono_level) {
            return Err(FilterError::InvalidParameter(format!(
                "Karaoke mono_level must be 0.0–1.0, got {}",
                mono_level
            )));
        }

        Ok(Self {
            level,
            mono_level,
            filter_band,
            filter_width,
        })
    }
}

impl AudioFilter for KaraokeFilter {
    fn process(&mut self, samples: &mut [i16], _sample_rate: u32) -> Result<(), FilterError> {
        if samples.len() % 2 != 0 {
            return Err(FilterError::BufferSizeMismatch);
        }

        for chunk in samples.chunks_exact_mut(2) {
            let left = chunk[0] as f64;
            let right = chunk[1] as f64;

            let mid = (left + right) * 0.5;
            let side = (left - right) * 0.5;

            let filtered_mid = mid * (1.0 - self.level);
            let filtered_side = side * self.mono_level;

            let new_left = filtered_mid + filtered_side;
            let new_right = filtered_mid - filtered_side;

            chunk[0] = new_left.clamp(i16::MIN as f64, i16::MAX as f64) as i16;
            chunk[1] = new_right.clamp(i16::MIN as f64, i16::MAX as f64) as i16;
        }

        Ok(())
    }

    fn is_active(&self) -> bool {
        self.level.abs() > f64::EPSILON || (self.mono_level - 1.0).abs() > f64::EPSILON
    }

    fn name(&self) -> &'static str {
        "Karaoke"
    }

    fn reset(&mut self) {}
}
