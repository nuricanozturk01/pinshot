#!/usr/bin/env bash
# Builds pinshot-audiotap for the machine's architecture (run on macOS). Output: packaging/bin/pinshot-audiotap
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
out="${1:-$here/../bin}"
mkdir -p "$out"
arch="$(uname -m)"
swiftc -O -swift-version 5 -target "${arch}-apple-macos13.0" "$here/audiotap.swift" -o "$out/pinshot-audiotap"
"$out/pinshot-audiotap" --check
echo "built $out/pinshot-audiotap ($arch)"
