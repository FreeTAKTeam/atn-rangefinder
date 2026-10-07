# Evidence register

## Labels used throughout

- **CAPTURE-REPORTED:** observations established by analysis of the authorized test capture; raw captures are intentionally not published.
- **APK-DERIVED:** static behavior of Tactical Map 1.0.5-34. It shows what that application implements, not everything device firmware accepts.
- **UNVERIFIED:** assumptions, unresolved semantics or untested implementation behavior.
- **SYNTHETIC:** deliberately invented test input; not evidence of a real device value.

## Application identity and reproducibility

The official [Google Play listing](https://play.google.com/store/apps/details?id=com.atn.tacticalnav) identifies Tactical Map, American Technologies Network Corp, package `com.atn.tacticalnav`.

The analyzed sample was obtained from the public [APKPure listing](https://apkpure.net/tactical-map/com.atn.tacticalnav/download), not directly from ATN or Google Play. Its manifest reports version name `1.0.5-34`, version code `34`, minimum Android API 24 and target API 35.

- XAPK SHA-256: `ba22e6bf19a8ffff533306e4c786f8e550fd14578fd432130047ce571c587fe7`
- Base APK SHA-256: `0d53bc025d4721568188637d6037ba0dd30e4035297d45504f73ff88361d21e6`
- Signing certificate SHA-256: `6fe0d1bf0b9fd6ee1af265a4b6c461cac3f1c4e6f2be821dc0ea6ab6b313cc18`
- Signing certificate SHA-1: `483d0d5dc2e1589d26b7c702a1ae2c900254b91f`

Google's apksig 8.11.1 verified base and two configuration APK signatures under schemes v2 and v3, with no warnings or errors. This proves signature integrity for those files and consistency with the distributor's fingerprint. An independent official Play-sourced signer comparison was not performed.

Static analysis used official jadx 1.5.6. It processed 6,143 classes and reported 115 decompilation errors. Some coroutine methods needed simple-mode inspection. No APK was installed or executed for this static analysis.

## Static evidence map

Names below identify methods in the analyzed binary. Generated Java line numbers are conveniences for that exact decompilation, not original source line numbers. No decompiled source is redistributed.

| Finding | Evidence |
|---|---|
| Service/read/write UUIDs | `com.atn.radarcommunication.ble.RadarRepository.getUuidService/getUuidRead/getUuidWrite`, generated lines 276–293 |
| BLE scan and connection | `com.atn.networkcore.provider.ble.BleNetworkProvider`, 300, 435, 487–500 |
| Notifications and byte-array delivery | `BleNetworkProvider.listenData/setCharacteristicNotification/onReceiveCharacteristic`, 583–639 |
| Write without response | `BleNetworkProvider$sendCommandAsync$2$1.invokeSuspend`, 50–64 |
| Incoming measurement fields | `GetPositionResponse` constructor, 16–21; `ConstantsKt.toShort`, 63–65 |
| Response routing | `RadarRepository.onReceiveCommandMessage`, 439–473; push handler, 517–522 |
| Outbound marker record | `SetTargetPositionRequest.toBytes`, 144–154 |
| Icon frame header | `SendIconRadarRequest.toBytes`, 137–156 |
| Icon fragmentation and PNG | `RadarRepository.getImagePartsFromBitmap/bitmapToByteArray/cropBitmapForRadar`, 1564–1644 |
| Comment excluded from device marker | `com.atn.tacticalnav.data.NavMarker.toRadarMarker`, 158–162 |
| New marker then icon; independent updates | `RadarRepository.setMarker`, 1037–1102 |
| Phone position plus relative observation | `MarkerModel.saveMarker`, 171–178; `Coordinate`, 97–107; `LocationCalculator`, 60–81 |

## Capture observations available at initial drafting

CAPTURE-REPORTED: discovery mapped notification to ATT handle 0x0009 and writes to 0x000c for the observed session. These are session/device-specific handles, not constants to ship.

CAPTURE-REPORTED: the test trace shows MTU request and response both 517, while observed full application ATT values were 509 bytes. This separates negotiated MTU from the app's own 512-based chunk sizing.

CAPTURE-REPORTED: incoming eight-byte command-1 measurements occurred in a measurement test. The decoder interpretation has not been independently correlated against all displayed range/bearing/pitch values.

CAPTURE-REPORTED: adding an application POI produced a visible POI on the binocular. Outbound traffic included a ten-byte command-2 value followed by command-7 values. Reported value lengths were 509/454 and 509/509/280. Six-byte inbound notifications accompanied these transfers. The exact acknowledgment tails remain unresolved.

CAPTURE-REPORTED: a user-entered test label was a comment, not a name. No corresponding plaintext appeared in outgoing writes, consistent with the APK's comment-excluding mapping.

Private evidence must remain private. Public summaries should omit actual coordinates, device addresses, serial numbers, exact operational timestamps and original packet payloads.

## Reference documents

- [ATN product description](https://publicsector.atncorp.com/ttbn-4/): high-level Bluetooth measurement/map behavior, not a protocol specification.
- [Android Bluetooth debug logging](https://source.android.com/docs/core/connect/bluetooth/verifying_debugging): enable full HCI logging and restart Bluetooth.
- [Android bugreport export](https://developer.android.com/studio/debug/bug-report).
- [Wireshark ATT reference](https://www.wireshark.org/docs/dfref/b/btatt.html).

## Additional capture validation supplied during drafting

Capture analysis reconstructed six complete PNGs. All are 40×40 RGBA, with valid PNG chunk CRCs and exact declared lengths: five are 935 bytes (495+440 payload fragments), one is 1,256 bytes (495+495+266). These are validated image files, not inferred solely from packet lengths. The application header's separate CRC-named field is zero.

Replies to commands 2 and 7 carry the corresponding command, answer byte 3 and three trailing zero bytes. The app's generic response reader ignores the trailing bytes. Their exact firmware meaning remains unknown; no captured payload is reproduced as a public fixture.
