# Radio coexistence: ATN BLE alongside REM's RNode link

**None of the operation described here has been exercised on hardware yet.** It describes what the
software is built to do and how the operator is expected to run it; the corresponding checks are
open gates in [`goals/atn-rem-plugin/qualification.md`](goals/atn-rem-plugin/qualification.md).

## What the plugin does and does not touch

The ATN plugin owns exactly one Bluetooth LE GATT client: the device the operator picked, on the
ATN service. REM owns its RNode link (BLE or Bluetooth Classic) in its own process. The two share
the phone's Bluetooth controller but no software state.

Code review of 2026-10-17 (`android/app/src/main/java/org/freetak/atn/`):

| File | Evidence |
|---|---|
| `BleTransport.java` | Connects only to the operator-selected address with `connectGatt(…, autoConnect=false, TRANSPORT_LE)`; refuses to start when the adapter is off (never enables or disables it); never bonds, unbonds or lists bonded devices; `close()` disconnects and closes only its own `BluetoothGatt`; after service discovery it requires the ATN service with its notify and write-without-response characteristics or closes; notifications are accepted only from the ATN notification characteristic of the current GATT object; one GATT object at a time, no reconnect. |
| `SettingsActivity.java` | Discovery starts only from **Scan and choose a device**; the scan filter is the ATN service UUID unless the operator ticks **scan all**; it stops after the configured `scanTimeoutMs`, on selection, and when the activity pauses or is destroyed; the discovered list is bounded; no device is selected automatically; Bluetooth must already be on (the activity asks the operator to enable it in Android settings). |
| `AtnPluginService.java` | The service only drives `BleTransport` (connect on the operator's request, close on disconnect, host loss, native `disconnect` or write failure); writes go through one serial queue to the ATN write characteristic; no adapter, bonding, profile or other-device call exists. |
| `AndroidManifest.xml` | `BLUETOOTH_SCAN` is `neverForLocation`; `BLUETOOTH`/`BLUETOOTH_ADMIN` are limited to API ≤ 30, where Android requires them for scanning. |

The native boundary carries the transport identity Rust needs to keep a later goTenna plugin
separate: `connected` reports the connection ID (a fresh UUID per connection, the connection
generation), the negotiated write capacity and the bound service, notification and write
characteristic UUIDs (Rust refuses any other identity); `notification` names its characteristic
(Rust decodes measurements and replies only from the ATN notification characteristic); and
`transportResult` echoes the write characteristic, connection, session, operation and frame, with a
typed `error {code, gattStatus?}` (`gattStatus`, `writeRejected`, `timeout`, `cancelled`,
`notAuthorized`, `queueRejected`) when a write did not complete.

## Supported arrangements (unverified)

- **RNode over BLE + ATN over BLE.** Both are BLE GATT clients on the same controller. Connect the
  RNode in REM first and let the REM node start; then connect the ATN device from the plugin. The
  plugin never disconnects the RNode. If the controller refuses a second connection, the ATN
  connection fails with a status message and REM is unaffected.
- **RNode over Bluetooth Classic + ATN over BLE.** Classic and LE share the controller's radio
  time. The same order applies: REM link first, ATN second.

Scanning (especially **scan all**) uses radio time that the existing links also need. Scan only
when choosing a device, keep `scanTimeoutMs` short, and do not leave the settings screen scanning.

## Switching between RNode and goTenna

Switching REM's link between an RNode and a goTenna is **operator-managed; there is no software
interlock** in this plugin or in REM. Disconnect the current link in its own app or plugin before
connecting the other one. The switch must leave the REM node running and the ATN plugin bound and
connected: the ATN connection, capture session and display sessions are not part of the RNode or
goTenna link and are not stopped by switching. If the ATN connection drops anyway, reconnect it
explicitly; nothing is resumed automatically, and display sessions must be shared again.

A goTenna plugin does not exist yet; see [`goals/gotenna-plugin/architecture.md`](goals/gotenna-plugin/architecture.md).
ATN + goTenna coexistence is a future qualification gate.
