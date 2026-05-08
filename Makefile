.PHONY: help init init-aws init-tencent deploy-aws deploy-tencent destroy-aws destroy-tencent build-rust

help:
	@echo "Dataset Gateway - OpenTofu + Rust Deployment"
	@echo ""
	@echo "Usage:"
	@echo "  make build-rust      Build Rust Lambda function"
	@echo "  make init-aws        Initialize AWS backend only"
	@echo "  make init-tencent    Initialize Tencent Cloud backend only"
	@echo "  make deploy-aws      Deploy AWS infrastructure"
	@echo "  make deploy-tencent  Deploy Tencent Cloud infrastructure"
	@echo "  make destroy-aws     Destroy AWS infrastructure"
	@echo "  make destroy-tencent Destroy Tencent Cloud infrastructure"
	@echo ""
	@echo "Prerequisites:"
	@echo "  - OpenTofu installed"
	@echo "  - cargo-lambda installed (for Rust build)"
	@echo "  - AWS credentials configured"

build-rust:
	@echo "Building Rust Lambda..."
	@./scripts/build-rust.sh
	@cp aws/target/lambda/dataset-gateway-auth/bootstrap.zip aws/bootstrap.zip 2>/dev/null || \
	cp aws/target/lambda/*/bootstrap.zip aws/bootstrap.zip 2>/dev/null || \
	echo "Build complete. Copy bootstrap.zip manually if above failed."

init-aws:
	@echo "Initializing AWS backend..."
	@cd aws && tofu init

init-tencent:
	@echo "Initializing Tencent Cloud backend..."
	@cd tencent && tofu init

deploy-aws: build-rust
	@echo "Deploying AWS infrastructure..."
	@cp aws/target/lambda/dataset-gateway-auth/bootstrap.zip aws/bootstrap.zip 2>/dev/null || true
	@cd aws && tofu apply -var="aws_region=$${AWS_REGION:-us-east-1}" -var="project_name=$${PROJECT_NAME:-dataset-gateway}"

deploy-tencent:
	@echo "Deploying Tencent Cloud infrastructure..."
	@cd tencent && tofu apply -var="tencent_region=$${TENCENT_REGION:-ap-beijing}" -var="project_name=$${PROJECT_NAME:-dataset-gateway}"

destroy-aws:
	@echo "Destroying AWS infrastructure..."
	@cd aws && tofu destroy -var="aws_region=$${AWS_REGION:-us-east-1}" -var="project_name=$${PROJECT_NAME:-dataset-gateway}"

destroy-tencent:
	@echo "Destroying Tencent Cloud infrastructure..."
	@cd tencent && tofu destroy -var="tencent_region=$${TENCENT_REGION:-ap-beijing}" -var="project_name=$${PROJECT_NAME:-dataset-gateway}"
