# atn-protocol

Pure Rust codecs for the provisional [protocol evidence](../../atn-rangefinder-docs/docs/protocol.md). No Bluetooth connection, location calculation, device write, or REM interaction occurs in this crate. License: EPL-2.0. Rust edition 2024; declared MSRV 1.85.

`parse_inbound` accepts exactly eight-byte command-1 measurements and six-byte command-2/7 replies. Measurements retain signed big-endian integers; no units, calibration, compass reference, or observation validity are inferred. Reply answer and tail bytes remain opaque. Other valid-signature commands are retained as bounded diagnostic frames, including 0x70. Invalid supported lengths and signatures return typed errors.

`DisplayMarker::new` rejects integer narrowing, slots greater than 127, marker types outside 0/1 and colors outside 0 through 7. `encode` produces the ten-byte little-endian command-2 record. The signed 16-bit numeric bounds and seven-bit slot bounds describe representability; permitted physical values, firmware slot counts and slot persistence remain unverified. Negative distance can be represented by the codec and must be rejected by any physical-semantic policy that requires nonnegative range.

`fragment_payload` is the byte-framing primitive for synthetic boundary tests. `fragment_png` first validates the supplied PNG. Both receive the backend's actual writable ATT-value capacity, including the 14-byte application header; the capacity is not a requested MTU. Empty opaque payloads produce no frames; empty PNGs fail image validation. Every positive exact multiple retains its final full fragment. No fragment acknowledgement policy is inferred.

`IconReassembler` receives explicit memory limits. It accepts frames out of order, treats byte-identical duplicates as idempotent, rejects conflicting duplicates, enforces consistent transfer identity/header fields and aggregate byte counts, and returns an explicit incomplete-transfer error from `finish`. Only the currently documented zero CRC-named application-header field is supported; PNG chunk CRC validation is independent. Callers own timeout, cancellation and disconnect disposal.

`validate_png` supports RGBA8, noninterlaced PNGs with explicit `PngLimits`: encoded bytes, dimensions, pixels, decoded bytes and chunk count. Defaults are convenient for the synthetic 40x40 pipeline and are not a firmware capability claim. It validates signature, header/chunk structure and CRCs, consecutive IDAT chunks, IEND termination, a complete single zlib stream, exact decoded scanline lengths and supported row filter bytes. It rejects truncated streams, concatenated streams, compressed trailing bytes and decompression beyond the expected size. This validates the image container and raster representation; it does not prove successful device rendering.

```sh
CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=/usr/bin/gcc cargo test -p atn-protocol --offline
CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=/usr/bin/gcc cargo clippy -p atn-protocol --all-targets --offline -- -D warnings
```

All fixture and generated test inputs are synthetic. Tests include byte-exact fixture comparison, signed boundaries, integer overflow, malformed/unknown frames, arbitrary-order and duplicate transfers, incomplete/mixed transfers, fragmentation boundaries at multiple write capacities, PNG CRC/truncation/ordering, decoded bounds and zlib termination. No physical-device acceptance is claimed.
