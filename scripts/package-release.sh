#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 4 ]]; then
  echo "usage: $0 version target executable_path archive_path" >&2
  exit 2
fi

version=$1
target=$2
executable=$3
archive=$4

if [[ -z "$version" || "$version" == v* ]]; then
  echo "version must be non-empty and must not start with v" >&2
  exit 2
fi
if [[ ! -f "$executable" ]]; then
  echo "executable not found: $executable" >&2
  exit 1
fi
for source in README.md LICENSE frontend/dist/index.html; do
  if [[ ! -f "$source" ]]; then
    echo "release input not found: $source" >&2
    exit 1
  fi
done

package="sepia-${version}-${target}"
staging=$(mktemp -d)
trap 'rm -rf "$staging"' EXIT
mkdir -p "$staging/$package/bin" "$staging/$package/frontend"
executable_name=sepia-gateway
if [[ "$target" == windows-* ]]; then
  executable_name=sepia-gateway.exe
fi
cp "$executable" "$staging/$package/bin/$executable_name"
cp README.md LICENSE "$staging/$package/"
cp -R frontend/dist "$staging/$package/frontend/"
mkdir -p "$(dirname "$archive")"
tar -czf "$archive" -C "$staging" "$package"
