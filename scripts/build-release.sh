#!/usr/bin/env bash
# Build a distributable macOS .app:
#   1. cef-host as a release .app bundle (cef-rs's bundle-cef-app produces a debug
#      layout; we replace the binaries with release builds in-place).
#   2. Tauri release build (frontend talks to the server backend).
#   3. Copy cef-host.app into MyApp.app/Contents/Resources/.
#   4. Strip the quarantine xattr and zip the result.
#
# Output: dist-release/<AppName>.zip
#
# Usage: ./scripts/build-release.sh

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CEF_RS_PATH="${CEF_RS_PATH:-$HOME/cef-rs}"
BACKEND_TARGET="${VITE_BACKEND_TARGET:-server}"

if [ ! -d "$CEF_RS_PATH" ]; then
    echo "CEF_RS_PATH ($CEF_RS_PATH) does not exist. Clone cef-rs there or set CEF_RS_PATH."
    exit 1
fi

# ---------------------------------------------------------------------------
# 1. cef-host bundle (debug layout from bundle-cef-app, then swap to release)
# ---------------------------------------------------------------------------
echo "==> Bundling cef-host structure (frameworks + helpers)..."
"$ROOT/scripts/build-cef-host.sh" >/dev/null

echo "==> Building release binaries for cef-host..."
(
    cd "$ROOT/cef-host"
    cargo build --release --bin cef-host --bin cef_host_helper
)

CEF_BUNDLE="$ROOT/cef-host/target/bundle/cef-host.app"
RELEASE_DIR="$ROOT/cef-host/target/release"

echo "==> Replacing debug binaries with release..."
cp "$RELEASE_DIR/cef-host" "$CEF_BUNDLE/Contents/MacOS/cef-host"
for kind in "" " (GPU)" " (Renderer)" " (Plugin)" " (Alerts)"; do
    HELPER_APP="$CEF_BUNDLE/Contents/Frameworks/cef-host Helper${kind}.app"
    cp "$RELEASE_DIR/cef_host_helper" "$HELPER_APP/Contents/MacOS/cef-host Helper${kind}"
done

# ---------------------------------------------------------------------------
# 2. Tauri release build
# ---------------------------------------------------------------------------
echo "==> Building Tauri (frontend target: $BACKEND_TARGET)..."
cd "$ROOT"
VITE_BACKEND_TARGET="$BACKEND_TARGET" bun run tauri:build:"$BACKEND_TARGET"

# Find the produced .app — there may be variants per arch under bundle/macos
APP_BUNDLE=$(find "$ROOT/src-tauri/target/release/bundle/macos" -maxdepth 1 -name "*.app" -type d | head -1)
if [ -z "$APP_BUNDLE" ] || [ ! -d "$APP_BUNDLE" ]; then
    echo "Could not find built Tauri .app under src-tauri/target/release/bundle/macos/"
    exit 1
fi
APP_NAME="$(basename "$APP_BUNDLE")"
echo "==> Tauri produced: $APP_BUNDLE"

# ---------------------------------------------------------------------------
# 3. Embed cef-host.app inside the Tauri bundle
# ---------------------------------------------------------------------------
echo "==> Embedding cef-host.app into $APP_NAME/Contents/Resources/..."
TARGET_RES="$APP_BUNDLE/Contents/Resources/cef-host.app"
rm -rf "$TARGET_RES"
cp -R "$CEF_BUNDLE" "$TARGET_RES"

# ---------------------------------------------------------------------------
# 4. Strip Gatekeeper quarantine on our own files (so the local copy runs).
#    The friend's Mac will still mark the downloaded zip as quarantined — they
#    must right-click → Open on first launch (or run `xattr -dr` themselves).
# ---------------------------------------------------------------------------
echo "==> Stripping quarantine xattrs (local copy)..."
xattr -dr com.apple.quarantine "$APP_BUNDLE" 2>/dev/null || true

# ---------------------------------------------------------------------------
# 5. Zip for distribution
# ---------------------------------------------------------------------------
DIST_DIR="$ROOT/dist-release"
mkdir -p "$DIST_DIR"
ZIP_PATH="$DIST_DIR/${APP_NAME%.app}.zip"
rm -f "$ZIP_PATH"
echo "==> Zipping to $ZIP_PATH..."
ditto -c -k --sequesterRsrc --keepParent "$APP_BUNDLE" "$ZIP_PATH"

SIZE=$(du -sh "$ZIP_PATH" | awk '{print $1}')

cat <<EOF

============================================================
Built: $APP_BUNDLE
Zip:   $ZIP_PATH ($SIZE)
Backend target: $BACKEND_TARGET

Give your friend the zip. Tell them:
  1. Unzip and move "$APP_NAME" to /Applications (or anywhere).
  2. First launch will fail Gatekeeper. Either:
       • Right-click the app → Open → confirm in the dialog, OR
       • Run in Terminal:
           xattr -dr com.apple.quarantine "/Applications/$APP_NAME"
  3. The app talks to backend target: $BACKEND_TARGET
     (currently hardcoded — rebuild with VITE_BACKEND_TARGET=local for local docker).
============================================================
EOF
