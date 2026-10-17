# Execution handoff

Plan: `plan.md` in this directory. Status and evidence: `../../implementation-status.md`.

| Stage | State | Where |
|---|---|---|
| 1. Host API migration to REM plugin API 1.3 | implemented, software-verified 2026-10-17 | `crates/atn-adapter/src/{rch.rs,config.rs,runtime.rs,runtime/*}`, `android/app/src/main/{AndroidManifest.xml,java/org/freetak/atn/AtnPluginService.java,java/org/freetak/atn/SettingsActivity.java,assets/rem-plugin-config/index.html}`, `docs/map-contract.md` |
| 2. Automatic measurement capture | partly in place: per-observation idempotent creation, typed outcomes, co-location gate, bounded provenance notes | missing: explicit Start/Stop capture controls, bounded same-key replay of an unknown outcome while still bound |
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
- The SDK AAR is built from the REM checkout into an external scratch directory; nothing is
  vendored and REM is not modified. Build inputs stay in environment variables.
