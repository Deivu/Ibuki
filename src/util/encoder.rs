use crate::models::ApiTrackInfo;
use crate::util::errors::Base64EncodeError;
use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use byteorder::BigEndian;
use byteorder::WriteBytesExt;
use std::io::Cursor;
use std::io::Write;

fn write_string(wtr: &mut Cursor<Vec<u8>>, message: &str) -> Result<(), Base64EncodeError> {
    wtr.write_u16::<BigEndian>(message.len() as u16)?;
    wtr.write_all(message.as_bytes())?;
    Ok(())
}

fn optional_write_string(
    wtr: &mut Cursor<Vec<u8>>,
    opt: &Option<String>,
) -> Result<(), Base64EncodeError> {
    match opt {
        Some(s) => {
            wtr.write_u8(1)?;
            write_string(wtr, s)?;
        }
        None => {
            wtr.write_u8(0)?;
        }
    }
    Ok(())
}

/// Encode a string using Modified UTF-8 (Java/Lavalink compatible).
fn encode_modified_utf8(value: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(value.len());
    for ch in value.chars() {
        let code = ch as u32;
        if code >= 0x0001 && code <= 0x007F {
            bytes.push(code as u8);
        } else if code == 0x0000 || (code >= 0x0080 && code <= 0x07FF) {
            bytes.push(0xC0 | ((code >> 6) & 0x1F) as u8);
            bytes.push(0x80 | (code & 0x3F) as u8);
        } else {
            bytes.push(0xE0 | ((code >> 12) & 0x0F) as u8);
            bytes.push(0x80 | ((code >> 6) & 0x3F) as u8);
            bytes.push(0x80 | (code & 0x3F) as u8);
        }
    }
    bytes
}

/// Write a Modified UTF-8 encoded string with a 2-byte length prefix.
fn write_modified_utf8_string(buf: &mut Vec<u8>, value: &str) -> Result<(), Base64EncodeError> {
    let encoded = encode_modified_utf8(value);
    if encoded.len() > 65535 {
        return Err(Base64EncodeError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Encode Error: UTF string too long",
        )));
    }
    buf.extend_from_slice(&(encoded.len() as u16).to_be_bytes());
    buf.extend_from_slice(&encoded);
    Ok(())
}

/// Write a nullable Modified UTF-8 string (1-byte presence flag + optional string).
fn write_nullable_modified_utf8(
    buf: &mut Vec<u8>,
    opt: &Option<String>,
) -> Result<(), Base64EncodeError> {
    match opt {
        Some(s) => {
            buf.push(1);
            write_modified_utf8_string(buf, s)?;
        }
        None => {
            buf.push(0);
        }
    }
    Ok(())
}

/// Encode a track using Lavalink-compatible format.
pub fn encode_track(track_info: &ApiTrackInfo) -> Result<String, Base64EncodeError> {
    let mut message = Vec::new();
    let version: u8 = if track_info.artwork_url.is_some() || track_info.isrc.is_some() {
        3
    } else if track_info.uri.is_some() {
        2
    } else {
        1
    };

    let flags: u32 = 1;
    message.push(version);
    write_modified_utf8_string(&mut message, &track_info.title)?;
    write_modified_utf8_string(&mut message, &track_info.author)?;
    message.extend_from_slice(&(track_info.length as i64).to_be_bytes());
    write_modified_utf8_string(&mut message, &track_info.identifier)?;
    message.push(if track_info.is_stream { 1 } else { 0 });

    if version >= 2 {
        write_nullable_modified_utf8(&mut message, &track_info.uri)?;
    }
    if version >= 3 {
        write_nullable_modified_utf8(&mut message, &track_info.artwork_url)?;
        write_nullable_modified_utf8(&mut message, &track_info.isrc)?;
    }

    write_modified_utf8_string(&mut message, &track_info.source_name)?;
    message.extend_from_slice(&(track_info.position as i64).to_be_bytes());
    let seekable_trailer = if track_info.is_seekable {
        Some("IBUKI:seekableY".to_string())
    } else {
        Some("IBUKI:seekableN".to_string())
    };
    write_nullable_modified_utf8(&mut message, &seekable_trailer)?;
    let header = ((message.len() as u32) & 0x3FFFFFFF) | ((flags & 0x3) << 30);
    let mut result = Vec::with_capacity(4 + message.len());
    result.extend_from_slice(&(header as i32).to_be_bytes());
    result.extend_from_slice(&message);

    Ok(BASE64_STANDARD.encode(result))
}
