#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT"

case "$(uname -s)" in
  Linux) PLATFORM="linux" ;;
  Darwin) PLATFORM="macos" ;;
  *) echo "Unsupported Unix packaging host: $(uname -s)" >&2; exit 1 ;;
esac

case "$(uname -m)" in
  x86_64|amd64) ARCH="x64" ;;
  arm64|aarch64) ARCH="arm64" ;;
  *) echo "Unsupported release architecture: $(uname -m)" >&2; exit 1 ;;
esac

echo "Packaging Stream Archive for ${PLATFORM}-${ARCH}"
echo "host=$(rustc -vV | awk '/^host:/ {print $2}')"
rustc --version
cargo --version

cargo build --locked --release --manifest-path "$ROOT/rust-runtime/Cargo.toml"

CLI="$ROOT/rust-runtime/target/release/stream-archive-cli"
SERVER="$ROOT/rust-runtime/target/release/stream-archive-server"
[[ -x "$CLI" ]] || { echo "Missing release CLI: $CLI" >&2; exit 1; }
[[ -x "$SERVER" ]] || { echo "Missing release server: $SERVER" >&2; exit 1; }

STAGING_PARENT="$ROOT/dist/unix-${PLATFORM}-${ARCH}"
PACKAGE_ROOT="$STAGING_PARENT/stream-archive"
RELEASE_DIR="$ROOT/dist/release"
ARCHIVE="$RELEASE_DIR/stream-archive-${PLATFORM}-${ARCH}.tar.gz"
ARCHIVE_CHECKSUM="${ARCHIVE}.sha256"

rm -rf "$STAGING_PARENT"
mkdir -p "$PACKAGE_ROOT/bin" "$PACKAGE_ROOT/backend/vod" "$PACKAGE_ROOT/data" "$PACKAGE_ROOT/docs" "$RELEASE_DIR"

install -m 0755 "$CLI" "$PACKAGE_ROOT/bin/stream-archive-cli"
install -m 0755 "$SERVER" "$PACKAGE_ROOT/bin/stream-archive-server"
cp "$ROOT/docs/UNIX_CLI.md" "$PACKAGE_ROOT/docs/UNIX_CLI.md"
cp "$ROOT/docs/OPERATIONS.md" "$PACKAGE_ROOT/docs/OPERATIONS.md"
cp "$ROOT/LICENSE" "$PACKAGE_ROOT/LICENSE"
cp "$ROOT/THIRD_PARTY_NOTICES.md" "$PACKAGE_ROOT/THIRD_PARTY_NOTICES.md"

"$ROOT/maintenance/Write-ReleaseMetadata.sh" \
  "$PACKAGE_ROOT/RELEASE_INFO.txt" \
  "$ROOT/rust-runtime/Cargo.toml"

(
  cd "$PACKAGE_ROOT"
  shasum -a 256 \
    "bin/stream-archive-cli" \
    "bin/stream-archive-server" > "SHA256SUMS.txt"
)

"$ROOT/maintenance/Verify-UnixPackage.sh" "$PACKAGE_ROOT"

rm -f "$ARCHIVE" "$ARCHIVE_CHECKSUM"
tar -C "$STAGING_PARENT" -czf "$ARCHIVE" stream-archive
(
  cd "$RELEASE_DIR"
  shasum -a 256 "$(basename "$ARCHIVE")" > "$(basename "$ARCHIVE_CHECKSUM")"
)

"$ROOT/maintenance/Verify-UnixPackage.sh" "$PACKAGE_ROOT" "$ARCHIVE"

echo "PACKAGE_ROOT=$PACKAGE_ROOT"
echo "ARCHIVE=$ARCHIVE"
echo "ARCHIVE_CHECKSUM=$ARCHIVE_CHECKSUM"
