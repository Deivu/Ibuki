use super::SECRET_IV;
use crate::util::seek::{ChunkTransform, SeekableSource};
use async_trait::async_trait;
use blowfish::Blowfish;
use cbc::cipher::{KeyIvInit, block_padding::NoPadding};
use cbc::{Decryptor, cipher::BlockDecryptMut};
use songbird::input::{AudioStream, AudioStreamError, Compose, HttpRequest};
use std::io::{Error as IoError, ErrorKind, Result as IoResult};
use symphonia::core::io::MediaSource;

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

        let transform = DeezerDecryptTransform::new(self.key);
        let source = SeekableSource::<DeezerDecryptTransform>::new(request.input, transform);

        Ok(AudioStream {
            input: Box::new(source) as Box<dyn MediaSource>,
            hint,
        })
    }

    fn should_create_async(&self) -> bool {
        self.request.should_create_async()
    }
}

impl DeezerHttpStream {
    pub fn new(request: HttpRequest, key: [u8; 16]) -> Self {
        Self { request, key }
    }
}

pub struct DeezerDecryptTransform {
    key: [u8; 16],
}

impl DeezerDecryptTransform {
    pub fn new(key: [u8; 16]) -> Self {
        Self { key }
    }
}

impl ChunkTransform for DeezerDecryptTransform {
    fn transform_chunk(&mut self, data: &mut [u8], chunk_index: usize) -> IoResult<usize> {
        let data_len = data.len();

        if chunk_index % 3 == 0 && data_len == self.chunk_size() {
            let decryptor: Decryptor<Blowfish> = Decryptor::new_from_slices(&self.key, &SECRET_IV)
                .map_err(|error| IoError::new(ErrorKind::Unsupported, error))?;

            decryptor
                .decrypt_padded_mut::<NoPadding>(data)
                .map_err(|error| IoError::new(ErrorKind::InvalidInput, error.to_string()))?;
        }

        Ok(data_len)
    }

    fn chunk_size(&self) -> usize {
        2048
    }
}
