# Implementation and acceptance status

**Current contract: REM plugin map API 1.3** (see the 2026-10-17 migration section at the end;
earlier sections describe the superseded proposed contract and remain as history).

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

## REM map API 1.3 migration (2026-10-17, Stage 1 of `docs/goals/atn-rem-plugin/plan.md`)

REM `91124cffd8f7fa7df94684b672c2228d97f0522d` (release 1.5.1) implements plugin map API minors 2
and 3. The proposed `map.adapter.describe`, `map.observer.get`, `map.ple.*` and
`map.observation.draft.put` contract was removed from the production adapter and Android path and
replaced with the implemented operations, events, capabilities and record shape documented in
`docs/map-contract.md`:

- `crates/atn-adapter`: `RemMapObject` decodes REM's RCH `api_value()` shape (nested `position`,
  RFC 3339 `updated_at`, `textTruncated`, unknown fields tolerated) with a dependency-free RFC 3339
  parser; `AdapterConfig` gained `hostMarker` (catalog symbol/category/name prefix) and the
  ordered `slotPool`. The runtime declares API 1.3 through `hostStarted{apiMinor}`, checks the
  configured symbol against `map.symbols.list`, creates one shared marker per eligible observation
  with the connection-scoped idempotency key, keeps typed per-observation host outcomes
  (`created`, `alreadyExisted`, `rejected`, `unknown`, `retained`), serves one REM selection session
  at a time with atomic slot allocation and `map.session.stop` rejection, recovers dropped
  `started` events through `map.session.list`, refreshes the observer snapshot and holds writes on
  an ineligible fix, and requires an explicit connection-scoped co-location confirmation.
- Android: manifest declares `rem.plugin.apiMinor` 3 and the four map capabilities; the service
  reads the negotiated `apiMinor` from the start session, admits writes only for the
  `sessionAuthorized` session/connection pair, and replaces the manual selected-PLE entry with
  co-location confirmation; REM's **Share with ATN Rangefinder** starts display sessions.
- Documentation: `docs/map-contract.md` rewritten for the implemented API; README,
  configuration, execution, alignment and Android docs updated; `examples/synthetic-config.json`
  gained `hostMarker` and `slotPool`. Browser console, `atn-host-service` and other untracked local
  work were left untouched.

Local checks on 2026-10-17 (Rust 1.88 toolchain): `cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` (31 synthetic
exchange tests against real API 1.3 envelopes plus 1 parser, 23 domain, 22 protocol and the
web/host-service crates' tests) and `bash scripts/test-jni.sh` passed. The SDK AAR was rebuilt
from the REM checkout at `91124cf…` in an external scratch directory (REM untouched):
`rem-plugin-sdk-release.aar` SHA-256 `db90e2ecb239765c418ec471aee0f8a27e97c4e85ec048229d9d88ec0ff5fb29`.
`scripts/build-android.sh --offline` (ARM64, NDK 27.0.12077973, Gradle 8.11.1, AGP 8.7.2,
compile SDK 35) passed: 4 JVM scheduler tests, lint with no issues, debug APK SHA-256
`8ef417a0e56f87cae1d245540a93612814c4fe7facfd97d1e5bf7ad08f5bc553` built with the synthetic
all-zero host fingerprint, which cannot authenticate a real REM host.

Status after Stage 1: **implemented but unproven**. Not performed: binding to a real REM 1.5.1
host, real marker visibility and replication, real session events, Android OS BLE/Binder lifecycle
on a device, physical binocular rendering, and the echo check for outbound display traffic.
Remaining stages: explicit Start/Stop capture controls and bounded result-unknown resolution
(Stage 2), concurrent sessions with disjoint allocations and periodic reconciliation (Stage 3),
goTenna compatibility notes (Stage 4), hardware qualification.
