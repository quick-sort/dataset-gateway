#!/bin/bash
# Deploy Tencent Cloud infrastructure using OpenTofu

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
TENCENT_DIR="$PROJECT_DIR/tencent"

echo "=== Deploying Tencent Cloud Infrastructure ==="
echo "Region: ${TENCENT_REGION:-ap-beijing}"
echo "Project: ${PROJECT_NAME:-dataset-gateway}"

cd "$TENCENT_DIR"

# Initialize OpenTofu
echo "Initializing OpenTofu..."
tofu init

# Plan the deployment
echo "Planning infrastructure..."
tofu plan -var="tencent_region=${TENCENT_REGION:-ap-beijing}" -var="project_name=${PROJECT_NAME:-dataset-gateway}"

# Show plan summary
echo ""
echo "=== Plan Summary ==="
read -p "Proceed with deployment? (y/n) " -n 1 -r
echo ""
if [[ ! $REPLY =~ ^[Yy]$ ]]; then
    echo "Deployment cancelled."
    exit 0
fi

# Deploy
echo "Applying infrastructure..."
tofu apply -var="tencent_region=${TENCENT_REGION:-ap-beijing}" -var="project_name=${PROJECT_NAME:-dataset-gateway}" -auto-approve

echo "=== Deployment Complete ==="
