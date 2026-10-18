# Device and firmware evidence matrix

No model/firmware combination has completed physical acceptance yet. The Binox 4K is the main target and the built-in defaults are for it; its partial field evidence is below.

| Model | Evidence | Firmware | Adapter acceptance |
|---|---|---|---|
| Binox4k (application model ID 3) | Application discovery behavior in original research; operator field test 2026-10-18 (Pixel 7, REM 1.5.1) | Unrecorded | Main target with built-in defaults. Confirmed: BLE connection without pairing, notifications, 512-byte write capacity, distance 1 raw unit = 1 m (74 m, 818 m) and compass 1 raw unit = 1° (147°) against the display. To confirm: north reference, pitch sign, slant vs horizontal range, display slots, rendering |
| Binox4t (application model ID 4) | Application discovery behavior in original research | Unverified | Same unresolved physical gates; no inferred cross-firmware compatibility |
| Synthetic test device | Invented fixture/configuration | Fixture only | Codec, geometry and session tests; not hardware support |

Model/firmware, verification evidence, units, coordinate/bearing reference, inclination convention, accepted incoming/outgoing wire ranges and freshness/accuracy policy must be configured explicitly. The adapter does not infer model compatibility from an advertised model ID or service UUID alone.

Complete the stationary field checks in the original implementation plan and the gates in [`goals/atn-rem-plugin/qualification.md`](goals/atn-rem-plugin/qualification.md), and retain private evidence before setting verification flags. Record actual displayed units, bearing reference, invalid-value behavior, negotiated capacity, each marker/icon operation and separately observed rendering. Use only nonliving stationary surfaces and manufacturer-approved operation. Publish sanitized summaries, never captures, real locations or device identifiers.
