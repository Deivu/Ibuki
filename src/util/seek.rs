use songbird::input::AudioStream;
use std::{
    cmp::min,
    io::{Error as IoError, ErrorKind, Read, Result as IoResult, Seek, SeekFrom},
};
use symphonia::core::{io::MediaSource, probe::Hint};
use tokio::task::block_in_place;

static CHUNK_SIZE: usize = 128;

pub struct SeekableSource {
    source: Box<dyn MediaSource>,
    position: usize,
    downloaded: Vec<u8>,
    downloaded_bytes: usize,
    total_bytes: Option<usize>,
}

impl Read for SeekableSource {
    fn read(&mut self, buf: &mut [u8]) -> IoResult<usize> {
        if !self.is_seekable() {
            let bytes_read = block_in_place(|| self.source.read(buf))?;

            self.position += bytes_read;
            self.downloaded_bytes += bytes_read;

            return Ok(bytes_read);
        };

        if self.position < self.downloaded_bytes {
            let mut read_up_to = self.position + min(buf.len(), CHUNK_SIZE);

            if read_up_to > self.downloaded_bytes {
                read_up_to = self.downloaded_bytes;
            }

            let bytes = &self.downloaded[self.position..read_up_to];
            let bytes_read = bytes.len();

            buf[0..bytes_read].copy_from_slice(bytes);

            self.position += bytes_read;

            return Ok(bytes_read);
        }

        let bytes_read = block_in_place(|| self.source.read(buf))?;

        if bytes_read == 0 {
            return Ok(bytes_read);
        }

        self.position += bytes_read;
        self.downloaded_bytes += bytes_read;

        self.downloaded.extend(buf[0..bytes_read].iter());

        Ok(bytes_read)
    }
}

impl Seek for SeekableSource {
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

impl MediaSource for SeekableSource {
    fn is_seekable(&self) -> bool {
        self.total_bytes.is_some()
    }

    fn byte_len(&self) -> Option<u64> {
        self.total_bytes.map(|len| len as u64)
    }
}

impl SeekableSource {
    pub fn new(source: Box<dyn MediaSource>) -> Self {
        let total_bytes = block_in_place(|| source.byte_len().map(|size| size as usize));

        let downloaded = if let Some(bytes) = total_bytes {
            Vec::with_capacity(bytes)
        } else {
            // rust will reallocate if needed
            Vec::with_capacity(5e+6 as usize)
        };

        Self {
            source,
            position: 0,
            downloaded,
            downloaded_bytes: 0,
            total_bytes,
        }
    }

    pub fn into_audio_stream(self, hint: Option<Hint>) -> AudioStream<Box<dyn MediaSource>> {
        AudioStream {
            input: Box::new(self) as Box<dyn MediaSource>,
            hint,
        }
    }
}
