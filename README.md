# dataset-gateway

A lightweight dataset access gateway built with Rust (actix-web + Redis). Clients authenticate with a Bearer token, the gateway checks access scope and rate limits, then serves files from S3 (302 redirect to presigned URL) or local filesystem (gzip body).

## Architecture

```
Client                  Gateway                    Storage
  |                       |                          |
  | GET /get/datasets/x   |                          |
  | Authorization: Bearer |                          |
  |---------------------->|                          |
  |                       | Lookup token in Redis    |
  |                       | Check path access scope  |
  |                       | Check rate limit         |
  |                       | Resolve storage route    |
  |                       |                          |
  |                       |-------- S3: presign ---->|
  |<-- 302 Location ------|                          |
  |                       |                          |
  |                       |------ local: read file --|
  |<-- 200 gzip body -----|                          |
```

**Two-tier auth:**

- **Admin token** — loaded at startup from env or `config.yaml`. Grants access to `/admin/keys` CRUD endpoints only.
- **API keys** — stored in Redis as `apikey:{key}` → JSON with access scope and rate limit. Validated per request.

**Storage is transparent to the client.** The gateway config defines path prefix → storage backend mappings. API keys only specify which prefixes they're allowed to access.

## Configuration

### config.yaml

```yaml
admin_token: "your-admin-token-here"
redis_url: "redis://localhost:6379"
redis_password: ""
listen_addr: "0.0.0.0:8080"
rate_limit_window_secs: 60
presign_expiry_secs: 900

# Path prefix → storage backend routing (longest match wins)
# Each S3 route specifies its own region.
storage_routes:
  datasets/:
    storage_type: s3
    target: my-data-bucket
    region: us-east-1
    key_prefix: ""
  images/:
    storage_type: s3
    target: my-image-bucket
    region: ap-southeast-1
    key_prefix: "processed/"
  local/:
    storage_type: local
    target: /data/files
  datasets/premium/:
    storage_type: s3
    target: premium-bucket
    region: eu-west-1
    key_prefix: ""
```

### Environment variables

All config values can be set via env vars (override `config.yaml`):

| Variable | Default | Description |
|---|---|---|
| `ADMIN_TOKEN` | required | Admin bearer token |
| `REDIS_URL` | `redis://localhost:6379` | Redis connection URL |
| `REDIS_PASSWORD` | | Injected into REDIS_URL if not already embedded |
| `LISTEN_ADDR` | `0.0.0.0:8080` | HTTP listen address |
| `RATE_LIMIT_WINDOW_SECS` | `60` | Sliding window for rate limiting |
| `PRESIGN_EXPIRY_SECS` | `900` | S3 presigned URL TTL |
| `CONFIG_FILE` | `config.yaml` | Path to YAML config file |

S3 region is set per storage route in `config.yaml`, not globally.

## API Keys (stored in Redis)

Each API key has an access scope (list of allowed path prefixes) and a rate limit:

```json
{
  "prefixes": ["datasets/", "local/"],
  "rate_limit": 100
}
```

**Routing example:**

| Gateway config | API key scope | Client request | Result |
|---|---|---|---|
| `datasets/` → S3 `my-bucket` (us-east-1) | `["datasets/"]` | `GET /get/datasets/train.csv` | 302 → S3 presigned URL |
| `local/` → local `/data/files` | `["local/"]` | `GET /get/local/readme.txt` | 200 gzip body |
| `datasets/` → S3, `local/` → local | `["datasets/"]` | `GET /get/local/readme.txt` | 403 (not in scope) |
| `datasets/` → S3, `datasets/premium/` → S3 | `["datasets/"]` | `GET /get/datasets/premium/gold.csv` | 302 → premium bucket (longest prefix match) |

## API Endpoints

### File access

```
GET /get/{path}
Authorization: Bearer <api-key>
```

- `200` — file body (local storage, gzip-encoded)
- `302` — redirect to presigned S3 URL
- `401` — missing or invalid token
- `403` — path not in key's access scope
- `404` — file not found
- `429` — rate limit exceeded

### Usage

```
GET /usage
Authorization: Bearer <api-key>
```

Returns usage info for the caller's API key:

```json
{
  "key": "client-alpha-2024",
  "prefixes": ["datasets/", "local/"],
  "rate_limit": 100,
  "usage_today": 42
}
```

### Admin endpoints

All admin endpoints require `Authorization: Bearer <admin-token>`.

```
GET    /admin/keys              List all API key names
POST   /admin/keys              Create a new API key
GET    /admin/keys/{key}        Get key details + today's usage
PUT    /admin/keys/{key}        Update key (partial)
DELETE /admin/keys/{key}        Delete key
```

#### Create key

```json
POST /admin/keys
{
  "key": "client-alpha-2024",
  "prefixes": ["datasets/", "local/"],
  "rate_limit": 100
}
```

#### Get key

```json
GET /admin/keys/client-alpha-2024

{
  "key": "client-alpha-2024",
  "prefixes": ["datasets/", "local/"],
  "rate_limit": 100,
  "usage_today": 42
}
```

#### Update key

```json
PUT /admin/keys/client-alpha-2024
{
  "prefixes": ["datasets/", "local/", "images/"],
  "rate_limit": 200
}
```

## Redis Schema

| Key | Type | Description |
|---|---|---|
| `apikey:{key}` | string (JSON) | `{"prefixes":["..."],"rate_limit":N}` |
| `usage:{key}:{date}` | integer | Daily request counter, auto-expires after 7 days |
| `ratelimit:{key}` | sorted set | Sliding-window rate limit timestamps |

## File Convention

Files are stored gzip-compressed with a `.gz` suffix:

```
datasets/train.csv     → datasets/train.csv.gz     (on S3 or local disk)
images/photo.png       → images/photo.png.gz
```

The gateway appends `.gz` automatically. For local storage, responses include `Content-Encoding: gzip` so clients decompress transparently. For S3, the bucket should have `Content-Encoding: gzip` set on objects.

## Quick Start

### Docker Compose

```bash
cp config.yaml.example config.yaml
# Edit config.yaml: set admin_token, storage_routes, etc.

export ADMIN_TOKEN=$(grep admin_token config.yaml | cut -d'"' -f2)
make compose-up
```

### Manual

```bash
# Start Redis
docker run -d -p 6379:6379 redis:7-alpine

# Run gateway
ADMIN_TOKEN=my-secret cargo run

# Create an API key
curl -X POST http://localhost:8080/admin/keys \
  -H "Authorization: Bearer my-secret" \
  -H "Content-Type: application/json" \
  -d '{"key":"test-key","prefixes":["datasets/"],"rate_limit":100}'

# Request a file
curl -v http://localhost:8080/get/datasets/train.csv \
  -H "Authorization: Bearer test-key"
```

### Build and Test

```bash
make build    # cargo build --release
make test     # cargo test (Redis integration tests skip gracefully)
make docker   # build Docker image
```

## License

MIT
