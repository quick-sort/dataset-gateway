#!/bin/bash
set -e

cd "$(dirname "$0")/../aws"

echo "=== Building Rust Lambda Function ==="

# Check for cargo-lambda
if ! command -v cargo-lambda &> /dev/null; then
    echo "Installing cargo-lambda..."
    curl -LsSf https://cargo-lambda.xyz/cargo-lambda-install.sh | sh
    export PATH="$HOME/.cargo/bin:$PATH"
fi

# Build for Lambda (AL2023)
echo "Building for Lambda (AL2023) with arm64..."
cargo lambda build --release --arm64 --output-format zip

# The build output is in target/lambda/<package>/bootstrap.zip
PACKAGE_DIR=$(find target/lambda -name "bootstrap.zip" -type f 2>/dev/null | head -1)

if [ -z "$PACKAGE_DIR" ]; then
    # Try alternative location
    PACKAGE_DIR="target/lambda/dataset-gateway-auth/bootstrap.zip"
fi

echo ""
echo "=== Build Complete ==="
echo "Bootstrap file: $PACKAGE_DIR"
echo ""
echo "To deploy:"
echo "  1. cp $PACKAGE_DIR bootstrap.zip"
echo "  2. cd aws && tofu apply"
