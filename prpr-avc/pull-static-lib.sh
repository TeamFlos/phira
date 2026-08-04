#!/bin/bash

AUTO_YES=false

while getopts "y" opt; do
    case $opt in
        y) AUTO_YES=true ;;
        *) echo "Usage: $0 [-y]"; exit 1 ;;
    esac
done

TARGETS=(
    "aarch64-apple-darwin"
    "aarch64-apple-ios-sim"
    "aarch64-apple-ios"
    "aarch64-unknown-linux-ohos"
    "x86_64-unknown-linux-gnu"
    "x86_64-unknown-linux-musl"
    "aarch64-pc-windows-msvc"
    "aarch64-pc-windows-gnullvm"
    "x86_64-pc-windows-gnu"
    "x86_64-pc-windows-msvc"
    "aarch64-linux-android"
    "armv7-linux-androideabi"
)
BASE_URL="https://github.com/TeamFlos/prpr-avc-ffmpeg/releases/latest/download"

echo "=== Fetching latest version from GitHub ==="
VERSION=$(curl -sf "https://api.github.com/repos/TeamFlos/prpr-avc-ffmpeg/releases/latest" \
    | grep '"tag_name"' \
    | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/')
if [ -z "$VERSION" ]; then
    echo "Error: Failed to fetch latest version (network issue or rate limit?)"
    exit 1
fi
echo "Latest version: $VERSION"

all_ok=true

for target in "${TARGETS[@]}"; do
    DEST_DIR="static-lib/$target"
    FILE_NAME="$target.tar.gz"
    URL="$BASE_URL/$FILE_NAME"

    echo "=== Downloading target: $target ==="

    if [ -d "$DEST_DIR" ]; then
        if $AUTO_YES; then
            rm -rf "$DEST_DIR"
            echo "Old directory deleted."
        else
            read -p "Warning: Directory $DEST_DIR already exists. Do you want to delete and re-download? [y/N]: " confirm
            if [[ $confirm == [yY] ]]; then
                rm -rf "$DEST_DIR"
                echo "Old directory deleted."
            else
                echo "Skipping $target."
                continue
            fi
        fi
    fi

    mkdir -p "$DEST_DIR"
    echo "Downloading: $URL"
    curl -L "$URL" | tar -xz -C "$DEST_DIR"

    if [ $? -eq 0 ]; then
        echo "Successfully extracted to $DEST_DIR"
    else
        echo "Error processing $target"
        rm -rf "$DEST_DIR"
        all_ok=false
    fi
    echo
done

if $all_ok; then
    echo "$VERSION" > static-lib/.version
    echo "=== Version $VERSION written to static-lib/.version ==="
else
    echo "=== Some targets failed, .version not updated ==="
    exit 1
fi