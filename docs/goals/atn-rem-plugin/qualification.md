# Hardware and host qualification checklist

**Status: none of these gates has been performed.** Synthetic tests, the JNI smoke test and the
Android build prove software behaviour only. A gate passes only with the evidence listed for it.

Rules for all evidence: keep raw logs, captures, screenshots with map positions, real locations,
device addresses/serials and REM identity hashes private. Commit only sanitized summaries (date,
REM version/commit, plugin APK hash, phone model and Android version, ATN model/firmware as
reported, pass/fail per step, and free-text observations without coordinates or identifiers).
Record results in `docs/implementation-status.md` and the evidence matrix in
`docs/device-support.md`. Use only stationary, non-living targets and manufacturer-approved
operation.

Prerequisites: a release build signed for the real REM host (real `REM_HOST_FINGERPRINTS`, not
the synthetic all-zero value), REM 1.5.1 (`91124cf…`) or later with plugin API 1.3, and a
configuration whose `protocolVerified`/`calibrationConfirmed` flags are set only from completed
device evidence.

| # | Gate | What to do | Evidence to retain (sanitized) | Passes when |
|---|---|---|---|---|
| 1 | Real API 1.3 binding | Install, trust and enable the plugin in REM; grant `mapMarkersRead`, `mapMarkersWrite`, `mapObserverRead`, `mapSelectionRead`, `notificationsRaise`; start the node. Then revoke one map capability while bound. | Plugin status text showing `apiMinor` 3 and `hostReady`; status after revocation. | REM binds without "Incompatible"; `hostReady` becomes true; revocation makes the adapter read-only and ends capture/sessions without a crash. |
| 2 | Marker visibility and replication | Connect, confirm co-location, start capture, take three measurements of fixed targets (two with identical values). | Count of markers created, their `created`/`alreadyExisted` outcomes, notes length, whether a peer/RCH sees them. | One marker per measurement (duplicates included), visible in REM and on a peer; notes untruncated; no marker before Start or after Stop. |
| 3 | Session events | Share 1, then 4 concurrent selections; update and delete a shared marker; stop from REM; restart REM mid-session. | Per step: sessions and slots listed in plugin status, notifications shown for rejections. | Disjoint slots, a fifth/oversized share rejected with a REM notification, updates rewritten, removals release slots, stops and restarts end sessions; reconciliation recovers within `reconcileIntervalMs`. |
| 4 | RNode BLE + ATN BLE | With an RNode connected to REM over BLE and the node running, connect the ATN device; run gates 2 and 3 briefly; disconnect/reconnect ATN. | RNode link state before/after each ATN action; ATN status. | The RNode link and REM node stay up throughout; ATN works; no unrelated device is disconnected. |
| 5 | RNode Classic + ATN BLE | As gate 4 with the RNode on Bluetooth Classic. | As gate 4. | As gate 4. |
| 6 | Binocular rendering | Share known points at known relative positions; send one icon; record what the binocular shows for each slot. | Per slot: expected vs displayed distance/bearing/pitch (relative values only), marker type/colour, icon appearance; operator display confirmations entered. | Every displayed marker matches its expected relative geometry and style within the tolerance recorded for the device profile before the test; released slots behave as documented (not cleared). |
| 7 | Outbound-traffic echo check | While capture runs, send position and icon writes; watch native status for observations during and after each write. | Observation counts and timestamps around each write. | No measurement observation is produced by display writes or their replies. |
| 8 | Android lifecycle on device | Exercise screen off, app backgrounding, Bluetooth permission revocation, Bluetooth off, REM force-stop and plugin update while connected. | Plugin status after each event. | Each event ends the affected authorizations, nothing reconnects or replays by itself, and the foreground notification is removed when disconnected. |
| 9 | ATN + goTenna coexistence (future) | Once a goTenna plugin exists: run gates 4 and 6 with goTenna as REM's link, and switch RNode ↔ goTenna by disconnect-before-connect. | Link states across the switch; ATN status. | REM node and ATN plugin stay running across the switch; ATN behaviour unchanged. |
