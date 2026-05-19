# Podcast Uploader

Tauri + Vue frontend and a Rust backend for podcast metadata, uploads, and RSS feeds.

## Production Backend

On a clean server:

```bash
git clone <repo-url>
cd upload_podcast_to_youtube
docker compose up -d --build
```

Nginx will listen on ports `80` and `443`, proxying requests to the backend inside Docker. Public RSS/media URLs default to:

```text
http://31.130.132.238
```

To change production values, copy `.env.example` to `.env` and edit it:

```bash
cp .env.example .env
docker compose up -d --build
```

Useful checks:

```bash
curl http://127.0.0.1/health
curl -k https://127.0.0.1/health
docker compose ps
docker compose logs -f nginx
docker compose logs -f podcast-backend
```

Persistent data is stored in Docker volumes:

- `nginx_certs` for HTTPS certificates
- `podcast_backend_data` for JSON state
- `podcast_backend_resources` for uploaded files

RSS feed URL format:

```text
http://31.130.132.238/podcast/<feedSlug>
```

HTTPS works out of the box with a self-signed certificate. For real production TLS, put your certificate and key into the `nginx_certs` volume as:

```text
/etc/nginx/certs/fullchain.pem
/etc/nginx/certs/privkey.pem
```

Then set:

```env
PODCAST_PUBLIC_BASE_URL=https://your-domain.com
NGINX_SERVER_NAME=your-domain.com
```

## Tauri Backend Target

The frontend supports two backend targets:

- `local` -> `http://127.0.0.1:8787`
- `server` -> `http://31.130.132.238`

Run Tauri against local backend:

```bash
npm run tauri:dev:local
```

Run Tauri against server backend:

```bash
npm run tauri:dev:server
```

Build Tauri with a fixed target:

```bash
npm run tauri:build:local
npm run tauri:build:server
```

You can override both targets with an exact URL:

```bash
VITE_BACKEND_URL=https://example.com npm run tauri dev
```

## Development

Frontend:

```bash
npm run dev:local
npm run dev:server
```

Backend without Docker:

```bash
cargo run --manifest-path backend/Cargo.toml
```
