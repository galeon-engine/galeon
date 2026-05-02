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

uses_windows_paths() {
  local tool="$1"

  if [[ "$tool" == *.exe ]]; then
    return 0
  fi

  case "$(uname -s 2>/dev/null || true)" in
    MINGW*|MSYS*|CYGWIN*)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

normalize_path_for_tool() {
  local path="$1"
  local tool="$2"

  if uses_windows_paths "$tool"; then
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

normalize_file_dep_path() {
  local path="$1"
  local tool="$2"

  if uses_windows_paths "$tool"; then
    if command -v cygpath >/dev/null 2>&1; then
      cygpath -m "$path"
      return 0
    fi

    if command -v wslpath >/dev/null 2>&1; then
      wslpath -m "$path"
      return 0
    fi
  fi

  printf '%s\n' "$path"
}

escape_pwsh_single_quoted() {
  printf '%s' "$1" | sed "s/'/''/g"
}

run_galeon_in_project() {
  if uses_windows_paths "$GALEON_BIN"; then
    local project_dir_tool
    local galeon_bin_tool
    local command
    local arg
    local encoded_command

    project_dir_tool="$(normalize_path_for_tool "$PROJECT_DIR" "$GALEON_BIN")"
    galeon_bin_tool="$(normalize_path_for_tool "$GALEON_BIN" "$GALEON_BIN")"
    command="\$ProgressPreference = 'SilentlyContinue'; Set-Location -LiteralPath '$(escape_pwsh_single_quoted "$project_dir_tool")'; & '$(escape_pwsh_single_quoted "$galeon_bin_tool")'"

    for arg in "$@"; do
      command="$command '$(escape_pwsh_single_quoted "$arg")'"
    done

    encoded_command="$(printf '%s' "$command" | iconv -f UTF-8 -t UTF-16LE | base64 | tr -d '\n')"
    powershell.exe -NoProfile -ExecutionPolicy Bypass -EncodedCommand "$encoded_command"
    return
  fi

  (cd "$PROJECT_DIR" && "$GALEON_BIN" "$@")
}

CARGO_BIN="$(find_tool cargo)"
BUN_BIN="$(find_tool bun)"
INSTALL_ROOT_TOOL="$(normalize_path_for_tool "$INSTALL_ROOT" "$CARGO_BIN")"
CLI_SOURCE_PATH="$(normalize_path_for_tool "$REPO_ROOT/crates/galeon-cli" "$CARGO_BIN")"
ENGINE_CRATE_DEP_PATH="$(normalize_file_dep_path "$REPO_ROOT/crates/engine" "$CARGO_BIN")"
THREE_SYNC_CRATE_DEP_PATH="$(normalize_file_dep_path "$REPO_ROOT/crates/engine-three-sync" "$CARGO_BIN")"
RUNTIME_PACKAGE_DEP_PATH="$(normalize_file_dep_path "$REPO_ROOT/packages/runtime" "$BUN_BIN")"
RENDER_CORE_PACKAGE_DEP_PATH="$(normalize_file_dep_path "$REPO_ROOT/packages/render-core" "$BUN_BIN")"
THREE_PACKAGE_DEP_PATH="$(normalize_file_dep_path "$REPO_ROOT/packages/three" "$BUN_BIN")"

cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

install_galeon_cli() {
  if [[ -n "${GALEON_REQUIRE_PUBLISHED:-}" ]] && [[ -z "${GALEON_INSTALL_VERSION:-}" ]]; then
    echo "GALEON_REQUIRE_PUBLISHED requires GALEON_INSTALL_VERSION to be set" >&2
    return 1
  fi

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

replace_line_in_file() {
  local file="$1"
  local prefix="$2"
  local replacement="$3"
  local file_tool

  file_tool="$(normalize_path_for_tool "$file" "$BUN_BIN")"

  "$BUN_BIN" -e '
const fs = require("node:fs");
const [file, prefix, replacement] = process.argv.slice(1);
const lines = fs.readFileSync(file, "utf8").split(/\r?\n/);
let found = false;
const updated = lines.map((line) => {
  if (line.startsWith(prefix)) {
    found = true;
    return replacement;
  }
  return line;
});
if (!found) {
  console.error(`expected line starting with "${prefix}" in ${file}`);
  process.exit(1);
}
fs.writeFileSync(file, `${updated.join("\n").replace(/\n*$/, "\n")}`);
' "$file_tool" "$prefix" "$replacement"
}

assert_file_contains() {
  local file="$1"
  local snippet="$2"
  local file_tool

  file_tool="$(normalize_path_for_tool "$file" "$BUN_BIN")"

  "$BUN_BIN" -e '
const fs = require("node:fs");
const [file, snippet] = process.argv.slice(1);
if (!fs.readFileSync(file, "utf8").includes(snippet)) {
  console.error(`expected ${file} to contain ${snippet}`);
  process.exit(1);
}
' "$file_tool" "$snippet"
}

configure_source_mode_scaffold() {
  if [[ -n "${GALEON_INSTALL_VERSION:-}" ]]; then
    return 0
  fi

  # Source-installed CLI validation should exercise current repo code, not
  # unreleased registry versions that only exist after a publish step.
  # Build packages/three (which transitively builds render-core via project
  # references) so the scaffold's `file:` overrides resolve to real `dist/`
  # artifacts rather than empty directories.
  pushd "$REPO_ROOT" >/dev/null
  "$BUN_BIN" install
  "$BUN_BIN" x tsc --build packages/three/tsconfig.json
  popd >/dev/null

  replace_line_in_file \
    "$PROJECT_DIR/crates/protocol/Cargo.toml" \
    "galeon-engine = " \
    "galeon-engine = { path = \"$ENGINE_CRATE_DEP_PATH\" }"
  replace_line_in_file \
    "$PROJECT_DIR/crates/domain/Cargo.toml" \
    "galeon-engine = " \
    "galeon-engine = { path = \"$ENGINE_CRATE_DEP_PATH\" }"
  replace_line_in_file \
    "$PROJECT_DIR/crates/client/Cargo.toml" \
    "galeon-engine = " \
    "galeon-engine = { path = \"$ENGINE_CRATE_DEP_PATH\" }"
  replace_line_in_file \
    "$PROJECT_DIR/crates/client/Cargo.toml" \
    "galeon-engine-three-sync = " \
    "galeon-engine-three-sync = { path = \"$THREE_SYNC_CRATE_DEP_PATH\" }"

  "$BUN_BIN" -e '
const fs = require("node:fs");
const [file, runtime, renderCore, three] = process.argv.slice(1);
const pkg = JSON.parse(fs.readFileSync(file, "utf8"));
pkg.dependencies["@galeon/render-core"] = renderCore;
pkg.dependencies["@galeon/three"] = three;
// Pin transitive @galeon/* dependencies so bun resolves them to the local
// `file:` checkouts rather than the published registry versions.
pkg.overrides ??= {};
pkg.overrides["@galeon/runtime"] = runtime;
pkg.overrides["@galeon/render-core"] = renderCore;
pkg.overrides["@galeon/three"] = three;
fs.writeFileSync(file, `${JSON.stringify(pkg, null, 2)}\n`);
' \
    "$(normalize_path_for_tool "$PROJECT_DIR/package.json" "$BUN_BIN")" \
    "file:$RUNTIME_PACKAGE_DEP_PATH" \
    "file:$RENDER_CORE_PACKAGE_DEP_PATH" \
    "file:$THREE_PACKAGE_DEP_PATH"

  assert_file_contains "$PROJECT_DIR/crates/protocol/Cargo.toml" 'galeon-engine = { path = "'
  assert_file_contains "$PROJECT_DIR/crates/client/Cargo.toml" 'galeon-engine-three-sync = { path = "'
  assert_file_contains "$PROJECT_DIR/package.json" '"@galeon/render-core": "file:'
  assert_file_contains "$PROJECT_DIR/package.json" '"@galeon/three": "file:'
  assert_file_contains "$PROJECT_DIR/package.json" '"overrides": {'
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

configure_source_mode_scaffold

run_galeon_in_project generate manifest >/dev/null

pushd "$PROJECT_DIR" >/dev/null
test -f generated/manifest.json
"$BUN_BIN" install
"$BUN_BIN" run check
"$BUN_BIN" run build
popd >/dev/null
