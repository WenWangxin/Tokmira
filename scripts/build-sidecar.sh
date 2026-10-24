#!/bin/sh
# Build the token-audit CLI and copy it to src-tauri/binaries/ for Tauri's externalBin.
# Tauri looks up binaries/token-audit-<target-triple>[.exe] and strips the triple
# when placing the file next to the desktop executable.
set -eu

if [ "$#" -ne 1 ] || [ -z "$1" ]; then
  echo "usage: scripts/build-sidecar.sh <target>" >&2
  exit 1
fi

target=$1
case $target in
  *[!A-Za-z0-9._-]*)
    echo "error: invalid target: $target" >&2
    exit 1
    ;;
esac

root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
cd "$root"

bin_dir="$root/src-tauri/binaries"
mkdir -p "$bin_dir"
target_root=${CARGO_TARGET_DIR:-$root/target}

copy_bin() {
  triple=$1
  ext=$2
  src="$target_root/$triple/release/token-audit$ext"
  dest="$bin_dir/token-audit-$triple$ext"
  if [ ! -f "$src" ]; then
    echo "error: cargo did not produce $src" >&2
    exit 1
  fi
  cp -f "$src" "$dest"
  if [ -z "$ext" ]; then
    chmod +x "$dest"
  fi
  echo "sidecar: $dest"
}

# `tauri build --target universal-apple-darwin` compiles each architecture
# separately, so the per-arch names must exist at compile time. The bundler
# then picks up token-audit-universal-apple-darwin.
if [ "$target" = "universal-apple-darwin" ]; then
  if ! command -v lipo >/dev/null 2>&1; then
    echo "error: lipo is required to build a universal sidecar" >&2
    exit 1
  fi
  cargo build --release -p token-audit-cli --target aarch64-apple-darwin
  cargo build --release -p token-audit-cli --target x86_64-apple-darwin
  copy_bin aarch64-apple-darwin ""
  copy_bin x86_64-apple-darwin ""
  dest="$bin_dir/token-audit-universal-apple-darwin"
  rm -f "$dest"
  lipo -create \
    "$bin_dir/token-audit-aarch64-apple-darwin" \
    "$bin_dir/token-audit-x86_64-apple-darwin" \
    -output "$dest"
  chmod +x "$dest"
  echo "sidecar: $dest"
  exit 0
fi

case $target in
  *windows*) ext=.exe ;;
  *) ext= ;;
esac

cargo build --release -p token-audit-cli --target "$target"
copy_bin "$target" "$ext"
