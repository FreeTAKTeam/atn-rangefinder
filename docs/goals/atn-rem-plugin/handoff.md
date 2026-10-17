# Execution handoff

Plan: `plan.md` in this directory. Status and evidence: `../../implementation-status.md`.

| Stage | State | Where |
|---|---|---|
| 1. Host API migration to REM plugin API 1.3 | implemented, software-verified 2026-10-17 | `crates/atn-adapter/src/{rch.rs,config.rs,runtime.rs,runtime/*}`, `android/app/src/main/{AndroidManifest.xml,java/org/freetak/atn/AtnPluginService.java,java/org/freetak/atn/SettingsActivity.java,assets/rem-plugin-config/index.html}`, `docs/map-contract.md` |
| 2. Automatic measurement capture | implemented, software-verified 2026-10-17 (synthetic tests, JNI smoke, Android build with a synthetic host fingerprint; no real REM host or hardware) | `crates/atn-adapter/src/{config.rs,runtime.rs,runtime/{control,observations,host}.rs}`, `crates/atn-adapter/tests/exchange.rs`, `android/app/src/main/java/org/freetak/atn/{AtnPluginService,SettingsActivity}.java`, `docs/map-contract.md` |
| 3. Selected-point synchronization | single session in place: atomic slot allocation, rejection via `map.session.stop`, session-list recovery, observer refresh/hold | missing: up to 4 concurrent sessions with disjoint allocations, configured reconciliation interval, `notifications.raise` for rejection reasons |
| 4. Bluetooth and goTenna compatibility | unchanged transport boundary | documentation note pending |
| Hardware qualification | not performed | real REM 1.5.1 host, Binox, RNode BLE/Classic coexistence |

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

- The SDK AAR is built from the REM checkout into an external scratch directory; nothing is
  vendored and REM is not modified. Build inputs stay in environment variables.
