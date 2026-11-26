use abi_stable::RRef;
use abi_stable::std_types::{RBox, RErr, ROk, RSlice, RSliceMut};
use impero_filter::filter::AudioFilter_TO;
use impero_filter::object::FilterValue;
use songbird::constants::SAMPLE_RATE;
use songbird::driver::{AudioFilter, FilterError};
use std::any::Any;

pub struct PluginFilter(pub AudioFilter_TO<'static, RBox<()>>);

impl AudioFilter for PluginFilter {
    fn configure(&mut self, config: Box<dyn Any + Send>) -> Result<(), FilterError> {
        let config = config
            .downcast::<FilterValue>()
            .map_err(|_| "Filter config is not a FilterValue")?;

        match self.0.configure(RRef::new(&*config)) {
            ROk(()) => Ok(()),
            RErr(error) => Err(error.into()),
        }
    }

    fn process(
        &mut self,
        input: &[f32],
        output: &mut [f32],
        channels: usize,
        last: bool,
    ) -> Result<usize, FilterError> {
        let input_frames = (input.len() / channels) as u32;
        let output_frames = (output.len() / channels) as u32;
        let result = self.0.process(
            RSlice::from_slice(input),
            input_frames,
            RSliceMut::from_mut_slice(output),
            output_frames,
            SAMPLE_RATE,
            channels as u32,
            last,
        );

        match result {
            ROk(frames) => Ok(frames as usize),
            RErr(error) => Err(error.into()),
        }
    }

    fn reset(&mut self) {
        self.0.reset();
    }

    fn is_idle(&self) -> bool {
        self.0.is_idle()
    }
}
