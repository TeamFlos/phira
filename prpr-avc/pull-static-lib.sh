#!/bin/bash

TARGETS=(
    "aarch64-apple-darwin"
    "aarch64-apple-ios-sim"
    "aarch64-apple-ios"
    "x86_64-apple-darwin"
    "x86_64-apple-ios"
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

for target in "${TARGETS[@]}"; do
    DEST_DIR="static-lib/$target"
    FILE_NAME="$target.tar.gz"
    URL="$BASE_URL/$FILE_NAME"

    echo "=== Downloading target: $target ==="

    # Check if the directory exists
    if [ -d "$DEST_DIR" ]; then
        read -p "Warning: Directory $DEST_DIR already exists. Do you want to delete and re-download? [y/N]: " confirm
        if [[ $confirm == [yY] ]]; then
            rm -rf "$DEST_DIR"
            echo "Old directory deleted."
        else
            echo "Skipping $target."
            continue
        fi
    fi

    # Create directory and download
    mkdir -p "$DEST_DIR"
    echo "Downloading: $URL"

    # Use curl to download and directly extract to the target directory
    # --strip-components=1 depends on whether the archive contains a single root directory with the same name, remove if unsure
    curl -L "$URL" | tar -xz -C "$DEST_DIR"

    if [ $? -eq 0 ]; then
        echo "Successfully extracted to $DEST_DIR"
    else
        echo "Error processing $target"
    fi

    echo
done
