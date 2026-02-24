use super::{AudioFilter, FilterError};

pub struct RotationFilter {
    rotation_hz: f64,
    phase: f64,
}

impl RotationFilter {
    pub fn new(rotation_hz: f64) -> Result<Self, FilterError> {
        Ok(Self {
            rotation_hz,
            phase: 0.0,
        })
    }
}

impl AudioFilter for RotationFilter {
    fn process(&mut self, samples: &mut [i16], sample_rate: u32) -> Result<(), FilterError> {
        use std::f64::consts::PI;

        let phase_inc = 2.0 * PI * self.rotation_hz / sample_rate as f64;

        for chunk in samples.chunks_exact_mut(2) {
            let left = chunk[0] as f64;
            let right = chunk[1] as f64;

            let pan_left = (self.phase + 0.25 * PI).cos();
            let pan_right = self.phase.sin();

            let mono = (left + right) * 0.5;
            let left_out = mono * pan_left;
            let right_out = mono * pan_right;

            chunk[0] = left_out.clamp(i16::MIN as f64, i16::MAX as f64) as i16;
            chunk[1] = right_out.clamp(i16::MIN as f64, i16::MAX as f64) as i16;

            self.phase += phase_inc;
            if self.phase > 2.0 * PI {
                self.phase -= 2.0 * PI;
            } else if self.phase < 0.0 {
                self.phase += 2.0 * PI;
            }
        }

        Ok(())
    }

    fn is_active(&self) -> bool {
        self.rotation_hz.abs() > f64::EPSILON
    }

    fn name(&self) -> &'static str {
        "Rotation"
    }

    fn reset(&mut self) {
        self.phase = 0.0;
    }
}
