#!/usr/bin/env bash
# Builds libopenrich_android.so into demo/src/main/jniLibs.
#   platforms/android/build-native.sh [--debug] [abi...]   (default: arm64-v8a)
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../.." && pwd)"
sdk="${ANDROID_HOME:-$HOME/Library/Android/sdk}"
ndk="${ANDROID_NDK_HOME:-$(ls -d "$sdk"/ndk/* | sort -V | tail -1)}"
host=$(ls "$ndk/toolchains/llvm/prebuilt" | head -1)
bin="$ndk/toolchains/llvm/prebuilt/$host/bin"
api=26
profile=release; flag=--release
if [[ "${1:-}" == "--debug" ]]; then profile=debug; flag=; shift; fi
abis=("$@"); [[ ${#abis[@]} -gt 0 ]] || abis=(arm64-v8a)
export ANDROID_NDK_HOME="$ndk" ANDROID_NDK="$ndk" ANDROID_NDK_ROOT="$ndk"
for abi in "${abis[@]}"; do
  case "$abi" in
    arm64-v8a) target=aarch64-linux-android; clang=aarch64-linux-android$api-clang ;;
    x86_64) target=x86_64-linux-android; clang=x86_64-linux-android$api-clang ;;
    *) echo "ABI tidak didukung: $abi" >&2; exit 1 ;;
  esac
  env_target=$(echo "$target" | tr 'a-z-' 'A-Z_')
  export "CARGO_TARGET_${env_target}_LINKER=$bin/$clang"
  # 16 KB pages are required from Android 15 on.
  export "CARGO_TARGET_${env_target}_RUSTFLAGS=-C link-arg=-Wl,-z,max-page-size=16384"
  export "CC_${target//-/_}=$bin/$clang" "CXX_${target//-/_}=$bin/$clang++" "AR_${target//-/_}=$bin/llvm-ar"
  (cd "$root" && cargo build -p openrich-android --target "$target" $flag)
  out="$here/demo/src/main/jniLibs/$abi"
  mkdir -p "$out"
  cp "$root/target/$target/$profile/libopenrich_android.so" "$out/"
  "$bin/llvm-strip" --strip-unneeded "$out/libopenrich_android.so"
  ls -la "$out"
done
