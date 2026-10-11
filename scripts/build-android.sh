#!/usr/bin/env bash
set -euo pipefail

# Build inputs remain external. This script never publishes or copies the SDK source.
atn_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
: "${REM_SDK_AAR:?Set an absolute path to the external, licensed REM SDK AAR}"
: "${REM_SDK_SHA256:?Set the exact SHA-256 of that AAR}"
: "${REM_SDK_COMMIT:?Set its full REM source commit}"
: "${REM_HOST_PACKAGE:?Set the trusted REM host package}"
: "${REM_HOST_FINGERPRINTS:?Set the trusted REM host SHA-256 certificate fingerprints}"
: "${ANDROID_HOME:?Set the installed Android SDK path}"
atn_gradle="${ATN_GRADLE:-gradle}"
command -v "$atn_gradle" >/dev/null || { printf '%s\n' 'Set ATN_GRADLE to Gradle 8.11.1.' >&2; exit 1; }
atn_variant=Debug
atn_cargo_profile=debug
atn_cargo_args=()
atn_offline=()
for atn_arg in "$@"; do
    case "$atn_arg" in
        --release) atn_variant=Release; atn_cargo_profile=release; atn_cargo_args+=(--release) ;;
        --offline) atn_offline+=(--offline) ;;
        *) printf 'Unknown argument: %s\n' "$atn_arg" >&2; exit 2 ;;
    esac
done

if [[ -n "${ATN_NATIVE_LIBS_DIR:-}" ]]; then
    atn_libs_dir="$ATN_NATIVE_LIBS_DIR"
else
    : "${ANDROID_NDK_HOME:?Set NDK 27.0.12077973 or provide ATN_NATIVE_LIBS_DIR}"
    : "${ATN_ANDROID_ABIS:?Set the Android ABIs to build, e.g. arm64-v8a,x86_64}"
    [[ "$(uname -s)-$(uname -m)" == Linux-x86_64 ]] || { printf '%s\n' 'For this host, provide prebuilt ATN_NATIVE_LIBS_DIR.' >&2; exit 1; }
    atn_toolchain="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin"
    atn_libs_dir="$atn_root/android/build/native-libs"
    read -r -a atn_abis <<< "${ATN_ANDROID_ABIS//,/ }"
    for atn_abi in "${atn_abis[@]}"; do
        case "$atn_abi" in
            arm64-v8a) atn_target=aarch64-linux-android; atn_clang=aarch64-linux-android26-clang ;;
            armeabi-v7a) atn_target=armv7-linux-androideabi; atn_clang=armv7a-linux-androideabi26-clang ;;
            x86_64) atn_target=x86_64-linux-android; atn_clang=x86_64-linux-android26-clang ;;
            x86) atn_target=i686-linux-android; atn_clang=i686-linux-android26-clang ;;
            *) printf 'Unsupported Android ABI: %s\n' "$atn_abi" >&2; exit 2 ;;
        esac
        [[ -x "$atn_toolchain/$atn_clang" ]] || { printf '%s\n' 'NDK compiler unavailable.' >&2; exit 1; }
        atn_linker_var="CARGO_TARGET_${atn_target^^}_LINKER"
        atn_linker_var="${atn_linker_var//-/_}"
        env "$atn_linker_var=$atn_toolchain/$atn_clang" \
            cargo build --manifest-path "$atn_root/Cargo.toml" --locked -p atn-adapter \
            --target "$atn_target" "${atn_cargo_args[@]}" "${atn_offline[@]}"
        atn_target_dir="${CARGO_TARGET_DIR:-$atn_root/target}"
        mkdir -p "$atn_libs_dir/$atn_abi"
        cp "$atn_target_dir/$atn_target/$atn_cargo_profile/libatn_adapter.so" "$atn_libs_dir/$atn_abi/"
    done
fi

"$atn_gradle" --no-daemon -p "$atn_root/android" "${atn_offline[@]}" \
    "-PremSdkAar=$REM_SDK_AAR" "-PremSdkSha256=$REM_SDK_SHA256" "-PremSdkCommit=$REM_SDK_COMMIT" \
    "-PremHostPackage=$REM_HOST_PACKAGE" "-PremHostFingerprints=$REM_HOST_FINGERPRINTS" \
    "-PnativeLibsDir=$atn_libs_dir" \
    ":app:test${atn_variant}UnitTest" ":app:lint${atn_variant}" ":app:assemble${atn_variant}"

printf 'APK output: %s/android/app/build/outputs/apk/%s/\n' "$atn_root" "${atn_variant,,}"
printf '%s\n' 'Local APK build only. Release signing, SDK redistribution rights, REM map host support and hardware verification remain separate gates.'
