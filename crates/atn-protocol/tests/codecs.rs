//! All inputs are original synthetic vectors, not captured traffic.
use atn_protocol::*;
use serde_json::Value;

fn hex(input: &str) -> Vec<u8> {
    assert_eq!(input.len() % 2, 0);
    input
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

fn vectors() -> Value {
    serde_json::from_str(include_str!(
        "../../../atn-rangefinder-docs/fixtures/codec-vectors.json"
    ))
    .unwrap()
}

#[test]
fn fixture_measurement_is_big_endian_and_retains_raw_signs() {
    let fixture = vectors();
    let frame = hex(fixture["measurement"]["att_value_hex"].as_str().unwrap());
    assert_eq!(
        parse_inbound(&frame).unwrap(),
        InboundFrame::Measurement(RawObservation {
            distance: 120,
            pitch: -5,
            compass: 210
        })
    );
    assert_eq!(
        parse_inbound(&[1, 1, 0x80, 0, 0x7f, 0xff, 0xff, 0xff]).unwrap(),
        InboundFrame::Measurement(RawObservation {
            distance: i16::MIN,
            pitch: i16::MAX,
            compass: -1
        })
    );
}

#[test]
fn fixture_marker_is_byte_exact_little_endian() {
    let fixture = vectors();
    let marker = DisplayMarker::new(2, 0, true, 4, 120, -5, 210).unwrap();
    assert_eq!(
        marker.encode().as_slice(),
        hex(fixture["display_marker"]["att_value_hex"].as_str().unwrap())
    );
    let boundary = DisplayMarker::new(
        127,
        1,
        false,
        7,
        i32::from(i16::MIN),
        i32::from(i16::MAX),
        -1,
    )
    .unwrap();
    assert_eq!(
        boundary.encode(),
        [1, 2, 255, 7, 0, 128, 255, 127, 255, 255]
    );
    assert_eq!(boundary.slot_id(), 127);
}

#[test]
fn numeric_overflow_is_rejected_before_narrowing() {
    for (slot, kind, color, distance, pitch, compass) in [
        (128, 0, 0, 0, 0, 0),
        (0, 2, 0, 0, 0, 0),
        (0, 0, 8, 0, 0, 0),
        (0, 0, 0, 32768, 0, 0),
        (0, 0, 0, -32769, 0, 0),
        (0, 0, 0, 0, i32::MAX, 0),
        (0, 0, 0, 0, 0, i32::MIN),
    ] {
        assert!(DisplayMarker::new(slot, kind, true, color, distance, pitch, compass).is_err());
    }
}

#[test]
fn replies_preserve_opaque_tails_and_signed_answers() {
    let fixture = vectors();
    let frame = hex(fixture["opaque_reply"]["att_value_hex"].as_str().unwrap());
    assert_eq!(
        parse_inbound(&frame).unwrap(),
        InboundFrame::Reply(OpaqueReply {
            command: 2,
            answer_signed: 1,
            opaque_tail: [0xa1, 0xb2, 0xc3]
        })
    );
    assert_eq!(
        parse_inbound(&[1, 7, 0x80, 4, 5, 6]).unwrap(),
        InboundFrame::Reply(OpaqueReply {
            command: 7,
            answer_signed: -128,
            opaque_tail: [4, 5, 6]
        })
    );
    for length in [2, 3, 4, 5, 7, 8] {
        let mut bytes = vec![0; length];
        bytes[0] = 1;
        bytes[1] = 2;
        assert!(parse_inbound(&bytes).is_err());
    }
}

#[test]
fn malformed_measurements_and_unknown_commands_are_distinct() {
    let fixture = vectors();
    for invalid in fixture["invalid_measurements"].as_array().unwrap() {
        let frame = hex(invalid["hex"].as_str().unwrap());
        if frame[1] == 5 {
            assert!(matches!(
                parse_inbound(&frame).unwrap(),
                InboundFrame::Unknown(_)
            ));
        } else {
            assert!(parse_inbound(&frame).is_err());
        }
    }
    assert_eq!(
        parse_inbound(&[1, 0x70, 0, 1]).unwrap(),
        InboundFrame::Unknown(UnknownFrame {
            command: 0x70,
            bytes: vec![1, 0x70, 0, 1]
        })
    );
    assert!(parse_inbound(&[]).is_err());
    assert!(parse_inbound(&[1]).is_err());
    assert!(parse_inbound(&vec![1; MAX_INBOUND_BYTES + 1]).is_err());
}

#[test]
fn deterministic_adversarial_inbound_never_panics() {
    let mut seed = 0x1234_5678u32;
    for length in 0..1024 {
        let mut bytes = Vec::with_capacity(length);
        for _ in 0..length {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            bytes.push(seed.to_le_bytes()[0]);
        }
        let _ = parse_inbound(&bytes);
        if length >= 2 {
            bytes[0] = 1;
            bytes[1] = 1;
            let _ = parse_inbound(&bytes);
        }
    }
}

#[test]
fn raw_observation_serde_has_no_physical_semantics() {
    let raw = RawObservation {
        distance: -1,
        pitch: -32768,
        compass: 32767,
    };
    let json = serde_json::to_string(&raw).unwrap();
    assert_eq!(serde_json::from_str::<RawObservation>(&json).unwrap(), raw);
}
