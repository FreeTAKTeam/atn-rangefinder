use crate::ProtocolError;
use flate2::{Decompress, FlushDecompress, Status};
use serde::{Deserialize, Serialize};

const SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PngLimits {
    pub max_encoded_bytes: usize,
    pub max_width: u32,
    pub max_height: u32,
    pub max_pixels: u64,
    pub max_decoded_bytes: usize,
    pub max_chunks: usize,
    /// The observed raster pipeline is 40x40. A configurable local limit is not
    /// evidence that firmware accepts different dimensions.
    pub expected_dimensions: Option<(u32, u32)>,
}

impl Default for PngLimits {
    fn default() -> Self {
        Self {
            max_encoded_bytes: usize::from(u16::MAX),
            max_width: 40,
            max_height: 40,
            max_pixels: 1600,
            max_decoded_bytes: 6440,
            max_chunks: 128,
            expected_dimensions: Some((40, 40)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PngInfo {
    pub width: u32,
    pub height: u32,
    pub decoded_bytes: usize,
}

/// Validates the supported RGBA8, noninterlaced PNG subset, including every
/// chunk CRC, chunk ordering, complete single zlib stream, decoded scanline
/// count, filter bytes, and no bytes after IEND. Decompression is bounded.
///
/// # Errors
/// Rejects malformed PNG structure/CRC, unsupported format, configured limits,
/// incomplete or trailing zlib data, and invalid scanline sizes/filter bytes.
pub fn validate_png(bytes: &[u8], limits: PngLimits) -> Result<PngInfo, ProtocolError> {
    if bytes.len() > limits.max_encoded_bytes {
        return Err(ProtocolError::LimitExceeded("encoded PNG bytes"));
    }
    if !bytes.starts_with(SIGNATURE) {
        return Err(ProtocolError::InvalidPng("signature"));
    }
    let mut offset = 8usize;
    let mut chunks = 0usize;
    let mut dimensions = None;
    let mut idat = Vec::new();
    let mut seen_idat = false;
    let mut idat_ended = false;
    let mut seen_plte = false;
    let mut seen_iend = false;
    while offset < bytes.len() {
        chunks += 1;
        if chunks > limits.max_chunks {
            return Err(ProtocolError::LimitExceeded("PNG chunks"));
        }
        let Chunk {
            kind,
            data,
            end: chunk_end,
        } = read_chunk(bytes, offset)?;
        if chunks == 1 && kind != b"IHDR" {
            return Err(ProtocolError::InvalidPng("IHDR must be first"));
        }
        if seen_idat && kind != b"IDAT" {
            idat_ended = true;
        }
        match kind {
            b"IHDR" => {
                if dimensions.is_some() || data.len() != 13 {
                    return Err(ProtocolError::InvalidPng("IHDR length or duplicate"));
                }
                let width = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
                let height = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
                if width == 0
                    || height == 0
                    || width > limits.max_width
                    || height > limits.max_height
                    || u64::from(width) * u64::from(height) > limits.max_pixels
                {
                    return Err(ProtocolError::LimitExceeded("PNG dimensions"));
                }
                if limits
                    .expected_dimensions
                    .is_some_and(|expected| expected != (width, height))
                {
                    return Err(ProtocolError::InvalidPng("unexpected dimensions"));
                }
                if data[8..] != [8, 6, 0, 0, 0] {
                    return Err(ProtocolError::InvalidPng(
                        "only noninterlaced RGBA8 is supported",
                    ));
                }
                dimensions = Some((width, height));
            }
            b"PLTE" => {
                if seen_plte
                    || seen_idat
                    || data.is_empty()
                    || data.len() > 768
                    || data.len() % 3 != 0
                {
                    return Err(ProtocolError::InvalidPng("PLTE structure or order"));
                }
                seen_plte = true;
            }
            b"IDAT" => {
                if idat_ended {
                    return Err(ProtocolError::InvalidPng("nonconsecutive IDAT"));
                }
                seen_idat = true;
                idat.extend_from_slice(data);
            }
            b"IEND" => {
                if !seen_idat || !data.is_empty() {
                    return Err(ProtocolError::InvalidPng("IEND structure or order"));
                }
                if chunk_end != bytes.len() {
                    return Err(ProtocolError::InvalidPng("bytes after IEND"));
                }
                seen_iend = true;
            }
            _ if kind[0].is_ascii_uppercase() => {
                return Err(ProtocolError::InvalidPng("unsupported critical chunk"));
            }
            // Ancillary metadata is accepted structurally after validating CRC.
            _ => {}
        }
        offset = chunk_end;
    }
    if !seen_iend {
        return Err(ProtocolError::InvalidPng("missing IEND"));
    }
    decode_scanlines(
        &idat,
        dimensions.ok_or(ProtocolError::InvalidPng("missing IHDR"))?,
        limits,
    )
}

struct Chunk<'a> {
    kind: &'a [u8],
    data: &'a [u8],
    end: usize,
}

fn read_chunk(bytes: &[u8], offset: usize) -> Result<Chunk<'_>, ProtocolError> {
    let header_end = offset
        .checked_add(8)
        .ok_or(ProtocolError::InvalidPng("chunk length overflow"))?;
    let header = bytes
        .get(offset..header_end)
        .ok_or(ProtocolError::InvalidPng("truncated chunk header"))?;
    let length = usize::try_from(u32::from_be_bytes([
        header[0], header[1], header[2], header[3],
    ]))
    .map_err(|_| ProtocolError::InvalidPng("chunk length overflow"))?;
    let kind = &header[4..8];
    if !kind.iter().all(u8::is_ascii_alphabetic) || kind[2].is_ascii_lowercase() {
        return Err(ProtocolError::InvalidPng("invalid chunk name"));
    }
    let data_end = header_end
        .checked_add(length)
        .ok_or(ProtocolError::InvalidPng("chunk length overflow"))?;
    let chunk_end = data_end
        .checked_add(4)
        .ok_or(ProtocolError::InvalidPng("chunk length overflow"))?;
    let data = bytes
        .get(header_end..data_end)
        .ok_or(ProtocolError::InvalidPng("truncated chunk"))?;
    let crc = bytes
        .get(data_end..chunk_end)
        .ok_or(ProtocolError::InvalidPng("truncated chunk CRC"))?;
    let mut hasher = crc32fast::Hasher::new();
    hasher.update(kind);
    hasher.update(data);
    if hasher.finalize() != u32::from_be_bytes([crc[0], crc[1], crc[2], crc[3]]) {
        return Err(ProtocolError::InvalidPng("chunk CRC"));
    }
    Ok(Chunk {
        kind,
        data,
        end: chunk_end,
    })
}

fn decode_scanlines(
    idat: &[u8],
    (width, height): (u32, u32),
    limits: PngLimits,
) -> Result<PngInfo, ProtocolError> {
    let row_bytes = u64::from(width)
        .checked_mul(4)
        .and_then(|count| count.checked_add(1))
        .ok_or(ProtocolError::LimitExceeded("decoded PNG bytes"))?;
    let decoded_bytes = row_bytes
        .checked_mul(u64::from(height))
        .and_then(|count| usize::try_from(count).ok())
        .filter(|count| *count <= limits.max_decoded_bytes)
        .ok_or(ProtocolError::LimitExceeded("decoded PNG bytes"))?;
    let decode_limit = decoded_bytes
        .checked_add(1)
        .ok_or(ProtocolError::LimitExceeded("decoded PNG bytes"))?;
    let mut decoder = Decompress::new(true);
    let mut scanlines = vec![0; decode_limit];
    let status = decoder
        .decompress(idat, &mut scanlines, FlushDecompress::Finish)
        .map_err(|_| ProtocolError::InvalidPng("zlib stream"))?;
    if status != Status::StreamEnd {
        return Err(ProtocolError::InvalidPng(
            "incomplete or oversized zlib stream",
        ));
    }
    if usize::try_from(decoder.total_out()).ok() != Some(decoded_bytes) {
        return Err(ProtocolError::InvalidPng("decoded scanline length"));
    }
    scanlines.truncate(decoded_bytes);
    if usize::try_from(decoder.total_in()).ok() != Some(idat.len()) {
        return Err(ProtocolError::InvalidPng("trailing compressed bytes"));
    }
    let stride =
        usize::try_from(row_bytes).map_err(|_| ProtocolError::LimitExceeded("row bytes"))?;
    if scanlines.chunks_exact(stride).any(|row| row[0] > 4) {
        return Err(ProtocolError::InvalidPng("scanline filter"));
    }
    Ok(PngInfo {
        width,
        height,
        decoded_bytes,
    })
}
