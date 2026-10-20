#!/usr/bin/env bash
# Builds the web shell into platforms/web/dist (the @rs-editor/web package).
#   crates/rs-editor-web/build.sh [--debug]
# Needs Emscripten (EMSDK, default ~/emsdk) and the wasm32-unknown-emscripten target.
set -euo pipefail
cd "$(dirname "$0")/../.."
export EMSDK="${EMSDK:-$HOME/emsdk}"
export EMSDK_PYTHON="${EMSDK_PYTHON:-$(command -v python3.13 || command -v python3)}"
# shellcheck disable=SC1091
source "$EMSDK/emsdk_env.sh" >/dev/null 2>&1
profile=release
flag=--release
if [[ "${1:-}" == "--debug" ]]; then profile=debug; flag=; fi
export EMCC_CFLAGS="-s ERROR_ON_UNDEFINED_SYMBOLS=0 -s MAX_WEBGL_VERSION=2 -s MIN_WEBGL_VERSION=2 \
  -s MODULARIZE=1 -s EXPORT_NAME=createRsEditor -s ALLOW_MEMORY_GROWTH=1 \
  -s EXPORTED_RUNTIME_METHODS=GL,HEAPU8"
# Skia's C libraries (libpng, libjpeg, FreeType) use JavaScript-style
# setjmp, which cannot mix with wasm exceptions; without unwinding Rust
# needs no exceptions at all.
export CARGO_PROFILE_RELEASE_PANIC=abort CARGO_PROFILE_DEV_PANIC=abort
export CARGO_TARGET_WASM32_UNKNOWN_EMSCRIPTEN_LINKER="$PWD/crates/rs-editor-web/emcc-link.sh"
cargo build -p rs-editor-web --target wasm32-unknown-emscripten $flag
out=platforms/web/dist
mkdir -p "$out"
cp "target/wasm32-unknown-emscripten/$profile/rs-editor-web.js" "$out/"
cp "target/wasm32-unknown-emscripten/$profile/rs_editor_web.wasm" "$out/"
ls -la "$out"
