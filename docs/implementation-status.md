# Implementation and acceptance status

Verified locally on 2026-10-10. Three agents implemented/reviewed the protocol, domain and Android layers; the main agent integrated the JNI runtime, RCH-compatible records, configuration, documentation and synthetic host exchange.

## Plan coverage

| Phase | Implemented | Evidence / outstanding acceptance |
|---|---|---|
| 0: contract/policies | Required configurable semantics, wire bounds, freshness/accuracy, slot/session ownership, RCH record mapping; proposed map contract | REM/RCH source pins verified. No calibrated default. Hardware support matrix is unverified; actual REM map extension is absent. |
| 1: codecs | Strict incoming endian/signature/length parsing, opaque replies, validated marker encoding, actual-capacity fragmentation, bounded reassembly and PNG integrity/decompression | 22 synthetic fixture/adversarial tests pass. Phase 1 software acceptance passed. |
| 2: Android boundary | Real SDK/Binder service, explicit permissions/device selection, discovered UUIDs/CCCD, negotiated capacity, notification timestamps, cancellation/timeouts and no replay | ARM64 native build and APK compile/package pass; 4 scheduler JVM tests, lint and real JNI smoke pass. Android OS BLE/Binder lifecycle and physical semantics remain unperformed. |
| 3: receive into map | Raw observations retained separately; provenance-preserving draft derivation and host request with explicit source/reference gates | Synthetic host tests prove request/action flow and gating. Actual REM host map visibility remains blocked by missing host operations. |
| 4: selected PLE to device | Explicit selected/owned slots, automatic scoped updates, unsent-position coalescing, independent style/icons, stop/disconnect and separate transport/reply/display evidence | Synthetic lifecycle/exchange tests pass. Real selected-PLE exchange and device display confirmation remain unperformed. |

**The full plan's completion bar is not met:** it requires a compatible REM map/observer host contract, verified firmware semantics and actual map/device acceptance. The user directed adapter implementation following local RCH code; REM/RCH source and networking core were not changed. The current host's unsupported-operation response leaves geographic exchange and outbound synchronization disabled, while configured raw observation remains available.

## Local checks

- `cargo fmt --all -- --check`: passed.
- `cargo clippy --workspace --all-targets --offline --locked -- -D warnings`: passed.
- `cargo test --workspace --offline --locked`: **63 tests passed** (22 protocol, 23 domain, 18 adapter) on Rust 1.88.
- Declared MSRV Rust 1.85: the same 63 tests passed using an isolated target, incremental compilation disabled and debug information disabled. An initial redundant debug build exhausted the sandbox quota; removing only that task-created temporary target and reducing its artifact size resolved it.
- `bash scripts/test-jni.sh --offline`: real JVM/JNI export, configuration, read-only decode and error smoke passed.
- Android `:app:testDebugUnitTest :app:lintDebug :app:assembleDebug`: passed offline; **4 tests**, no failures/errors/skips; lint **No issues found**.
- `apksigner verify`: debug APK verified. No release signing or installation performed.
- `.github/workflows/ci.yml` supplies Rust/MSRV/JNI checks; hosted GitHub CI has not run for these local changes.

Reviews fixed cross-session callbacks, early-final-reply ordering, stale/conflicting observer updates, send-time eligibility, outdated queued geometry, unrelated marker-removal cancellation, native/platform request-table cancellation, negotiation startup and first-run timeout scheduling. Final independent domain/integration review reported no unresolved P1/P2 finding in reviewed source paths. This is supporting software evidence, not proof of field behavior.

## Build provenance

REM source: `ec6aadc81b3c18e2353211e97225a89d73af84dc`; RCH source: `2a599275ee36ea809dbd5ff773ebda6bf9d2a8d1`.

The external SDK was freshly built into `/tmp`, with no source vendoring or edits to REM:

- AAR SHA-256: `dd81949b0ab07a55469736c147505949a901abc62512a3e524fde2dfa61150bd`.
- Unstripped ARM64 native input SHA-256: `b7bb2762bdf8aaaf1f2def844ad5c7a2b9cd5afe9655ba21e8ab032e5d6505ca`, byte-identical to Gradle's merged native input.
- Packaged ARM64 native SHA-256: `f4697b31bb18f4db7b0ecdf1e77fe6e02618e7329cd957192d77fa3297bbe08d`, byte-identical to Gradle's normal stripped output.
- Debug APK SHA-256: `edb987429b59a8d35d8aef830e17ac813960b9adbfb533346ab53d837d04880d`.

Generated ignored APK: `android/app/build/outputs/apk/debug/app-debug.apk`. The build uses a deliberately synthetic all-zero host certificate fingerprint; this compile artifact cannot authenticate a real REM host. Build with the intended host's actual package/certificate and obtain SDK redistribution permission before deployment/distribution. Android compile SDK 35, min SDK 26, Java 17 source compatibility, Gradle 8.11.1, AGP 8.7.2 and NDK 27.0.12077973 were used.

## Remaining gates

1. REM-owned map operations, capability grants, draft persistence/visibility and selected-set event delivery described in `map-contract.md`.
2. Explicit verified model/firmware profile and observer/reference rules; see `device-support.md`.
3. Android lifecycle tests on a target device, including permission revocation, Binder/process restart and partial-transfer disconnect.
4. Actual map exchange in both directions and separately observed device marker/icon rendering.

The original plan's acceptance requirements remain in force. No raw private traffic, actual locations, device identifiers, proprietary APKs, SDK source or signing keys are included in committed source.
