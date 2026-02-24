//
// Lavalink seems to encode tracks into binary, then serializing it via base64
// And I suck at dealing with binary
// Thanks to @Takase (https://github.com/takase1121) for helping me with this
//
use crate::constants::TRACK_INFO_VERSIONED;
use crate::models::ApiTrackInfo;
use crate::util::errors::Base64DecodeError;
use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use byteorder::BigEndian;
use byteorder::ReadBytesExt;
use std::io::Cursor;
use std::io::Read;

fn read_string(rdr: &mut Cursor<Vec<u8>>) -> Result<String, Base64DecodeError> {
    let len = rdr.read_u16::<BigEndian>()?;
    let mut buf: Vec<u8> = vec![0; len as usize];
    rdr.read_exact(&mut buf)?;
    Ok(String::from_utf8(buf)?)
}

fn optional_read_string(rdr: &mut Cursor<Vec<u8>>) -> Result<Option<String>, Base64DecodeError> {
    if rdr.read_u8()? != 0 {
        Ok(Some(read_string(rdr)?))
    } else {
        Ok(None)
    }
}

/// Decode a Modified UTF-8 string (Java format).
fn decode_modified_utf8(bytes: &[u8]) -> Result<String, Base64DecodeError> {
    let mut chars = Vec::new();
    let mut i = 0;
    let end = bytes.len();

    while i < end {
        let c = bytes[i];

        if c < 0x80 {
            // 1-byte character (0x01-0x7F)
            chars.push(c as char);
            i += 1;
        } else if (c & 0xE0) == 0xC0 {
            // 2-byte character
            if i + 1 >= end {
                return Err(Base64DecodeError::Custom("Malformed Modified UTF-8: incomplete 2-byte sequence".to_string()));
            }
            let c2 = bytes[i + 1];
            if (c2 & 0xC0) != 0x80 {
                return Err(Base64DecodeError::Custom("Malformed Modified UTF-8: invalid continuation byte".to_string()));
            }
            let ch = (((c & 0x1F) as u32) << 6) | ((c2 & 0x3F) as u32);
            chars.push(char::from_u32(ch).ok_or_else(|| Base64DecodeError::Custom("Invalid character code".to_string()))?);
            i += 2;
        } else if (c & 0xF0) == 0xE0 {
            // 3-byte character
            if i + 2 >= end {
                return Err(Base64DecodeError::Custom("Malformed Modified UTF-8: incomplete 3-byte sequence".to_string()));
            }
            let c2 = bytes[i + 1];
            let c3 = bytes[i + 2];
            if (c2 & 0xC0) != 0x80 || (c3 & 0xC0) != 0x80 {
                return Err(Base64DecodeError::Custom("Malformed Modified UTF-8: invalid continuation byte".to_string()));
            }
            let ch = (((c & 0x0F) as u32) << 12) | (((c2 & 0x3F) as u32) << 6) | ((c3 & 0x3F) as u32);
            chars.push(char::from_u32(ch).ok_or_else(|| Base64DecodeError::Custom("Invalid character code".to_string()))?);
            i += 3;
        } else {
            return Err(Base64DecodeError::Custom("Malformed Modified UTF-8: invalid byte".to_string()));
        }
    }

    Ok(chars.into_iter().collect())
}

/// Read a Modified UTF-8 string with 2-byte length prefix.
fn read_modified_utf8_string(buffer: &[u8], position: &mut usize) -> Result<String, Base64DecodeError> {
    if *position + 2 > buffer.len() {
        return Err(Base64DecodeError::Custom("Unexpected end of buffer reading string length".to_string()));
    }
    let len = u16::from_be_bytes([buffer[*position], buffer[*position + 1]]) as usize;
    *position += 2;

    if *position + len > buffer.len() {
        return Err(Base64DecodeError::Custom(format!("Unexpected end of buffer: need {} bytes", len)));
    }

    let bytes = &buffer[*position..*position + len];
    *position += len;

    decode_modified_utf8(bytes)
}

/// Read a nullable Modified UTF-8 string (1-byte flag + optional string).
fn read_nullable_modified_utf8(buffer: &[u8], position: &mut usize) -> Result<Option<String>, Base64DecodeError> {
    if *position + 1 > buffer.len() {
        return Err(Base64DecodeError::Custom("Unexpected end of buffer reading nullable flag".to_string()));
    }
    let present = buffer[*position] != 0;
    *position += 1;

    if present {
        Ok(Some(read_modified_utf8_string(buffer, position)?))
    } else {
        Ok(None)
    }
}

/// Try to parse seekable trailer
fn try_parse_seekable_trailer(buffer: &[u8]) -> Option<bool> {
    let max_try = buffer.len().min(512);
    for cut in 1..=max_try {
        let tail = &buffer[buffer.len() - cut..];
        let mut pos = 0;
        
        if pos >= tail.len() {
            continue;
        }
        
        let present = tail[pos] != 0;
        pos += 1;
        
        if !present {
            continue;
        }
        if let Ok(s) = read_modified_utf8_string(tail, &mut pos) {
            if pos == tail.len() {
                if s == "IBUKI:seekableY" {
                    return Some(true);
                } else if s == "IBUKI:seekableN" {
                    return Some(false);
                }
            }
        }
    }
    None
}

/// Decode a track using lavalink/ibuki format (Modified UTF-8 with seekable trailer).
pub fn decode_track(encoded: &str) -> Result<ApiTrackInfo, Base64DecodeError> {
    if encoded.is_empty() {
        return Err(Base64DecodeError::Custom("Decode Error: Input string is null or empty".to_string()));
    }

    let buffer = BASE64_STANDARD.decode(encoded)?;
    
    if buffer.len() < 4 {
        return Err(Base64DecodeError::Custom("Decode Error: Buffer too short".to_string()));
    }

    let mut position = 0;
    let header = i32::from_be_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]);
    position += 4;

    let _flags = ((header as u32) >> 30) & 0x3;
    let message_size = (header & 0x3FFFFFFF) as usize;

    if message_size == 0 {
        return Err(Base64DecodeError::Custom("Decode Error: message size is 0".to_string()));
    }

    if position + message_size > buffer.len() {
        return Err(Base64DecodeError::Custom(format!("Decode Error: message size {} exceeds buffer", message_size)));
    }

    let mut message_buf = &buffer[position..position + message_size];
    position += message_size;

    // Try to parse seekable trailer
    let (seekable, message_without_trailer) = if let Some(seekable_value) = try_parse_seekable_trailer(message_buf) {
        let mut found_cut = 0;
        let max_try = message_buf.len().min(512);
        for cut in 1..=max_try {
            let tail = &message_buf[message_buf.len() - cut..];
            let mut pos = 0;
            if pos < tail.len() && tail[pos] != 0 {
                pos += 1;
                if let Ok(s) = read_modified_utf8_string(tail, &mut pos) {
                    if pos == tail.len() && (s == "IBUKI:seekableY" || s == "IBUKI:seekableN") {
                        found_cut = cut;
                        break;
                    }
                }
            }
        }
        (Some(seekable_value), &message_buf[..message_buf.len() - found_cut])
    } else {
        (None, message_buf)
    };

    message_buf = message_without_trailer;
    let mut msg_pos = 0;
    if msg_pos + 1 > message_buf.len() {
        return Err(Base64DecodeError::Custom("Decode Error: no version byte".to_string()));
    }
    let version = message_buf[msg_pos];
    msg_pos += 1;

    let title = read_modified_utf8_string(message_buf, &mut msg_pos)?;
    let author = read_modified_utf8_string(message_buf, &mut msg_pos)?;

    if msg_pos + 8 > message_buf.len() {
        return Err(Base64DecodeError::Custom("Decode Error: not enough bytes for length".to_string()));
    }
    let length = i64::from_be_bytes([
        message_buf[msg_pos], message_buf[msg_pos + 1], message_buf[msg_pos + 2], message_buf[msg_pos + 3],
        message_buf[msg_pos + 4], message_buf[msg_pos + 5], message_buf[msg_pos + 6], message_buf[msg_pos + 7],
    ]) as u64;
    msg_pos += 8;

    let identifier = read_modified_utf8_string(message_buf, &mut msg_pos)?;

    if msg_pos + 1 > message_buf.len() {
        return Err(Base64DecodeError::Custom("Decode Error: not enough bytes for isStream".to_string()));
    }
    let is_stream = message_buf[msg_pos] != 0;
    msg_pos += 1;

    let uri = if version >= 2 {
        read_nullable_modified_utf8(message_buf, &mut msg_pos)?
    } else {
        None
    };

    let artwork_url = if version >= 3 {
        read_nullable_modified_utf8(message_buf, &mut msg_pos)?
    } else {
        None
    };

    let isrc = if version >= 3 {
        read_nullable_modified_utf8(message_buf, &mut msg_pos)?
    } else {
        None
    };

    let source_name = read_modified_utf8_string(message_buf, &mut msg_pos)?;

    // Position is the last 8 bytes of the message (before any trailer)
    if message_buf.len() < 8 {
        return Err(Base64DecodeError::Custom("Decode Error: message too short for position".to_string()));
    }
    let position_offset = message_buf.len() - 8;
    let position_value = i64::from_be_bytes([
        message_buf[position_offset], message_buf[position_offset + 1], message_buf[position_offset + 2], message_buf[position_offset + 3],
        message_buf[position_offset + 4], message_buf[position_offset + 5], message_buf[position_offset + 6], message_buf[position_offset + 7],
    ]) as u64;

    let is_seekable = seekable.unwrap_or(!is_stream);

    Ok(ApiTrackInfo {
        title,
        author,
        length,
        identifier,
        is_stream,
        is_seekable,
        uri,
        artwork_url,
        isrc,
        source_name,
        position: position_value,
    })
}

/**
 * This decodes lavalink base64 strings just fine
 */
pub fn decode_base64(encoded: &String) -> Result<ApiTrackInfo, Base64DecodeError> {
    let decoded = BASE64_STANDARD.decode(encoded)?;

    let mut rdr = Cursor::new(decoded);

    let value = rdr.read_u32::<BigEndian>()?;
    let flags = (value & 0xC0000000) >> 30;

    let version = if flags & TRACK_INFO_VERSIONED != 0 {
        rdr.read_u8()?
    } else {
        1
    };

    if version > 3 || version == 0 {
        return Err(Base64DecodeError::UnknownVersion(version));
    }

    let title = read_string(&mut rdr)?;
    let author = read_string(&mut rdr)?;
    let length = rdr.read_u64::<BigEndian>()?;
    let identifier = read_string(&mut rdr)?;
    let is_stream = rdr.read_u8()? != 0;

    let uri = optional_read_string(&mut rdr)?;
    let artwork_url = optional_read_string(&mut rdr)?;
    let isrc = optional_read_string(&mut rdr)?;

    let source_name = read_string(&mut rdr)?;

    let position = rdr.read_u64::<BigEndian>()?;

    Ok(ApiTrackInfo {
        title,
        author,
        length,
        identifier,
        is_stream,
        is_seekable: !is_stream,
        uri,
        artwork_url,
        isrc,
        source_name,
        position,
    })
}
