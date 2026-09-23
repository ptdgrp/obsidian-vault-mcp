#!/usr/bin/env bash
set -euo pipefail

assets_dir="${1:?pass the directory containing platform archives}"
platforms=(linux-x64 linux-arm64 darwin-x64 darwin-arm64 win32-x64)

for platform in "${platforms[@]}"; do
  archive="${assets_dir}/obsidian-vault-mcp-${platform}.tar.gz"
  test -f "$archive"
  mkdir -p "npm/vendor/${platform}"
  tar -xzf "$archive" -C "npm/vendor/${platform}"
  if [[ "$platform" != win32-* ]]; then
    chmod +x "npm/vendor/${platform}/obsidian-vault-mcp"
  fi
done

cp LICENSE npm/LICENSE
npm pack ./npm --pack-destination "$assets_dir"
