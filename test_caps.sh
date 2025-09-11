#!/bin/bash
#
# Test script for capabara capability analysis
#
# Examples:
#   ./test_caps.sh -F thread,net,fread -d 3
#   ./test_caps.sh -F alloc,time
#   ./test_caps.sh -F all --verbose
#   ./test_caps.sh -F fread,fwrite
#

set -e

# Default values
FEATURES=""
CAPABARA_ARGS=()

# Parse command line arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        -F|--features)
            FEATURES="$2"
            shift 2
            ;;
        *)
            # All remaining arguments go to capabara
            CAPABARA_ARGS+=("$1")
            shift
            ;;
    esac
done

# Convert comma-separated features to cargo format
if [[ -n "$FEATURES" ]]; then
    CARGO_FEATURES="--features $FEATURES"
else
    CARGO_FEATURES=""
fi

rm -f target/release/deps/libtest_caps-*

echo "Building test_caps with features: ${FEATURES:-none}"
cargo build --release -p test_caps $CARGO_FEATURES

# Build the main capabara binary if needed
echo "Building symbols binary..."
cargo build --release --bin symbols

# Run capabara on the built test_caps library
RLIB_PATH="target/release/deps/libtest_caps-*.rlib"
if ls $RLIB_PATH 1> /dev/null 2>&1; then
    RLIB_FILE=$(ls $RLIB_PATH | head -1)
    echo "Running capabara on: $RLIB_FILE"

    cargo run --release --bin symbols -- "$RLIB_FILE" "${CAPABARA_ARGS[@]}"
else
    echo "Error: Could not find libtest_caps-*.rlib in target/release/deps/"
    echo "Make sure the build completed successfully."
    exit 1
fi
