#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: $0 package_directory port" >&2
  exit 2
fi

package=$(cd "$1" && pwd)
port=$2
runtime=$(mktemp -d)
executable=sepia-gateway
if [[ -f "$package/bin/sepia-gateway.exe" ]]; then
  executable=sepia-gateway.exe
fi
if [[ ! -f "$package/LICENSE" || ! -f "$package/frontend/dist/index.html" ]]; then
  echo "package is missing LICENSE or console assets" >&2
  exit 1
fi

pid=
cleanup() {
  status=$?
  if [[ -n "$pid" ]]; then
    kill "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
  fi
  if [[ $status -ne 0 && -f "$runtime/gateway.log" ]]; then
    cat "$runtime/gateway.log" >&2
  fi
  rm -rf "$runtime"
  exit "$status"
}
trap cleanup EXIT

cd "$runtime"
SEPIA_DATA_DIR="$runtime/data" \
SEPIA_LISTEN="127.0.0.1:$port" \
SEPIA_MASTER_KEY=MDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDA= \
SEPIA_BOOTSTRAP_TOKEN=release-smoke \
  "$package/bin/$executable" >"$runtime/gateway.log" 2>&1 &
pid=$!

for _ in {1..60}; do
  if status=$(curl -fsS "http://127.0.0.1:$port/api/v1/auth/status" 2>/dev/null); then
    if [[ "$status" == '{"initialized":false}' ]]; then
      break
    fi
  fi
  if ! kill -0 "$pid" 2>/dev/null; then
    echo "packaged gateway exited before becoming ready" >&2
    exit 1
  fi
  sleep 1
done
if [[ "${status:-}" != '{"initialized":false}' ]]; then
  echo "packaged gateway did not report an uninitialized status" >&2
  exit 1
fi
curl -fsS "http://127.0.0.1:$port/" | grep -F '<div id="root"></div>' >/dev/null
