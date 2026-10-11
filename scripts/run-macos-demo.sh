#!/bin/sh
set -eu
ROOT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
APP_DIR="$ROOT_DIR/target/OpenRichDemo.app"
CONTENTS_DIR="$APP_DIR/Contents"
mkdir -p "$CONTENTS_DIR/MacOS"
cd "$ROOT_DIR"
cargo build -p openrich-demo --locked
swiftc -parse-as-library -module-cache-path "${TMPDIR:-/tmp}/openrich-swift-modules" crates/openrich-demo/macos/OpenRichDemo.swift -o "$CONTENTS_DIR/MacOS/OpenRichDemo"
cat > "$CONTENTS_DIR/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key><string>OpenRichDemo</string>
  <key>CFBundleIdentifier</key><string>org.rs-ecosystem.openrich-demo</string>
  <key>CFBundleName</key><string>OpenRich Editor</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>NSPrincipalClass</key><string>NSApplication</string>
</dict>
</plist>
PLIST
if ! open -n "$APP_DIR" --args "$ROOT_DIR/target/debug/openrich-demo"; then
  # Launch Services can reject an unsigned local bundle even though its binary
  # is valid. Running the SwiftUI executable directly still creates the app
  # window and starts the same local Rust server.
  nohup "$CONTENTS_DIR/MacOS/OpenRichDemo" "$ROOT_DIR/target/debug/openrich-demo" \
    >/tmp/openrich-demo.log 2>&1 </dev/null &
fi
