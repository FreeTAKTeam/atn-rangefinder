use super::{
    Adapter, BTreeMap, DECODER_VERSION, InboundFrame, OPERATIONS, ObservationRecord, Purpose,
    Value, decode, derive_draft, integer, json, parse_inbound,
};

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
                self.observations.push_back((id, record.clone()));
                if self.host_ready {
                    self.request(
                        Purpose::Measurement(id),
                        OPERATIONS[1],
                        json!({}),
                        now,
                        actions,
                    )?;
                }
                Ok(
                    json!({"id":id,"observation":record,"mapStatus":if self.host_ready {"observer requested"} else {"map contract unavailable"}}),
                )
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

    pub(super) fn publish_draft(
        &mut self,
        id: u64,
        now: u64,
        actions: &mut Vec<Value>,
    ) -> Result<(), String> {
        let Some((_, record)) = self.observations.iter().find(|(key, _)| *key == id) else {
            actions
                .push(json!({"kind":"status","message":"observation evicted before derivation"}));
            return Ok(());
        };
        let c = self.config.as_ref().ok_or("configuration required")?;
        // A configured profile is evidence provided by the operator, not a
        // claim that static codec validation established firmware semantics.
        if !c.device_profile.protocol_verified {
            actions.push(json!({"kind":"status","message":"raw observation retained; device profile unverified"}));
            return Ok(());
        }
        match derive_draft(
            record,
            self.observer.as_ref(),
            &c.semantics,
            &c.eligibility,
            now,
        ) {
            Ok(draft) => self.request(
                Purpose::Draft,
                OPERATIONS[4],
                json!({"observationId":format!("{}/{}",record.source_session,id),"draft":draft}),
                now,
                actions,
            ),
            Err(error) => {
                actions.push(json!({"kind":"status","message":format!("raw observation retained; derivation blocked: {error}")}));
                Ok(())
            }
        }
    }
}
