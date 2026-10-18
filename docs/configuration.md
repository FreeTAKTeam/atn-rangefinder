# Configuration and operation

Convention over configuration: the plugin works with your Binox 4K out of the box. There is
nothing to fill in.

## Operating it

1. In REM, trust and enable **ATN Rangefinder**, grant its map capabilities and notifications,
   and start the node.
2. Open the plugin, tap **Scan for Binox** (Bluetooth permission is requested the first time) and
   tap your Binox. Do not pair it in Android's Bluetooth settings.
3. Answer **Is this phone with the binocular?** REM uses the phone's position for the Binox's.
   - **Yes:** each new measurement becomes a shared REM marker, and REM markers within the
     Binox's range appear on the Binox automatically, nearest first.
     This needs a one-time REM 1.5.2 grant: **Settings → Plugins → ATN Rangefinder → Read all map
     markers near this device**. REM's per-marker **Share with ATN Rangefinder** still works too.
   - **No:** measurements stay on the phone and shares are refused.
4. The top of the screen says what is happening in plain words: REM, Binox, whether
   measurements go to REM, how many REM markers are on the Binox, and the last measurement.

**Send Binox measurements to REM as markers** (on by default) turns marker creation off and on.
Measurements taken while it is off, or before connecting, are never sent later.

The automatic set is refreshed every `reconcileIntervalMs` (5 s) from REM's `map.markers.nearby`
within `semantics.maxDistanceM` (10 km) of the phone. It uses the display slots that REM shares
leave free (slots 1–16 by default). Unchanged markers are not rewritten; when the nearest set
changes, the set is replaced. **Stop showing REM markers** ends the automatic set and all REM
shares until the next Connect. Releasing a slot does not clear the Binox display. Up to four REM
shares run at once; a share that does not fit is refused on its own, with the reason raised as a
REM notification.

**Phone position.** With **Yes**, the plugin keeps this phone's GPS running (1 s updates) while
the Binox is connected, asking once for location permission. That fix places measurements and
aims markers. REM's own position is the fallback; it is only refreshed at REM's telemetry
interval (6 min by default) and not while REM is in the background. Both must be under 10 minutes
old and within 50 m accuracy. The Binox display is re-aimed only after the phone moved more than
10 m. On GrapheneOS without network location, the first GPS fix outdoors can take a minute or
more, and there may be none indoors.

**Colours and icons.** Each marker's Binox colour follows its REM symbol's NATO affiliation:
hostile red, unknown yellow, neutral green, friendly blue. Generic symbols use `color` (blue 4).
`markerType` 1, the Binox 4K default, makes the radar dots show those colours. With `sendIcons`
(on by default), each marker also gets a 40×40 RGBA PNG icon after its position: the white NATO
frame (rectangle, diamond, square, clover) or a dot on a circle in its colour. The icon is resent
only when the symbol changes.

**REM permissions.** Allow ATN Rangefinder these in REM (**Settings → Plugins**):
- "Read its own map markers" and "Add and edit its own map markers";
- "Read this device's location";
- "Read map objects you share with it" and "Read all map markers near this device";
- "Raise notifications".

A missing grant only disables its own feature. The plugin's status says which grant to allow.

The plugin logs every REM event and decision under the Android log tag `AtnPlugin`
(`adb logcat -s AtnPlugin`), without coordinates or device addresses.

## Built-in defaults

All settings come from `crates/atn-adapter/src/binox4k-defaults.json`:

- Device: Binox 4K, treated as verified for this target.
- Units: 1 raw unit = 1 m and 1°, both matched against the device display in the 2026-10-18 field
  test.
- Geometry: true north, positive-up pitch, slant range and WGS84. These are still to confirm on
  the device.
- REM marker: symbol `marker`, name prefix `ATN`.
- Display slots: 1–16 (still to confirm).
- Phone position: fix up to 30 s old, accuracy within 25 m.
- Bluetooth: 512-byte MTU request, 5 s scan, 10 s connect, 3 s write timeouts.
- REM timing: 2 s request timeout, 500 ms create-replay delay, 5 s session reconciliation.

The Rust validation gates (verified profile, confirmed semantics, observer freshness and
accuracy, co-location) still apply behind these defaults. The defaults are what satisfies them.

## Overrides (advanced, optional)

Under **Show advanced → Configuration override**, enter only what should differ, as partial JSON
merged onto the defaults, for example `{"color":2}` or `{"transport":{"scanTimeoutMs":8000}}`.
Objects merge key by key, other values replace the default, unknown keys are rejected, and `{}`
(or **Reset to Binox 4K defaults**) returns to the defaults. **Show effective configuration**
displays the merged result. The native `configure` and `resolveConfig` commands take the same
partial object; omitted, they use the defaults. Field meanings are listed in
[`map-contract.md`](map-contract.md); `examples/synthetic-config.json` is a full synthetic
example used by tests.

Bluetooth characteristic UUIDs and wire formats are protocol constants, and WGS84 constants are
mathematical definitions. GATT write capacity is negotiated at connect. Implementation ceilings
(4096 queued/request/retained entries, 16 MiB decoded image/queued bytes, the protocol's 16-bit
icon length) only bound memory.

Running the Binox alongside REM's RNode link, and switching REM between RNode and goTenna, is
described in [`radio-coexistence.md`](radio-coexistence.md); it is operator-managed and has not
yet been exercised on hardware.

Transport completion, the Binox's reply and actual display are shown separately; only the
operator's display confirmation (under Advanced) records that a marker was seen on the Binox.
