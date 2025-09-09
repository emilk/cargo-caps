#!/bin/bash

# Extract and demangle symbols from capabara library
# Usage: ./bootstrap.sh [--module "module_name"]
#
# Now analyzes the .rlib library file which shows the pure library symbols
# without the main() function and other executable-specific code.

set -e

# Build capabara if it doesn't exist or is older than source
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CAPABARA_BINARY="$SCRIPT_DIR/target/release/capabara"
CAPABARA_LIBRARY="$SCRIPT_DIR/target/release/libcapabara.rlib"

echo "Building capabara..."
cd "$SCRIPT_DIR"
cargo build --release --quiet

# Run capabara on the library, passing through any arguments
"$CAPABARA_BINARY" "$CAPABARA_LIBRARY" "$@"
