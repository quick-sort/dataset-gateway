# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project overview

Local dataset access gateway (Rust + actix-web + Redis). Clients call the gateway with an `Authorization: Bearer <token>` header; the gateway looks up the token in Redis, checks the requested path against the token's allowed prefixes, increments daily usage and checks rate limits, then serves the file from S3 (302 redirect) or local filesystem (200 gzip body). Storage backend is transparent to the client — configured at the gateway level, not per key.

An admin token (loaded from env or `config.yaml`) enables CRUD endpoints for managing API keys.

## Repository layout

- `src/main.rs` — entry point, wires actix-web routes and AppState
- `src/config.rs` — loads config from env vars or `config.yaml`, defines `StorageRoute`
- `src/handlers.rs` — HTTP handlers: `GET /get/{path}` and `/admin/keys` CRUD
- `src/redis.rs` — Redis operations: API key storage, daily usage counters, sliding-window rate limits
- `src/storage.rs` — S3 presigned URL generation + local file reading
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
- **API keys** — stored in Redis as `apikey:{key}` → JSON `{prefixes: [...], rate_limit: N}`. Each key has an access scope (list of allowed path prefixes). Validated per request via Redis lookup. Missing/invalid → `401` + `WWW-Authenticate: Bearer`. Valid key with disallowed path → `403`. Rate limit exceeded → `429`.

**Storage routing.** Gateway-level config maps path prefixes to storage backends:
```yaml
storage_routes:
  datasets/:
    storage_type: s3
    target: my-bucket
    key_prefix: ""
  local/:
    storage_type: local
    target: /data/files
```
Longest prefix match wins. This is separate from API key access scope — keys only control *which* prefixes a client can reach, not *how* they're stored.

**Path convention.** Clients request `GET /get/{path}`; the handler appends `.gz` and resolves storage. For S3: presigns `s3://bucket/{key_prefix}{path-minus-prefix}.gz`. For local: reads `{target}/{key_prefix}{path-minus-prefix}.gz` from disk.

**Redis schema.**
- `apikey:{key}` → JSON string `{prefixes: [...], rate_limit: N}`
- `usage:{key}:YYYY-MM-DD` → integer, auto-expires after 7 days
- `ratelimit:{key}` → sorted set of timestamps (sliding window)

**Admin endpoints.**
- `GET /admin/keys` — list all keys
- `POST /admin/keys` — create key `{key, prefixes, rate_limit}`
- `GET /admin/keys/{key}` — get key detail + today's usage
- `PUT /admin/keys/{key}` — update key (partial)
- `DELETE /admin/keys/{key}` — delete key

**Rate limiting.** Sliding window using Redis sorted set: `ZREMRANGEBYSCORE` prunes old entries, `ZADD` adds current request, `ZCARD` counts. Window duration defaults to 60s, configurable via `RATE_LIMIT_WINDOW_SECS`.

## Conventions worth preserving

- `Cargo.toml` release profile uses `opt-level = "z"`, `lto`, `codegen-units = 1`, `panic = "abort"`, `strip` for small binaries.
- Config tests use a `Mutex` lock to serialize env var mutations.
- Redis integration tests skip gracefully when Redis is unavailable.
