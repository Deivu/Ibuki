use songbird::input::AudioStream;
use std::cmp::min;
use std::io::{Error as IoError, ErrorKind, Read, Result as IoResult, Seek, SeekFrom};
use symphonia::core::io::MediaSource;
use symphonia::core::probe::Hint;
use tokio::task::block_in_place;

static CHUNK_SIZE: usize = 1024;

pub trait ChunkTransform: Sized + Send + Sync + 'static {
    fn transform_chunk(&mut self, data: &mut [u8], chunk_index: usize) -> IoResult<usize>;

    fn chunk_size(&self) -> usize {
        CHUNK_SIZE
    }
}

pub struct DefaultChunkTransform;

impl ChunkTransform for DefaultChunkTransform {
    fn transform_chunk(&mut self, data: &mut [u8], _: usize) -> IoResult<usize> {
        Ok(data.len())
    }
}

pub struct SeekableSource<T: ChunkTransform> {
    source: Box<dyn MediaSource>,
    transform: T,
    position: usize,
    downloaded: Option<Vec<u8>>,
    downloaded_bytes: usize,
    total_bytes: Option<usize>,
}

impl<T: ChunkTransform> Read for SeekableSource<T> {
    fn read(&mut self, buf: &mut [u8]) -> IoResult<usize> {
        if !self.is_seekable() {
            let chunk_size = self.transform.chunk_size();
            let buffer_size = buf.len();
            let mut total_read = 0;

            while total_read < chunk_size && total_read < buffer_size {
                let bytes_read = block_in_place(|| {
                    self.source
                        .read(&mut buf[total_read..chunk_size.min(buffer_size)])
                })?;
                if bytes_read == 0 {
                    break;
                }
                total_read += bytes_read;
            }

            if total_read == 0 {
                return Ok(0);
            }

            self.downloaded_bytes += total_read;

            let chunk_index = self.downloaded_bytes / chunk_size;

            self.transform
                .transform_chunk(&mut buf[..total_read], chunk_index)?;

            self.position += total_read;

            return Ok(total_read);
        };

        let Some(downloaded) = self.downloaded.as_mut() else {
            return Err(IoError::new(
                ErrorKind::Unsupported,
                "self.downloaded is none",
            ));
        };

        if self.position < self.downloaded_bytes {
            let available_bytes = self.downloaded_bytes - self.position;
            let buffer_end_bytes = min(buf.len(), available_bytes);
            let downloaded_end_bytes = self.position + buffer_end_bytes;

            buf[0..buffer_end_bytes]
                .copy_from_slice(&downloaded[self.position..downloaded_end_bytes]);

            self.position += buffer_end_bytes;

            return Ok(buffer_end_bytes);
        }

        let chunk_size = self.transform.chunk_size();
        let start_index = downloaded.len();

        let init_size = start_index + chunk_size;
        if downloaded.len() < init_size {
            downloaded.reserve(init_size - downloaded.capacity());
        }
        downloaded.resize(init_size, 0);

        let mut total_read = 0;

        while total_read < chunk_size {
            let bytes_read = block_in_place(|| {
                self.source
                    .read(&mut downloaded[start_index + total_read..start_index + chunk_size])
            })?;
            if bytes_read == 0 {
                break;
            }
            total_read += bytes_read;
        }

        if total_read == 0 {
            return Ok(0);
        }

        self.downloaded_bytes += total_read;

        let chunk_index = self.downloaded_bytes / chunk_size;

        self.transform.transform_chunk(
            &mut downloaded[start_index..start_index + total_read],
            chunk_index,
        )?;

        let available_bytes = self.downloaded_bytes - self.position;
        let buffer_end_bytes = min(buf.len(), available_bytes);
        let downloaded_end_bytes = self.position + buffer_end_bytes;

        buf[0..buffer_end_bytes].copy_from_slice(&downloaded[self.position..downloaded_end_bytes]);

        self.position += buffer_end_bytes;

        Ok(buffer_end_bytes)
    }
}

impl<T: ChunkTransform> Seek for SeekableSource<T> {
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

        self.position = if let Some(total) = self.total_bytes {
            new_position.min(total)
        } else {
            new_position
        };

        Ok(self.position as u64)
    }
}

impl<T: ChunkTransform> MediaSource for SeekableSource<T> {
    fn is_seekable(&self) -> bool {
        self.total_bytes.is_some() && self.downloaded.is_some()
    }

    fn byte_len(&self) -> Option<u64> {
        self.total_bytes.map(|len| len as u64)
    }
}

impl<T> SeekableSource<T>
where
    T: ChunkTransform,
    Self: MediaSource,
{
    pub fn new(source: Box<dyn MediaSource>, transform: T) -> Self {
        let total_bytes = block_in_place(|| source.byte_len().map(|size| size as usize));

        let downloaded = total_bytes.map(|bytes| Vec::with_capacity(bytes));

        Self {
            source,
            transform,
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

impl SeekableSource<DefaultChunkTransform> {
    pub fn new_default(source: Box<dyn MediaSource>) -> Self {
        Self::new(source, DefaultChunkTransform)
    }
}
