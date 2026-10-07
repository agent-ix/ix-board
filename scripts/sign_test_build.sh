#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != Darwin ]]; then
  echo "sign_test_build: macOS is required" >&2
  exit 2
fi

if [[ $# -ne 1 || ! -f "$1" ]]; then
  echo "usage: IX_BOARD_SIGNING_IDENTITY=<identity> scripts/sign_test_build.sh <ix-board-binary>" >&2
  exit 2
fi

if [[ -z "${IX_BOARD_SIGNING_IDENTITY:-}" || "$IX_BOARD_SIGNING_IDENTITY" == "-" ]]; then
  echo "sign_test_build: set IX_BOARD_SIGNING_IDENTITY to a stable code signing certificate; ad-hoc signing is insufficient" >&2
  exit 2
fi

binary=$1
codesign --force --sign "$IX_BOARD_SIGNING_IDENTITY" \
  --identifier org.agent-ix.ix-board.test \
  --timestamp=none "$binary"
codesign --verify --strict --verbose=2 "$binary"
codesign -d -r- "$binary"
