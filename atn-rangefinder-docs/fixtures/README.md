# Synthetic test fixtures

All files in this directory are original invented codec test material. They contain no real coordinates, device IDs, private captures or extracted ATN graphics. They must never be presented as evidence of device behavior.

- `codec-vectors.json`: incoming/outgoing endian distinction, signed raw inclination, defensive frame validation and opaque reply tails. A synthetic reply code is not an assertion of firmware status meaning.
- `synthetic-icon.png`: deterministic original 40×40 RGBA pixel pattern, not an ATN icon.
- `icon-transfer.json`: full command-7 ATT values containing that PNG, split at 495 payload bytes with a 14-byte header and synthetic slot ID 2. Each header is independently checkable. The declared 495-byte capacity is a fixture assumption, not a connection negotiation.

For a future test suite:
1. Decode the measurement vector exactly, retaining raw values without asserting calibrated units.
2. Encode the synthetic marker and compare byte-for-byte.
3. Parse every icon header, join payloads by frame number, and compare exact PNG bytes and SHA-256.
4. Validate PNG signature, IHDR dimensions/type, chunk CRCs, decompressed scanline length and IEND termination.
5. Exercise malformed/reordered/duplicate/missing frames, inconsistent total size/slot/count, oversized allocation requests and truncated payloads.
6. Exercise listed fragmentation boundary lengths using synthetic opaque byte arrays. They are fragmentation tests, not valid-image acceptance tests. Never discard the last full fragment at exact multiples of capacity.
7. Distinguish unknown frames from supported frames with invalid content; neither should be coerced into a geographic point.

No executable driver, BLE sender or REM implementation is supplied.
