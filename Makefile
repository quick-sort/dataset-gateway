.PHONY: help build run test docker compose-up compose-down

help:
	@echo "Dataset Gateway - Local S3 presigned URL gateway"
	@echo ""
	@echo "  make build        Build release binary"
	@echo "  make run          Run gateway (requires Redis)"
	@echo "  make test         Run tests"
	@echo "  make docker       Build Docker image"
	@echo "  make compose-up   Start gateway + Redis via Docker Compose"
	@echo "  make compose-down Stop Docker Compose"

build:
	cargo build --release

run:
	cargo run

test:
	cargo test

docker:
	docker build -t dataset-gateway .

compose-up:
	ADMIN_TOKEN=${ADMIN_TOKEN:?set ADMIN_TOKEN} docker compose up --build -d

compose-down:
	docker compose down
