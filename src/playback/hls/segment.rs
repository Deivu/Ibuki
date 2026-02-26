use aes::Aes128;
use aes::cipher::{BlockDecryptMut, KeyIvInit, block_padding::Pkcs7};
use bytes::Bytes;
use cbc::Decryptor;
use reqwest::Client;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use super::playlist::{Key, Map, Segment};
use crate::util::errors::ResolverError;
use crate::util::http::{HttpOptions, http1_make_request};

type Aes128CbcDec = Decryptor<Aes128>;

pub struct SegmentFetcher {
    client: Client,
    key_cache: Arc<Mutex<HashMap<String, Vec<u8>>>>,
}

impl SegmentFetcher {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            key_cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn fetch_key(&self, key: &Key) -> Result<Vec<u8>, ResolverError> {
        if key.method == "NONE" {
            return Ok(Vec::new());
        }

        {
            let cache = self.key_cache.lock().await;
            if let Some(k) = cache.get(&key.uri) {
                tracing::debug!("Using cached encryption key");
                return Ok(k.clone());
            }
        }

        tracing::debug!("Fetching encryption key from: {}", key.uri);
        let options = HttpOptions::default();
        let res = http1_make_request(&key.uri, &self.client, options).await?;

        if !res.status.is_success() {
            tracing::error!("Key fetch failed with status: {}", res.status);
            return Err(ResolverError::Custom(format!(
                "Key fetch failed: {}",
                res.status
            )));
        }

        let bytes = res
            .body
            .ok_or(ResolverError::Custom("Empty body".to_string()))?
            .to_vec();

        tracing::debug!(
            "Encryption key fetched successfully, length: {} bytes",
            bytes.len()
        );

        let mut cache = self.key_cache.lock().await;
        if cache.len() >= 20 {
            if let Some(k) = cache.keys().next().cloned() {
                cache.remove(&k);
            }
        }
        cache.insert(key.uri.clone(), bytes.clone());

        Ok(bytes)
    }

    pub async fn fetch_map(
        &self,
        map: &Map,
        key: Option<&Key>,
        sequence: Option<u64>,
    ) -> Result<Bytes, ResolverError> {
        tracing::debug!("Fetching map segment: {}", map.uri);

        let mut options = HttpOptions::default();

        if let Some(range) = map.byte_range {
            let end = range.offset + range.length - 1;
            options.headers.insert(
                "Range",
                format!("bytes={}-{}", range.offset, end).parse().unwrap(),
            );
        }

        let res = http1_make_request(&map.uri, &self.client, options).await?;

        if !res.status.is_success() && res.status != reqwest::StatusCode::PARTIAL_CONTENT {
            return Err(ResolverError::Custom(format!(
                "Map fetch failed: {}",
                res.status
            )));
        }

        let body = res
            .body
            .ok_or(ResolverError::Custom("Empty map body".to_string()))?;
        if let Some(key) = key {
            if key.method != "NONE" {
                let iv = match &key.iv {
                    Some(iv) => iv.clone(),
                    None => {
                        if let Some(seq) = sequence {
                            let mut iv_bytes = [0u8; 16];
                            iv_bytes[8..16].copy_from_slice(&seq.to_be_bytes());
                            iv_bytes.to_vec()
                        } else {
                            tracing::error!(
                                "Map segment encrypted but missing IV and no sequence provided"
                            );
                            return Err(ResolverError::Custom(
                                "Missing IV for map segment".to_string(),
                            ));
                        }
                    }
                };

                tracing::debug!("Decrypting map segment");
                let key_data = self.fetch_key(key).await?;
                return self.decrypt(body, &key_data, &iv, &key.method);
            }
        }

        tracing::debug!("Map segment fetched, size: {} bytes", body.len());
        Ok(body)
    }

    pub async fn fetch_segment(&self, segment: &Segment) -> Result<Bytes, ResolverError> {
        let mut options = HttpOptions::default();

        if let Some(range) = segment.byte_range {
            let end = range.offset + range.length - 1;
            options.headers.insert(
                "Range",
                format!("bytes={}-{}", range.offset, end).parse().unwrap(),
            );
            tracing::debug!("Fetching segment with byte range: {}-{}", range.offset, end);
        }

        let res = http1_make_request(&segment.url, &self.client, options).await?;

        if !res.status.is_success() && res.status != reqwest::StatusCode::PARTIAL_CONTENT {
            tracing::error!("Segment fetch failed with status: {}", res.status);
            return Err(ResolverError::Custom(format!(
                "Segment fetch failed: {}",
                res.status
            )));
        }

        let body = res
            .body
            .ok_or(ResolverError::Custom("Empty body".to_string()))?;

        if let Some(key) = &segment.key {
            if key.method != "NONE" {
                let key_data = self.fetch_key(key).await?;
                let iv = key.iv.clone().unwrap_or_else(|| {
                    let mut iv = [0u8; 16];
                    let seq_bytes = segment.sequence.to_be_bytes();
                    iv[8..16].copy_from_slice(&seq_bytes);
                    iv.to_vec()
                });

                tracing::debug!(
                    "Decrypting segment {} with method: {}",
                    segment.sequence,
                    key.method
                );
                return self.decrypt(body, &key_data, &iv, &key.method);
            }
        }

        Ok(body)
    }

    fn decrypt(
        &self,
        data: Bytes,
        key: &[u8],
        iv: &[u8],
        method: &str,
    ) -> Result<Bytes, ResolverError> {
        if method == "AES-128" {
            if key.len() != 16 || iv.len() != 16 {
                tracing::error!("Invalid key/iv length: key={}, iv={}", key.len(), iv.len());
                return Err(ResolverError::Custom(
                    "Invalid key/iv length for AES-128".to_string(),
                ));
            }

            let cipher = Aes128CbcDec::new(key.into(), iv.into());
            let mut buffer = data.to_vec();

            if buffer.len() % 16 != 0 {
                tracing::warn!(
                    "Data length {} is not a multiple of 16, may fail",
                    buffer.len()
                );
                return Err(ResolverError::Custom(
                    "Data length not multiple of 16 for AES decryption".to_string(),
                ));
            }

            let decrypted = cipher
                .decrypt_padded_mut::<Pkcs7>(&mut buffer)
                .map_err(|e| {
                    tracing::error!("Decryption failed: {:?}", e);
                    ResolverError::Custom(format!("Decryption failed: {:?}", e))
                })?;

            tracing::debug!("Successfully decrypted {} bytes", decrypted.len());
            return Ok(Bytes::copy_from_slice(decrypted));
        }

        tracing::error!("Unsupported encryption method: {}", method);
        Err(ResolverError::Custom(format!(
            "Unsupported encryption method: {}",
            method
        )))
    }
}
