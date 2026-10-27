#!/bin/sh
# Builds the universal macOS app and dmg locally. The updater package must be signed, so
# the key is read from ~/.tauri unless TAURI_SIGNING_PRIVATE_KEY is already set (M13).
set -eu

KEY_DIR="$HOME/.tauri"
if [ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" ] && [ -f "$KEY_DIR/token-audit.key" ]; then
  TAURI_SIGNING_PRIVATE_KEY=$(cat "$KEY_DIR/token-audit.key")
  export TAURI_SIGNING_PRIVATE_KEY
  if [ -z "${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}" ] && [ -f "$KEY_DIR/token-audit.key.password" ]; then
    TAURI_SIGNING_PRIVATE_KEY_PASSWORD=$(cat "$KEY_DIR/token-audit.key.password")
    export TAURI_SIGNING_PRIVATE_KEY_PASSWORD
  fi
fi

sh scripts/build-sidecar.sh universal-apple-darwin
CI=true pnpm tauri build --target universal-apple-darwin --bundles app,dmg "$@"
