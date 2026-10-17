# Implementation execution

Goal: implement the supplied research plan as an ATN adapter with a Rust validation core and Android REM plugin boundary.

Plan: `atn-rangefinder-docs/docs/implementation-plan.md`.

Intent: exchange device observations and explicitly selected map points with configured physical references.
Truth owners: ATN codec in `atn-protocol`; eligibility/geometry/session rules in `atn-domain`; Android owns BLE, permissions, and Binder; REM owns marker identity/storage/replication, the observer fix and selection sessions.
Contract boundary: real REM APK/Binder SDK, externally built pinned AAR. Since REM `91124cf` (1.5.1) the host implements plugin map API 1.3; the adapter declares that minor, checks the configured symbol against the host catalog and fails read-only on permission denial. Older hosts never bind it.
Cutover: replace documentation-only status with tested codecs, domain rules, adapter and Android sources; retain original research provenance.
Displaced path: no previous implementation exists.
Acceptance evidence: synthetic codec and integration tests; Android compile; target REM map exchange and hardware rendering separately reported.
Kill criteria: missing or unverified references gate conversion; stop/disconnect/timeout ends write session and discards queued writes.
Forbidden moves: no marker clear-all, replay on reconnect, assumed north/units/location, proprietary vendoring, LXMF/network-core changes, hardware/security changes or credentials in source.

User authorized agents for implementation. Parallel file ownership:

| Task | Owner | Files | Evidence | Dependency |
|---|---|---|---|---|
| Protocol codecs and PNG validation | codec agent | crates/atn-protocol | synthetic fixture/adversarial tests | existing protocol research |
| Geographic and selected session rules | domain agent | crates/atn-domain | geometry/eligibility/lifecycle tests | protocol API |
| Android boundary and configuration UI | android agent | android, scripts/build-android.sh | SDK-backed compilation | pinned REM SDK, native dispatcher |
| Runtime integration and JNI | main agent | crates/atn-adapter, root/docs/scripts | request/action integration tests | agents' APIs |
| Final review and evidence | agents + main agent | assigned review boundaries | findings fixed; limitations recorded | integrated build |

Operational policies are user-supplied configuration, with no calibrated model, freshness, accuracy, or slot defaults. User requested following local Reticulum Community Hub code; inspect that implementation before finalizing the map exchange boundary.

## Final checkpoint

Protocol, domain, runtime/JNI, Android source, configuration, RCH compatibility and software validation are implemented. Agent review findings were integrated. All 63 Rust tests, real JNI smoke, MSRV tests, four Android scheduler tests, strict lint/format and Android assembly passed; exact provenance is in implementation-status.md. Hosted CI, real REM map exchange, Android OS lifecycle and physical firmware/display acceptance remain unperformed. The separate hardware and host gates prevent declaring the original full completion bar achieved.
