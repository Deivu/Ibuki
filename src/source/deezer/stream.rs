use super::SECRET_IV;
use async_trait::async_trait;
use blowfish::Blowfish;
use cbc::cipher::{KeyIvInit, block_padding::NoPadding};
use cbc::{Decryptor, cipher::BlockDecryptMut};
use songbird::input::{AudioStream, AudioStreamError, Compose, HttpRequest};
use std::cmp::min;
use std::io::{Error as IoError, ErrorKind, Read, Result as IoResult, Seek, SeekFrom};
use symphonia::core::io::MediaSource;
use tokio::task::block_in_place;

pub struct DeezerHttpStream {
    request: HttpRequest,
    key: [u8; 16],
}

#[async_trait]
impl Compose for DeezerHttpStream {
    fn create(&mut self) -> Result<AudioStream<Box<dyn MediaSource>>, AudioStreamError> {
        self.request.create()
    }

    async fn create_async(
        &mut self,
    ) -> Result<AudioStream<Box<dyn MediaSource>>, AudioStreamError> {
        let request = self.request.create_async().await?;
        let hint = request.hint;

        Ok(AudioStream {
            input: Box::new(DeezerMediaSource::new(request.input, self.key))
                as Box<dyn MediaSource>,
            hint,
        })
    }

    fn should_create_async(&self) -> bool {
        self.request.should_create_async()
    }
}

/**
 * Deezer don't support the global seeker due to custom deserializing, hence it needs to be implemented manually
 * PS I could probably have the universal seek support this. Probably I'll do it at some point
 */
pub struct DeezerMediaSource {
    source: Box<dyn MediaSource>,
    key: [u8; 16],
    buffer: [u8; 2048],
    position: usize,
    downloaded: Vec<u8>,
    downloaded_bytes: usize,
    total_bytes: Option<usize>,
}

impl Read for DeezerMediaSource {
    fn read(&mut self, buf: &mut [u8]) -> IoResult<usize> {
        if self.position < self.downloaded_bytes {
            let mut read_up_to = self.position + buf.len();

            if read_up_to > self.downloaded_bytes {
                read_up_to = self.downloaded_bytes;
            }

            let bytes = &self.downloaded[self.position..read_up_to];
            let bytes_read = bytes.len();

            buf[0..bytes_read].copy_from_slice(bytes);

            self.position += bytes_read;

            return Ok(bytes_read);
        }

        let mut total_read = 0;
        let chunk_size = self.buffer.len();

        while total_read < chunk_size {
            let bytes_read = block_in_place(|| self.source.read(&mut self.buffer[total_read..]))?;

            if bytes_read == 0 {
                break;
            }

            total_read += bytes_read;
        }

        let current_chunk = (self.downloaded_bytes as f64 / chunk_size as f64).ceil() as usize;

        if current_chunk % 3 > 0 || total_read < chunk_size {
            self.downloaded.extend(self.buffer[..total_read].iter());
        } else {
            let decryptor: Decryptor<Blowfish> = Decryptor::new_from_slices(&self.key, &SECRET_IV)
                .map_err(|error| IoError::new(ErrorKind::Unsupported, error))?;

            let decrypted = decryptor
                .decrypt_padded_mut::<NoPadding>(&mut self.buffer[..total_read])
                .map_err(|error| IoError::new(ErrorKind::InvalidInput, error.to_string()))?;

            self.downloaded.extend(decrypted[..total_read].iter());
        }

        self.downloaded_bytes += total_read;

        let mut read_up_to = self.position + min(buf.len(), self.downloaded_bytes);

        if read_up_to > self.downloaded_bytes {
            read_up_to = self.downloaded_bytes;
        }

        let bytes = &self.downloaded[self.position..read_up_to];
        let bytes_read = bytes.len();

        buf[0..bytes_read].copy_from_slice(bytes);

        self.position += bytes_read;

        Ok(bytes_read)
    }
}

impl Seek for DeezerMediaSource {
    fn seek(&mut self, position: SeekFrom) -> IoResult<u64> {
        let new_position = match position {
            SeekFrom::Start(n) => n as usize,
            SeekFrom::Current(offset) => {
                let pos = self.position as i64 + offset;

                if pos < 0 {
                    return Err(IoError::new(ErrorKind::InvalidInput, "Negative seek"));
                }

                pos as usize
            }
            SeekFrom::End(offset) => {
                let length = self
                    .total_bytes
                    .ok_or_else(|| IoError::new(ErrorKind::Unsupported, "Length unknown"))?;

                let pos = length as i64 + offset;

                if pos < 0 {
                    return Err(IoError::new(ErrorKind::InvalidInput, "Negative seek"));
                }

                pos as usize
            }
        };

        self.position = new_position.min(self.total_bytes.unwrap_or(usize::MAX));

        Ok(self.position as u64)
    }
}

impl MediaSource for DeezerMediaSource {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        self.source.byte_len()
    }
}

impl DeezerMediaSource {
    pub fn new(source: Box<dyn MediaSource>, key: [u8; 16]) -> Self {
        let total_bytes = block_in_place(|| source.byte_len().map(|size| size as usize));

        let downloaded = if let Some(bytes) = total_bytes {
            Vec::with_capacity(bytes)
        } else {
            // rust will reallocate if needed
            Vec::with_capacity(5e+6 as usize)
        };

        Self {
            source,
            key,
            buffer: [0; 2048],
            downloaded,
            position: 0,
            downloaded_bytes: 0,
            total_bytes,
        }
    }
}

impl DeezerHttpStream {
    pub fn new(request: HttpRequest, key: [u8; 16]) -> Self {
        Self { request, key }
    }
}
