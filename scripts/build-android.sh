#!/usr/bin/env bash
# Local Android build: produces target/aarch64-linux-android/{debug,release}/libphira.so
set -euo pipefail

ANDROID_NDK="${ANDROID_NDK_HOME:-/usr/local/Caskroom/android-ndk/29/AndroidNDK14206865.app/Contents/NDK}"
if [[ ! -d "$ANDROID_NDK" ]]; then
    echo "Android NDK not found at $ANDROID_NDK" >&2
    echo "Override with ANDROID_NDK_HOME=/path/to/ndk" >&2
    exit 1
fi

export ANDROID_NDK_HOME="$ANDROID_NDK"
export ANDROID_NDK_ROOT="$ANDROID_NDK"

PROFILE="${1:-debug}"
PROFILE_FLAG=""
if [[ "$PROFILE" == "release" ]]; then
    PROFILE_FLAG="--release"
fi

cd "$(dirname "$0")/.."
cargo ndk -t arm64-v8a --platform 35 \
    --manifest-path phira/Cargo.toml \
    build $PROFILE_FLAG "$@" || true
ls -lh "target/aarch64-linux-android/$PROFILE/libphira.so" 2>/dev/null || true
