use super::{AudioFilter, FilterError};
use crate::models::Equalizer;
use std::f64::consts::PI;

pub struct EqualizerFilter {
    bands: [BiquadFilter; 15],
    gains: [f64; 15],
}

impl EqualizerFilter {
    pub const FREQUENCIES: [f64; 15] = [
        25.0, 40.0, 63.0, 100.0, 160.0, 250.0, 400.0, 630.0, 1000.0, 1600.0, 2500.0, 4000.0,
        6300.0, 10000.0, 16000.0,
    ];

    const Q: f64 = 1.0;

    pub fn new(sample_rate: u32) -> Self {
        let bands = Self::FREQUENCIES.map(|freq| {
            BiquadFilter::peaking_eq(sample_rate as f64, freq, Self::Q, 0.0)
        });

        Self {
            bands,
            gains: [0.0; 15],
        }
    }

    pub fn from_bands(band_configs: &[Equalizer], sample_rate: u32) -> Result<Self, FilterError> {
        let mut filter = Self::new(sample_rate);

        for config in band_configs {
            if config.band > 14 {
                return Err(FilterError::InvalidParameter(format!(
                    "Band index {} out of range (0-14)",
                    config.band
                )));
            }

            if !(-0.25..=1.0).contains(&config.gain) {
                return Err(FilterError::InvalidParameter(format!(
                    "Gain {} out of range (-0.25 to 1.0)",
                    config.gain
                )));
            }

            let idx = config.band as usize;
            filter.gains[idx] = config.gain;
            filter.update_band(idx, config.gain, sample_rate);
        }

        Ok(filter)
    }

    fn update_band(&mut self, band_index: usize, gain: f64, sample_rate: u32) {
        let db = gain * 6.0;
        self.bands[band_index] =
            BiquadFilter::peaking_eq(sample_rate as f64, Self::FREQUENCIES[band_index], Self::Q, db);
    }
}

impl AudioFilter for EqualizerFilter {
    fn process(&mut self, samples: &mut [i16], _sample_rate: u32) -> Result<(), FilterError> {
        if samples.len() % 2 != 0 {
            return Err(FilterError::BufferSizeMismatch);
        }

        for chunk in samples.chunks_exact_mut(2) {
            let mut left = chunk[0] as f32;
            let mut right = chunk[1] as f32;

            for (i, band) in self.bands.iter_mut().enumerate() {
                if self.gains[i].abs() > f64::EPSILON {
                    left = band.process_left(left);
                    right = band.process_right(right);
                }
            }

            chunk[0] = left.clamp(i16::MIN as f32, i16::MAX as f32) as i16;
            chunk[1] = right.clamp(i16::MIN as f32, i16::MAX as f32) as i16;
        }

        Ok(())
    }

    fn is_active(&self) -> bool {
        self.gains.iter().any(|&g| g.abs() > f64::EPSILON)
    }

    fn name(&self) -> &'static str {
        "Equalizer"
    }

    fn reset(&mut self) {
        for band in &mut self.bands {
            band.reset();
        }
    }
}

struct BiquadFilter {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,

    lx1: f32,
    lx2: f32,
    ly1: f32,
    ly2: f32,

    rx1: f32,
    rx2: f32,
    ry1: f32,
    ry2: f32,
}

impl BiquadFilter {
    fn peaking_eq(sample_rate: f64, frequency: f64, q: f64, gain_db: f64) -> Self {
        let a = 10_f64.powf(gain_db / 40.0);
        let omega = 2.0 * PI * frequency / sample_rate;
        let sin_omega = omega.sin();
        let cos_omega = omega.cos();
        let alpha = sin_omega / (2.0 * q);

        let b0 = 1.0 + alpha * a;
        let b1 = -2.0 * cos_omega;
        let b2 = 1.0 - alpha * a;
        let a0 = 1.0 + alpha / a;
        let a1 = -2.0 * cos_omega;
        let a2 = 1.0 - alpha / a;

        Self {
            b0: (b0 / a0) as f32,
            b1: (b1 / a0) as f32,
            b2: (b2 / a0) as f32,
            a1: (a1 / a0) as f32,
            a2: (a2 / a0) as f32,
            lx1: 0.0,
            lx2: 0.0,
            ly1: 0.0,
            ly2: 0.0,
            rx1: 0.0,
            rx2: 0.0,
            ry1: 0.0,
            ry2: 0.0,
        }
    }

    fn process_left(&mut self, input: f32) -> f32 {
        let output =
            self.b0 * input + self.b1 * self.lx1 + self.b2 * self.lx2
                - self.a1 * self.ly1
                - self.a2 * self.ly2;

        self.lx2 = self.lx1;
        self.lx1 = input;
        self.ly2 = self.ly1;
        self.ly1 = output;

        output
    }

    fn process_right(&mut self, input: f32) -> f32 {
        let output =
            self.b0 * input + self.b1 * self.rx1 + self.b2 * self.rx2
                - self.a1 * self.ry1
                - self.a2 * self.ry2;

        self.rx2 = self.rx1;
        self.rx1 = input;
        self.ry2 = self.ry1;
        self.ry1 = output;

        output
    }

    fn reset(&mut self) {
        self.lx1 = 0.0;
        self.lx2 = 0.0;
        self.ly1 = 0.0;
        self.ly2 = 0.0;
        self.rx1 = 0.0;
        self.rx2 = 0.0;
        self.ry1 = 0.0;
        self.ry2 = 0.0;
    }
}
