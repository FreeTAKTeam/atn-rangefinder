# Observed application protocol

This is a provisional interoperability description for map observations and POI display. It is not an ATN-issued specification. Offsets refer to the ATT characteristic value, after Bluetooth/HCI/ATT headers have been removed.

## Transport

APK-DERIVED: BLE GATT, with the phone as central/client in the inspected path.

| Role | UUID |
|---|---|
| Service | `d973f2e0-b19e-11e2-9e96-0800200c9a66` |
| Device → host notifications | `d973f2e1-b19e-11e2-9e96-0800200c9a66` |
| Host → device writes | `d973f2e2-b19e-11e2-9e96-0800200c9a66` |
| Notification CCCD | `00002902-0000-1000-8000-00805f9b34fb` |

The app enables notifications and writes binary values with Android write type 1, WRITE_TYPE_NO_RESPONSE. Application replies are distinct from an ATT write response. Do not infer successful device rendering from a completed host write.

The application requests MTU 512. CAPTURE-REPORTED: the actual Android trace shows request and response MTU 517, while full application values remain 509 bytes. Its fragment calculation uses that requested value; the inspected MTU callback does not store the negotiated value. A new implementation must respect the backend's actual write capacity. Support for other fragment sizes must be tested, not inferred from this app's fixed request.

APK discovery recognizes ATN service-data representations associated with model IDs 3 (Binox4k) and 4 (Binox4t), alongside other ATN models. Do not treat this as proof that all firmware variants implement the same commands. Use service discovery; never hard-code captured ATT handles.

## Lifecycle and state

APK-DERIVED path: scan → connect GATT → request MTU → discover services → enable notifications → initialize the display-marker state → process measurements and queued marker updates.

The app's connection initializer clears device markers and enables its radar display before observing its local marker list. A future adapter must not silently copy that destructive initialization: first define ownership of display slots and obtain appropriate user intent. Reconnection is not permission to clear all existing markers.

New display markers allocate a slot and queue position before icon transfer. Existing marker position and icon changes are compared and sent independently. Slot IDs are distinct from REM identifiers. Their persistent meaning, maximum supported count and behavior across reconnects need hardware verification.

The byte repository passes each notification as a byte array without measurement-level stream reassembly. It does not use the separate JSON-buffering path. No measurement checksum or length prefix was found in that path. Firmware capabilities outside it remain unknown.

## Device → host measurement: command 0x01

APK-DERIVED:

| Offset | Width | Meaning |
|---:|---:|---|
| 0 | 1 | Signature, expected by constants to be 0x01 |
| 1 | 1 | Command 0x01 |
| 2 | 2 | Distance, signed 16-bit BIG-endian |
| 4 | 2 | Pitch, signed 16-bit BIG-endian |
| 6 | 2 | Compass, signed 16-bit BIG-endian |

The constructor consumes at least eight bytes and ignores trailing bytes. It does not itself validate signature or exact length. A defensive adapter should validate its supported signature/command/length and preserve unknown frames as diagnostics rather than silently reinterpret them.

The application passes values without scaling to calculations expecting meters and degrees. This establishes its interpretation. It does not independently establish physical units under every device setting, validity/sentinel ranges, pitch polarity, compass north reference or calibrated accuracy. Signed parsing does not mean every negative distance is a valid observation.

### Deriving a map position

The app combines an observation with the phone's current fused-location latitude/longitude, accounts for inclination in horizontal distance, and projects along its compass field. The incoming packet contains no geographic coordinates.

A REM adapter must keep raw observation and derived position separate. Derivation requires an explicit observer fix, timestamps/freshness, coordinate reference/datum, co-location or known offset, and a bearing reference. Magnetic-vs-true north, device declination compensation and the appropriate location datum are UNVERIFIED. The app's spherical projection is not evidence of survey accuracy. Do not silently label a derived point as a verified device GNSS fix.

## Host → device display marker: command 0x02

APK-DERIVED, ten bytes:

| Offset | Width | Meaning |
|---:|---:|---|
| 0 | 1 | 0x01 |
| 1 | 1 | 0x02 |
| 2 | 1 | `(slot_id << 1) + marker_type` |
| 3 | 1 | `(enabled << 4) + color` |
| 4 | 2 | Distance, LITTLE-endian 16-bit |
| 6 | 2 | Pitch, LITTLE-endian 16-bit |
| 8 | 2 | Compass, LITTLE-endian 16-bit |

The app narrows integers to signed shorts before reversing the byte order. Do not reproduce unchecked truncation or wrapping: reject unsupported values until valid device limits are known. Marker-type values are 0 and 1; their application enum names do not establish a REM classification policy. Color values are red 0, orange 1, yellow 2, green 3, blue 4, gray 5, dark gray 6, and offline-user gray 7.

The app converts map coordinates into relative range/bearing using the current phone location and supplies pitch zero for this conversion. No latitude, longitude, comment or human-readable name is present in this record. A slot is an application-assigned display slot, not proof of persistent device storage.

## Host → device icon: command 0x07

APK-DERIVED: 14-byte header, followed by one fragment of a PNG file. All header fields wider than one byte are LITTLE-endian.

| Offset | Width | Meaning |
|---:|---:|---|
| 0 | 1 | 0x01 |
| 1 | 1 | 0x07 |
| 2 | 2 | Total frame count |
| 4 | 2 | Total PNG byte count |
| 6 | 2 | Current frame number, starting at 1 |
| 8 | 2 | Fragment byte count, excluding header |
| 10 | 2 | Slot ID |
| 12 | 2 | Field named CRC; this encoder supplies zero |
| 14 | variable | PNG fragment |

The app uses `requested_mtu - 3 - 14` bytes per full fragment. At 512, each full ATT value is 509 bytes and carries 495 PNG bytes. Reported lengths 509+454 imply 935 PNG bytes; 509+509+280 imply 1,256 PNG bytes. This arithmetic is not a substitute for validating each header and reassembled file.

The icon pipeline uses a selected drawable and marker background color, circle-crops/resizes content to 30×30, draws it at (5,5) on a 40×40 ARGB bitmap, then encodes PNG. This is a raster icon associated with structured marker data, not an image of the entire map. PNG compression is the image codec, not an additional documented ATN compression envelope. The CRC-named field being zero does not establish that checksums are unsupported by firmware.

CAPTURE-REPORTED: six reconstructed PNG files validate as 40×40 RGBA with correct lengths and PNG chunk CRCs; five are 935 bytes and one is 1,256 bytes. This corroborates the raster-icon interpretation.

The map marker conversion does not include its comment. Absence of a comment string in traffic is expected and does not imply encrypted text.

Static edge case: the inspected fragmentation loop uses the remainder for its final fragment even when total PNG length is an exact positive multiple of capacity. That appears to produce an empty final fragment. Treat this as an apparent app implementation defect requiring verification; do not copy it into a new encoder. Preserve all bytes in a correct synthetic reassembly test.

## Replies and six-byte notifications

APK-DERIVED `RadarResponse` reads byte 0 as signature, byte 1 as command and byte 2 as signed answer. Its success helper considers a nonnegative answer successful. The pending-request dispatcher, however, matches command byte and completes without invoking that helper. Remaining bytes are not interpreted by that class.

CAPTURE-REPORTED: six-byte notifications accompany marker/icon transfers. Replies carry the corresponding command (2 or 7), answer byte 3 and a three-byte zero tail. The nonnegative-success helper is consistent with accepting 3 but does not explain its meaning. Length alone cannot identify their fields or prove success. Do not label bytes 3–5 as slot, sequence, CRC or status without evidence. No eight-byte measurement is expected merely because a POI was added.

A command constant 0x70 is named as an answer constant but no active reference was located. Do not implement a speculative 0x70 ACK format. Simple-mode inspection suggests synthetic internal completion for single-frame icon transfers; this is not a wire acknowledgment.

Future implementation must define serialization, timeout and retry behavior conservatively. In particular, no blind replay on reconnect and no assumption that each PNG fragment receives an individual ACK. Device appearance of a POI, transport delivery and application status are separate observations.
