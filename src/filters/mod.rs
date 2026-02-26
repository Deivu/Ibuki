use thiserror::Error;

pub mod channel_mix;
pub mod distortion;
pub mod equalizer;
pub mod karaoke;
pub mod low_pass;
pub mod processor;
pub mod rotation;
pub mod source;
pub mod timescale;
pub mod tremolo;
pub mod vibrato;
pub mod volume;

#[derive(Error, Clone, Debug)]
pub enum FilterError {
    #[error("Invalid parameter: {0}")]
    InvalidParameter(String),

    #[error("Processing error: {0}")]
    ProcessingError(String),

    #[error("Buffer size mismatch: expected even number of samples for stereo")]
    BufferSizeMismatch,
}

pub trait AudioFilter: Send + Sync {
    fn process(&mut self, samples: &mut [i16], sample_rate: u32) -> Result<(), FilterError>;
    fn is_active(&self) -> bool;
    fn name(&self) -> &'static str;
    fn reset(&mut self);
}
