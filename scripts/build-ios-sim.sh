#!/usr/bin/env bash
# Local iOS Simulator build: produces target/aarch64-apple-ios-sim/{debug,release}/libphira.{rlib,dylib}
set -euo pipefail
cd "$(dirname "$0")/.."
PROFILE="${1:-debug}"
PROFILE_FLAG=""
if [[ "$PROFILE" == "release" ]]; then
    PROFILE_FLAG="--release"
fi
cargo build -p phira --target aarch64-apple-ios-sim $PROFILE_FLAG
