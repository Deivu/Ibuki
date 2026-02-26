use super::{AudioFilter, FilterError};

pub struct VolumeFilter {
    multiplier: f32,
}

impl VolumeFilter {
    pub fn new(volume: f64) -> Result<Self, FilterError> {
        if !(0.0..=5.0).contains(&volume) {
            return Err(FilterError::InvalidParameter(format!(
                "Volume must be between 0.0 and 5.0, got {}",
                volume
            )));
        }

        Ok(Self {
            multiplier: volume as f32,
        })
    }

    pub fn multiplier(&self) -> f32 {
        self.multiplier
    }
}

impl AudioFilter for VolumeFilter {
    fn process(&mut self, samples: &mut [i16], _sample_rate: u32) -> Result<(), FilterError> {
        for sample in samples.iter_mut() {
            let adjusted = (*sample as f32) * self.multiplier;
            *sample = adjusted.clamp(i16::MIN as f32, i16::MAX as f32) as i16;
        }
        Ok(())
    }

    fn is_active(&self) -> bool {
        (self.multiplier - 1.0).abs() > f32::EPSILON
    }

    fn name(&self) -> &'static str {
        "Volume"
    }

    fn reset(&mut self) {}
}
