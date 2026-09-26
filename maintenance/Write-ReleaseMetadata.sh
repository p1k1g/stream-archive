#!/usr/bin/env bash
set -euo pipefail

OUTPUT_PATH="${1:?usage: Write-ReleaseMetadata.sh <output-path> [manifest-path]}"
MANIFEST_PATH="${2:-./rust-runtime/Cargo.toml}"

METADATA_JSON="$(cargo metadata --locked --no-deps --format-version 1 --manifest-path "$MANIFEST_PATH")"
VERSION="$(printf '%s' "$METADATA_JSON" | python3 -c '
import json, sys
data = json.load(sys.stdin)
for package in data["packages"]:
    if package["name"] == "stream-archive-server":
        print(package["version"])
        break
else:
    raise SystemExit("stream-archive-server package not found")
')"

COMMIT="unknown"
if command -v git >/dev/null 2>&1; then
  if CANDIDATE="$(git rev-parse --short=12 HEAD 2>/dev/null)" && [[ -n "$CANDIDATE" ]]; then
    if DIRTY_STATE="$(git status --porcelain --untracked-files=normal 2>/dev/null)"; then
      COMMIT="$CANDIDATE"
      if [[ -n "$DIRTY_STATE" ]]; then
        COMMIT="${COMMIT}-dirty"
      fi
    fi
  fi
fi

mkdir -p "$(dirname "$OUTPUT_PATH")"
cat > "$OUTPUT_PATH" <<EOF
product=Stream Archive
version=$VERSION
commit=$COMMIT
built_at=$(date -u '+%Y-%m-%dT%H:%M:%SZ')
EOF

echo "Release metadata written: version=$VERSION commit=$COMMIT"
