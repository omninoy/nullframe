# NullFrame Sync

The server behind [sync in NullFrame](https://nullframe.app/docs/sync). It
authenticates devices, gives each account its own part of one S3-compatible
bucket, and signs short-lived URLs, so profile files go directly between each
device and storage.

To run your own server, use the Docker image `nullframe/nullframe-sync`. The
[self-hosting guide](https://nullframe.app/docs/self-hosting) has the
compose file and every environment variable.

## Build and run

```bash
cargo build --release
SYNC_TOKEN=... S3_ENDPOINT=... S3_ACCESS_KEY_ID=... S3_SECRET_ACCESS_KEY=... \
  S3_BUCKET=... ./target/release/nullframe-sync
```

`.env.example` lists the variables. A `.env` file in the working directory is
also read.

With `SYNC_AT_REST_KEY` set, the server encrypts every object it stores.
Downloads and uploads from current apps go through it, large files in parts.
Older apps still upload straight to storage, and the server encrypts those
files minutes later. Objects stored before
stay readable; `nullframe-sync migrate-at-rest --help` encrypts them in place, and
`nullframe-sync verify-at-rest` checks that none is left in plaintext.

## Test

The end-to-end tests run the binary against the S3 server in
`docker-compose.yml`:

```bash
docker compose -f docker-compose.yml up -d --wait
cargo test
docker compose -f docker-compose.yml down -v
```
