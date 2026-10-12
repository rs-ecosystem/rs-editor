#!/usr/bin/env bash
# Linker for wasm32-unknown-emscripten (set by build.sh).
#
# rustc always links with -fwasm-exceptions, but Skia's prebuilt C
# libraries (libpng, libjpeg, FreeType) use Emscripten's JavaScript-based
# setjmp/longjmp, which cannot be combined with wasm exceptions. The web
# build uses panic=abort, so Rust needs no exceptions: drop the flag and
# select JavaScript setjmp/longjmp instead. The precompiled standard library
# still names the C++ exception tag, so define it (nothing ever throws).
set -euo pipefail
args=()
for arg in "$@"; do
  [[ "$arg" == "-fwasm-exceptions" ]] || args+=("$arg")
done
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
printf '.globl __cpp_exception\n.tagtype __cpp_exception i32\n__cpp_exception:\n' > "$work/tag.s"
emcc -c -mexception-handling "$work/tag.s" -o "$work/tag.o"
emcc "${args[@]}" "$work/tag.o" -sSUPPORT_LONGJMP=emscripten
