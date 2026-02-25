use songbird::input::{AudioStream, Input, LiveInput};
use reqwest::Client;
use std::time::Duration;
use std::collections::HashSet;
use symphonia::core::io::MediaSource;
use symphonia::core::probe::Hint;

use super::playlist::{Playlist, PlaylistParser};
use super::segment::SegmentFetcher;
use crate::util::errors::ResolverError;
use crate::util::http::{http1_make_request, HttpOptions};
use std::io::{self, Read, Seek, SeekFrom};
use bytes::Bytes;
use flume::{Receiver, Sender};

pub struct HlsHandler {
    // Placeholder
}

pub struct HlsStreamReader {
    rx: Receiver<Result<Bytes, std::io::Error>>,
    current_chunk: Option<std::io::Cursor<Bytes>>,
    position: u64,
}

impl HlsStreamReader {
    pub fn new(rx: Receiver<Result<Bytes, std::io::Error>>) -> Self {
        Self {
            rx,
            current_chunk: None,
            position: 0,
        }
    }
}

impl Read for HlsStreamReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            if let Some(cursor) = &mut self.current_chunk {
                let read = cursor.read(buf)?;
                if read > 0 {
                    self.position += read as u64;
                    return Ok(read);
                }
            }
            
            // Current chunk exhausted, get next
            match self.rx.recv() {
                Ok(Ok(bytes)) => {
                    tracing::debug!("HlsStreamReader: Received chunk of {} bytes", bytes.len());
                    self.current_chunk = Some(std::io::Cursor::new(bytes));
                },
                Ok(Err(e)) => {
                    tracing::error!("HlsStreamReader: Error receiving chunk: {:?}", e);
                    return Err(e);
                },
                Err(_) => {
                    tracing::debug!("HlsStreamReader: Reached EOF (channel closed)");
                    return Ok(0); // EOF
                }
            }
        }
    }
}

impl Seek for HlsStreamReader {
    fn seek(&mut self, _pos: SeekFrom) -> io::Result<u64> {
        Err(io::Error::new(io::ErrorKind::Other, "Seeking not supported in HLS stream"))
    }
}

impl MediaSource for HlsStreamReader {
    fn is_seekable(&self) -> bool {
        false
    }
    
    fn byte_len(&self) -> Option<u64> {
        None
    }
}

unsafe impl Send for HlsStreamReader {}
unsafe impl Sync for HlsStreamReader {}

/// Determine the appropriate format hint based on codec information
fn get_format_hint_from_codec(codec: Option<&str>) -> &'static str {
    if let Some(codec_str) = codec {
        let codec_lower = codec_str.to_lowercase();
        if codec_lower.contains("mp4a") {
            return "mp4";
        }
        if codec_lower.contains("opus") {
            return "ogg";
        }
    }
    "ts"
}

pub async fn start_hls_stream(url: String, client: Client) -> Input {
    // Pre-fetch master playlist to determine codec
    let codec = detect_codec_from_url(&url, &client).await;
    start_hls_stream_with_codec(url, client, codec).await
}

/// Fetches the HLS master playlist and detects the codec from the selected variant
async fn detect_codec_from_url(url: &str, client: &Client) -> Option<String> {
    let options = HttpOptions::default();
    let res = http1_make_request(url, client, options).await.ok()?;
    
    if !res.status.is_success() {
        return None;
    }
    
    let content = res.text?;
    let playlist = PlaylistParser::parse(&content, url)?;
    
    // If it's a master playlist, select the best variant and get its codec
    if playlist.is_master {
        let variant = playlist.variants.iter().find(|v| {
            if let Some(ref codecs) = v.codecs {
                let codecs_lower = codecs.to_lowercase();
                (codecs_lower.contains("mp4a") || codecs_lower.contains("opus")) 
                    && !codecs_lower.contains("avc1")
            } else {
                false
            }
        })?;
        
        tracing::debug!("Detected codec from master playlist: {:?}", variant.codecs);
        return variant.codecs.clone();
    }
    
    None
}

pub async fn start_hls_stream_with_codec(url: String, client: Client, codec: Option<String>) -> Input {
    let (tx, rx) = flume::bounded(10); // Buffer 10 chunks
    
    let client_clone = client.clone();
    tokio::spawn(async move {
        // HLS Loop
        if let Err(e) = run_hls_loop(url, client_clone, tx, None, false).await {
            tracing::error!("HLS Loop Error: {:?}", e);
        }
    });
    
    let reader = HlsStreamReader::new(rx);
    
    // Create AudioStream from our MediaSource
    let mut hint = Hint::new();
    let format_ext = get_format_hint_from_codec(codec.as_deref());
    tracing::debug!("Using format hint '{}' for codec: {:?}", format_ext, codec);
    hint.with_extension(format_ext);
    
    let audio_stream = AudioStream {
        input: Box::new(reader) as Box<dyn MediaSource>,
        hint: Some(hint),
    };
    
    // Return as LiveInput::Raw
    Input::Live(LiveInput::Raw(audio_stream), None)
}

/// Select the best audio variant from master playlist.
/// Prioritizes audio-only streams with mp4a/opus codecs.
fn select_best_variant(playlist: &Playlist) -> Option<String> {
    if playlist.variants.is_empty() {
        return None;
    }

    // First, try to find audio-only variant with preferred codecs
    let audio_variant = playlist.variants.iter().find(|v| {
        if let Some(ref codecs) = v.codecs {
            let codecs_lower = codecs.to_lowercase();
            (codecs_lower.contains("mp4a") || codecs_lower.contains("opus")) 
                && !codecs_lower.contains("avc1") // Exclude video codecs
        } else {
            false
        }
    });

    if let Some(variant) = audio_variant {
        tracing::debug!("Selected audio-only variant: bandwidth={}, codecs={:?}", variant.bandwidth, variant.codecs);
        return Some(variant.url.clone());
    }

    // Fallback: any variant with mp4a or opus
    let fallback_variant = playlist.variants.iter().find(|v| {
        if let Some(ref codecs) = v.codecs {
            let codecs_lower = codecs.to_lowercase();
            codecs_lower.contains("mp4a") || codecs_lower.contains("opus")
        } else {
            false
        }
    });

    if let Some(variant) = fallback_variant {
        tracing::debug!("Selected fallback variant: bandwidth={}, codecs={:?}", variant.bandwidth, variant.codecs);
        return Some(variant.url.clone());
    }

    // Last resort: highest bandwidth variant
    tracing::debug!("Selecting highest bandwidth variant: {}", playlist.variants[0].bandwidth);
    Some(playlist.variants[0].url.clone())
}

async fn run_hls_loop(
    url: String, 
    client: Client, 
    tx: Sender<Result<Bytes, std::io::Error>>,
    start_time: Option<f64>,
    _is_live: bool,
) -> Result<(), ResolverError> {
    let master_url = url.clone();
    let mut current_url = url;
    let fetcher = SegmentFetcher::new(client.clone());
    
    let mut processed_segments: HashSet<u64> = HashSet::new();
    let mut highest_sequence: i64 = -1;
    let mut last_media_sequence: i64 = -1;
    let mut master_refresh_counter = 0;
    const MASTER_REFRESH_INTERVAL: u32 = 3;
    const MAX_RETRIES: u32 = 3;
    let mut init_segment_sent = false;
    
    loop {
        // Fetch playlist with retry
        let mut playlist_attempt = 0;
        let playlist_content = loop {
            playlist_attempt += 1;
            let options = HttpOptions::default();
            match http1_make_request(&current_url, &client, options).await {
                Ok(res) if res.status.is_success() => {
                    if let Some(content) = res.text {
                        break content;
                    } else {
                        if playlist_attempt >= MAX_RETRIES {
                            return Err(ResolverError::Custom("Empty playlist content".to_string()));
                        }
                    }
                },
                Ok(res) if res.status.as_u16() == 403 || res.status.as_u16() == 410 => {
                    tracing::warn!("Playlist fetch returned {}, falling back to master", res.status);
                    if current_url != master_url {
                        current_url = master_url.clone();
                        continue;
                    }
                    return Err(ResolverError::Custom(format!("Playlist unavailable: {}", res.status)));
                },
                Ok(res) => {
                    if playlist_attempt >= MAX_RETRIES {
                        return Err(ResolverError::Custom(format!("Playlist fetch failed: {}", res.status)));
                    }
                },
                Err(e) => {
                    if playlist_attempt >= MAX_RETRIES {
                        return Err(e);
                    }
                }
            }
            tokio::time::sleep(Duration::from_secs(2u64.pow(playlist_attempt - 1))).await;
        };
        
        let playlist = PlaylistParser::parse(&playlist_content, &current_url)
            .ok_or_else(|| ResolverError::Custom("Failed to parse playlist".to_string()))?;
        
        // Handle master playlist
        if playlist.is_master {
            let selected_url = select_best_variant(&playlist)
                .ok_or_else(|| ResolverError::Custom("No variants in master playlist".to_string()))?;
            
            tracing::info!("Selected HLS variant from master playlist");
            current_url = selected_url;
            continue;
        }
        
        let target_duration = playlist.target_duration;
        let is_vod = !playlist.is_live;
        
        tracing::debug!(
            "Processing playlist: live={}, segments={}, target_duration={:.1}s",
            playlist.is_live, playlist.segments.len(), target_duration
        );
        
        // Check for discontinuity or sequence jump
        if last_media_sequence != -1 {
            let seq_diff = playlist.media_sequence as i64 - last_media_sequence;
            if seq_diff < 0 || seq_diff > 30 {
                tracing::warn!(
                    "Playlist sequence discontinuity: {} -> {}. Resetting.",
                    last_media_sequence, playlist.media_sequence
                );
                processed_segments.clear();
                highest_sequence = -1;
            }
        }
        last_media_sequence = playlist.media_sequence as i64;
        
        // Handle start time for VOD
        if let Some(start_secs) = start_time {
            if !processed_segments.is_empty() || is_vod {
                let mut elapsed = 0.0;
                for segment in &playlist.segments {
                    if elapsed + segment.duration <= start_secs {
                        elapsed += segment.duration;
                        processed_segments.insert(segment.sequence);
                        if segment.sequence as i64 > highest_sequence {
                            highest_sequence = segment.sequence as i64;
                        }
                    } else {
                        break;
                    }
                }
                tracing::debug!("Skipped segments for start_time={:.1}s, elapsed={:.1}s", start_secs, elapsed);
            }
        }
        
        // Fetch new segments
        let mut fetched_any = false;
        for segment in &playlist.segments {
            let seq = segment.sequence as i64;
            
            // For fMP4 streams, fetch and send the initialization segment first
            if !init_segment_sent {
                if let Some(ref map) = segment.map {
                    tracing::info!("Fetching fMP4 initialization segment: {}", map.uri);
                    match fetcher.fetch_map(map, segment.key.as_ref(), Some(segment.sequence as u64)).await {
                        Ok(init_data) => {
                            tracing::info!("fMP4 initialization segment fetched: {} bytes", init_data.len());
                            if tx.send_async(Ok(init_data)).await.is_err() {
                                tracing::debug!("HLS: Receiver dropped while sending init segment");
                                return Ok(());
                            }
                            init_segment_sent = true;
                        }
                        Err(e) => {
                            tracing::error!("Failed to fetch fMP4 initialization segment: {:?}", e);
                            return Err(e);
                        }
                    }
                } else {
                    // Not an fMP4 stream, no init segment needed
                    init_segment_sent = true;
                }
            }
            
            // Skip already processed or old segments
            if seq <= highest_sequence || processed_segments.contains(&segment.sequence) {
                continue;
            }
            
            // Handle discontinuity
            if segment.discontinuity && playlist.is_live {
                tracing::warn!("Discontinuity detected, resetting state");
                processed_segments.clear();
                highest_sequence = -1;
                break;
            }
            
            // Fetch segment with retry
            let mut retry_count = 0;
            let segment_data = loop {
                retry_count += 1;
                match fetcher.fetch_segment(segment).await {
                    Ok(data) => break Some(data),
                    Err(e) => {
                        if retry_count >= MAX_RETRIES {
                            tracing::error!("Segment fetch failed after {} retries: {:?}", MAX_RETRIES, e);
                            break None;
                        }
                        tracing::warn!("Segment fetch attempt {}/{} failed, retrying...", retry_count, MAX_RETRIES);
                        tokio::time::sleep(Duration::from_millis(500 * retry_count as u64)).await;
                    }
                }
            };
            
            if let Some(data) = segment_data {
                processed_segments.insert(segment.sequence);
                if seq > highest_sequence {
                    highest_sequence = seq;
                }
                
                if tx.send_async(Ok(data)).await.is_err() {
                    tracing::debug!("HLS: Receiver dropped, stopping loop");
                    return Ok(());
                }
                
                fetched_any = true;
            }
        }
        
        // VOD: stop when all segments are fetched
        if is_vod && !fetched_any {
            tracing::info!("HLS VOD: All segments fetched");
            break;
        }
        
        // Live: periodic refresh
        if playlist.is_live {
            master_refresh_counter += 1;
            if master_refresh_counter >= MASTER_REFRESH_INTERVAL {
                master_refresh_counter = 0;
                current_url = master_url.clone();
            }
            
            let delay = Duration::from_secs_f64((target_duration / 2.0).max(0.5));
            tokio::time::sleep(delay).await;
        } else {
            break;
        }
    }
    
    Ok(())
}
