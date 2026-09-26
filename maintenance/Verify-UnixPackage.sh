#!/usr/bin/env bash
set -euo pipefail

PACKAGE_ROOT="${1:?usage: Verify-UnixPackage.sh <package-root> [archive-path]}"
ARCHIVE_PATH="${2:-}"

fail() {
  echo "[ERROR] $*" >&2
  exit 1
}

verify_tree() {
  local root="$1"
  [[ -d "$root" ]] || fail "Package root does not exist: $root"

  local required=(
    "bin/stream-archive-cli"
    "bin/stream-archive-server"
    "docs/UNIX_CLI.md"
    "docs/OPERATIONS.md"
    "LICENSE"
    "THIRD_PARTY_NOTICES.md"
    "RELEASE_INFO.txt"
    "SHA256SUMS.txt"
  )
  for relative in "${required[@]}"; do
    [[ -f "$root/$relative" ]] || fail "Missing package file: $relative"
  done
  for relative in "backend" "backend/vod" "data"; do
    [[ -d "$root/$relative" ]] || fail "Missing package directory: $relative"
  done

  [[ -x "$root/bin/stream-archive-cli" ]] || fail "stream-archive-cli is not executable"
  [[ -x "$root/bin/stream-archive-server" ]] || fail "stream-archive-server is not executable"

  if [[ -n "$(find "$root/data" -mindepth 1 -print -quit)" ]]; then
    fail "Release package data directory must be empty"
  fi

  local forbidden
  forbidden="$(find "$root" -type f \( \
    -name 'streamlink' -o -name 'streamlink.exe' -o \
    -name 'yt-dlp' -o -name 'yt-dlp.exe' -o \
    -name 'ffmpeg' -o -name 'ffmpeg.exe' -o \
    -name 'stream-archive-launcher*' -o -name 'RUN_WEB.bat' -o \
    -name 'RUN_SERVER_CONSOLE.bat' -o -name 'Caddyfile.example' -o \
    -name 'REVERSE_PROXY.md' -o -name 'LOCAL_LAUNCHER.md' -o \
    -name 'SOOP_LIVE_SETTING.ini' -o -name 'SOOP_LIVE_CHANNELS.txt' -o \
    -name 'SOOP_VOD_SETTING.ini' -o -name '*.db' -o -name '*.db-wal' -o \
    -name '*.db-shm' -o -name '*.log' -o -name '*.stream-archive.claim' \
  \) -print -quit)"
  [[ -z "$forbidden" ]] || fail "Forbidden runtime/legacy artifact found: $forbidden"

  grep -qx 'product=Stream Archive' "$root/RELEASE_INFO.txt" || fail "release metadata product missing"
  local version
  version="$(awk -F= '$1=="version" {print $2; exit}' "$root/RELEASE_INFO.txt")"
  [[ -n "$version" ]] || fail "release metadata version missing"

  grep -Eq '^[0-9a-fA-F]{64}[[:space:]]+bin/stream-archive-cli$' "$root/SHA256SUMS.txt" \
    || fail "CLI checksum entry missing"
  grep -Eq '^[0-9a-fA-F]{64}[[:space:]]+bin/stream-archive-server$' "$root/SHA256SUMS.txt" \
    || fail "server checksum entry missing"
  (
    cd "$root"
    shasum -a 256 -c SHA256SUMS.txt
  )

  "$root/bin/stream-archive-cli" version | grep -F "$version" >/dev/null \
    || fail "packaged CLI version does not match RELEASE_INFO.txt"
  "$root/bin/stream-archive-cli" help >/dev/null
}

verify_tree "$PACKAGE_ROOT"

if [[ -n "$ARCHIVE_PATH" ]]; then
  [[ -f "$ARCHIVE_PATH" ]] || fail "Archive does not exist: $ARCHIVE_PATH"
  CHECKSUM_PATH="${ARCHIVE_PATH}.sha256"
  [[ -f "$CHECKSUM_PATH" ]] || fail "Archive checksum does not exist: $CHECKSUM_PATH"

  expected="$(awk 'NR==1 {print $1}' "$CHECKSUM_PATH")"
  actual="$(shasum -a 256 "$ARCHIVE_PATH" | awk '{print $1}')"
  [[ "$expected" == "$actual" ]] || fail "Archive checksum mismatch"

  scratch="$(mktemp -d "${TMPDIR:-/tmp}/Stream Archive Release 테스트.XXXXXX")"
  trap 'rm -rf "$scratch"' EXIT
  tar -C "$scratch" -xzf "$ARCHIVE_PATH"
  extracted="$scratch/stream-archive"
  verify_tree "$extracted"

  (
    cd "$extracted"
    export STREAM_ARCHIVE_BACKEND_DIR="$extracted/backend"
    export STREAM_ARCHIVE_DATA_DIR="$extracted/data"
    "$extracted/bin/stream-archive-cli" init >/dev/null
    status_json="$("$extracted/bin/stream-archive-cli" status --json)"
    printf '%s\n' "$status_json" | grep -q '"runtime_ready"'
  )

  rm -rf "$scratch"
  trap - EXIT
fi

echo "Unix package verification passed: $PACKAGE_ROOT"
