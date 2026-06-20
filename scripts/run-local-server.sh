#!/bin/sh
set -eu
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
cargo run -p playora-server -- --db ./server.db --bind 0.0.0.0:8080
