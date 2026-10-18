//! Minimal PNG encoder for device icons: 8-bit RGBA, no filtering, one IDAT.
//! Output is validated by [`crate::validate_png`] like any other icon.
use crate::ProtocolError;
use flate2::{Compression, write::ZlibEncoder};
use std::io::Write;

const SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

fn chunk(out: &mut Vec<u8>, kind: [u8; 4], data: &[u8]) -> Result<(), ProtocolError> {
    let length =
        u32::try_from(data.len()).map_err(|_| ProtocolError::LimitExceeded("PNG chunk"))?;
    out.extend_from_slice(&length.to_be_bytes());
    let mut crc = crc32fast::Hasher::new();
    crc.update(&kind);
    crc.update(data);
    out.extend_from_slice(&kind);
    out.extend_from_slice(data);
    out.extend_from_slice(&crc.finalize().to_be_bytes());
    Ok(())
}

/// Encodes `width`×`height` RGBA8 pixels (row-major, 4 bytes each).
///
/// # Errors
/// Rejects zero or mismatched dimensions.
pub fn encode_rgba_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, ProtocolError> {
    let row = usize::try_from(width)
        .ok()
        .and_then(|w| w.checked_mul(4))
        .ok_or(ProtocolError::LimitExceeded("PNG width"))?;
    let rows = usize::try_from(height).map_err(|_| ProtocolError::LimitExceeded("PNG height"))?;
    if row == 0 || rows == 0 || rows.checked_mul(row) != Some(rgba.len()) {
        return Err(ProtocolError::InvalidPng("pixel buffer size"));
    }
    let mut filtered = Vec::with_capacity(rgba.len() + rows);
    for line in rgba.chunks(row) {
        filtered.push(0); // filter type None
        filtered.extend_from_slice(line);
    }
    let mut zlib = ZlibEncoder::new(Vec::new(), Compression::best());
    zlib.write_all(&filtered)
        .map_err(|_| ProtocolError::InvalidPng("deflate"))?;
    let idat = zlib
        .finish()
        .map_err(|_| ProtocolError::InvalidPng("deflate"))?;
    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    header.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit RGBA, deflate, no filter, no interlace
    let mut out = SIGNATURE.to_vec();
    chunk(&mut out, *b"IHDR", &header)?;
    chunk(&mut out, *b"IDAT", &idat)?;
    chunk(&mut out, *b"IEND", &[])?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::encode_rgba_png;
    use crate::{PngLimits, validate_png};

    #[test]
    fn encoded_icon_validates_as_a_40_by_40_rgba_png() {
        let mut pixels = vec![0_u8; 40 * 40 * 4];
        for (i, px) in pixels.chunks_mut(4).enumerate() {
            px.copy_from_slice(&[u8::try_from(i % 251).unwrap(), 10, 200, 255]);
        }
        let png = encode_rgba_png(40, 40, &pixels).unwrap();
        let limits = PngLimits {
            expected_dimensions: Some((40, 40)),
            ..PngLimits::default()
        };
        let info = validate_png(&png, limits).unwrap();
        assert_eq!((info.width, info.height), (40, 40));
        assert!(encode_rgba_png(40, 40, &pixels[4..]).is_err());
        assert!(encode_rgba_png(0, 40, &[]).is_err());
    }
}
