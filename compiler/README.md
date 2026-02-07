# Compiler

The `compiler` microservice processes Camelyon17 whole slide images from S3 and inserts them into the storage backend.

## Subcommands

### dispatch

Scans an S3 bucket for `.tif` files and dispatches `ProcessSlideEvent` messages via NATS JetStream.

Features:
- Lists all `.tif` and `.tiff` files in the specified bucket/prefix
- Uses PostgreSQL transactions with row locking for exactly-once dispatch semantics
- Tracks dispatch state in the `compiler_dispatch` table
- Skips already-dispatched files

```bash
eosin-compiler dispatch \
  --bucket camelyon17 \
  --path-prefix slides/ \
  --nats-url nats://localhost:4222 \
  --nats-user user \
  --nats-password pass \
  --postgres-host localhost \
  --postgres-database eosin
```

### process

A worker that consumes `ProcessSlideEvent` messages and downloads the TIF files locally.

Features:
- Durable pull consumer for reliable message processing
- Downloads files to configurable local directory
- Graceful shutdown on SIGTERM/SIGINT
- Skips already-downloaded files

```bash
eosin-compiler process \
  --bucket camelyon17 \
  --path-prefix slides/ \
  --nats-url nats://localhost:4222 \
  --nats-user user \
  --nats-password pass \
  --download-dir /tmp/eosin/full
```

## Environment Variables

### S3 Configuration
- `S3_BUCKET` - S3 bucket name
- `S3_PATH_PREFIX` - Path prefix for scanning
- `S3_ENDPOINT` - Custom S3 endpoint (for S3-compatible storage)
- `S3_REGION` - AWS region (default: us-east-1)

### NATS Configuration
- `NATS_URL` - NATS server URL
- `NATS_USER` - NATS username
- `NATS_PASSWORD` - NATS password
- `STREAM_NAME` - JetStream stream name (default: eosin)
- `CONSUMER_NAME` - Consumer name for process worker (default: compiler)

### PostgreSQL Configuration (dispatch only)
- `POSTGRES_HOST` - PostgreSQL host
- `POSTGRES_PORT` - PostgreSQL port (default: 5432)
- `POSTGRES_DATABASE` - Database name
- `POSTGRES_USERNAME` - Username
- `POSTGRES_PASSWORD` - Password
- `POSTGRES_CA_CERT` - CA certificate for TLS
- `POSTGRES_SSL_MODE` - SSL mode (default: prefer)

### Process Worker Configuration
- `DOWNLOAD_DIR` - Directory for downloaded files (default: /tmp/eosin/full)
