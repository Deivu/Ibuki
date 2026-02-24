use super::channel_mix::ChannelMixFilter;
use super::distortion::DistortionFilter;
use super::equalizer::EqualizerFilter;
use super::karaoke::KaraokeFilter;
use super::low_pass::LowPassFilter;
use super::rotation::RotationFilter;
use super::timescale::TimescaleFilter;
use super::tremolo::TremoloFilter;
use super::vibrato::VibratoFilter;
use super::volume::VolumeFilter;
use super::{AudioFilter, FilterError};
use crate::models::LavalinkFilters;

pub struct FilterChain {
    volume: Option<VolumeFilter>,
    equalizer: Option<EqualizerFilter>,
    timescale: Option<TimescaleFilter>,
    tremolo: Option<TremoloFilter>,
    vibrato: Option<VibratoFilter>,
    rotation: Option<RotationFilter>,
    distortion: Option<DistortionFilter>,
    karaoke: Option<KaraokeFilter>,
    channel_mix: Option<ChannelMixFilter>,
    low_pass: Option<LowPassFilter>,

    sample_rate: u32,
    enabled: bool,
}

impl FilterChain {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            volume: None,
            equalizer: None,
            timescale: None,
            tremolo: None,
            vibrato: None,
            rotation: None,
            distortion: None,
            karaoke: None,
            channel_mix: None,
            low_pass: None,
            sample_rate,
            enabled: true,
        }
    }

    pub fn update_from_config(&mut self, config: &LavalinkFilters) -> Result<(), FilterError> {
        self.volume = match config.volume {
            Some(vol) => Some(VolumeFilter::new(vol)?),
            None => None,
        };

        self.equalizer = match &config.equalizer {
            Some(bands) if !bands.is_empty() => {
                Some(EqualizerFilter::from_bands(bands, self.sample_rate)?)
            }
            _ => None,
        };

        self.timescale = match &config.timescale {
            Some(ts) => Some(TimescaleFilter::new(
                ts.speed.unwrap_or(1.0),
                ts.pitch.unwrap_or(1.0),
                ts.rate.unwrap_or(1.0),
            )?),
            None => None,
        };

        self.tremolo = match &config.tremolo {
            Some(t) => Some(TremoloFilter::new(
                t.frequency.unwrap_or(2.0),
                t.depth.unwrap_or(0.5),
            )?),
            None => None,
        };

        self.vibrato = match &config.vibrato {
            Some(v) => Some(VibratoFilter::new(
                v.frequency.unwrap_or(2.0),
                v.depth.unwrap_or(0.5),
            )?),
            None => None,
        };

        self.rotation = match &config.rotation {
            Some(r) => Some(RotationFilter::new(r.rotation_hz.unwrap_or(0.0))?),
            None => None,
        };

        self.distortion = match &config.distortion {
            Some(d) => Some(DistortionFilter::new(
                d.sin_offset.unwrap_or(0.0),
                d.sin_scale.unwrap_or(1.0),
                d.cos_offset.unwrap_or(0.0),
                d.cos_scale.unwrap_or(1.0),
                d.tan_offset.unwrap_or(0.0),
                d.tan_scale.unwrap_or(1.0),
                d.offset.unwrap_or(0.0),
                d.scale.unwrap_or(1.0),
            )?),
            None => None,
        };

        self.karaoke = match &config.karaoke {
            Some(k) => Some(KaraokeFilter::new(
                k.level.unwrap_or(1.0),
                k.mono_level.unwrap_or(1.0),
                k.filter_band.unwrap_or(220.0),
                k.filter_width.unwrap_or(100.0),
            )?),
            None => None,
        };

        self.channel_mix = match &config.channel_mix {
            Some(cm) => Some(ChannelMixFilter::new(
                cm.left_to_left.unwrap_or(1.0),
                cm.left_to_right.unwrap_or(0.0),
                cm.right_to_left.unwrap_or(0.0),
                cm.right_to_right.unwrap_or(1.0),
            )?),
            None => None,
        };

        self.low_pass = match &config.low_pass {
            Some(lp) => Some(LowPassFilter::new(lp.smoothing.unwrap_or(1.0))?),
            None => None,
        };

        Ok(())
    }

    pub fn process(&mut self, samples: &mut [i16]) -> Result<(), FilterError> {
        if !self.enabled || samples.is_empty() {
            return Ok(());
        }

        let sr = self.sample_rate;

        macro_rules! apply {
            ($filter:expr) => {
                if let Some(f) = &mut $filter {
                    if f.is_active() {
                        f.process(samples, sr)?;
                    }
                }
            };
        }

        apply!(self.volume);
        apply!(self.equalizer);
        apply!(self.timescale);
        apply!(self.tremolo);
        apply!(self.vibrato);
        apply!(self.rotation);
        apply!(self.distortion);
        apply!(self.karaoke);
        apply!(self.channel_mix);
        apply!(self.low_pass);

        Ok(())
    }

    pub fn has_active_filters(&self) -> bool {
        macro_rules! check {
            ($filter:expr) => {
                if let Some(f) = &$filter {
                    if f.is_active() {
                        return true;
                    }
                }
            };
        }
        check!(self.volume);
        check!(self.equalizer);
        check!(self.timescale);
        check!(self.tremolo);
        check!(self.vibrato);
        check!(self.rotation);
        check!(self.distortion);
        check!(self.karaoke);
        check!(self.channel_mix);
        check!(self.low_pass);
        false
    }

    pub fn clear(&mut self) {
        self.volume = None;
        self.equalizer = None;
        self.timescale = None;
        self.tremolo = None;
        self.vibrato = None;
        self.rotation = None;
        self.distortion = None;
        self.karaoke = None;
        self.channel_mix = None;
        self.low_pass = None;
    }

    pub fn reset_state(&mut self) {
        macro_rules! reset {
            ($filter:expr) => {
                if let Some(f) = &mut $filter {
                    f.reset();
                }
            };
        }
        reset!(self.volume);
        reset!(self.equalizer);
        reset!(self.timescale);
        reset!(self.tremolo);
        reset!(self.vibrato);
        reset!(self.rotation);
        reset!(self.distortion);
        reset!(self.karaoke);
        reset!(self.channel_mix);
        reset!(self.low_pass);
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn set_sample_rate(&mut self, sample_rate: u32) {
        self.sample_rate = sample_rate;
    }
}
