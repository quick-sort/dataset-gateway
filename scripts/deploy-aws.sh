#!/bin/bash
# Deploy AWS infrastructure using OpenTofu

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
AWS_DIR="$PROJECT_DIR/aws"

echo "=== Deploying AWS Infrastructure ==="
echo "Region: ${AWS_REGION:-us-east-1}"
echo "Project: ${PROJECT_NAME:-dataset-gateway}"

cd "$AWS_DIR"

# Initialize OpenTofu
echo "Initializing OpenTofu..."
tofu init

# Plan the deployment
echo "Planning infrastructure..."
tofu plan -var="aws_region=${AWS_REGION:-us-east-1}" -var="project_name=${PROJECT_NAME:-dataset-gateway}"

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
tofu apply -var="aws_region=${AWS_REGION:-us-east-1}" -var="project_name=${PROJECT_NAME:-dataset-gateway}" -auto-approve

echo "=== Deployment Complete ==="
