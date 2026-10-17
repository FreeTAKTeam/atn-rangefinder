# Android REM plugin boundary

This standalone APK subclasses the real REM `RemPluginService`, using an externally supplied SDK AAR. It calls the Rust `atn-adapter` through JNI; Java owns Android permission UI, device selection, GATT, bounded Binder requests and serial writes. It does not copy proprietary APK code or SDK source.

## Build

Required tools: Java 17 or later, Gradle 8.11.1, Android SDK 35 and NDK 27.0.12077973 when compiling Rust. Install the Rust target for each ABI you explicitly choose. The APK minimum API is 26. No Gradle wrapper binary or AAR is vendored.

Provide external inputs through environment variables:

```sh
export ATN_GRADLE=/path/to/gradle-8.11.1/bin/gradle
export ANDROID_HOME=/path/to/android-sdk
export ANDROID_NDK_HOME="$ANDROID_HOME/ndk/27.0.12077973"
export ATN_ANDROID_ABIS=arm64-v8a,x86_64
export REM_SDK_AAR=/absolute/path/to/rem-plugin-sdk.aar
export REM_SDK_SHA256=<sha256-of-that-aar>
export REM_SDK_COMMIT=<full-source-commit-for-that-aar>
export REM_HOST_PACKAGE=<host-package>
export REM_HOST_FINGERPRINTS=<sha256-host-signing-certificate>
bash scripts/build-android.sh --offline
```

Alternatively set `ATN_NATIVE_LIBS_DIR` to a generated directory containing `arm64-v8a/libatn_adapter.so` (or another supported ABI) and omit NDK/ABI compilation inputs. SDK checksum and full source pin are validated before compiling. Supply real host identity values for installation; a synthetic fingerprint used in compilation tests trusts no real host. The SDK checks package and certificate on every Binder call. The Android SDK AAR currently has no verified source redistribution license or Maven publication; obtain appropriate permission before distributing an APK that embeds it. Local build validation does not resolve that decision.

`--release` builds an unsigned release APK. Sign it using your private external signing process, then trust that publisher in REM. Keep keys and APKs out of Git. `--offline` uses cached dependencies; omit it when installing approved dependencies on a new build machine. The script runs JVM tests and Android lint before packaging. It does not install, connect to hardware or publish.

## Configure and use

Open the **ATN Rangefinder** launcher activity, or **Open Android settings** from its REM configuration panel (REM loads `assets/rem-plugin-config/index.html` and launches the activity through `actionResult.activity`). Grant Bluetooth permissions, save explicit transport policy, then tap a discovered device. No device is chosen automatically. Service-filtered discovery is the initial scan mode; **Scan all BLE devices** is an explicit option for devices whose advertisement does not list the service UUID. Both modes use the configured timeout and result bound. Connection always verifies the required ATN service, characteristics and CCCD. **Connect selected device (read-only)** must be pressed after the REM node has enabled this trusted plugin; no connection or write queue resumes across restart or reconnect.

Transport settings require `requestMtu`, `connectTimeoutMs`, `writeTimeoutMs`, `hostRequestTimeoutMs`, `maxPendingHostRequests`, `maxQueuedWrites` and `scanTimeoutMs`. Timeouts are positive milliseconds, with a platform ceiling of one hour; queue bounds are positive, with a platform ceiling of 4096. These are resource safety ceilings, not an approved device policy. MTU is bounded by the BLE protocol's 23–517 range. Actual write capacity comes from the negotiated MTU, capped by the 512-byte GATT attribute limit. CCCD registration must complete before the connection becomes ready. The scan result list uses the configured pending-request bound.

Paste and edit the map/device configuration from [`examples/synthetic-config.json`](../examples/synthetic-config.json). Its verification flags remain false and its inputs are synthetic. Obtain real supported model/firmware and reference evidence before enabling derived points or display writes. The activity validates configuration through Rust, combines it with the saved transport policy, and persists it locally. No location, device address or raw bytes are sent to Android logs; selected addresses remain in private preferences and are excluded from backups.

Host Binder envelopes retain the SDK's 65,536-byte UTF-8 bound. The local JNI request bound is 393,216 bytes, permitting a full u16-length PNG represented as a JSON byte array. Icon reads are also bounded by the operator's `maxEncodedBytes` setting and the protocol's 65,535-byte length.

After connecting, **Confirm phone is at the binocular** with a short evidence note; the confirmation is connection-scoped. **Start capture** (enabled only after that confirmation, and refused by the native core unless the host is ready, the profile verified and the semantics valid) makes each new measurement of this connection a shared REM marker; **Stop capture** ends it, and it also ends on disconnect, REM host restart/stop, configuration change, withdrawn co-location or `PermissionDenied`. The capture state is shown under the buttons and in the REM configuration page state; it is never saved. Share points with the plugin from a REM marker sheet; the adapter allocates `slotPool` slots in REM's selection order, rejects and stops a selection that does not fit, and serves one session at a time. **Stop display session** asks REM to stop the session and retains read-only notifications when no write is in flight. Cancelling an in-flight GATT write tears down that connection so its late callback cannot complete a replacement write. Reconnect and authorize again explicitly. Choose a local PNG for a selected PLE or change selected-set type/color through the activity. Use display confirmation only after visually observing the expected result; application replies are opaque and transport completion does not prove rendering.

## Host and acceptance boundary

The plugin declares REM plugin API 1.3 and the `mapMarkersRead`, `mapMarkersWrite`, `mapObserverRead` and `mapSelectionRead` capabilities; the operator grants them in REM. REM `91124cffd8f7fa7df94684b672c2228d97f0522d` (1.5.1) implements this API; an older REM shows the plugin as incompatible and never binds it. On start the adapter checks the configured symbol against `map.symbols.list`; during a started capture, eligible observations become shared markers via `map.markers.create`, and REM selection sessions (**Share with ATN Rangefinder**) drive device display. No generic event or telemetry record is used to fake a map point.

Lifecycle defenses cover connection setup timeout, subscription failure, permission revocation, Binder death, pending-host timeout, bounded write scheduling, cancellation and write timeout. The committed JVM scheduler tests establish queue ordering, bounds, ownership of bytes and cancellation without replay. They do not establish Android OS lifecycle behavior or device semantics. No real REM map exchange, supported firmware, physical BLE connection or display rendering is claimed by a local compile/APK build.

## Local build evidence

On 2026-10-10 the SDK was rebuilt from the inspected REM source into an external temporary build directory, with its source checkout unchanged. Its AAR SHA-256 is `dd81949b0ab07a55469736c147505949a901abc62512a3e524fde2dfa61150bd`. Offline Java compilation, four JVM queue tests, Android lint (no issues) and debug APK assembly passed with an ARM64 native library. The build used a deliberately synthetic all-zero host-certificate fingerprint, so the resulting APK cannot authenticate a real REM host. It is a local packaging check; it is not a field-ready installation or verified map exchange. Build output and external AAR are ignored and are not source deliverables.
