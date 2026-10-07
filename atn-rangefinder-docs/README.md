# atn-rangefinder

Documentation and synthetic test material for a planned REM plugin connecting an ATN rangefinder/binocular to REM's map-facing interface. The planned architecture is a Rust protocol core with an Android service/JNI wrapper using REM's APK/Binder plugin boundary. This repository does **not** implement a device driver or a REM plugin.

## Boundary

- Device → adapter → REM: receive relative range/bearing/inclination observations; preserve the raw observation and, only with a suitable observer-position fix and explicit reference assumptions, derive a map observation.
- REM → adapter → device: translate a selected map POI into the device's relative display-marker representation and optional raster icon.
- REM integration contracts must be verified against the actual REM repository. No API, trait, event, coordinate structure or plugin registration mechanism is prescribed here.

Excluded: LXMF, network transport/core changes, map rendering, ballistic calculations, aiming, fire control, firmware modification and automatic transmission to other systems.

## Read first

1. [Evidence and limitations](docs/evidence.md)
2. [Protocol observations](docs/protocol.md)
3. [REM integration evidence](docs/rem-contract.md)
4. [Implementation phases and acceptance criteria](docs/implementation-plan.md)
5. [Synthetic fixtures](fixtures/README.md)

The byte layout is substantially better established than its physical semantics. In particular, do not equate a field called `compass` with verified true-north azimuth. Do not treat the phone's position as the binocular's position without an explicit co-location assumption.

No APK, decompiled source, private packet capture, bugreport, account/device identifier or real-world location is included. Fixtures are invented codec examples, not field evidence. Research date: 2026-10-07.
