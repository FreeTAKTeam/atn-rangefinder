//! Generated non-geographic RGBA pixels and malformed derivatives only.
use atn_protocol::*;
use flate2::{Compression, write::ZlibEncoder};
use std::io::Write;

const FIXTURE: &[u8] = include_bytes!("../../../atn-rangefinder-docs/fixtures/synthetic-icon.png");

fn chunk(kind: &[u8], data: &[u8]) -> Vec<u8> {
    let mut bytes = u32::try_from(data.len()).unwrap().to_be_bytes().to_vec();
    bytes.extend_from_slice(kind);
    bytes.extend_from_slice(data);
    let mut crc = crc32fast::Hasher::new();
    crc.update(kind);
    crc.update(data);
    bytes.extend_from_slice(&crc.finalize().to_be_bytes());
    bytes
}

fn image(width: u32, height: u32, filter: u8, raw_len: usize) -> Vec<Vec<u8>> {
    let mut ihdr = width.to_be_bytes().to_vec();
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    let mut raw = vec![17; raw_len];
    let stride = usize::try_from(width).unwrap() * 4 + 1;
    for index in (0..raw.len()).step_by(stride) {
        raw[index] = filter;
    }
    let mut zlib = ZlibEncoder::new(Vec::new(), Compression::default());
    zlib.write_all(&raw).unwrap();
    vec![
        chunk(b"IHDR", &ihdr),
        chunk(b"IDAT", &zlib.finish().unwrap()),
        chunk(b"IEND", &[]),
    ]
}

fn png(chunks: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    for chunk in chunks {
        bytes.extend_from_slice(chunk);
    }
    bytes
}

fn idat_data(chunks: &[Vec<u8>]) -> Vec<u8> {
    chunks[1][8..chunks[1].len() - 4].to_vec()
}

#[test]
fn validates_fixture_and_all_png_filters() {
    assert_eq!(
        validate_png(FIXTURE, PngLimits::default()).unwrap(),
        PngInfo {
            width: 40,
            height: 40,
            decoded_bytes: 6440
        }
    );
    for filter in 0..=4 {
        validate_png(&png(&image(40, 40, filter, 6440)), PngLimits::default()).unwrap();
    }
}

#[test]
fn configurable_limits_are_explicit_and_serde_rejects_unknown_fields() {
    let small = png(&image(1, 1, 0, 5));
    assert!(validate_png(&small, PngLimits::default()).is_err());
    let limits = PngLimits {
        expected_dimensions: Some((1, 1)),
        ..PngLimits::default()
    };
    validate_png(&small, limits).unwrap();
    let value = serde_json::to_value(limits).unwrap();
    assert!(value.get("maxEncodedBytes").is_some());
    assert_eq!(
        serde_json::from_value::<PngLimits>(value.clone()).unwrap(),
        limits
    );
    let mut invalid = value;
    invalid["unknown"] = serde_json::json!(true);
    assert!(serde_json::from_value::<PngLimits>(invalid).is_err());
}

#[test]
fn corrupted_signature_crc_and_all_fixture_truncations_are_rejected() {
    let mut broken = FIXTURE.to_vec();
    broken[0] ^= 1;
    assert!(validate_png(&broken, PngLimits::default()).is_err());
    let mut broken = FIXTURE.to_vec();
    broken[45] ^= 1;
    assert_eq!(
        validate_png(&broken, PngLimits::default()),
        Err(ProtocolError::InvalidPng("chunk CRC"))
    );
    for length in 0..FIXTURE.len() {
        assert!(
            validate_png(&FIXTURE[..length], PngLimits::default()).is_err(),
            "accepted length {length}"
        );
    }
}

#[test]
fn image_size_and_allocation_limits_precede_decompression() {
    for (width, height) in [(0, 40), (40, 0), (41, 40), (40, 41), (u32::MAX, u32::MAX)] {
        let mut chunks = image(40, 40, 0, 6440);
        let mut ihdr = width.to_be_bytes().to_vec();
        ihdr.extend_from_slice(&height.to_be_bytes());
        ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
        chunks[0] = chunk(b"IHDR", &ihdr);
        assert!(validate_png(&png(&chunks), PngLimits::default()).is_err());
    }
    for limits in [
        PngLimits {
            max_encoded_bytes: FIXTURE.len() - 1,
            ..PngLimits::default()
        },
        PngLimits {
            max_pixels: 1599,
            ..PngLimits::default()
        },
        PngLimits {
            max_decoded_bytes: 6439,
            ..PngLimits::default()
        },
        PngLimits {
            max_chunks: 2,
            ..PngLimits::default()
        },
    ] {
        assert!(matches!(
            validate_png(FIXTURE, limits),
            Err(ProtocolError::LimitExceeded(_))
        ));
    }
}

#[test]
fn rejects_decode_bombs_bad_filters_and_wrong_scanline_counts() {
    for raw_len in [0, 6439, 6441, 1_000_000] {
        assert!(validate_png(&png(&image(40, 40, 0, raw_len)), PngLimits::default()).is_err());
    }
    assert_eq!(
        validate_png(&png(&image(40, 40, 5, 6440)), PngLimits::default()),
        Err(ProtocolError::InvalidPng("scanline filter"))
    );
}

#[test]
fn rejects_incomplete_checksum_and_extra_zlib_streams() {
    let original = image(40, 40, 0, 6440);
    let compressed = idat_data(&original);
    for removed in 1..=6 {
        let mut chunks = original.clone();
        chunks[1] = chunk(b"IDAT", &compressed[..compressed.len() - removed]);
        assert!(validate_png(&png(&chunks), PngLimits::default()).is_err());
    }
    let mut chunks = original.clone();
    let mut tail = compressed.clone();
    tail.extend_from_slice(&compressed);
    chunks[1] = chunk(b"IDAT", &tail);
    assert_eq!(
        validate_png(&png(&chunks), PngLimits::default()),
        Err(ProtocolError::InvalidPng("trailing compressed bytes"))
    );
    let mut chunks = original;
    let mut broken = compressed;
    let last = broken.len() - 1;
    broken[last] ^= 1;
    chunks[1] = chunk(b"IDAT", &broken);
    assert!(validate_png(&png(&chunks), PngLimits::default()).is_err());
}

#[test]
fn chunk_structure_order_and_supported_format_are_enforced() {
    let original = image(40, 40, 0, 6440);
    let mut duplicate = original.clone();
    duplicate.insert(1, original[0].clone());
    let mut critical = original.clone();
    critical.insert(1, chunk(b"ABCD", &[1]));
    let mut invalid_name = original.clone();
    invalid_name.insert(1, chunk(b"abca", &[1]));
    let mut malformed_iend = original.clone();
    malformed_iend[2] = chunk(b"IEND", &[1]);
    let mut missing_idat = original.clone();
    missing_idat.remove(1);
    let mut missing_iend = original.clone();
    missing_iend.remove(2);
    let mut incorrect_order = original.clone();
    incorrect_order.swap(0, 1);
    let mut nonconsecutive = original.clone();
    nonconsecutive.insert(2, chunk(b"tEXt", b"original"));
    nonconsecutive.insert(3, chunk(b"IDAT", &[]));
    let mut short_ihdr = original.clone();
    short_ihdr[0] = chunk(b"IHDR", &[0; 12]);
    for chunks in [
        duplicate,
        critical,
        invalid_name,
        malformed_iend,
        missing_idat,
        missing_iend,
        incorrect_order,
        nonconsecutive,
        short_ihdr,
    ] {
        assert!(validate_png(&png(&chunks), PngLimits::default()).is_err());
    }
    for (index, unsupported) in [(8, 16), (9, 2), (10, 1), (11, 1), (12, 1)] {
        let mut chunks = original.clone();
        let mut ihdr = chunks[0][8..21].to_vec();
        ihdr[index] = unsupported;
        chunks[0] = chunk(b"IHDR", &ihdr);
        assert!(validate_png(&png(&chunks), PngLimits::default()).is_err());
    }
    let mut trailing = png(&original);
    trailing.push(0);
    assert!(validate_png(&trailing, PngLimits::default()).is_err());
}

#[test]
fn consecutive_idat_and_ancillary_metadata_preserve_valid_image() {
    let original = image(40, 40, 0, 6440);
    let compressed = idat_data(&original);
    let midpoint = compressed.len() / 2;
    let chunks = vec![
        original[0].clone(),
        chunk(b"tEXt", b"label\0synthetic"),
        chunk(b"IDAT", &compressed[..midpoint]),
        chunk(b"IDAT", &compressed[midpoint..]),
        original[2].clone(),
    ];
    validate_png(&png(&chunks), PngLimits::default()).unwrap();
}
