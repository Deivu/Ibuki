use super::{AudioFilter, FilterError};

pub struct TremoloFilter {
    frequency: f64,
    depth: f64,
    phase: f64,
}

impl TremoloFilter {
    pub fn new(frequency: f64, depth: f64) -> Result<Self, FilterError> {
        if frequency <= 0.0 {
            return Err(FilterError::InvalidParameter(format!(
                "Tremolo frequency must be > 0, got {}",
                frequency
            )));
        }
        if !(0.0..=1.0).contains(&depth) {
            return Err(FilterError::InvalidParameter(format!(
                "Tremolo depth must be 0.0–1.0, got {}",
                depth
            )));
        }

        Ok(Self {
            frequency,
            depth,
            phase: 0.0,
        })
    }
}

impl AudioFilter for TremoloFilter {
    fn process(&mut self, samples: &mut [i16], sample_rate: u32) -> Result<(), FilterError> {
        use std::f64::consts::PI;

        let phase_increment = 2.0 * PI * self.frequency / sample_rate as f64;

        for chunk in samples.chunks_exact_mut(2) {
            let modulation = 1.0 - self.depth * (0.5 * (1.0 - self.phase.sin()));

            let left = (chunk[0] as f64 * modulation)
                .clamp(i16::MIN as f64, i16::MAX as f64) as i16;
            let right = (chunk[1] as f64 * modulation)
                .clamp(i16::MIN as f64, i16::MAX as f64) as i16;

            chunk[0] = left;
            chunk[1] = right;

            self.phase += phase_increment;
            if self.phase > 2.0 * PI {
                self.phase -= 2.0 * PI;
            }
        }

        Ok(())
    }

    fn is_active(&self) -> bool {
        self.depth.abs() > f64::EPSILON
    }

    fn name(&self) -> &'static str {
        "Tremolo"
    }

    fn reset(&mut self) {
        self.phase = 0.0;
    }
}
