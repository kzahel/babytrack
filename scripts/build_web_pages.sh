#!/usr/bin/env bash
# Build the web client for Cloudflare Pages: hosted apart from its relay, with
# the same security headers the same-origin preview serves through Caddy.
set -euo pipefail
relay="${1:?usage: build_web_pages.sh RELAY_ORIGIN}"
if [[ ! $relay =~ ^https://[A-Za-z0-9.-]+$ ]]; then
  echo "relay origin must be a bare https origin: $relay" >&2
  exit 2
fi
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"
build_id="$(git rev-parse --short HEAD)"
VITE_RELAY_ORIGIN="$relay" VITE_BUILD_ID="$build_id" bash scripts/build_web.sh
dist=apps/web/dist
cat > "$dist/_headers" <<HEADERS
/*
  Content-Security-Policy: default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self'; connect-src 'self' $relay; img-src 'self' data:; object-src 'none'; base-uri 'none'; frame-ancestors 'none'
  X-Content-Type-Options: nosniff
  Referrer-Policy: no-referrer
HEADERS
printf '{"build":"%s","relay":"%s"}\n' "$build_id" "$relay" > "$dist/version.json"
echo "Built $dist for relay $relay at $build_id"
