use async_trait::async_trait;
use songbird::input::{AudioStream, AudioStreamError, Compose};
use crate::filters::processor::FilterChain;
use std::io::{self, Read, Seek, SeekFrom};
use std::sync::{Arc, Mutex};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::formats::{FormatOptions, FormatReader};
use symphonia::core::io::{MediaSource, MediaSourceStream};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use songbird::input::codecs::{get_codec_registry, get_probe};

const WAV_HEADER_SIZE: usize = 44;

pub struct FilteredSource {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn symphonia::core::codecs::Decoder>,
    track_id: u32,
    filter_chain: Arc<Mutex<FilterChain>>,

    pcm_buffer: Vec<u8>,
    pcm_pos: usize,
    current_pcm_frame: u64,

    header: [u8; WAV_HEADER_SIZE],
    header_pos: usize,
    header_sent: bool,

    _sample_rate: u32,
    channels: usize,

    seekable: bool,
}

impl FilteredSource {
    pub fn new(
        source: Box<dyn MediaSource>,
        hint: Hint,
        filter_chain: Arc<Mutex<FilterChain>>,
        sample_rate: u32,
        channels: usize,
    ) -> Result<Self, io::Error> {
        tracing::info!("FilteredSource::new ENTRY: sample_rate={}, channels={}, hint={:?}", sample_rate, channels, hint);

        let seekable = source.is_seekable();
        tracing::debug!("Creating MediaSourceStream, seekable={}", seekable);
        let mss = MediaSourceStream::new(source, Default::default());
        tracing::debug!("MediaSourceStream created, starting probe (with Opus support)...");

        let probed = get_probe()
            .format(
                &hint,
                mss,
                &FormatOptions::default(),
                &MetadataOptions::default(),
            )
            .map_err(|e| {
                tracing::error!("Probe FAILED: {e}");
                io::Error::new(io::ErrorKind::Other, format!("Probe failed: {e}"))
            })?;

        tracing::info!("Probe SUCCESS");
        let format = probed.format;

        let track = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::Other, "No supported audio track found")
            })?;

        let track_id = track.id;

        let actual_sr = track.codec_params.sample_rate.unwrap_or(sample_rate);
        let actual_ch = track
            .codec_params
            .channels
            .map(|c| c.count())
            .unwrap_or(channels);

        tracing::debug!("Creating decoder for track_id={}, codec={:?}", track_id, track.codec_params.codec);
        let decoder = get_codec_registry()
            .make(&track.codec_params, &DecoderOptions::default())
            .map_err(|e| {
                tracing::error!("Decoder creation failed: {e}");
                io::Error::new(io::ErrorKind::Other, format!("Decoder creation failed: {e}"))
            })?;

        tracing::info!(
            "Decoder created successfully. Actual sample_rate={}, channels={}",
            actual_sr,
            actual_ch
        );

        if let Ok(mut chain) = filter_chain.lock() {
            chain.set_sample_rate(actual_sr);
        }

        let header = build_wav_header(actual_sr, actual_ch as u16);

        tracing::info!(
            "FilteredSource created successfully: {}Hz, {} channels, seekable={}",
            actual_sr,
            actual_ch,
            seekable
        );

        Ok(Self {
            format,
            decoder,
            track_id,
            filter_chain,
            pcm_buffer: Vec::with_capacity(8192),
            pcm_pos: 0,
            current_pcm_frame: 0,
            header,
            header_pos: 0,
            header_sent: false,
            _sample_rate: actual_sr,
            channels: actual_ch,
            seekable,
        })
    }
}


impl Read for FilteredSource {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if !self.header_sent {
            let avail = WAV_HEADER_SIZE - self.header_pos;
            if avail > 0 {
                let n = buf.len().min(avail);
                buf[..n]
                    .copy_from_slice(&self.header[self.header_pos..self.header_pos + n]);
                self.header_pos += n;
                if self.header_pos >= WAV_HEADER_SIZE {
                    self.header_sent = true;
                }
                return Ok(n);
            }
            self.header_sent = true;
        }

        loop {
            if self.pcm_pos < self.pcm_buffer.len() {
                let avail = self.pcm_buffer.len() - self.pcm_pos;
                let n = buf.len().min(avail);
                buf[..n]
                    .copy_from_slice(&self.pcm_buffer[self.pcm_pos..self.pcm_pos + n]);
                self.pcm_pos += n;
                return Ok(n);
            }

            self.pcm_buffer.clear();
            self.pcm_pos = 0;

            let packet = match self.format.next_packet() {
                Ok(p) => p,
                Err(symphonia::core::errors::Error::IoError(ref e))
                    if e.kind() == io::ErrorKind::UnexpectedEof =>
                {
                    return Ok(0);
                }
                Err(symphonia::core::errors::Error::ResetRequired) => {
                    self.decoder.reset();
                    continue;
                }
                Err(e) => {
                    return Err(io::Error::new(
                        io::ErrorKind::Other,
                        format!("Format read error: {e}"),
                    ));
                }
            };


            if packet.track_id() != self.track_id {
                continue;
            }

            let decoded = match self.decoder.decode(&packet) {
                Ok(d) => d,
                Err(symphonia::core::errors::Error::DecodeError(_)) => continue,
                Err(e) => {
                    return Err(io::Error::new(
                        io::ErrorKind::Other,
                        format!("Decode error: {e}"),
                    ));
                }
            };


            let spec = *decoded.spec();
            let frames = decoded.frames();
            if frames == 0 {
                continue;
            }
            self.current_pcm_frame += frames as u64;

            let mut sample_buf = SampleBuffer::<i16>::new(frames as u64, spec);
            sample_buf.copy_interleaved_ref(decoded);
            let mut samples: Vec<i16> = sample_buf.samples().to_vec();

            if let Ok(mut chain) = self.filter_chain.lock() {
                let _ = chain.process(&mut samples);
            }

            self.pcm_buffer.reserve(samples.len() * 2);
            for &s in &samples {
                self.pcm_buffer.extend_from_slice(&s.to_le_bytes());
            }
        }
    }
}

impl Seek for FilteredSource {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        match pos {
            SeekFrom::Start(byte_pos) => {
                if byte_pos < WAV_HEADER_SIZE as u64 {
                    if !self.seekable {
                        return Err(io::Error::new(
                            io::ErrorKind::Unsupported,
                            "Source is not seekable backwards to WAV header",
                        ));
                    }

                    if let Err(e) = self.format.seek(
                        symphonia::core::formats::SeekMode::Accurate,
                        symphonia::core::formats::SeekTo::TimeStamp {
                            ts: 0,
                            track_id: self.track_id,
                        },
                    ) {
                        return Err(io::Error::new(io::ErrorKind::InvalidInput, e.to_string()));
                    }

                    self.header_sent = false;
                    self.header_pos = byte_pos as usize;
                    self.pcm_buffer.clear();
                    self.pcm_pos = 0;
                    self.decoder.reset();
                    if let Ok(mut chain) = self.filter_chain.lock() {
                        chain.reset_state();
                    }
                    self.current_pcm_frame = 0;

                    return Ok(byte_pos);
                }

                let pcm_bytes = byte_pos - WAV_HEADER_SIZE as u64;
                let bytes_per_frame = 2 * self.channels as u64;
                let frame_offset = pcm_bytes / bytes_per_frame;

                if !self.seekable && frame_offset < self.current_pcm_frame {
                    return Err(io::Error::new(
                        io::ErrorKind::Unsupported,
                        "Source is not seekable backwards",
                    ));
                }

                match self.format.seek(
                    symphonia::core::formats::SeekMode::Accurate,
                    symphonia::core::formats::SeekTo::TimeStamp {
                        ts: frame_offset,
                        track_id: self.track_id,
                    },
                ) {
                    Ok(_) => {
                        self.decoder.reset();
                        self.pcm_buffer.clear();
                        self.pcm_pos = 0;
                        self.header_sent = true;
                        self.current_pcm_frame = frame_offset;
                        if let Ok(mut chain) = self.filter_chain.lock() {
                            chain.reset_state();
                        }
                        Ok(byte_pos)
                    }
                    Err(e) => {
                        tracing::debug!("Inner format seek failed ({}). Resolving via manual packet discard to frame {}", e, frame_offset);

                        if self.format.seek(
                            symphonia::core::formats::SeekMode::Accurate,
                            symphonia::core::formats::SeekTo::TimeStamp {
                                ts: 0,
                                track_id: self.track_id,
                            },
                        ).is_ok() {
                            self.current_pcm_frame = 0;
                        }
                        
                        self.decoder.reset();
                        self.pcm_buffer.clear();
                        self.pcm_pos = 0;
                        self.header_sent = true;
                        
                        loop {
                            let packet = match self.format.next_packet() {
                                Ok(p) => p,
                                Err(symphonia::core::errors::Error::ResetRequired) => {
                                    self.decoder.reset();
                                    continue;
                                },
                                Err(_) => break, // EOF or fatal read error, stop skipping
                            };
                            
                            if packet.track_id() != self.track_id {
                                continue;
                            }
                            
                            let decoded = match self.decoder.decode(&packet) {
                                Ok(d) => d,
                                Err(symphonia::core::errors::Error::DecodeError(_)) => continue,
                                Err(_) => continue,
                            };
                            
                            self.current_pcm_frame += decoded.frames() as u64;
                            if self.current_pcm_frame >= frame_offset {
                                break;
                            }
                        }
                        
                        if let Ok(mut chain) = self.filter_chain.lock() {
                            chain.reset_state();
                        }
                        Ok(byte_pos)
                    }
                }
            }
            _ => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Only SeekFrom::Start is supported",
            )),
        }
    }
}

impl MediaSource for FilteredSource {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        None
    }
}

fn build_wav_header(sample_rate: u32, num_channels: u16) -> [u8; WAV_HEADER_SIZE] {
    let bits_per_sample: u16 = 16;
    let block_align = num_channels * (bits_per_sample / 8);
    let byte_rate = sample_rate * block_align as u32;
    let data_size: u32 = 0x7FFF_FF00;

    let mut h = [0u8; WAV_HEADER_SIZE];
    h[0..4].copy_from_slice(b"RIFF");
    h[4..8].copy_from_slice(&(36 + data_size).to_le_bytes());
    h[8..12].copy_from_slice(b"WAVE");
    h[12..16].copy_from_slice(b"fmt ");
    h[16..20].copy_from_slice(&16u32.to_le_bytes());
    h[20..22].copy_from_slice(&1u16.to_le_bytes());
    h[22..24].copy_from_slice(&num_channels.to_le_bytes());
    h[24..28].copy_from_slice(&sample_rate.to_le_bytes());
    h[28..32].copy_from_slice(&byte_rate.to_le_bytes());
    h[32..34].copy_from_slice(&block_align.to_le_bytes());
    h[34..36].copy_from_slice(&bits_per_sample.to_le_bytes());
    h[36..40].copy_from_slice(b"data");
    h[40..44].copy_from_slice(&data_size.to_le_bytes());
    h
}

pub struct FilteredCompose {
    inner: Box<dyn Compose>,
    filter_chain: Arc<Mutex<FilterChain>>,
    sample_rate: u32,
    channels: usize,
}

impl FilteredCompose {
    pub fn new(
        inner: Box<dyn Compose>,
        filter_chain: Arc<Mutex<FilterChain>>,
        sample_rate: u32,
        channels: usize,
    ) -> Self {
        Self {
            inner,
            filter_chain,
            sample_rate,
            channels,
        }
    }

    fn build_filtered_blocking(
        stream: AudioStream<Box<dyn MediaSource>>,
        filter_chain: Arc<Mutex<FilterChain>>,
        sample_rate: u32,
        channels: usize,
    ) -> Result<AudioStream<Box<dyn MediaSource>>, AudioStreamError> {
        tracing::debug!("FilteredCompose::build_filtered_blocking called, hint={:?}", stream.hint);

        let hint = stream.hint.unwrap_or_default();
        let filtered = FilteredSource::new(
            stream.input,
            hint,
            filter_chain,
            sample_rate,
            channels,
        )
        .map_err(|e| {
            let err_msg = format!("{e}");
            if err_msg.contains("unsupported codec") {
                tracing::warn!(
                    "Unsupported codec detected (likely Opus/WebM). Filters cannot be applied. \
                     Consider using JioSaavn or other AAC/MP3 sources for filter support."
                );
            } else {
                tracing::error!("FilteredSource::new failed in build_filtered_blocking: {e}");
            }
            AudioStreamError::Fail(Box::new(e))
        })?;

        tracing::debug!("FilteredCompose::build_filtered_blocking succeeded, returning WAV stream");

        Ok(AudioStream {
            input: Box::new(filtered) as Box<dyn MediaSource>,
            hint: Some({
                let mut h = Hint::new();
                h.with_extension("wav");
                h
            }),
        })
    }
}

#[async_trait]
impl Compose for FilteredCompose {
    fn create(&mut self) -> Result<AudioStream<Box<dyn MediaSource>>, AudioStreamError> {
        tracing::debug!("FilteredCompose::create (sync) called");
        let stream = self.inner.create()?;
        Self::build_filtered_blocking(
            stream,
            self.filter_chain.clone(),
            self.sample_rate,
            self.channels,
        )
    }

    async fn create_async(
        &mut self,
    ) -> Result<AudioStream<Box<dyn MediaSource>>, AudioStreamError> {
        tracing::debug!("FilteredCompose::create_async called, resolving inner compose...");
        let stream = self.inner.create_async().await.map_err(|e| {
            tracing::error!("Inner compose create_async failed: {:?}", e);
            e
        })?;
        tracing::debug!(
            "Inner compose resolved successfully, spawning blocking task for symphonia probe..."
        );
        
        let filter_chain = self.filter_chain.clone();
        let sample_rate = self.sample_rate;
        let channels = self.channels;
        
        tokio::task::spawn_blocking(move || {
            Self::build_filtered_blocking(stream, filter_chain, sample_rate, channels)
        })
        .await
        .map_err(|e| {
            AudioStreamError::Fail(Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Blocking task failed: {e}"),
            )))
        })?
    }

    fn should_create_async(&self) -> bool {
        self.inner.should_create_async()
    }
}
