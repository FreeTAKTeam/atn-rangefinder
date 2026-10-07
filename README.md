# atn-rangefinder

Protocol research, integration planning, and original synthetic fixtures for a planned ATN rangefinder/binocular adapter for [Reticulum Emergency Management (REM)](https://github.com/FreeTAKTeam/reticulum_mobile_emergency_management).

## Project status

This repository currently contains **documentation and synthetic test material only**. It does not yet provide a working device driver, Rust crate, Android APK, or REM plugin. Hardware semantics and the required REM map-point contract still need verification.

The planned architecture is a Rust protocol/validation core with an Android service/JNI wrapper using REM's APK/Binder plugin boundary.

## Intended scope

- Receive device range, bearing, and inclination observations, preserving their raw values and reference metadata.
- Derive map observations only when suitable observer-position and coordinate-reference information is available.
- Send selected REM map points (PLEs) to the device display within an explicitly authorized synchronization session.

LXMF, network-core changes, map rendering, ballistics, aiming, fire control, and firmware modification are outside scope.

## Documentation

The supplied research folder is preserved under [`atn-rangefinder-docs/`](atn-rangefinder-docs/):

1. [Research overview](atn-rangefinder-docs/README.md)
2. [Evidence and limitations](atn-rangefinder-docs/docs/evidence.md)
3. [Protocol observations](atn-rangefinder-docs/docs/protocol.md)
4. [REM integration contract](atn-rangefinder-docs/docs/rem-contract.md)
5. [Implementation plan and acceptance criteria](atn-rangefinder-docs/docs/implementation-plan.md)
6. [Synthetic fixtures](atn-rangefinder-docs/fixtures/README.md)

The fixtures are invented codec examples, not field evidence. No APK, decompiled proprietary source, private packet capture, actual location, or device identifier is included. No build or executable test suite is currently supplied.

## Contributing

Read the [handoff instructions](atn-rangefinder-docs/AGENTS.md) and evidence limitations before proposing implementation work. Keep synthetic examples separate from device observations, and do not commit credentials, raw private captures, proprietary binaries, or real location data.

## License

Licensed under the [Eclipse Public License 2.0](LICENSE) (`EPL-2.0`). This license covers the original material in this repository; it does not grant rights to external SDKs, applications, or other third-party material referenced by the research.
