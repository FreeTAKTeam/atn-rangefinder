//! Invented coordinates, calibration policies and values; no hardware evidence.
#![allow(clippy::float_cmp)] // Synthetic exact field/state expectations.
use std::collections::BTreeMap;

use atn_domain::*;
use atn_protocol::{DisplayMarker, OpaqueReply, PngLimits, RawObservation};

const NOW: u64 = 10_000;

fn semantics() -> SemanticsPolicy {
    SemanticsPolicy {
        evidence_id: "synthetic-test-calibration".to_owned(),
        calibration_confirmed: true,
        distance_meters_per_unit: 1.0,
        angle_degrees_per_unit: 1.0,
        range_kind: RangeKind::Slant,
        pitch_convention: PitchConvention::PositiveUp,
        bearing_reference: BearingReference::TrueNorth,
        datum: Datum::Wgs84,
        max_distance_m: 50_000.0,
        observation_bounds: raw_bounds(),
        display_bounds: raw_bounds(),
    }
}

fn raw_bounds() -> RawFieldBounds {
    RawFieldBounds {
        distance: IntegerBounds {
            min: 0,
            max: i16::MAX,
        },
        pitch: IntegerBounds { min: -90, max: 90 },
        compass: IntegerBounds { min: 0, max: 360 },
    }
}

#[test]
fn configured_receive_and_display_limits_reject_sentinels_and_have_no_defaults() {
    let mut policy = semantics();
    policy.observation_bounds.distance.max = 500;
    assert!(matches!(
        draft(&observation(501, 0, 0), &observer(), &policy),
        Err(DomainError::Invalid(
            "distance outside configured wire bounds"
        ))
    ));
    policy.observation_bounds = raw_bounds();
    policy.observation_bounds.compass = IntegerBounds {
        min: -180,
        max: 180,
    };
    let west = draft(&observation(100, 0, -90), &observer(), &policy).unwrap();
    assert_eq!(west.true_bearing_degrees, 270.0);
    policy.display_bounds.distance.max = 50;
    assert!(matches!(
        marker_for_ple(
            &ple(west.position),
            Some(&observer()),
            &policy,
            &eligibility(),
            &style(),
            MarkerPitchPolicy::HorizontalDisplay,
            NOW
        ),
        Err(DomainError::Invalid(
            "distance outside configured wire bounds"
        ))
    ));
    policy.display_bounds.pitch = IntegerBounds { min: 1, max: -1 };
    assert!(policy.validate_configuration().is_err());
    let mut json = serde_json::to_value(semantics()).unwrap();
    json.as_object_mut().unwrap().remove("observationBounds");
    assert!(serde_json::from_value::<SemanticsPolicy>(json).is_err());
}

fn eligibility() -> EligibilityPolicy {
    EligibilityPolicy {
        max_observer_age_ms: 1_000,
        max_observation_age_ms: 1_000,
        max_declination_age_ms: 1_000,
        max_horizontal_accuracy_m: 5.0,
    }
}

fn observer() -> ObserverFix {
    ObserverFix {
        position: GeographicPosition {
            latitude: 0.0,
            longitude: 0.0,
            altitude_m: Some(100.0),
        },
        captured_at_ms: NOW,
        horizontal_accuracy_m: 2.0,
        source: ObserverSource::VerifiedColocated,
        evidence_id: "synthetic-co-location".to_owned(),
    }
}

fn observation(distance: i16, pitch: i16, compass: i16) -> ObservationRecord {
    let mut raw_bytes = vec![1, 1];
    raw_bytes.extend(distance.to_be_bytes());
    raw_bytes.extend(pitch.to_be_bytes());
    raw_bytes.extend(compass.to_be_bytes());
    ObservationRecord {
        raw: RawObservation {
            distance,
            pitch,
            compass,
        },
        observed_at_ms: NOW,
        source_session: "synthetic-connection".to_owned(),
        decoder_version: "test-codec-v1".to_owned(),
        raw_bytes,
        metadata: BTreeMap::from([("source".to_owned(), "synthetic".to_owned())]),
    }
}

fn draft(
    obs: &ObservationRecord,
    fix: &ObserverFix,
    policy: &SemanticsPolicy,
) -> Result<DerivedDraft, DomainError> {
    derive_draft(obs, Some(fix), policy, &eligibility(), NOW)
}

fn style() -> MarkerStyle {
    MarkerStyle {
        slot_id: 3,
        marker_type: 0,
        color: 2,
        enabled: true,
    }
}

fn ple(position: GeographicPosition) -> PlePoint {
    PlePoint {
        id: "synthetic-ple".to_owned(),
        revision: "revision-1".to_owned(),
        position,
        metadata: serde_json::json!({"source":"synthetic"}),
    }
}

#[test]
fn east_equator_matches_wgs84_semi_major_axis_and_retains_provenance() {
    let obs = observation(1000, 0, 90);
    let fix = observer();
    let derived = draft(&obs, &fix, &semantics()).unwrap();
    let expected_longitude = (1000.0_f64 / 6_378_137.0).to_degrees();
    assert!((derived.position.latitude - 0.0).abs() < 1.0e-10);
    assert!((derived.position.longitude - expected_longitude).abs() < 1.0e-10);
    assert_eq!(derived.observation, obs);
    assert_eq!(derived.observer, fix);
    assert_eq!(derived.state, MapState::Draft);
    assert_eq!(derived.uncertainty.derived_accuracy_m, None);
    assert!(!derived.uncertainty.measurement_accuracy_known);
}

#[test]
fn slant_horizontal_and_inclination_sign_are_explicit() {
    let obs = observation(1000, 60, 0);
    let mut policy = semantics();
    let up = draft(&obs, &observer(), &policy).unwrap();
    assert!((up.horizontal_distance_m - 500.0).abs() < 1.0e-10);
    assert!(
        (up.position.altitude_m.unwrap() - (100.0 + 1000.0 * 60.0_f64.to_radians().sin())).abs()
            < 1.0e-10
    );
    policy.pitch_convention = PitchConvention::PositiveDown;
    let down = draft(&obs, &observer(), &policy).unwrap();
    assert_eq!(up.position.latitude, down.position.latitude);
    assert!(down.position.altitude_m.unwrap() < 100.0);
    policy.range_kind = RangeKind::Horizontal;
    assert_eq!(
        draft(&obs, &observer(), &policy)
            .unwrap()
            .horizontal_distance_m,
        1000.0
    );
}

#[test]
fn magnetic_declination_changes_bearing_and_inverse_restores_device_reference() {
    let mut policy = semantics();
    policy.bearing_reference = BearingReference::MagneticNorth {
        declination_degrees: 10.0,
        captured_at_ms: NOW,
        evidence_id: "synthetic-declination".to_owned(),
    };
    let derived = draft(&observation(500, 0, 80), &observer(), &policy).unwrap();
    assert_eq!(derived.true_bearing_degrees, 90.0);
    let encoded = marker_for_ple(
        &ple(derived.position),
        Some(&observer()),
        &policy,
        &eligibility(),
        &style(),
        MarkerPitchPolicy::HorizontalDisplay,
        NOW,
    )
    .unwrap();
    assert_eq!(encoded.distance(), 500);
    assert_eq!(encoded.compass(), 80);
}

#[test]
fn no_position_or_unverified_source_units_north_and_datum_block_mapping() {
    let obs = observation(100, 0, 0);
    assert!(matches!(
        derive_draft(&obs, None, &semantics(), &eligibility(), NOW),
        Err(DomainError::MissingObserver)
    ));
    let mut fix = observer();
    fix.source = ObserverSource::Unverified;
    assert!(matches!(
        draft(&obs, &fix, &semantics()),
        Err(DomainError::Unverified(_))
    ));
    let mut policy = semantics();
    policy.calibration_confirmed = false;
    assert!(policy.validate_configuration().is_ok());
    assert!(matches!(
        draft(&obs, &observer(), &policy),
        Err(DomainError::Unverified(_))
    ));
    policy.calibration_confirmed = true;
    policy.bearing_reference = BearingReference::Unverified;
    assert!(matches!(
        draft(&obs, &observer(), &policy),
        Err(DomainError::Unverified(_))
    ));
    policy.bearing_reference = BearingReference::TrueNorth;
    policy.datum = Datum::Unverified;
    assert!(matches!(
        draft(&obs, &observer(), &policy),
        Err(DomainError::Unverified(_))
    ));
}

#[test]
fn fix_observation_and_declination_freshness_accuracy_are_independent_gates() {
    let mut fix = observer();
    let mut obs = observation(100, 0, 0);
    fix.captured_at_ms = NOW - 1001;
    assert!(matches!(
        draft(&obs, &fix, &semantics()),
        Err(DomainError::Stale("observer"))
    ));
    fix.captured_at_ms = NOW + 1;
    assert!(matches!(
        draft(&obs, &fix, &semantics()),
        Err(DomainError::Future("observer"))
    ));
    fix.captured_at_ms = NOW;
    fix.horizontal_accuracy_m = 5.01;
    assert!(matches!(
        draft(&obs, &fix, &semantics()),
        Err(DomainError::InaccurateObserver)
    ));
    fix.horizontal_accuracy_m = 5.0;
    obs.observed_at_ms = NOW - 1001;
    assert!(matches!(
        draft(&obs, &fix, &semantics()),
        Err(DomainError::Stale("observation"))
    ));
    obs.observed_at_ms = NOW + 1;
    assert!(matches!(
        draft(&obs, &fix, &semantics()),
        Err(DomainError::Future("observation"))
    ));
    obs.observed_at_ms = NOW;
    let mut policy = semantics();
    policy.bearing_reference = BearingReference::MagneticNorth {
        declination_degrees: 0.0,
        captured_at_ms: NOW - 1001,
        evidence_id: "synthetic".to_owned(),
    };
    assert!(matches!(
        draft(&obs, &fix, &policy),
        Err(DomainError::Stale("declination"))
    ));
}

#[test]
fn nan_infinity_negative_ranges_and_invalid_coordinates_are_rejected() {
    let obs = observation(10, 0, 0);
    for scale in [f64::NAN, f64::INFINITY, 0.0, -1.0] {
        let mut policy = semantics();
        policy.distance_meters_per_unit = scale;
        assert!(draft(&obs, &observer(), &policy).is_err());
    }
    for latitude in [f64::NAN, f64::INFINITY, -90.01, 90.01] {
        let mut fix = observer();
        fix.position.latitude = latitude;
        assert!(draft(&obs, &fix, &semantics()).is_err());
    }
    for obs in [
        observation(-1, 0, 0),
        observation(10, 91, 0),
        observation(10, 0, -1),
        observation(10, 0, 361),
    ] {
        assert!(draft(&obs, &observer(), &semantics()).is_err());
    }
}

#[test]
fn retained_frame_cannot_disagree_with_typed_raw_observation() {
    let mut obs = observation(10, 0, 0);
    obs.raw.distance = 11;
    assert!(matches!(
        draft(&obs, &observer(), &semantics()),
        Err(DomainError::Invalid(_))
    ));
}

#[test]
fn outgoing_altitude_and_scaling_round_trip_without_overflow_or_wrapping() {
    let mut policy = semantics();
    policy.distance_meters_per_unit = 0.5;
    policy.angle_degrees_per_unit = 0.5;
    let derived = draft(&observation(2000, 60, 90), &observer(), &policy).unwrap();
    let encoded = marker_for_ple(
        &ple(derived.position),
        Some(&observer()),
        &policy,
        &eligibility(),
        &style(),
        MarkerPitchPolicy::UseAltitudes,
        NOW,
    )
    .unwrap();
    assert_eq!(encoded.distance(), 2000);
    assert_eq!(encoded.pitch(), 60);
    assert_eq!(encoded.compass(), 90);
    policy.angle_degrees_per_unit = 0.001;
    let east = GeographicPosition {
        latitude: 0.0,
        longitude: 0.001,
        altitude_m: None,
    };
    assert!(matches!(
        marker_for_ple(
            &ple(east),
            Some(&observer()),
            &policy,
            &eligibility(),
            &style(),
            MarkerPitchPolicy::HorizontalDisplay,
            NOW
        ),
        Err(DomainError::NumericOverflow("compass"))
    ));
}

#[test]
fn altitude_unknown_stays_unknown_and_altitude_mapping_requires_both() {
    let mut fix = observer();
    fix.position.altitude_m = None;
    let result = draft(&observation(100, 30, 0), &fix, &semantics()).unwrap();
    assert_eq!(result.position.altitude_m, None);
    assert!(
        marker_for_ple(
            &ple(result.position),
            Some(&fix),
            &semantics(),
            &eligibility(),
            &style(),
            MarkerPitchPolicy::UseAltitudes,
            NOW
        )
        .is_err()
    );
}

#[test]
fn dateline_normalizes_and_antipodal_inverse_fails_instead_of_inventing_bearing() {
    let mut fix = observer();
    fix.position.longitude = 179.999;
    let result = draft(&observation(1000, 0, 90), &fix, &semantics()).unwrap();
    assert!(result.position.longitude < -179.0);
    let antipode = GeographicPosition {
        latitude: 0.0,
        longitude: 180.0,
        altitude_m: None,
    };
    assert!(matches!(
        marker_for_ple(
            &ple(antipode),
            Some(&observer()),
            &semantics(),
            &eligibility(),
            &style(),
            MarkerPitchPolicy::HorizontalDisplay,
            NOW
        ),
        Err(DomainError::Geodesic)
    ));
}

#[test]
fn synthetic_midlatitude_high_latitude_and_dateline_relative_round_trips() {
    for (latitude, longitude) in [(45.0, 12.0), (-70.0, -170.0), (89.99, 179.0)] {
        let mut fix = observer();
        fix.position.latitude = latitude;
        fix.position.longitude = longitude;
        for compass in [0, 37, 123, 270] {
            for pitch in [-10, 0, 10] {
                let source = observation(10_000, pitch, compass);
                let result = draft(&source, &fix, &semantics()).unwrap();
                let marker = marker_for_ple(
                    &ple(result.position),
                    Some(&fix),
                    &semantics(),
                    &eligibility(),
                    &style(),
                    MarkerPitchPolicy::UseAltitudes,
                    NOW,
                )
                .unwrap();
                assert_eq!(marker.distance(), source.raw.distance);
                assert_eq!(marker.pitch(), source.raw.pitch);
                assert_eq!(marker.compass().rem_euclid(360), source.raw.compass);
            }
        }
    }
}

#[test]
fn repeated_observations_are_independent_drafts_and_json_policies_have_no_defaults() {
    let obs = observation(0, 0, 0);
    assert_eq!(
        draft(&obs, &observer(), &semantics()).unwrap(),
        draft(&obs, &observer(), &semantics()).unwrap()
    );
    assert!(serde_json::from_str::<SemanticsPolicy>("{}").is_err());
    let json = serde_json::to_value(observer()).unwrap();
    assert_eq!(json["source"], "verifiedColocated");
    assert!(json.get("capturedAtMs").is_some());
    assert_eq!(
        serde_json::from_value::<ObserverFix>(json).unwrap(),
        observer()
    );
}

fn config() -> SessionConfig {
    SessionConfig {
        session_id: "synthetic-session".to_owned(),
        selected: vec![
            SelectedPle {
                ple_id: "selected".to_owned(),
                slot_id: 3,
            },
            SelectedPle {
                ple_id: "other".to_owned(),
                slot_id: 4,
            },
        ],
        queue_capacity: 4,
        max_queued_bytes: 10_000,
        queue_ttl_ms: 1000,
        operation_timeout_ms: 500,
    }
}

fn marker(slot: u16) -> DisplayMarker {
    DisplayMarker::new(slot, 0, true, 2, 100, 0, 90).unwrap()
}
fn reply(command: u8) -> OpaqueReply {
    OpaqueReply {
        command,
        answer_signed: 3,
        opaque_tail: [8, 9, 10],
    }
}

#[test]
fn session_rejects_empty_duplicate_selection_duplicate_slots_and_deadline_overflow() {
    for config in [
        SessionConfig {
            selected: vec![],
            ..config()
        },
        SessionConfig {
            queue_capacity: 0,
            ..config()
        },
        SessionConfig {
            selected: vec![
                SelectedPle {
                    ple_id: "a".to_owned(),
                    slot_id: 1,
                },
                SelectedPle {
                    ple_id: "a".to_owned(),
                    slot_id: 2,
                },
            ],
            ..config()
        },
        SessionConfig {
            selected: vec![
                SelectedPle {
                    ple_id: "a".to_owned(),
                    slot_id: 1,
                },
                SelectedPle {
                    ple_id: "b".to_owned(),
                    slot_id: 1,
                },
            ],
            ..config()
        },
    ] {
        assert!(SyncSession::start(config, NOW).is_err());
    }
    assert!(SyncSession::start(config(), u64::MAX).is_err());
}

#[test]
fn selection_and_owned_slots_gate_every_update_without_expanding_scope() {
    let mut session = SyncSession::start(config(), NOW).unwrap();
    assert!(matches!(
        session.enqueue_position("unselected", &marker(3), NOW),
        Err(DomainError::NotSelected)
    ));
    assert!(matches!(
        session.enqueue_position("selected", &marker(4), NOW),
        Err(DomainError::SlotOwnership)
    ));
    session
        .enqueue_position("selected", &marker(3), NOW)
        .unwrap();
    session
        .enqueue_position("selected", &marker(3), NOW)
        .unwrap();
    assert_eq!(session.snapshot().selected.len(), 2);
    assert_eq!(session.snapshot().queued_operations, 1);
    assert_eq!(
        session.snapshot().completed[0].cancelled.as_deref(),
        Some("superseded by newer position")
    );
}

#[test]
fn latest_unsent_position_replaces_old_geometry_without_touching_active_or_icon() {
    let png = include_bytes!("../../../atn-rangefinder-docs/fixtures/synthetic-icon.png");
    let mut session = SyncSession::start(config(), NOW).unwrap();
    let active_id = session.enqueue_position("other", &marker(4), NOW).unwrap();
    session.next_write(NOW).unwrap().unwrap();
    let old_id = session
        .enqueue_position("selected", &marker(3), NOW)
        .unwrap();
    let icon_id = session
        .enqueue_icon("selected", png, 512, PngLimits::default(), NOW)
        .unwrap();
    let updated = DisplayMarker::new(3, 0, true, 4, 250, 0, 180).unwrap();
    let new_id = session
        .enqueue_position("selected", &updated, NOW + 10)
        .unwrap();
    assert_ne!(old_id, new_id);
    assert_eq!(session.snapshot().active.unwrap().operation_id, active_id);
    assert_eq!(session.snapshot().queued_operations, 2);
    assert_eq!(session.snapshot().completed[0].operation_id, old_id);
    assert_eq!(
        session.snapshot().completed[0].cancelled.as_deref(),
        Some("superseded by newer position")
    );
    session.transport_completed(active_id, 1, NOW + 10).unwrap();
    session.observe_reply(reply(2), NOW + 10).unwrap();
    let latest = session.next_write(NOW + 10).unwrap().unwrap();
    assert_eq!(latest.operation_id, new_id);
    assert_eq!(latest.bytes, updated.encode());
    session.transport_completed(new_id, 1, NOW + 10).unwrap();
    session.observe_reply(reply(2), NOW + 10).unwrap();
    assert_eq!(
        session.next_write(NOW + 10).unwrap().unwrap().operation_id,
        icon_id
    );
}

#[test]
fn replacement_respects_queue_bound_at_capacity_and_never_replaces_submitted_position() {
    let mut session = SyncSession::start(
        SessionConfig {
            queue_capacity: 1,
            max_queued_bytes: 10,
            ..config()
        },
        NOW,
    )
    .unwrap();
    let old = session
        .enqueue_position("selected", &marker(3), NOW)
        .unwrap();
    let new = session
        .enqueue_position("selected", &marker(3), NOW + 1)
        .unwrap();
    assert_ne!(old, new);
    assert_eq!(session.snapshot().queued_bytes, 10);
    let frame = session.next_write(NOW + 1).unwrap().unwrap();
    assert_eq!(frame.operation_id, new);
    assert!(matches!(
        session.enqueue_position("selected", &marker(3), NOW + 1),
        Err(DomainError::QueueFull)
    ));
    assert_eq!(session.snapshot().active.unwrap().operation_id, new);
}

#[test]
fn transport_opaque_reply_and_display_confirmation_are_independent() {
    let mut session = SyncSession::start(config(), NOW).unwrap();
    let operation = session
        .enqueue_position("selected", &marker(3), NOW)
        .unwrap();
    session.enqueue_position("other", &marker(4), NOW).unwrap();
    let frame = session.next_write(NOW).unwrap().unwrap();
    assert_eq!(frame.operation_id, operation);
    assert_eq!(frame.frame_number, 1);
    assert!(session.next_write(NOW).unwrap().is_none());
    assert!(matches!(
        session.transport_completed(operation, 2, NOW),
        Err(DomainError::TransferMismatch)
    ));
    let transport = session.transport_completed(operation, 1, NOW).unwrap();
    assert!(transport.transport_complete);
    assert_eq!(transport.reply, None);
    assert_eq!(transport.display_confirmation, None);
    assert!(session.next_write(NOW).unwrap().is_none());
    assert!(matches!(
        session.observe_reply(reply(7), NOW),
        Err(DomainError::UnexpectedReply)
    ));
    let receipt = session.observe_reply(reply(2), NOW).unwrap();
    assert_eq!(receipt.reply.unwrap().opaque_tail, [8, 9, 10]);
    assert_eq!(receipt.display_confirmation, None);
    let observed = session
        .confirm_display(operation, "synthetic-user-rendering-evidence".to_owned())
        .unwrap();
    assert!(observed.display_confirmation.is_some());
    assert_eq!(session.next_write(NOW).unwrap().unwrap().ple_id, "other");
}

#[test]
fn early_final_reply_is_retained_without_claiming_transport_or_display_completion() {
    let mut session = SyncSession::start(config(), NOW).unwrap();
    let operation = session
        .enqueue_position("selected", &marker(3), NOW)
        .unwrap();
    assert!(matches!(
        session.observe_reply(reply(2), NOW),
        Err(DomainError::UnexpectedReply)
    ));
    session.next_write(NOW).unwrap().unwrap();
    let receipt = session.observe_reply(reply(2), NOW).unwrap();
    assert!(!receipt.transport_complete);
    assert!(receipt.reply.is_some());
    assert_eq!(receipt.display_confirmation, None);
    let completed = session.transport_completed(operation, 1, NOW).unwrap();
    assert!(completed.transport_complete);
    assert!(completed.reply.is_some());
    assert_eq!(session.snapshot().completed.len(), 1);
}

#[test]
fn icon_frames_are_serialized_and_do_not_expect_per_fragment_acknowledgement() {
    let png = include_bytes!("../../../atn-rangefinder-docs/fixtures/synthetic-icon.png");
    let mut session = SyncSession::start(config(), NOW).unwrap();
    let operation = session
        .enqueue_icon("selected", png, 64, PngLimits::default(), NOW)
        .unwrap();
    let mut count = 0;
    loop {
        let Some(frame) = session.next_write(NOW).unwrap() else {
            break;
        };
        count += 1;
        assert_eq!(frame.frame_number, count);
        assert!(frame.bytes.len() <= 64);
        if count == 1 {
            assert!(matches!(
                session.observe_reply(reply(7), NOW),
                Err(DomainError::TransportPending)
            ));
        }
        let completed = session
            .transport_completed(operation, frame.frame_number, NOW)
            .unwrap();
        if completed.transport_complete {
            break;
        }
    }
    assert!(count > 1);
    assert!(
        session
            .observe_reply(reply(7), NOW)
            .unwrap()
            .transport_complete
    );
}

#[test]
fn partial_icon_timeout_cancels_all_queued_operations_without_retry() {
    let png = include_bytes!("../../../atn-rangefinder-docs/fixtures/synthetic-icon.png");
    let mut session = SyncSession::start(config(), NOW).unwrap();
    let operation = session
        .enqueue_icon("selected", png, 64, PngLimits::default(), NOW)
        .unwrap();
    session.enqueue_position("other", &marker(4), NOW).unwrap();
    let first = session.next_write(NOW).unwrap().unwrap();
    session
        .transport_completed(operation, first.frame_number, NOW)
        .unwrap();
    assert!(matches!(
        session.tick(NOW + 500),
        Err(DomainError::TimedOut)
    ));
    assert_eq!(session.snapshot().queued_operations, 0);
    assert_eq!(session.snapshot().queued_bytes, 0);
    assert!(
        session
            .snapshot()
            .completed
            .iter()
            .all(|report| report.cancelled.is_some())
    );
    assert!(matches!(
        session.next_write(NOW + 501),
        Err(DomainError::SessionStopped)
    ));
}

#[test]
fn ttl_capacity_bytes_stop_disconnect_and_clock_rollback_are_bounded() {
    let mut session = SyncSession::start(
        SessionConfig {
            queue_capacity: 1,
            ..config()
        },
        NOW,
    )
    .unwrap();
    session
        .enqueue_position("selected", &marker(3), NOW)
        .unwrap();
    assert!(matches!(
        session.enqueue_position("other", &marker(4), NOW),
        Err(DomainError::QueueFull)
    ));
    assert!(matches!(
        session.tick(NOW + 1000),
        Err(DomainError::TimedOut)
    ));
    let mut session = SyncSession::start(
        SessionConfig {
            max_queued_bytes: 9,
            ..config()
        },
        NOW,
    )
    .unwrap();
    assert!(matches!(
        session.enqueue_position("selected", &marker(3), NOW),
        Err(DomainError::QueueFull)
    ));
    let mut session = SyncSession::start(config(), NOW).unwrap();
    session
        .enqueue_position("selected", &marker(3), NOW)
        .unwrap();
    session.disconnect();
    assert_eq!(session.snapshot().queued_bytes, 0);
    assert!(matches!(
        session.enqueue_position("selected", &marker(3), NOW),
        Err(DomainError::SessionStopped)
    ));
    // Reconnection alone has no resume method. A new instance has no replay queue.
    let mut new_session = SyncSession::start(config(), NOW).unwrap();
    assert!(new_session.next_write(NOW).unwrap().is_none());
    assert!(new_session.tick(NOW - 1).is_err());
    assert!(matches!(
        new_session.snapshot().state,
        SessionState::Stopped { .. }
    ));
}

#[test]
fn selected_removal_cancels_its_updates_and_never_clears_device_markers() {
    let mut session = SyncSession::start(config(), NOW).unwrap();
    session
        .enqueue_position("selected", &marker(3), NOW)
        .unwrap();
    session.enqueue_position("other", &marker(4), NOW).unwrap();
    let removed = session.remove_selected("selected").unwrap();
    assert_eq!(removed.slot_id, 3);
    assert!(!removed.device_marker_cleared);
    assert_eq!(session.next_write(NOW).unwrap().unwrap().ple_id, "other");
    assert!(matches!(
        session.enqueue_position("selected", &marker(3), NOW),
        Err(DomainError::NotSelected)
    ));
    let mut session = SyncSession::start(config(), NOW).unwrap();
    session
        .enqueue_position("selected", &marker(3), NOW)
        .unwrap();
    session.next_write(NOW).unwrap().unwrap();
    session.remove_selected("selected").unwrap();
    assert!(matches!(
        session.snapshot().state,
        SessionState::Stopped { .. }
    ));
}
