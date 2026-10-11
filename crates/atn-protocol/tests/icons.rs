//! Original synthetic PNG and opaque payloads only.
use atn_protocol::*;
use serde_json::Value;

const PNG: &[u8] = include_bytes!("../../../atn-rangefinder-docs/fixtures/synthetic-icon.png");

fn hex(input: &str) -> Vec<u8> {
    input
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

fn put_u16(frame: &mut [u8], offset: usize, value: u16) {
    frame[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

#[test]
fn fixture_icon_is_byte_exact_through_encoding_and_reverse_reassembly() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../atn-rangefinder-docs/fixtures/icon-transfer.json"
    ))
    .unwrap();
    let frames = fragment_png(PNG, 2, 509, PngLimits::default()).unwrap();
    let expected: Vec<_> = fixture["frames"]
        .as_array()
        .unwrap()
        .iter()
        .map(|frame| hex(frame["att_value_hex"].as_str().unwrap()))
        .collect();
    assert_eq!(frames, expected);
    let mut reassembly = IconReassembler::new(ReassemblyLimits::default());
    for frame in frames.iter().rev() {
        reassembly.push(frame).unwrap();
    }
    assert_eq!(
        reassembly.push(&frames[0]).unwrap(),
        ReassemblyProgress::Duplicate { frame_number: 1 }
    );
    assert_eq!(reassembly.finish_png(PngLimits::default()).unwrap(), PNG);
}

#[test]
fn boundaries_preserve_every_byte_and_actual_write_capacity() {
    for capacity in [15, 20, 64, 509, 514] {
        let payload_capacity = capacity - ICON_HEADER_BYTES;
        for length in [
            0,
            1,
            payload_capacity - 1,
            payload_capacity,
            payload_capacity + 1,
            payload_capacity * 2,
            payload_capacity * 2 + 1,
        ] {
            let bytes: Vec<_> = (0..length).map(|index| index.to_le_bytes()[0]).collect();
            let frames = fragment_payload(&bytes, 127, capacity).unwrap();
            assert_eq!(frames.len(), length.div_ceil(payload_capacity));
            if length == 0 {
                assert!(frames.is_empty());
                continue;
            }
            let mut reassembly = IconReassembler::new(ReassemblyLimits::default());
            for frame in &frames {
                assert!(frame.len() <= capacity);
                assert!(!IconFragment::parse(frame).unwrap().payload.is_empty());
                reassembly.push(frame).unwrap();
            }
            assert_eq!(reassembly.finish().unwrap(), bytes);
        }
    }
    assert!(fragment_payload(&[1], 0, 14).is_err());
    assert!(fragment_payload(&[1], 0, 0).is_err());
    assert!(fragment_payload(&vec![0; 65536], 0, 509).is_err());
}

#[test]
fn incomplete_and_duplicate_policy_is_explicit() {
    let frames = fragment_payload(&[1; 30], 2, 34).unwrap();
    let mut reassembly = IconReassembler::new(ReassemblyLimits::default());
    reassembly.push(&frames[0]).unwrap();
    let mut conflicting = frames[0].clone();
    conflicting[14] = 3;
    assert_eq!(
        reassembly.push(&conflicting),
        Err(ProtocolError::ConflictingDuplicate(1))
    );
    assert_eq!(
        reassembly.push(&frames[0]).unwrap(),
        ReassemblyProgress::Duplicate { frame_number: 1 }
    );
    assert_eq!(
        reassembly.finish(),
        Err(ProtocolError::IncompleteTransfer {
            expected: 2,
            received: 1
        })
    );
    assert!(matches!(
        IconReassembler::new(ReassemblyLimits::default()).finish(),
        Err(ProtocolError::IncompleteTransfer { .. })
    ));
}

#[test]
fn header_inconsistency_cannot_mix_transfers() {
    let frames = fragment_payload(&[1; 60], 2, 34).unwrap();
    for (offset, value) in [(2, 4), (4, 61), (10, 3)] {
        let mut reassembly = IconReassembler::new(ReassemblyLimits::default());
        reassembly.push(&frames[0]).unwrap();
        let mut changed = frames[1].clone();
        put_u16(&mut changed, offset, value);
        assert_eq!(
            reassembly.push(&changed),
            Err(ProtocolError::InconsistentTransfer)
        );
        for frame in &frames[1..] {
            reassembly.push(frame).unwrap();
        }
        assert_eq!(reassembly.finish().unwrap(), vec![1; 60]);
    }
}

#[test]
fn malformed_icon_headers_and_truncation_are_rejected() {
    let frame = fragment_payload(&[1; 10], 2, 509).unwrap().remove(0);
    for length in 0..frame.len() {
        assert!(IconFragment::parse(&frame[..length]).is_err());
    }
    for (offset, value) in [
        (2, 0),
        (2, 11),
        (4, 0),
        (4, 9),
        (6, 0),
        (6, 2),
        (8, 0),
        (8, 11),
        (12, 1),
    ] {
        let mut changed = frame.clone();
        put_u16(&mut changed, offset, value);
        assert!(
            IconFragment::parse(&changed).is_err(),
            "offset {offset}, value {value}"
        );
    }
    let mut signature = frame.clone();
    signature[0] = 2;
    assert!(IconFragment::parse(&signature).is_err());
    let mut command = frame;
    command[1] = 2;
    assert!(IconFragment::parse(&command).is_err());
}

#[test]
fn allocation_and_final_byte_count_are_bounded() {
    let frames = fragment_payload(&[1; 30], 2, 34).unwrap();
    for limits in [
        ReassemblyLimits {
            max_total_bytes: 29,
            ..ReassemblyLimits::default()
        },
        ReassemblyLimits {
            max_frames: 1,
            ..ReassemblyLimits::default()
        },
        ReassemblyLimits {
            max_frame_bytes: 33,
            ..ReassemblyLimits::default()
        },
    ] {
        assert!(IconReassembler::new(limits).push(&frames[0]).is_err());
    }
    for declared_total in [29, 31, 65535] {
        let mut reassembly = IconReassembler::new(ReassemblyLimits::default());
        let mut first = frames[0].clone();
        put_u16(&mut first, 4, declared_total);
        let mut last = frames[1].clone();
        put_u16(&mut last, 4, declared_total);
        reassembly.push(&first).unwrap();
        assert!(reassembly.push(&last).is_err());
    }
}

#[test]
fn opaque_bytes_are_never_accepted_as_png_icons() {
    assert!(fragment_png(&[1; 30], 2, 509, PngLimits::default()).is_err());
    let frames = fragment_payload(&[1; 30], 2, 509).unwrap();
    let mut reassembly = IconReassembler::new(ReassemblyLimits::default());
    reassembly.push(&frames[0]).unwrap();
    assert!(reassembly.finish_png(PngLimits::default()).is_err());
}
