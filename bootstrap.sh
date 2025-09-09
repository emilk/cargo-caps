#!/bin/bash

# Extract and demangle symbols from capabara itself
# Usage: ./bootstrap.sh [--module "module_name"]

set -e

# Build capabara if it doesn't exist or is older than source
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CAPABARA_BINARY="$SCRIPT_DIR/target/release/capabara"

if [ ! -f "$CAPABARA_BINARY" ] || [ "$SCRIPT_DIR/crates/capabara/src/main.rs" -nt "$CAPABARA_BINARY" ]; then
    echo "Building capabara..."
    cd "$SCRIPT_DIR"
    cargo build --release --quiet
fi

# Run capabara on itself, passing through any arguments
"$CAPABARA_BINARY" "$CAPABARA_BINARY" "$@"