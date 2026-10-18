//! Automatic display of REM markers around this device. With the operator's
//! one-time REM grant (`mapNearbyRead`), the lifecycle tick asks REM for the
//! markers within the Binox's measuring range and shows the nearest ones that
//! fit the free display slots, without per-marker sharing. Any REM session
//! keeps its own slots; the automatic set uses what is left.
use super::{Adapter, OP_MARKERS_NEARBY, Purpose, RemMapObject, Value, decode, json};
use std::collections::BTreeSet;

/// REM's bound for one nearby query.
const NEARBY_LIMIT_MAX: usize = 50;
const NEARBY_RADIUS_MAX_M: f64 = 100_000.0;
const NEARBY_PREFIX: &str = "rem-nearby-";

impl Adapter {
    fn nearby_index(&self) -> Option<usize> {
        self.displays.iter().position(|d| d.nearby)
    }

    /// Slots the automatic set may use: the free ones plus its own.
    fn nearby_capacity(&self) -> usize {
        let own = self
            .nearby_index()
            .map_or(0, |i| self.displays[i].sync.snapshot().selected.len());
        (self.free_slots().len() + own).min(NEARBY_LIMIT_MAX)
    }

    /// Requests REM's nearby markers once per reconciliation interval while
    /// the Binox is connected, co-located and REM is ready.
    pub(super) fn nearby_tick(&mut self, now: u64, actions: &mut Vec<Value>) -> Result<(), String> {
        if self.nearby_paused
            || !self.host_ready
            || self.write_gate(now).is_err()
            || self.pending_purpose(|p| matches!(p, Purpose::Nearby))
        {
            return Ok(());
        }
        let c = self.config.as_ref().ok_or("configuration required")?;
        let interval = c.reconcile_interval_ms;
        let radius = c.semantics.max_distance_m.min(NEARBY_RADIUS_MAX_M);
        if self
            .last_nearby_ms
            .is_some_and(|last| now.saturating_sub(last) < interval)
        {
            return Ok(());
        }
        let limit = self.nearby_capacity();
        if limit == 0 {
            return Ok(());
        }
        self.last_nearby_ms = Some(now);
        self.request(
            Purpose::Nearby,
            OP_MARKERS_NEARBY,
            json!({"radiusM":radius,"limit":limit}),
            now,
            actions,
        )
    }

    /// Keeps the automatic set equal to REM's nearest markers: unchanged
    /// membership only updates positions (unchanged objects write nothing);
    /// changed membership replaces the set, releasing old slots without a
    /// device clear.
    pub(super) fn nearby_result(
        &mut self,
        result: &Value,
        now: u64,
        actions: &mut Vec<Value>,
    ) -> Result<(), String> {
        if result.get("available").and_then(Value::as_bool) != Some(true) {
            self.nearby_report(
                "REM has no position for this phone; turn on REM telemetry",
                actions,
            );
            return Ok(());
        }
        self.nearby_problem = None;
        if self.nearby_paused || self.write_gate(now).is_err() {
            return Ok(());
        }
        let mut objects: Vec<RemMapObject> = decode(result, "items")?;
        objects.truncate(self.nearby_capacity());
        let ids: Vec<String> = objects
            .iter()
            .map(|o| o.object_destination_hash.clone())
            .collect();
        if let Some(index) = self.nearby_index() {
            let id = self.displays[index].sync.session_id().to_owned();
            let current: BTreeSet<String> = self.displays[index]
                .sync
                .snapshot()
                .selected
                .into_iter()
                .map(|item| item.ple_id)
                .collect();
            if !ids.is_empty() && current == ids.iter().cloned().collect() {
                return self.apply_objects(&id, objects, now, actions);
            }
            self.end_display(
                &id,
                "nearby REM markers changed",
                false,
                false,
                now,
                actions,
            );
        }
        if ids.is_empty() {
            return Ok(());
        }
        self.nearby_sequence = self.nearby_sequence.saturating_add(1);
        let id = format!("{NEARBY_PREFIX}{}", self.nearby_sequence);
        if let Some(reason) = self.start_display(&id, &ids, true, false, now, actions)? {
            self.nearby_report(&format!("nearby REM markers not shown: {reason}"), actions);
            return Ok(());
        }
        self.apply_objects(&id, objects, now, actions)?;
        self.refresh_observer(now, actions)
    }

    /// A refused or failed nearby query never cancels other work.
    pub(super) fn nearby_failed(&mut self, code: &str, message: &str, actions: &mut Vec<Value>) {
        let text = if code == "PermissionDenied" {
            "To show REM markers on the Binox automatically, allow \"Read all map markers near this device\" for ATN Rangefinder in REM (REM 1.5.2 or later)".to_owned()
        } else {
            format!("nearby REM markers unavailable: {code} {message}")
        };
        self.nearby_report(&text, actions);
    }

    fn nearby_report(&mut self, text: &str, actions: &mut Vec<Value>) {
        if self.nearby_problem.as_deref() != Some(text) {
            self.nearby_problem = Some(text.to_owned());
            actions.push(json!({"kind":"status","message":text}));
        }
    }
}
