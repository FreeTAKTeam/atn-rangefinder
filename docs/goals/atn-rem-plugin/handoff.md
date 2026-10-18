# Execution handoff

Plan: `plan.md` in this directory. Status and evidence: `../../implementation-status.md`.

| Stage | State | Where |
|---|---|---|
| 1. Host API migration to REM plugin API 1.3 | implemented, software-verified 2026-10-17 | `crates/atn-adapter/src/{rch.rs,config.rs,runtime.rs,runtime/*}`, `android/app/src/main/{AndroidManifest.xml,java/org/freetak/atn/AtnPluginService.java,java/org/freetak/atn/SettingsActivity.java,assets/rem-plugin-config/index.html}`, `docs/map-contract.md` |
| 2. Automatic measurement capture | implemented, software-verified 2026-10-17 (synthetic tests, JNI smoke, Android build with a synthetic host fingerprint; no real REM host or hardware) | `crates/atn-adapter/src/{config.rs,runtime.rs,runtime/{control,observations,host}.rs}`, `crates/atn-adapter/tests/exchange.rs`, `android/app/src/main/java/org/freetak/atn/{AtnPluginService,SettingsActivity}.java`, `docs/map-contract.md` |
| 3. Selected-point synchronization | implemented, software-verified 2026-10-17 (synthetic tests, JNI smoke, Android build with a synthetic host fingerprint; no real REM host or hardware) | `crates/atn-adapter/src/{config.rs,runtime.rs,runtime/{sessions,control,host,observations}.rs}`, `crates/atn-domain/src/session.rs` (read-only accessors), `crates/atn-adapter/tests/exchange.rs`, `android/app/src/main/{AndroidManifest.xml,java/org/freetak/atn/AtnPluginService.java,java/org/freetak/atn/SettingsActivity.java}`, `docs/map-contract.md` |
| 4. Bluetooth and goTenna compatibility | implemented and documented 2026-10-17 (code review, synthetic tests, JNI smoke, Android build; no hardware) | `docs/radio-coexistence.md`, `docs/goals/gotenna-plugin/architecture.md`, `android/app/src/main/java/org/freetak/atn/{BleTransport,AtnPluginService}.java`, `crates/atn-adapter/src/runtime/{control,observations,sessions}.rs` |
| Operator simplification | implemented 2026-10-18; installed on the operator's Pixel 7 with REM 1.5.1 | `crates/atn-adapter/src/{binox4k-defaults.json,config.rs}`, `android/app/src/main/java/org/freetak/atn/{SettingsActivity,AtnPluginService,PluginPreferences}.java`, `AGENTS.md`, `docs/configuration.md` |
| Hardware qualification | partly performed 2026-10-18 (BLE connect, measurement decoding against the display); the rest open | checklist: `qualification.md` in this directory |

Decisions taken in Stage 1:

- Declaring `apiMinor` 3 means an older REM never binds the plugin; REM reports the
  incompatibility, the plugin's status shows "host not bound". No lower-minor probing.
- The REM observer is the phone's own fix; the operator confirms co-location per connection
  (`confirmColocation`). Without it the fix is `unverified`, blocking conversion and writes.
- Marker creation timeouts, `HostError` and Binder failures are recorded as `unknown` and never
  retried automatically; `InvalidRequest` is `rejected`; `PermissionDenied` makes the adapter
  read-only.
- Write-side observer handling changed from "stale fix cancels the session" to "stale fix holds
  writes and requests a fresh `map.observer.snapshot`"; the lifecycle tick refreshes at half the
  configured maximum observer age.
- A local failure that ends a session also sends `map.session.stop` once, so REM's session list
  matches the device; the ID stays terminal.
Decisions taken in Stage 2:

- Capture is an explicit native session (`startCapture`/`stopCapture`). Starting needs host ready
  with the configured symbol confirmed, a connection, a verified profile, valid semantics and the
  connection's co-location confirmation; refusals return `captureStarted:false` without touching
  other work. Capture ends on stop, disconnect/new connection, `hostStarted`, `hostStopped`,
  `configure`, `clearColocation` and `PermissionDenied`, and a gate check after every native call
  ends it whenever its preconditions disappear. Measurements outside capture stay
  `retained {"capture not started"}` and are never published later.
- The capture nonce is minted in Rust (`c<16 hex>-<n>`): one OS-seeded `RandomState` draw per
  process plus a per-process capture counter. Android supplies nothing new. Keys are
  `<nonce>/<connectionId>/<sequence>`, within REM's 128 ASCII-graphic bound.
- A creation that becomes unknown (timeout, `HostError`, `expired`, `binder.*`) gets exactly one
  same-key, same-payload replay from the tick after `createRetryDelayMs` (new required config
  field), only while the same Binder host session and capture are active. Afterwards, or when
  capture/host/connection ends first, it is `unknown {resolution:"final"}` forever. Nothing is
  persisted. This supersedes the Stage 1 "never retried" rule for unknown creations only.
- Ending capture cancels unanswered observer snapshots of that capture but lets an already-sent
  `map.markers.create` record REM's answer.

Decisions taken in Stage 3:

- Concurrency composes one `atn-domain` `SyncSession` per REM session under a single scheduler in
  `atn-adapter` (`runtime/sessions.rs`) instead of generalizing the domain type: each session keeps
  its own queue, slot ownership, coalescing and reports, and the scheduler allows one transfer
  (and one frame) device-wide, routes the next reply to that transfer's session and round-robins
  between sessions with queued work. `SyncSession` only gained read-only accessors.
- Operation IDs are per session, so echoes are matched on (connection, session, operation, frame)
  against the single recorded in-flight frame. An echo for an ended session frees the device and
  advances nothing. A single session ending emits `sessionEnded` rather than the global
  `cancelWrites`, so its frame already on the radio completes instead of tearing down GATT for
  the others.
- A partial icon transfer cut off by any session end (or timeout, or a global end) closes GATT and
  ends every session and allocation; reconnect plus new Share actions are required.
- Reconciliation (`reconcileIntervalMs`) lists sessions on the tick; only sessions accepted before
  the list request was sent can be judged absent. Transient list failures wait for the next
  interval; transient `objects.get` failures end only that session.
- `notifications.raise` (`{title, body}`, checked in REM `plugin_host_api.rs` and
  `PluginCoordinator.java`) is sent once per rejected session and is fail-soft.
- `setIcon`/`confirmDisplay` now require `sessionId`; refusing an unknown session/object answers
  `accepted:false` without ending sessions (previously any refusal ended the single session).

Decisions taken in the operator simplification (2026-10-18):

- The user's rule for all projects is convention over configuration, and the main target is the
  user's own Binox 4K. `AGENTS.md` now says so; it replaces "operational policies must be explicit
  configuration".
- One embedded defaults file (`binox4k-defaults.json`) is the single source; partial JSON
  overrides merge onto it in Rust (`AdapterConfig::resolve`, native `resolveConfig`, `configure`
  without `config`). Android reads Bluetooth limits from the effective configuration.
- The operator flow is Scan → tap Binox → "Is this phone with the binocular?". Yes confirms
  co-location for that connection and keeps capture running while "send measurements" is on; the
  Rust gates are unchanged and satisfied by the defaults. Typed evidence boxes, Start/Stop capture
  and JSON policy editors moved out of the main flow.
- North reference, pitch sign, range type, display slots and firmware are defaults marked "to
  confirm", not blockers.
- Diagnostics: the service logs REM events and decisions under `AtnPlugin`. The first REM share
  on the device never reached the plugin (no session listed, no rejection recorded); the cause is
  open and should be retested with these logs.

Decisions taken in Stage 4:

- Coexistence review found no adapter toggling, bonding, unrelated-device disconnects or
  unbounded/implicit discovery; per-file evidence is in `docs/radio-coexistence.md`. No behaviour
  change was needed there.
- The native boundary now carries GATT identity: `connected` must report the ATN service and its
  notification/write characteristics (otherwise refused), `notification` names its characteristic
  (only the notification characteristic is decoded) and `transportResult` names the write
  characteristic and a typed `error {code, gattStatus?}`. The connection ID remains the connection
  generation.
- RNode/goTenna switching is documented as operator-managed with no software interlock.
- The goTenna plugin is a separate APK/Binder service with its own Rust core; no shared transport
  library and no Linux console port. The alignment note it builds on
  (`docs/plugin-architecture-alignment.md`) is local and uncommitted, so the architecture note
  restates the parts it relies on.

- The SDK AAR is built from the REM checkout into an external scratch directory; nothing is
  vendored and REM is not modified. Build inputs stay in environment variables.
