#!/bin/sh
set -eu

CERT_DIR=/etc/nginx/certs
CERT_FILE="$CERT_DIR/fullchain.pem"
KEY_FILE="$CERT_DIR/privkey.pem"
SERVER_NAME="${NGINX_SERVER_NAME:-localhost}"

mkdir -p "$CERT_DIR"

if [ ! -s "$CERT_FILE" ] || [ ! -s "$KEY_FILE" ]; then
  echo "Generating self-signed HTTPS certificate for $SERVER_NAME"
  openssl req \
    -x509 \
    -newkey rsa:2048 \
    -sha256 \
    -days 3650 \
    -nodes \
    -keyout "$KEY_FILE" \
    -out "$CERT_FILE" \
    -subj "/CN=$SERVER_NAME" \
    -addext "subjectAltName=DNS:$SERVER_NAME,IP:31.130.132.238,IP:127.0.0.1"
fi
