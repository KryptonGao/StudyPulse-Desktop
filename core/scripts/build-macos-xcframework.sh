#!/bin/sh
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
CORE_DIR=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)
ROOT_DIR=$(CDPATH= cd -- "$CORE_DIR/.." && pwd)
OUTPUT_DIR="$ROOT_DIR/macOS/StudyPulseMac/Generated"
FRAMEWORK_PATH="$CORE_DIR/target/StudyPulseCore.xcframework"
BUILD_DIR="$CORE_DIR/target/studypulse-xcframework"
HEADERS_DIR="$BUILD_DIR/Headers"

export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-15.0}"

cd "$CORE_DIR"
rustup target add aarch64-apple-darwin x86_64-apple-darwin
cargo build --release -p studypulse-ffi --target aarch64-apple-darwin
cargo build --release -p studypulse-ffi --target x86_64-apple-darwin

mkdir -p "$BUILD_DIR" "$HEADERS_DIR" "$OUTPUT_DIR"
lipo -create \
  "$CORE_DIR/target/aarch64-apple-darwin/release/libstudypulse_ffi.a" \
  "$CORE_DIR/target/x86_64-apple-darwin/release/libstudypulse_ffi.a" \
  -output "$BUILD_DIR/libstudypulse_ffi.a"

cargo run -p studypulse-ffi --bin uniffi-bindgen-swift -- \
  "$CORE_DIR/target/aarch64-apple-darwin/release/libstudypulse_ffi.a" \
  "$OUTPUT_DIR" --swift-sources
cargo run -p studypulse-ffi --bin uniffi-bindgen-swift -- \
  "$CORE_DIR/target/aarch64-apple-darwin/release/libstudypulse_ffi.a" \
  "$HEADERS_DIR" --headers
cargo run -p studypulse-ffi --bin uniffi-bindgen-swift -- \
  "$CORE_DIR/target/aarch64-apple-darwin/release/libstudypulse_ffi.a" \
  "$HEADERS_DIR" --modulemap --xcframework \
  --module-name StudyPulseCoreFFI --modulemap-filename module.modulemap

if [ -e "$FRAMEWORK_PATH" ]; then
  rm -rf "$FRAMEWORK_PATH"
fi
xcodebuild -create-xcframework \
  -library "$BUILD_DIR/libstudypulse_ffi.a" \
  -headers "$HEADERS_DIR" \
  -output "$FRAMEWORK_PATH"
