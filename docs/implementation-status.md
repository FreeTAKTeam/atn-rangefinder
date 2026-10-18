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

## Automatic measurement capture (2026-10-17, Stage 2 of `docs/goals/atn-rem-plugin/plan.md`)

- `crates/atn-adapter`: native `startCapture`/`stopCapture`. Start requires host ready with the
  configured `hostMarker.symbol` confirmed by `map.symbols.list`, a connection, a
  `protocolVerified` profile, valid semantics and the connection's co-location confirmation;
  refusals return `captureStarted:false` with a reason. Outside capture, measurements are decoded
  and retained as `retained {"capture not started"}` with no observer snapshot or marker creation.
  Capture ends on stop, disconnect, new connection, `hostStarted`, `hostStopped`, `configure`,
  `clearColocation` and `PermissionDenied`, plus a gate check after every native call; it is never
  persisted and a new capture never publishes earlier observations. Status shows
  `capture {active, session, lastEnded}` and each observation's `captureSession`.
- Idempotency keys are `<captureNonce>/<connectionId>/<sequence>`; the nonce `c<16 hex>-<n>` is
  minted in Rust from one OS-seeded `RandomState` draw per process and a per-process capture
  counter. Android supplies nothing new.
- An unknown creation (timeout, `HostError`, `expired`, `binder.*`) gets exactly one replay of the
  identical key and payload from the first tick after the new required `createRetryDelayMs`, only
  while the same Binder host session and capture session are active. After it, or once capture,
  host or connection ends, the outcome is `unknown {resolution:"final"}` permanently.
- Android: **Start capture** / **Stop capture** buttons and capture status in `SettingsActivity`;
  `AtnPluginService` refuses Start without co-location, mirrors capture state from native status
  responses (every tick) into the configuration page state, and keeps all JNI calls on its one
  handler. `rem-plugin-config/index.html`, `docs/map-contract.md`, `docs/configuration.md`,
  `android/README.md` and the handoff were updated; `examples/synthetic-config.json` gained
  `createRetryDelayMs`.

Local checks on 2026-10-17: `cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` (41 synthetic
exchange tests, 10 of them new for capture gating, automatic end on each trigger, nonce/key shape,
one-shot replay returning `created:false`, failed replay staying final, replay withdrawal on host
restart, disconnect and capture stop, and no historical publication; plus 1 parser, 23 domain and
the remaining workspace crates' tests) and `bash scripts/test-jni.sh` (PASS) passed.
`scripts/build-android.sh --offline` (ARM64, NDK 27.0.12077973, Gradle 8.11.1, AGP 8.7.2) with the
same SDK AAR (SHA-256 `db90e2ecb239765c418ec471aee0f8a27e97c4e85ec048229d9d88ec0ff5fb29`, REM
`91124cffd8f7fa7df94684b672c2228d97f0522d`) and `REM_HOST_PACKAGE=network.reticulum.emergency`
passed: 4 JVM scheduler tests, lint with no issues, debug APK SHA-256
`7cf032b6ed2d1a0ad77b08f1f7b200dd00659c9c8eefbfbaa703adfc2f6e91b8`. The APK carries the synthetic
all-zero host fingerprint and cannot authenticate a real REM host.

Status after Stage 2: **implemented but unproven**. Not performed: a real REM 1.5.1 host binding,
real `map.markers.create` replay behaviour and marker visibility, Binder death/timeout on a device,
Android UI interaction on a phone, and any ATN hardware. Remaining: Stage 3 (concurrent sessions,
reconciliation interval, `notifications.raise`), Stage 4 notes, hardware qualification.

## Selected-object display synchronization (2026-10-17, Stage 3 of `docs/goals/atn-rem-plugin/plan.md`)

- `crates/atn-adapter`: up to four concurrent REM selection sessions, each with its own
  `atn-domain` `SyncSession`, allocated atomically to the first free `slotPool` slots in REM's
  `objectDestinationHashes` order (one slot per session for an object shared twice). A session
  that does not fit or fails a readiness gate is rejected alone: nothing allocated, others
  untouched, one `map.session.stop`, one best-effort `notifications.raise {title, body}`, terminal
  ID. One device-wide scheduler keeps a single transfer and frame in flight, routes replies to it
  and round-robins sessions; echoes must match the in-flight connection/session/operation/frame,
  and an echo for an ended session only frees the device. Ending a session emits `sessionEnded`
  and records `releasedSlots` with `deviceDisplayCleared:false`. A partial icon transfer cut off by
  any end closes GATT and ends every session. New required `reconcileIntervalMs` drives periodic
  `map.session.list` (sessions accepted before the request and absent from it end without
  `map.session.stop`) and `map.session.objects.get` for stale or event-touched sessions; unchanged
  objects write nothing. `stopSync` takes an optional `sessionId`; `setIcon` and `confirmDisplay`
  take `sessionId` and refuse unknown sessions/objects without ending anything.
- `crates/atn-domain`: read-only `SyncSession` accessors (`active_transfer`, `queued_operations`,
  `is_active`, `session_id`, `slot_of`); behaviour unchanged.
- Android: manifest declares `notificationsRaise`; the service admits writes per authorized
  session, handles `sessionEnded`, lists sessions with slots and per-session transport, reply and
  display-confirmation state plus released slots, and passes the REM session ID for stop, icon and
  display confirmation. All JNI calls stay on the one handler.
- Docs: `docs/map-contract.md` sessions section (replaces "one session at a time"),
  `docs/configuration.md`, `android/README.md`, handoff; `examples/synthetic-config.json` gained
  `reconcileIntervalMs`.

Local checks on 2026-10-17: `cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` (54 synthetic
exchange tests, 13 of them new for four concurrent sessions, overlapping selections, an exhausted
pool rejecting only the new session with slot reuse after release, dropped `started` discovered by
reconciliation, reordered/duplicate events and terminal IDs, unchanged reconciliation writing
nothing, `objects.get` omission, a session missing from the list, ended-session and
old-connection echoes, and partial icon transfers followed by session stop, revocation,
disconnect and timeout; 24 domain tests, 1 new; 1 parser test and the remaining workspace crates'
tests) and `bash scripts/test-jni.sh` (PASS) passed. Twelve existing exchange tests were adjusted,
each with a comment where behaviour changed: status now reports `sessions`/`endedSessions`, a
single session ending emits `sessionEnded` instead of `cancelWrites`, a second share is served
instead of rejected, and a refused `setIcon` no longer ends the session.
`scripts/build-android.sh --offline` (ARM64, NDK 27.0.12077973, Gradle 8.11.1, AGP 8.7.2, SDK AAR
SHA-256 `db90e2ecb239765c418ec471aee0f8a27e97c4e85ec048229d9d88ec0ff5fb29` from REM
`91124cffd8f7fa7df94684b672c2228d97f0522d`, `REM_HOST_PACKAGE=network.reticulum.emergency`)
passed: 4 JVM scheduler tests, lint with no issues, debug APK SHA-256
`9bdfc4086f24b8567125b4e84ea5e770b1424dd44eaa0609c0ca12c228ed317c`, built with the synthetic
all-zero host fingerprint, which cannot authenticate a real REM host.

Status after Stage 3: **implemented but unproven**. Not performed: a real REM 1.5.1 host with
four concurrent sessions, real `notifications.raise` display, real event loss and reconciliation,
Android BLE/Binder lifecycle on a device, binocular rendering of several slots, and the no-echo
check for outbound display traffic. Remaining: Stage 4 documentation and hardware qualification.

## Bluetooth coexistence and goTenna compatibility (2026-10-17, Stage 4 of `docs/goals/atn-rem-plugin/plan.md`)

- Coexistence code review of `BleTransport.java`, `SettingsActivity.java`, `AtnPluginService.java`
  and the manifest: the plugin never enables/disables the adapter, never bonds or disconnects
  unrelated devices, keeps one GATT object for the operator-selected device, and scans only on the
  operator's request, ATN-service-filtered unless **scan all** is ticked, bounded by
  `scanTimeoutMs` and the device-list bound. Per-file evidence is in `docs/radio-coexistence.md`.
- Transport boundary: `connected` now carries the bound service, notification and write
  characteristic UUIDs (Rust refuses anything else); `notification` names its characteristic
  (Rust decodes measurements and replies only from the notification characteristic);
  `transportResult` names the write characteristic and reports a typed `error {code, gattStatus?}`
  on failure. `BleTransport.Listener` passes the characteristic and GATT status through.
  `NativeSmoke.java` sends the new fields.
- Documentation: `docs/radio-coexistence.md` (RNode BLE + ATN BLE, RNode Classic + ATN BLE,
  operator-managed RNode/goTenna switching with no software interlock, REM node and ATN plugin kept
  running), `docs/goals/gotenna-plugin/architecture.md`,
  `docs/goals/atn-rem-plugin/qualification.md` (nine unperformed gates with evidence and pass
  criteria), and links from README, `docs/configuration.md` and `docs/device-support.md`.

Local checks on 2026-10-17: `cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` (105 tests: 57
synthetic exchange tests, 3 new for GATT identity on connect, notification-characteristic binding
and typed transport errors; 24 domain tests and the remaining workspace crates' tests) and
`bash scripts/test-jni.sh` (PASS) passed. `scripts/build-android.sh --offline` (same SDK AAR
`db90e2ecb239765c418ec471aee0f8a27e97c4e85ec048229d9d88ec0ff5fb29`, REM
`91124cffd8f7fa7df94684b672c2228d97f0522d`, ARM64, NDK 27.0.12077973, Gradle 8.11.1, AGP 8.7.2,
`REM_HOST_PACKAGE=network.reticulum.emergency`) passed: 4 JVM scheduler tests, lint with no
issues, debug APK SHA-256 `d122fae05642b6f85dbcc207309c3d1e81f2de5542e0f5c01b2d98440dad0cff`,
built with the synthetic all-zero host fingerprint, which cannot authenticate a real REM host.

Status after Stage 4: **implemented but unproven**. Hardware qualification is deferred by the
user; no gate in `docs/goals/atn-rem-plugin/qualification.md` has been performed, and no real REM
host, RNode, ATN device or goTenna was exercised.

## 16 KB page alignment of the native library (2026-10-18)

Installing the debug APK on an Android 17 phone showed the platform warning "This app isn't 16 KB
compatible ... `lib/arm64-v8a/libatn_adapter.so`: LOAD segment not aligned". The library still
loaded there because that phone uses 4 KB pages, but it would fail on 16 KB-page devices.
`scripts/build-android.sh` now links the Android targets with
`-C link-arg=-Wl,-z,max-page-size=16384` (target-scoped `CARGO_TARGET_<TRIPLE>_RUSTFLAGS`, next to
the NDK linker; host builds are unchanged).

Checks on 2026-10-18: `llvm-readelf -lW` (NDK 27.0.12077973) on the arm64-v8a
`libatn_adapter.so` shows all four LOAD segments with `Align 0x4000` (previously `0x1000`);
`zipalign -c -P 16 -v 4` on the debug APK reports `lib/arm64-v8a/libatn_adapter.so (OK)` and
"Verification successful". `cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` (105 tests),
`bash scripts/test-jni.sh` (PASS) and `scripts/build-android.sh --offline` (4 JVM tests, lint with
no issues, synthetic host fingerprint) passed. Not checked: loading on a 16 KB-page device.

## Operator simplification for the Binox 4K (2026-10-18)

On the operator's Pixel 7 with REM 1.5.1 the plugin bound to REM and connected to the Binox 4K,
and two measurements decoded correctly (74 m / 147° and 818 m / 139°, distance and bearing matched
the display). But the operator flow needed typed JSON policies, typed co-location evidence and a
separate Start capture, and was judged unusable. A REM share also never reached the plugin.

- Convention over configuration (`AGENTS.md`): built-in Binox 4K defaults in
  `crates/atn-adapter/src/binox4k-defaults.json`, partial overrides merged by
  `AdapterConfig::resolve`, native `configure` without `config` and new `resolveConfig`.
  `configure` returns the effective configuration. The "API 1.3 required" status is no longer
  emitted before REM starts the plugin.
- Android: one screen with a plain status block (REM, Binox, measurements, what is on the
  Binox, last measurement), Scan → tap Binox → "Is this phone with the binocular?", automatic
  capture behind a "send measurements" switch (default on), and everything else under
  **Show advanced**. Legacy `transport`/`config` preferences are removed on first start. The
  service logs REM events and decisions under `AtnPlugin`.
- `docs/configuration.md`, `android/README.md`, `docs/device-support.md` (Binox 4K field
  evidence) and the handoff were updated.

Checks on 2026-10-18: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D
warnings`, `cargo test --workspace` (107 tests; 2 new for defaults and partial overrides, 1
adjusted because an omitted value now takes its default) and `bash scripts/test-jni.sh` (PASS)
passed. `scripts/build-android.sh --offline` passed (4 JVM tests, lint with no issues); the APK
installed on the Pixel was built with REM 1.5.1's real host certificate fingerprint (not
committed), and REM bound it (`AtnPlugin: REM bound, plugin API 1.3`). Not yet re-tested on the
device after this change: Connect with the new question, automatic capture creating a REM marker,
and a REM share reaching the Binox.

Plugin version 0.2.0 (2026-10-18): the version now has a single source, `pluginVersion` in
`android/app/build.gradle`, which sets `versionName`, a derived `versionCode` (major·10000 +
minor·100 + patch = 200) and the manifest's `rem.plugin.version`. The settings screen shows it in
its title. It must be bumped for every build that is handed out; earlier builds all reported
0.1.0. Installed on the Pixel 7 as 0.2.0 / 200 (lint clean, 4 JVM tests).
