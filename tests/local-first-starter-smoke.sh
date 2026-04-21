#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only OR Commercial

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMP_BASE="$REPO_ROOT/target/smoke-tmp"
mkdir -p "$TMP_BASE"
TMP_DIR="$(mktemp -d "$TMP_BASE/local-first-starter-smoke.XXXXXX")"
PROJECT_NAME="starter-smoke"
PROJECT_DIR="$TMP_DIR/$PROJECT_NAME"
INSTALL_ROOT="$TMP_DIR/install-root"

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

  if [[ "$tool" == *.exe ]]; then
    if command -v cygpath >/dev/null 2>&1; then
      cygpath -w "$path"
      return 0
    fi

    if command -v wslpath >/dev/null 2>&1; then
      wslpath -w "$path"
      return 0
    fi
  fi

  printf '%s\n' "$path"
}

CARGO_BIN="$(find_tool cargo)"
BUN_BIN="$(find_tool bun)"
INSTALL_ROOT_TOOL="$(normalize_path_for_tool "$INSTALL_ROOT" "$CARGO_BIN")"
CLI_SOURCE_PATH="$(normalize_path_for_tool "$REPO_ROOT/crates/galeon-cli" "$CARGO_BIN")"

cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

install_galeon_cli() {
  if [[ -n "${GALEON_INSTALL_VERSION:-}" ]]; then
    local attempt
    for attempt in $(seq 1 20); do
      if "$CARGO_BIN" install --locked --root "$INSTALL_ROOT_TOOL" galeon-cli --version "$GALEON_INSTALL_VERSION"; then
        return 0
      fi
      if [[ "$attempt" -eq 20 ]]; then
        echo "failed to install galeon-cli ${GALEON_INSTALL_VERSION} from crates.io after ${attempt} attempts" >&2
        return 1
      fi
      echo "galeon-cli ${GALEON_INSTALL_VERSION} not installable yet (attempt ${attempt}/20); retrying in 10s..."
      sleep 10
    done
    return 1
  fi

  "$CARGO_BIN" install --locked --root "$INSTALL_ROOT_TOOL" --path "$CLI_SOURCE_PATH"
}

install_wasm_pack_if_missing() {
  if find_tool wasm-pack >/dev/null 2>&1; then
    return 0
  fi

  "$CARGO_BIN" install --locked --root "$INSTALL_ROOT_TOOL" wasm-pack
}

install_galeon_cli
install_wasm_pack_if_missing

export PATH="$INSTALL_ROOT/bin:$PATH"
GALEON_BIN="$(find_tool galeon)"
WASM_PACK_BIN="$(find_tool wasm-pack)"
[[ -n "$GALEON_BIN" ]] || { echo "failed to locate installed galeon binary" >&2; exit 1; }
[[ -n "$WASM_PACK_BIN" ]] || { echo "failed to locate wasm-pack" >&2; exit 1; }

"$GALEON_BIN" --help >/dev/null

pushd "$TMP_DIR" >/dev/null
"$GALEON_BIN" new "$PROJECT_NAME" --preset local-first
popd >/dev/null

pushd "$PROJECT_DIR" >/dev/null
"$GALEON_BIN" generate manifest >/dev/null
test -f generated/manifest.json
"$BUN_BIN" install
"$BUN_BIN" run check
"$BUN_BIN" run build
popd >/dev/null
