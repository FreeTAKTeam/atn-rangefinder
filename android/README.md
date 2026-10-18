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

Open the **ATN Rangefinder** launcher activity, or **Open Android settings** from its REM configuration panel. Nothing needs configuring: the native core's built-in Binox 4K defaults (`crates/atn-adapter/src/binox4k-defaults.json`) supply device semantics and Bluetooth limits. Tap **Scan for Binox** (Bluetooth permission is requested on first use; the scan is filtered to the ATN service unless **Scan all Bluetooth devices** is ticked under Advanced, and stops after the default timeout), tap your Binox and answer **Is this phone with the binocular?**. Do not pair the Binox in Android settings. Connection always verifies the ATN service, characteristics and notification subscription; nothing reconnects or resumes by itself.

With **Yes**, the service confirms co-location for that connection and keeps capture running while **Send Binox measurements to REM as markers** is on (the default, saved privately), retrying the native start every few seconds until REM and the Binox are both ready. With **No**, measurements stay local and REM shares are refused. REM markers within the Binox's range are shown automatically, nearest first, once the operator allows **Read all map markers near this device** for the plugin in REM 1.5.2 or later (`mapNearbyRead`, declared in the manifest). Points shared from a REM marker sheet are shown as well; up to four shares run at once, each taking the first free display slots, and a share that does not fit is refused on its own with a REM notification. The top of the screen shows REM, Binox, measurement and display state plus the last measurement in plain words.

**Show advanced** holds the optional partial JSON override (merged onto the defaults by the native `resolveConfig`/`configure` commands; `{}` restores the defaults), the effective configuration, the scan-all option, stopping one session by ID, marker style, a local 40×40 PNG icon for a shared marker, operator display confirmation, and raw native status. Builds before this one stored full `transport`/`config` JSON; those keys are removed on first start.

The service logs REM events and its decisions under the tag `AtnPlugin`, without coordinates, device addresses or raw bytes. Selected addresses remain in private preferences and are excluded from backups. Host Binder envelopes keep the SDK's 65,536-byte bound; the JNI request bound is 393,216 bytes.

## Host and acceptance boundary

The plugin declares REM plugin API 1.3 and the `mapMarkersRead`, `mapMarkersWrite`, `mapObserverRead`, `mapSelectionRead` and `notificationsRaise` capabilities; the operator grants them in REM. REM `91124cffd8f7fa7df94684b672c2228d97f0522d` (1.5.1) implements this API; an older REM shows the plugin as incompatible and never binds it. On start the adapter checks the configured symbol against `map.symbols.list`; while capture runs, eligible observations become shared markers via `map.markers.create`, and REM selection sessions (**Share with ATN Rangefinder**) drive device display. No generic event or telemetry record is used to fake a map point.

Lifecycle defenses cover connection setup timeout, subscription failure, permission revocation, Binder death, pending-host timeout, bounded write scheduling, cancellation and write timeout. The committed JVM scheduler tests establish queue ordering, bounds, ownership of bytes and cancellation without replay. They do not establish Android OS lifecycle behavior or device semantics. No real REM map exchange, supported firmware, physical BLE connection or display rendering is claimed by a local compile/APK build.

## Local build evidence

On 2026-10-10 the SDK was rebuilt from the inspected REM source into an external temporary build directory, with its source checkout unchanged. Its AAR SHA-256 is `dd81949b0ab07a55469736c147505949a901abc62512a3e524fde2dfa61150bd`. Offline Java compilation, four JVM queue tests, Android lint (no issues) and debug APK assembly passed with an ARM64 native library. The build used a deliberately synthetic all-zero host-certificate fingerprint, so the resulting APK cannot authenticate a real REM host. It is a local packaging check; it is not a field-ready installation or verified map exchange. Build output and external AAR are ignored and are not source deliverables.
