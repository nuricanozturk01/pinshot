#!/usr/bin/env bash
# Format everything in one go:  tools/fmt.sh          (write)
#                               tools/fmt.sh --check  (what CI runs)
set -euo pipefail
cd "$(dirname "$0")/.."
if [ "${1:-}" = "--install-hook" ]; then
  git config core.hooksPath tools/hooks
  echo "pre-commit hook enabled (cargo fmt on staged Rust files)"
elif [ "${1:-}" = "--check" ]; then
  cargo fmt --all --check
  npx --yes prettier@3 --check .
else
  cargo fmt --all
  npx --yes prettier@3 --write .
fi
