# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project overview

Local dataset access gateway (Rust + actix-web + Redis). Clients call the gateway with an `Authorization: Bearer <token>` header; the gateway looks up the token in Redis, checks the requested path against the token's allowed prefixes, increments daily usage and checks rate limits, then returns a `302` redirect to a presigned S3 URL. Files are stored gzip-compressed (`<path>.gz`) and served with `Content-Encoding: gzip` so clients decompress transparently.

An admin token (loaded from env or `config.yaml`) enables CRUD endpoints for managing API keys.

## Repository layout

- `src/main.rs` — entry point, wires actix-web routes and AppState
- `src/config.rs` — loads config from env vars or `config.yaml`
- `src/handlers.rs` — HTTP handlers: `GET /get/{path}` and `/admin/keys` CRUD
- `src/redis.rs` — Redis operations: API key storage, daily usage counters, sliding-window rate limits
- `src/presign.rs` — S3 presigned URL generation
- `Cargo.toml` / `Cargo.lock` — Rust dependencies
- `Dockerfile` — multi-stage Alpine build
- `docker-compose.yml` — gateway + Redis
- `config.yaml.example` — example config
- `Makefile` — build/run/test/docker targets

## Common commands

```bash
make build          # cargo build --release
make run            # cargo run (requires Redis + ADMIN_TOKEN env)
make test           # cargo test
make docker         # build Docker image
make compose-up     # docker compose up (set ADMIN_TOKEN)
make compose-down   # docker compose down
```

Running manually:

```bash
ADMIN_TOKEN=my-secret cargo run
```

## Architecture notes

**Auth model.** Two tiers:
- **Admin token** — loaded at startup from `ADMIN_TOKEN` env var or `config.yaml`. Grants access to `/admin/keys` CRUD endpoints only. Never stored in Redis.
- **API keys** — long random strings stored in Redis as `apikey:{key}` → JSON `{bucket, prefixes, rate_limit}`. Validated per request via Redis lookup. Missing/invalid → `401` + `WWW-Authenticate: Bearer`. Valid key with disallowed path → `403`. Rate limit exceeded → `429`.

**Path convention.** Clients request `GET /get/{path}`; the handler appends `.gz` and presigns `s3://bucket/{path}.gz`. Uploaders are responsible for gzipping and setting `Content-Encoding: gzip` + the original `Content-Type`.

**Redis schema.**
- `apikey:{key}` → JSON string `{bucket, prefixes, rate_limit}`
- `usage:{key}:YYYY-MM-DD` → integer, auto-expires after 7 days
- `ratelimit:{key}` → sorted set of timestamps (sliding window)

**Admin endpoints.**
- `GET /admin/keys` — list all keys
- `POST /admin/keys` — create key `{key, bucket, prefixes, rate_limit}`
- `GET /admin/keys/{key}` — get key detail + today's usage
- `PUT /admin/keys/{key}` — update key (partial)
- `DELETE /admin/keys/{key}` — delete key

**Rate limiting.** Sliding window using Redis sorted set: `ZREMRANGEBYSCORE` prunes old entries, `ZADD` adds current request, `ZCARD` counts. Window duration defaults to 60s, configurable via `RATE_LIMIT_WINDOW_SECS`.

## Conventions worth preserving

- `Cargo.toml` release profile uses `opt-level = "z"`, `lto`, `codegen-units = 1`, `panic = "abort"`, `strip` for small binaries.
- Config tests use a `Mutex` lock to serialize env var mutations.
- Redis integration tests skip gracefully when Redis is unavailable.
