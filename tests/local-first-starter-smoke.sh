#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only OR Commercial

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMP_DIR="$(mktemp -d)"
PROJECT_NAME="starter-smoke"
PROJECT_DIR="$TMP_DIR/$PROJECT_NAME"

find_tool() {
  local name="$1"
  local windows_home="${USERPROFILE:-}"

  if command -v "$name" >/dev/null 2>&1; then
    command -v "$name"
    return 0
  fi

  if command -v "${name}.exe" >/dev/null 2>&1; then
    command -v "${name}.exe"
    return 0
  fi

  if [[ -n "$windows_home" && -x "$windows_home/.cargo/bin/${name}.exe" ]]; then
    printf '%s\n' "$windows_home/.cargo/bin/${name}.exe"
    return 0
  fi

  if [[ -n "$windows_home" && -x "$windows_home/.bun/bin/${name}.exe" ]]; then
    printf '%s\n' "$windows_home/.bun/bin/${name}.exe"
    return 0
  fi

  return 1
}

normalize_path_for_tool() {
  local path="$1"
  local tool="$2"

  if [[ "$tool" == *.exe ]] && command -v cygpath >/dev/null 2>&1; then
    cygpath -w "$path"
    return 0
  fi

  printf '%s\n' "$path"
}

CARGO_BIN="$(find_tool cargo)"
BUN_BIN="$(find_tool bun)"
REPO_MANIFEST="$(normalize_path_for_tool "$REPO_ROOT/Cargo.toml" "$CARGO_BIN")"

cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

pushd "$TMP_DIR" >/dev/null
"$CARGO_BIN" run --manifest-path "$REPO_MANIFEST" -p galeon-cli -- new "$PROJECT_NAME" --preset local-first
popd >/dev/null

pushd "$PROJECT_DIR" >/dev/null
"$BUN_BIN" install
"$BUN_BIN" run check
"$BUN_BIN" run build
popd >/dev/null
