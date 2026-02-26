use super::{AudioFilter, FilterError};

pub struct DistortionFilter {
    sin_offset: f64,
    sin_scale: f64,
    cos_offset: f64,
    cos_scale: f64,
    tan_offset: f64,
    tan_scale: f64,
    offset: f64,
    scale: f64,
}

impl DistortionFilter {
    pub fn new(
        sin_offset: f64,
        sin_scale: f64,
        cos_offset: f64,
        cos_scale: f64,
        tan_offset: f64,
        tan_scale: f64,
        offset: f64,
        scale: f64,
    ) -> Result<Self, FilterError> {
        Ok(Self {
            sin_offset,
            sin_scale,
            cos_offset,
            cos_scale,
            tan_offset,
            tan_scale,
            offset,
            scale,
        })
    }

    #[inline]
    fn distort(&self, sample: f64) -> f64 {
        let transformed = (sample * self.sin_scale + self.sin_offset).sin()
            + (sample * self.cos_scale + self.cos_offset).cos()
            + (sample * self.tan_scale + self.tan_offset).tan()
            + self.offset;

        transformed * self.scale
    }
}

impl AudioFilter for DistortionFilter {
    fn process(&mut self, samples: &mut [i16], _sample_rate: u32) -> Result<(), FilterError> {
        for sample in samples.iter_mut() {
            let input = *sample as f64 / i16::MAX as f64;
            let distorted = self.distort(input);
            *sample = (distorted * i16::MAX as f64).clamp(i16::MIN as f64, i16::MAX as f64) as i16;
        }
        Ok(())
    }

    fn is_active(&self) -> bool {
        self.sin_offset.abs() > f64::EPSILON
            || (self.sin_scale - 1.0).abs() > f64::EPSILON
            || self.cos_offset.abs() > f64::EPSILON
            || (self.cos_scale - 1.0).abs() > f64::EPSILON
            || self.tan_offset.abs() > f64::EPSILON
            || (self.tan_scale - 1.0).abs() > f64::EPSILON
            || self.offset.abs() > f64::EPSILON
            || (self.scale - 1.0).abs() > f64::EPSILON
    }

    fn name(&self) -> &'static str {
        "Distortion"
    }

    fn reset(&mut self) {}
}
