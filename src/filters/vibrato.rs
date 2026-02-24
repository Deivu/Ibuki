use super::{AudioFilter, FilterError};

pub struct VibratoFilter {
    frequency: f64,
    depth: f64,
    phase: f64,
    delay_left: Vec<f32>,
    delay_right: Vec<f32>,
    write_pos: usize,
}

impl VibratoFilter {
    const MAX_DELAY: usize = 1024;

    pub fn new(frequency: f64, depth: f64) -> Result<Self, FilterError> {
        if !(0.0..=14.0).contains(&frequency) || frequency <= 0.0 {
            return Err(FilterError::InvalidParameter(format!(
                "Vibrato frequency must be > 0 and ≤ 14, got {}",
                frequency
            )));
        }
        if !(0.0..=1.0).contains(&depth) {
            return Err(FilterError::InvalidParameter(format!(
                "Vibrato depth must be 0.0–1.0, got {}",
                depth
            )));
        }

        Ok(Self {
            frequency,
            depth,
            phase: 0.0,
            delay_left: vec![0.0; Self::MAX_DELAY],
            delay_right: vec![0.0; Self::MAX_DELAY],
            write_pos: 0,
        })
    }
}

impl AudioFilter for VibratoFilter {
    fn process(&mut self, samples: &mut [i16], sample_rate: u32) -> Result<(), FilterError> {
        use std::f64::consts::PI;

        let phase_inc = 2.0 * PI * self.frequency / sample_rate as f64;
        let max_delay = (Self::MAX_DELAY as f64 * self.depth * 0.5).max(1.0);

        for chunk in samples.chunks_exact_mut(2) {
            self.delay_left[self.write_pos] = chunk[0] as f32;
            self.delay_right[self.write_pos] = chunk[1] as f32;

            let delay_samples = max_delay * (0.5 + 0.5 * self.phase.sin());

            let read_pos = self.write_pos as f64 - delay_samples;
            let read_pos = if read_pos < 0.0 {
                read_pos + Self::MAX_DELAY as f64
            } else {
                read_pos
            };

            let idx0 = read_pos.floor() as usize % Self::MAX_DELAY;
            let idx1 = (idx0 + 1) % Self::MAX_DELAY;
            let frac = read_pos.fract() as f32;

            let left_out =
                self.delay_left[idx0] * (1.0 - frac) + self.delay_left[idx1] * frac;
            let right_out =
                self.delay_right[idx0] * (1.0 - frac) + self.delay_right[idx1] * frac;

            chunk[0] = left_out.clamp(i16::MIN as f32, i16::MAX as f32) as i16;
            chunk[1] = right_out.clamp(i16::MIN as f32, i16::MAX as f32) as i16;

            self.write_pos = (self.write_pos + 1) % Self::MAX_DELAY;
            self.phase += phase_inc;
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
        "Vibrato"
    }

    fn reset(&mut self) {
        self.phase = 0.0;
        self.write_pos = 0;
        self.delay_left.fill(0.0);
        self.delay_right.fill(0.0);
    }
}
