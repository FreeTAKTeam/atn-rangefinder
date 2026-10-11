# atn-domain

Pure Rust geographic conversion and ephemeral selected-PLE synchronization.
This crate opens no Bluetooth connection, writes no files and publishes no map
points. Android and the REM contract remain separate boundaries.

`derive_draft` preserves the raw eight-byte observation, decoder provenance,
observer fix, co-location/source evidence, calibration policy and timestamps.
It returns an explicit draft. The observer's horizontal uncertainty is retained;
measurement uncertainty and derived survey accuracy remain unknown.

There is no calibrated default. The caller supplies distance and angle scaling,
slant/horizontal range interpretation, pitch polarity, true/magnetic north,
WGS84 datum, evidence and eligibility thresholds. Separate required raw
`observationBounds` and `displayBounds` configure each field's inclusive integer
interval and exclude unsupported values/sentinels. Bounds do not establish device
calibration; they must be verified for the chosen setup. Signed configured incoming
bearing values normalize to true azimuth. Outgoing bearings use the explicit
nonnegative azimuth representation and reject intervals that cannot represent it.
Magnetic declination is
east-positive and independently checked for freshness. Missing, stale, future,
inaccurate or unresolved observer fixes prevent conversion. An unverified
configuration can support read-only observations through
`SemanticsPolicy::validate_configuration`; it cannot derive positions.

Projection uses a bounded Vincenty calculation on the WGS84 ellipsoid. The
ellipsoid's defining constants and solver convergence tolerance are mathematical
constants; they do not establish device units or calibration. Indeterminate
antipodal inverses return an error. Dateline crossings are normalized. Optional
altitudes are ellipsoidal metres; no geoid conversion or implicit zero altitude
is introduced.

`marker_for_ple` converts geographic coordinates to relative range and bearing.
The caller explicitly chooses a level display or altitude-based inclination.
Altitude-based conversion requires both ellipsoidal altitudes. Rounded values
must fit the protocol's signed fields before encoding. The selected device model
and its physical limits still require independent verification.

`SyncSession` takes one explicit selection-to-slot map and caller-supplied queue
count, byte, lifetime and operation-timeout limits. Updates for unselected PLEs
or different slots are rejected. Position and PNG operations remain separate.
New relative positions replace unsent queued positions for the same selected
PLE, preserving their queue order and retaining a superseded cancellation report.
Already submitted transfers and independently queued icons are never replaced.
Writes serialize one frame at a time with explicit transport callbacks; PNG
fragments do not require invented fragment acknowledgements.

Final transport completion, an opaque command reply and observed device rendering
are independent report fields. A final reply arriving before the final transport
callback is retained without advancing the queue until both arrive. No reply
status is interpreted as firmware success. Command-only reply correlation cannot
prove a firmware transaction identity; real-device verification remains required.

Call `tick` from the lifecycle timer so a silent connection can expire. Timeout,
clock rollback, stop and disconnect cancel the queue and end the session. Removal
of a selected PLE cancels its queued updates; removal during its active transfer
ends the session. No operation clears device markers or retries a transfer.
Reconnection requires a newly authorized session and has no replay queue.

Synthetic tests cover geometric conversion, required policy gates, uncertainty,
integer overflow, JSON boundaries, owned slots, queue bounds, partial transfers,
reply ordering, cancellation and fresh-session behavior. These tests are software
evidence, not calibrated hardware or REM map-path acceptance.
