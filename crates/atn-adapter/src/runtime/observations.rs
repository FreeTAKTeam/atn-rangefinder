use super::{
    Adapter, BTreeMap, CreateAttempt, DECODER_VERSION, HOST_NAME_LIMIT, HOST_NOTES_LIMIT,
    HostOutcome, InboundFrame, NOTIFICATION_UUID, OP_MARKERS_CREATE, OP_OBSERVER_SNAPSHOT,
    ObservationRecord, Purpose, Resolution, StoredObservation, Value, decode, derive_draft,
    gatt_uuid, integer, json, parse_inbound,
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
        // Measurements and replies are decoded only from the ATN notification
        // characteristic, never from write completions or other channels.
        if !gatt_uuid(v, "characteristic", NOTIFICATION_UUID) {
            return Ok(json!({"ignored":"not the ATN notification characteristic"}));
        }
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
                // Only measurements arriving while capture is active on this
                // connection are ever offered to REM.
                let capture = self
                    .capture
                    .as_ref()
                    .filter(|capture| capture.connection_id == connection.0)
                    .map(|capture| capture.nonce.clone());
                let host = if capture.is_some() {
                    HostOutcome::ObserverRequested
                } else {
                    HostOutcome::Retained {
                        reason: "capture not started".into(),
                    }
                };
                let capturing = capture.is_some();
                self.observations.push_back(StoredObservation {
                    id,
                    record: record.clone(),
                    host: host.clone(),
                    capture_session: capture,
                    create: None,
                });
                if capturing {
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
            InboundFrame::Reply(reply) => self.route_reply(reply, now, actions),
            InboundFrame::Unknown(frame) => Ok(json!({"diagnostic":frame})),
        }
    }

    pub(super) fn set_outcome(&mut self, id: u64, outcome: HostOutcome) {
        if let Some(stored) = self.observations.iter_mut().find(|o| o.id == id) {
            stored.host = outcome;
        }
    }

    /// Records an indeterminate creation. Exactly one replay of the same key
    /// and payload is scheduled when the observation's capture session is
    /// still active in this Binder host session and it was not replayed yet.
    pub(super) fn creation_unknown(&mut self, id: u64, reason: String, now: u64) {
        let delay = self.config.as_ref().map(|c| c.create_retry_delay_ms);
        let capture = self.capture.as_ref().map(|c| c.nonce.clone());
        let host_ready = self.host_ready;
        let Some(stored) = self.observations.iter_mut().find(|o| o.id == id) else {
            return;
        };
        let same_capture = capture.is_some() && stored.capture_session == capture;
        let due = delay.and_then(|d| now.checked_add(d));
        let resolution = match stored.create.as_mut() {
            Some(attempt) if !attempt.replayed && same_capture && host_ready && due.is_some() => {
                attempt.replay_due_ms = due;
                Resolution::ReplayScheduled
            }
            _ => Resolution::Final,
        };
        stored.host = HostOutcome::Unknown { reason, resolution };
    }

    /// Withdraws every scheduled replay; their outcomes stay unknown.
    pub(super) fn finalize_replays(&mut self, why: &str) {
        for stored in &mut self.observations {
            if let HostOutcome::Unknown {
                reason,
                resolution: resolution @ Resolution::ReplayScheduled,
            } = &mut stored.host
            {
                *reason = format!("{reason}; replay withdrawn: {why}");
                *resolution = Resolution::Final;
                if let Some(attempt) = stored.create.as_mut() {
                    attempt.replay_due_ms = None;
                }
            }
        }
    }

    /// Sends due replays. REM answers a known key with the original marker
    /// (`created:false`) or creates it once; nothing is replayed twice.
    pub(super) fn replay_due(&mut self, now: u64, actions: &mut Vec<Value>) {
        let capture = self.capture.as_ref().map(|c| c.nonce.clone());
        let due: Vec<(u64, bool, Value)> = self
            .observations
            .iter_mut()
            .filter(|o| {
                matches!(
                    o.host,
                    HostOutcome::Unknown {
                        resolution: Resolution::ReplayScheduled,
                        ..
                    }
                )
            })
            .filter_map(|o| {
                let attempt = o.create.as_mut()?;
                if attempt.replay_due_ms.is_none_or(|due| due > now) {
                    return None;
                }
                attempt.replayed = true;
                attempt.replay_due_ms = None;
                let same = capture.is_some() && o.capture_session == capture;
                Some((o.id, same, attempt.payload.clone()))
            })
            .collect();
        for (id, same_capture, payload) in due {
            let outcome = if !same_capture || !self.host_ready {
                HostOutcome::Unknown {
                    reason: "replay withdrawn: capture or host session ended".into(),
                    resolution: Resolution::Final,
                }
            } else {
                match self.request(
                    Purpose::Create(id),
                    OP_MARKERS_CREATE,
                    payload,
                    now,
                    actions,
                ) {
                    Ok(()) => HostOutcome::ReplayRequested,
                    Err(error) => HostOutcome::Unknown {
                        reason: format!("replay not sent: {error}"),
                        resolution: Resolution::Final,
                    },
                }
            };
            self.set_outcome(id, outcome);
        }
    }

    /// Creates one ordinary shared REM marker for an eligible observation of
    /// the active capture. The idempotency key is
    /// `<captureNonce>/<connectionId>/<observation>`, so the host can never
    /// mint a second marker for the same observation, even across processes.
    pub(super) fn create_marker(
        &mut self,
        id: u64,
        now: u64,
        actions: &mut Vec<Value>,
    ) -> Result<(), String> {
        let Some((record, capture)) = self
            .observations
            .iter()
            .find(|o| o.id == id)
            .map(|o| (o.record.clone(), o.capture_session.clone()))
        else {
            actions
                .push(json!({"kind":"status","message":"observation evicted before derivation"}));
            return Ok(());
        };
        let Some(nonce) = capture.filter(|nonce| {
            self.capture
                .as_ref()
                .is_some_and(|active| &active.nonce == nonce)
        }) else {
            self.set_outcome(
                id,
                HostOutcome::Retained {
                    reason: "capture ended before creation".into(),
                },
            );
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
        let key = format!("{nonce}/{}/{id}", record.source_session);
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
        if let Some(stored) = self.observations.iter_mut().find(|o| o.id == id) {
            stored.host = HostOutcome::CreateRequested;
            stored.create = Some(CreateAttempt {
                payload: payload.clone(),
                replayed: false,
                replay_due_ms: None,
            });
        }
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
