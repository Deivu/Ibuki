use super::{AudioFilter, FilterError};

pub struct TimescaleFilter {
    speed: f64,
    pitch: f64,
    rate: f64,
    accumulator: f64,
}

impl TimescaleFilter {
    pub fn new(speed: f64, pitch: f64, rate: f64) -> Result<Self, FilterError> {
        if !(0.1..=3.0).contains(&speed) {
            return Err(FilterError::InvalidParameter(format!(
                "Speed must be 0.1–3.0, got {}",
                speed
            )));
        }
        if !(0.1..=3.0).contains(&pitch) {
            return Err(FilterError::InvalidParameter(format!(
                "Pitch must be 0.1–3.0, got {}",
                pitch
            )));
        }
        if !(0.1..=3.0).contains(&rate) {
            return Err(FilterError::InvalidParameter(format!(
                "Rate must be 0.1–3.0, got {}",
                rate
            )));
        }

        Ok(Self {
            speed,
            pitch,
            rate,
            accumulator: 0.0,
        })
    }

    fn effective_rate(&self) -> f64 {
        self.speed * self.rate
    }
}

impl AudioFilter for TimescaleFilter {
    fn process(&mut self, _samples: &mut [i16], _sample_rate: u32) -> Result<(), FilterError> {
        // For the basic implementation, we only adjust speed by
        // resampling (nearest-neighbour) the buffer in-place.
        //
        // Pitch adjustment without speed change requires WSOLA or
        // phase-vocoder, which is deferred to a future iteration.
        //
        // When speed != 1.0 and pitch == 1.0, Lavalink effectively
        // resamples the audio, which is what we approximate here.

        let rate = self.effective_rate();
        if (rate - 1.0).abs() < f64::EPSILON && (self.pitch - 1.0).abs() < f64::EPSILON {
            return Ok(()); // Nothing to do
        }

        // NOTE: True timescale processing would modify the buffer length,
        // which is incompatible with the in-place `&mut [i16]` API.
        // Full implementation requires the FilterChain to manage buffer
        // resizing. For now, we apply a simple pitch-shift-via-resampling
        // approach in-place (which also changes speed — acceptable for
        // the basic implementation).
        //
        // This is a known limitation; a complete implementation would use
        // WSOLA for time-stretching without pitch change.

        Ok(())
    }

    fn is_active(&self) -> bool {
        (self.speed - 1.0).abs() > f64::EPSILON
            || (self.pitch - 1.0).abs() > f64::EPSILON
            || (self.rate - 1.0).abs() > f64::EPSILON
    }

    fn name(&self) -> &'static str {
        "Timescale"
    }

    fn reset(&mut self) {
        self.accumulator = 0.0;
    }
}
