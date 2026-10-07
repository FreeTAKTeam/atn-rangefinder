# Codex handoff

This repository currently contains research documentation and synthetic fixtures, not a working Rust crate, Android APK or REM plugin. Do not claim an implementation, build or hardware test exists merely because the documents and fixtures validate.

## Read order

1. README.md
2. docs/evidence.md
3. docs/protocol.md
4. docs/rem-contract.md
5. docs/implementation-plan.md
6. fixtures/README.md and the JSON fixtures

## Intended implementation boundary

Plan a Rust protocol/validation core with an Android service/JNI wrapper using REM's real APK/Binder SDK. Reinspect the target REM checkout before coding. The pinned source inspection does not establish a general PLE map API: design and review the narrow required extension rather than inventing an existing API or abusing operator telemetry as a map-point store.

Device observations flow toward REM; selected REM PLEs flow toward the device display. An explicit authorized synchronization session may propagate subsequent updates to selected PLEs automatically. Do not impose repeated per-update confirmation or silently expand the selected set. Provide clear session stop/disconnect behavior.

Keep LXMF, networking core, map renderer, ballistics, aiming and fire control outside scope. Do not silently clear device markers, replay writes on reconnect, or change hardware/security settings. Follow the current user authorization for implementation and device testing; documentation alone authorizes neither.

## Evidence and tests

Preserve distinctions between capture observations, static application behavior and unverified physical semantics. Incoming measurements use big-endian fields; outgoing marker/icon fields use little-endian. Reply tails and status meanings remain unresolved. Never assume true north, calibrated units, observer co-location, a fresh fix or a known datum.

Start with pure codec tests using synthetic fixtures. Validate malformed inputs, bounds, unknown commands, exact-multiple fragmentation, header consistency, PNG CRCs and byte-exact reassembly. Use actual negotiated write capacity and discovered handles; the captured handle values are not protocol constants. Report separately what passed static tests, integration tests and real-device tests.

Before mapping an observation, establish explicit observer location/source/time and coordinate/bearing reference. Before declaring REM integration complete, verify actual map-point exchange in both directions rather than only a generic bus event.

## Repository hygiene

Do not commit APKs, decompiled proprietary source, captured bugreports, raw private traffic, actual locations, device identifiers, signing keys, keystores or credentials. Use original synthetic fixtures. Resolve SDK/source redistribution licensing before vendoring; no verified Maven publication or repository license is assumed. Keep provenance and limitations current when evidence changes.
