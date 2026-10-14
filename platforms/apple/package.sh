#!/usr/bin/env bash
# Builds the Rust core for Apple platforms with its Swift bindings:
# generated/RsEditorCore (an XCFramework and a Swift package), which
# RsEditorKit depends on. Release by default; --debug for a debug build.
set -euo pipefail
cd "$(dirname "$0")/../../crates/rs-editor-apple"
release=--release
[[ "${1:-}" == "--debug" ]] && release=""
rm -rf ../../platforms/apple/generated/RsEditorCore
# The incremental cache would double the target directory.
CARGO_INCREMENTAL=0 boltffi pack apple $release
