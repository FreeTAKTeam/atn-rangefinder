use super::{
    Adapter, BTreeMap, DECODER_VERSION, HOST_NAME_LIMIT, HOST_NOTES_LIMIT, HostOutcome,
    InboundFrame, OP_MARKERS_CREATE, OP_OBSERVER_SNAPSHOT, ObservationRecord, Purpose,
    StoredObservation, Value, decode, derive_draft, integer, json, parse_inbound,
};
use atn_domain::{BearingReference, DerivedDraft};

impl Adapter {
    pub(super) fn notification(
        &mut self,
        v: &Value,
        now: u64,
        actions: &mut Vec<Value>,
    ) -> Result<Value, String> {
        let bytes: Vec<u8> = decode(v, "bytes")?;
        let connection = self
            .connection
            .as_ref()
            .ok_or("notification from disconnected transport")?;
        match parse_inbound(&bytes).map_err(|e| e.to_string())? {
            InboundFrame::Measurement(raw) => {
                let c = self
                    .config
                    .as_ref()
                    .ok_or("configure observation retention before connecting")?;
                let observed_at_ms = integer(v, "observedAtMs")?;
                if observed_at_ms > now {
                    return Err("observation timestamp is in future".into());
                }
                self.sequence = self
                    .sequence
                    .checked_add(1)
                    .ok_or("observation identity exhausted")?;
                let id = self.sequence;
                let record = ObservationRecord {
                    raw,
                    observed_at_ms,
                    source_session: connection.0.clone(),
                    decoder_version: DECODER_VERSION.into(),
                    raw_bytes: bytes,
                    metadata: BTreeMap::from([
                        ("model".into(), c.device_profile.model.clone()),
                        ("firmware".into(), c.device_profile.firmware.clone()),
                    ]),
                };
                while self.observations.len() >= c.max_observations {
                    self.observations.pop_front();
                }
                let host = if self.host_ready {
                    HostOutcome::ObserverRequested
                } else {
                    HostOutcome::Retained {
                        reason: "REM map API unavailable".into(),
                    }
                };
                self.observations.push_back(StoredObservation {
                    id,
                    record: record.clone(),
                    host: host.clone(),
                });
                if self.host_ready {
                    self.request(
                        Purpose::Measurement(id),
                        OP_OBSERVER_SNAPSHOT,
                        json!({}),
                        now,
                        actions,
                    )?;
                }
                Ok(json!({"id":id,"observation":record,"host":host}))
            }
            InboundFrame::Reply(reply) => {
                let Some(session) = self.session.as_mut() else {
                    return Ok(json!({"reply":reply,"associated":false}));
                };
                match session.observe_reply(reply.clone(), now) {
                    Ok(report) => {
                        self.pump(now, actions)?;
                        Ok(json!({"report":report}))
                    }
                    Err(
                        atn_domain::DomainError::UnexpectedReply
                        | atn_domain::DomainError::TransportPending
                        | atn_domain::DomainError::SessionStopped,
                    ) => Ok(json!({"reply":reply,"associated":false})),
                    Err(error) => Err(error.to_string()),
                }
            }
            InboundFrame::Unknown(frame) => Ok(json!({"diagnostic":frame})),
        }
    }

    pub(super) fn set_outcome(&mut self, id: u64, outcome: HostOutcome) {
        if let Some(stored) = self.observations.iter_mut().find(|o| o.id == id) {
            stored.host = outcome;
        }
    }

    /// Creates one ordinary shared REM marker for an eligible observation. The
    /// idempotency key is the connection-scoped observation identity, so the
    /// host can never mint a second marker for the same observation.
    pub(super) fn create_marker(
        &mut self,
        id: u64,
        now: u64,
        actions: &mut Vec<Value>,
    ) -> Result<(), String> {
        let Some(record) = self
            .observations
            .iter()
            .find(|o| o.id == id)
            .map(|o| o.record.clone())
        else {
            actions
                .push(json!({"kind":"status","message":"observation evicted before derivation"}));
            return Ok(());
        };
        let c = self.config.as_ref().ok_or("configuration required")?;
        // A configured profile is evidence provided by the operator, not a
        // claim that static codec validation established firmware semantics.
        if !c.device_profile.protocol_verified {
            self.set_outcome(
                id,
                HostOutcome::Retained {
                    reason: "device profile unverified".into(),
                },
            );
            return Ok(());
        }
        let draft = match derive_draft(
            &record,
            self.observer.as_ref(),
            &c.semantics,
            &c.eligibility,
            now,
        ) {
            Ok(draft) => draft,
            Err(error) => {
                self.set_outcome(
                    id,
                    HostOutcome::Retained {
                        reason: format!("derivation blocked: {error}"),
                    },
                );
                return Ok(());
            }
        };
        let key = format!("{}/{id}", record.source_session);
        if key.len() > 128 || !key.bytes().all(|b| b.is_ascii_graphic()) {
            self.set_outcome(
                id,
                HostOutcome::Retained {
                    reason: "observation identity is not a valid idempotency key".into(),
                },
            );
            return Ok(());
        }
        let name = format!("{} {}", c.host_marker.name_prefix.trim(), id);
        let notes = provenance_notes(&draft, self.colocation.as_deref().unwrap_or(""));
        if name.chars().count() > HOST_NAME_LIMIT || notes.chars().count() > HOST_NOTES_LIMIT {
            return Err("marker provenance exceeds REM text bounds".into());
        }
        let mut payload = json!({"idempotencyKey":key,"symbol":c.host_marker.symbol,
            "name":name,"lat":draft.position.latitude,"lon":draft.position.longitude,"notes":notes});
        if let Some(category) = &c.host_marker.category {
            payload["category"] = json!(category);
        }
        self.set_outcome(id, HostOutcome::CreateRequested);
        self.request(
            Purpose::Create(id),
            OP_MARKERS_CREATE,
            payload,
            now,
            actions,
        )
    }
}

/// Bounded provenance for REM's 1024-character `notes`. Values and units are
/// the configured ones; no hardware identifier or REM identity is included.
fn provenance_notes(draft: &DerivedDraft, colocation: &str) -> String {
    let raw = &draft.observation.raw;
    let semantics = &draft.semantics;
    let reference = match &semantics.bearing_reference {
        BearingReference::TrueNorth => "true north".to_owned(),
        BearingReference::MagneticNorth {
            declination_degrees,
            evidence_id,
            ..
        } => format!("magnetic north, declination {declination_degrees:.2} deg ({evidence_id})"),
        BearingReference::Unverified => "unverified".to_owned(),
    };
    let mut notes = format!(
        "ATN observation {}/{}. raw distance {} pitch {} compass {} ({:?} range, {} m/unit, {} deg/unit); measured at {} ms; \
horizontal distance {:.1} m, true bearing {:.2} deg; observer {:.6},{:.6} accuracy {:.1} m at {} ms; \
profile {} {} ({}); semantics {}; north {}; co-location {}; method {}; uncertainty: observer accuracy only",
        draft.observation.source_session,
        draft.observation.decoder_version,
        raw.distance,
        raw.pitch,
        raw.compass,
        semantics.range_kind,
        semantics.distance_meters_per_unit,
        semantics.angle_degrees_per_unit,
        draft.observation.observed_at_ms,
        draft.horizontal_distance_m,
        draft.true_bearing_degrees,
        draft.observer.position.latitude,
        draft.observer.position.longitude,
        draft.observer.horizontal_accuracy_m,
        draft.observer.captured_at_ms,
        draft
            .observation
            .metadata
            .get("model")
            .map_or("", String::as_str),
        draft
            .observation
            .metadata
            .get("firmware")
            .map_or("", String::as_str),
        semantics.evidence_id,
        semantics.evidence_id,
        reference,
        colocation,
        draft.projection_method,
    );
    if notes.chars().count() > HOST_NOTES_LIMIT {
        notes = notes.chars().take(HOST_NOTES_LIMIT).collect();
    }
    notes
}
