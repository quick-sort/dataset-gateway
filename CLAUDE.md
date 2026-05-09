# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project overview

Serverless dataset access gateway deployed in parallel to AWS and Tencent Cloud. Clients call an API Gateway endpoint with an `Authorization: Bearer <token>` header; a function authorizes the token against an allowed bucket + path-prefix list, then returns a `302` redirect to a 15-minute presigned URL for the object in S3 / COS. Files are stored gzip-compressed (`<path>.gz`) and served with `Content-Encoding: gzip` so clients decompress transparently.

## Repository layout

- `aws/` — OpenTofu config + Rust Lambda source (`src/main.rs`, `Cargo.toml`). The Rust binary is the auth/redirect handler.
- `tencent/` — OpenTofu config + Python SCF handler (`scf_function.py`).
- `shared/variables.tf` — variable definitions intended to be reused; **not currently wired into either stack** (each cloud's `main.tf` redeclares its own variables).
- `scripts/` — `build-rust.sh`, `deploy-aws.sh`, `deploy-tencent.sh` wrappers around `cargo lambda` / `tofu`.
- `Makefile` — primary entry point for build/deploy/destroy.
- `README.md` — design doc (Chinese) for the dual-cloud architecture.

## Common commands

Build the Rust Lambda (cross-compiles to `provided.al2023` arm64, outputs `aws/target/lambda/dataset-gateway-auth/bootstrap.zip`, copied to `aws/bootstrap.zip` which `lambda.tf` references):

```bash
make build-rust
```

Deploy / destroy (each `deploy-*` target runs `tofu apply` directly, not the interactive `scripts/deploy-*.sh` wrappers):

```bash
make deploy-aws       # runs build-rust first, then tofu apply in aws/
make deploy-tencent   # tofu apply in tencent/
make destroy-aws
make destroy-tencent
```

Region/project overrides are passed via env: `AWS_REGION`, `TENCENT_REGION`, `PROJECT_NAME`.

Rust-only iteration (no deploy):

```bash
cd aws && cargo check
cd aws && cargo lambda build --release --arm64 --output-format zip
```

OpenTofu workflow inside either `aws/` or `tencent/`:

```bash
tofu init
tofu plan -var="aws_region=us-east-1" -var="project_name=dataset-gateway"
tofu apply ...
```

There is **no test suite** and **no linter configured** in this repo.

## Architecture notes

**Auth model.** Both handlers expect `Authorization: Bearer <token>` on the request and look the token up in a config of shape `{token: {bucket, allowed_prefixes: [...]}}`. Header matching is case-insensitive; the `Bearer ` scheme prefix is stripped before lookup. Missing/invalid tokens return `401` with `WWW-Authenticate: Bearer`; a valid token whose path falls outside `allowed_prefixes` returns `403`. The Rust handler reads the config from the `API_KEY_PERMISSIONS` env var (set by `lambda.tf` from the `api_key_permissions` Terraform variable, JSON-encoded). The Tencent SCF handler currently hardcodes the map at the top of `scf_function.py` — if you change the auth schema, update both the Rust struct (`BucketConfig`) and the Python dict, and consider lifting the Python config to an env var or external store to match the AWS side.

**Path convention.** Clients request `GET /get/{path}`; the handler appends `.gz` and presigns `s3://bucket/{path}.gz` (or COS equivalent). Uploaders are responsible for gzipping and setting `Content-Encoding: gzip` + the original `Content-Type` on the object — the gateway does not transcode. README §4 documents the upload conventions.

**AWS stack wiring.**
- `s3.tf` creates the dataset bucket with SSE-S3, versioning, and CORS allowing `GET` from any origin (exposes `Content-Encoding`).
- `lambda.tf` builds an IAM role granting `s3:GetObject` + `s3:ListBucket` on the dataset bucket only, then deploys `bootstrap.zip` as a `provided.al2023` runtime function with `API_KEY_PERMISSIONS` injected.
- `api_gateway.tf` exposes `GET /get/{path}` via REST API + `AWS_PROXY` integration. **Note:** `aws_api_gateway_method.authorization` is currently `NONE` and the `aws_api_gateway_method` does not set `api_key_required = true`. Auth happens entirely inside the Lambda by parsing the `Authorization: Bearer <token>` header. The provisioned API Gateway API key + usage plan are unused by the auth path; consider removing them or switching to a custom Lambda authorizer if you want gateway-level enforcement (note that API Gateway's built-in `api_key_required` check uses the `x-api-key` header, not Bearer).
- The `api_gateway_url` output interpolates `${var.project_name}` into the path, but the actual deployed path is `/get/{path}` — the output string is misleading.

**Tencent stack wiring.** `tencent/api_gateway.tf` only provisions the API service skeleton; it does not define the API/path/integration resources or wire the SCF function. The `scf.tf` definition does not specify `cos_bucket_name`/`cos_object_name` or `code_object` either — deploying as-is will not produce a working endpoint. Treat the Tencent side as scaffolding that needs to be completed (the README §3.2 describes what the final config should do, but Terraform does not yet implement it). Console-based setup + import is suggested in the file's own comment.

**Build artifact flow.** `make build-rust` is hardcoded to expect `target/lambda/dataset-gateway-auth/bootstrap.zip`; it falls back to a glob and finally to a manual-copy hint. `lambda.tf` reads `${path.module}/bootstrap.zip` (i.e., `aws/bootstrap.zip`), so the copy step is required before `tofu apply`. `make deploy-aws` performs this copy.

## Conventions worth preserving

- The `provided.al2023` + arm64 + `bootstrap` handler combo in `lambda.tf` matches `cargo lambda`'s default zip output — don't change one side without the other.
- `Cargo.toml` uses an aggressive size profile (`opt-level = "z"`, `lto`, `codegen-units = 1`, `panic = "abort"`, `strip`) to keep cold-start small; preserve when modifying.
- Terraform `local.tags` is the project-wide tag block; apply it to any new resource.
