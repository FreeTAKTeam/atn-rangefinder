# Device and firmware evidence matrix

No model/firmware combination currently has completed physical acceptance in this repository.

| Model | Evidence | Firmware | Adapter acceptance |
|---|---|---|---|
| Binox4k (application model ID 3) | Application discovery behavior in original research | Unverified | Raw protocol implementation only; physical units, north/pitch convention, valid wire bounds and rendering unverified |
| Binox4t (application model ID 4) | Application discovery behavior in original research | Unverified | Same unresolved physical gates; no inferred cross-firmware compatibility |
| Synthetic test device | Invented fixture/configuration | Fixture only | Codec, geometry and session tests; not hardware support |

Model/firmware, verification evidence, units, coordinate/bearing reference, inclination convention, accepted incoming/outgoing wire ranges and freshness/accuracy policy must be configured explicitly. The adapter does not infer model compatibility from an advertised model ID or service UUID alone.

Complete the stationary field checks in the original implementation plan and the gates in [`goals/atn-rem-plugin/qualification.md`](goals/atn-rem-plugin/qualification.md), and retain private evidence before setting verification flags. Record actual displayed units, bearing reference, invalid-value behavior, negotiated capacity, each marker/icon operation and separately observed rendering. Use only nonliving stationary surfaces and manufacturer-approved operation. Publish sanitized summaries, never captures, real locations or device identifiers.
