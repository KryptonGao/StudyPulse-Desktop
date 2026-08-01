#!/bin/sh
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
CORE_DIR=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)
ROOT_DIR=$(CDPATH= cd -- "$CORE_DIR/.." && pwd)
OUTPUT_DIR="$ROOT_DIR/macOS/StudyPulseMac/Generated"
FFI_DIR="$CORE_DIR/target/uniffi-swift"

export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-15.0}"

cd "$CORE_DIR"
cargo build -p studypulse-ffi
mkdir -p "$OUTPUT_DIR" "$FFI_DIR"

cargo run -p studypulse-ffi --bin uniffi-bindgen-swift -- \
  "$CORE_DIR/target/debug/libstudypulse_ffi.a" "$OUTPUT_DIR" --swift-sources
cargo run -p studypulse-ffi --bin uniffi-bindgen-swift -- \
  "$CORE_DIR/target/debug/libstudypulse_ffi.a" "$FFI_DIR" --headers
cargo run -p studypulse-ffi --bin uniffi-bindgen-swift -- \
  "$CORE_DIR/target/debug/libstudypulse_ffi.a" "$FFI_DIR" \
  --modulemap --module-name StudyPulseCoreFFI --modulemap-filename module.modulemap
