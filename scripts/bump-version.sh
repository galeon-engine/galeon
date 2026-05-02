#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only OR Commercial
#
# bump-version.sh — Update the shared version sources for Galeon's 9 lockstep published artifacts.
#
# Usage: scripts/bump-version.sh X.Y.Z
#
# Validates semver format, checks current versions are consistent,
# then updates all versioned locations across 8 files. `galeon-cli`
# inherits the workspace version, so it needs no separate bump file.
# Fails fast on any inconsistency or missing file.

set -euo pipefail

# ── Helpers ──────────────────────────────────────────────────────────

die()      { printf 'error: %s\n' "$1" >&2; exit 1; }
ok()       { printf '  ✓ %s\n' "$1"; }
strip_cr() { tr -d '\r'; }  # neutralise CRLF on Windows checkouts

BACKUP_DIR=""
ROLLBACK_NEEDED=0

restore_backups() {
  [[ $ROLLBACK_NEEDED -eq 1 ]] || return 0

  for f in "${ALL_FILES[@]}"; do
    [[ -f "$BACKUP_DIR/$f" ]] || continue
    cp "$BACKUP_DIR/$f" "$f"
  done

  printf 'Rolled back partial changes.\n' >&2
}

cleanup() {
  [[ -n "$BACKUP_DIR" && -d "$BACKUP_DIR" ]] && rm -rf "$BACKUP_DIR"
}

on_exit() {
  local status=$?

  if [[ $status -ne 0 ]]; then
    restore_backups
  fi

  cleanup
  exit "$status"
}

trap on_exit EXIT

# ── Args ─────────────────────────────────────────────────────────────

NEW_VERSION="${1:-}"

if [[ -z "$NEW_VERSION" ]]; then
  echo "Usage: scripts/bump-version.sh X.Y.Z"
  echo ""
  echo "Updates Galeon's shared version sources (11 edits across 8 files)."
  exit 1
fi

# Validate SemVer 2.0.0 (with optional prerelease/build metadata)
if ! [[ "$NEW_VERSION" =~ ^(0|[1-9][0-9]*)\.((0|[1-9][0-9]*))\.((0|[1-9][0-9]*))(-((0|[1-9][0-9]*|[0-9A-Za-z-]*[A-Za-z-][0-9A-Za-z-]*)(\.(0|[1-9][0-9]*|[0-9A-Za-z-]*[A-Za-z-][0-9A-Za-z-]*))*))?(\+([0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*))?$ ]]; then
  die "Invalid semver: '$NEW_VERSION' (expected X.Y.Z[-prerelease][+build])"
fi

# ── Resolve repo root ───────────────────────────────────────────────

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

# ── File paths ───────────────────────────────────────────────────────

WORKSPACE_CARGO="Cargo.toml"
ENGINE_CARGO="crates/engine/Cargo.toml"
THREE_SYNC_CARGO="crates/engine-three-sync/Cargo.toml"
RUNTIME_PKG="packages/runtime/package.json"
RENDER_CORE_PKG="packages/render-core/package.json"
THREE_PKG="packages/three/package.json"
R3F_PKG="packages/r3f/package.json"
SHELL_PKG="packages/shell/package.json"

ALL_FILES=(
  "$WORKSPACE_CARGO"
  "$ENGINE_CARGO"
  "$THREE_SYNC_CARGO"
  "$RUNTIME_PKG"
  "$RENDER_CORE_PKG"
  "$THREE_PKG"
  "$R3F_PKG"
  "$SHELL_PKG"
)

# ── Check all files exist ───────────────────────────────────────────

echo "Checking files exist..."
for f in "${ALL_FILES[@]}"; do
  [[ -f "$f" ]] || die "Missing file: $f"
done
echo ""

# ── Read current versions ───────────────────────────────────────────

echo "Reading current versions..."

# 1. Workspace version
CURRENT=$(sed -n '/\[workspace\.package\]/,/^\[/{s/^version *= *"\(.*\)"/\1/p}' "$WORKSPACE_CARGO" | strip_cr)
[[ -n "$CURRENT" ]] || die "Cannot read workspace version from $WORKSPACE_CARGO"
ok "$WORKSPACE_CARGO  workspace.package.version = $CURRENT"

# 2. engine dep on macros
V_ENGINE_MACROS=$(sed -n 's/^galeon-engine-macros.*version *= *"=\([^"]*\)".*/\1/p' "$ENGINE_CARGO" | strip_cr)
[[ -n "$V_ENGINE_MACROS" ]] || die "Cannot read galeon-engine-macros pin from $ENGINE_CARGO"
ok "$ENGINE_CARGO  galeon-engine-macros = =$V_ENGINE_MACROS"

# 3. three-sync dep on engine
V_THREE_ENGINE=$(sed -n 's/^galeon-engine.*version *= *"=\([^"]*\)".*/\1/p' "$THREE_SYNC_CARGO" | strip_cr)
[[ -n "$V_THREE_ENGINE" ]] || die "Cannot read galeon-engine pin from $THREE_SYNC_CARGO"
ok "$THREE_SYNC_CARGO  galeon-engine = =$V_THREE_ENGINE"

# 4-6. package.json versions (using grep to extract)
read_pkg_version() {
  sed -n 's/.*"version": *"\([^"]*\)".*/\1/p' "$1" | head -1 | strip_cr
}

read_pkg_dep_pin() {
  local file="$1"
  local dep="$2"
  sed -n "s|.*\"$dep\": *\"=\\([^\"]*\\)\".*|\\1|p" "$file" | strip_cr
}

V_RUNTIME=$(read_pkg_version "$RUNTIME_PKG")
[[ -n "$V_RUNTIME" ]] || die "Cannot read version from $RUNTIME_PKG"
ok "$RUNTIME_PKG  version = $V_RUNTIME"

V_RENDER_CORE=$(read_pkg_version "$RENDER_CORE_PKG")
[[ -n "$V_RENDER_CORE" ]] || die "Cannot read version from $RENDER_CORE_PKG"
ok "$RENDER_CORE_PKG  version = $V_RENDER_CORE"

V_THREE=$(read_pkg_version "$THREE_PKG")
[[ -n "$V_THREE" ]] || die "Cannot read version from $THREE_PKG"
ok "$THREE_PKG  version = $V_THREE"

V_THREE_RENDER_CORE=$(read_pkg_dep_pin "$THREE_PKG" "@galeon/render-core")
[[ -n "$V_THREE_RENDER_CORE" ]] || die "Cannot read @galeon/render-core pin from $THREE_PKG"
ok "$THREE_PKG  @galeon/render-core = =$V_THREE_RENDER_CORE"

V_R3F=$(read_pkg_version "$R3F_PKG")
[[ -n "$V_R3F" ]] || die "Cannot read version from $R3F_PKG"
ok "$R3F_PKG  version = $V_R3F"

V_R3F_RENDER_CORE=$(read_pkg_dep_pin "$R3F_PKG" "@galeon/render-core")
[[ -n "$V_R3F_RENDER_CORE" ]] || die "Cannot read @galeon/render-core pin from $R3F_PKG"
ok "$R3F_PKG  @galeon/render-core = =$V_R3F_RENDER_CORE"

V_R3F_THREE=$(read_pkg_dep_pin "$R3F_PKG" "@galeon/three")
[[ -n "$V_R3F_THREE" ]] || die "Cannot read @galeon/three pin from $R3F_PKG"
ok "$R3F_PKG  @galeon/three = =$V_R3F_THREE"

V_SHELL=$(read_pkg_version "$SHELL_PKG")
[[ -n "$V_SHELL" ]] || die "Cannot read version from $SHELL_PKG"
ok "$SHELL_PKG  version = $V_SHELL"
echo ""

# ── Consistency check ────────────────────────────────────────────────

echo "Checking consistency..."
ALL_VERSIONS=(
  "$CURRENT"
  "$V_ENGINE_MACROS"
  "$V_THREE_ENGINE"
  "$V_RUNTIME"
  "$V_RENDER_CORE"
  "$V_THREE"
  "$V_THREE_RENDER_CORE"
  "$V_R3F"
  "$V_R3F_RENDER_CORE"
  "$V_R3F_THREE"
  "$V_SHELL"
)

for v in "${ALL_VERSIONS[@]}"; do
  if [[ "$v" != "$CURRENT" ]]; then
    die "Version mismatch! Expected all to be '$CURRENT' but found '$v'. Fix manually before bumping."
  fi
done
ok "All 11 locations currently at $CURRENT"
echo ""

# ── No-op check ─────────────────────────────────────────────────────

if [[ "$NEW_VERSION" == "$CURRENT" ]]; then
  die "New version ($NEW_VERSION) is the same as current ($CURRENT). Nothing to do."
fi

# ── Apply updates ────────────────────────────────────────────────────

echo "Bumping $CURRENT → $NEW_VERSION ..."

# Escape dots for sed patterns
OLD_ESC="${CURRENT//./\\.}"
NEW_ESC="${NEW_VERSION//./\\.}"

BACKUP_DIR="$(mktemp -d)"
for f in "${ALL_FILES[@]}"; do
  mkdir -p "$BACKUP_DIR/$(dirname "$f")"
  cp "$f" "$BACKUP_DIR/$f"
done
ROLLBACK_NEEDED=1

# 1. Workspace Cargo.toml
sed -i "/\[workspace\.package\]/,/^\[/ s/version = \"$OLD_ESC\"/version = \"$NEW_VERSION\"/" "$WORKSPACE_CARGO"
ok "$WORKSPACE_CARGO"

# 2. engine dep on macros
sed -i "s/\(galeon-engine-macros.*version = \"=\)$OLD_ESC\"/\1$NEW_VERSION\"/" "$ENGINE_CARGO"
ok "$ENGINE_CARGO"

# 3. three-sync dep on engine
sed -i "s/\(galeon-engine.*version = \"=\)$OLD_ESC\"/\1$NEW_VERSION\"/" "$THREE_SYNC_CARGO"
ok "$THREE_SYNC_CARGO"

# 4. runtime package.json
sed -i "s/\"version\": \"$OLD_ESC\"/\"version\": \"$NEW_VERSION\"/" "$RUNTIME_PKG"
ok "$RUNTIME_PKG"

# 5. render-core package.json
sed -i "s/\"version\": \"$OLD_ESC\"/\"version\": \"$NEW_VERSION\"/" "$RENDER_CORE_PKG"
ok "$RENDER_CORE_PKG"

# 6. three package.json (version + render-core dep)
sed -i "s/\"version\": \"$OLD_ESC\"/\"version\": \"$NEW_VERSION\"/" "$THREE_PKG"
sed -i "s/\"@galeon\/render-core\": \"=$OLD_ESC\"/\"@galeon\/render-core\": \"=$NEW_VERSION\"/" "$THREE_PKG"
ok "$THREE_PKG"

# 7. r3f package.json (version + internal deps)
sed -i "s/\"version\": \"$OLD_ESC\"/\"version\": \"$NEW_VERSION\"/" "$R3F_PKG"
sed -i "s/\"@galeon\/render-core\": \"=$OLD_ESC\"/\"@galeon\/render-core\": \"=$NEW_VERSION\"/" "$R3F_PKG"
sed -i "s/\"@galeon\/three\": \"=$OLD_ESC\"/\"@galeon\/three\": \"=$NEW_VERSION\"/" "$R3F_PKG"
ok "$R3F_PKG"

# 8. shell package.json
sed -i "s/\"version\": \"$OLD_ESC\"/\"version\": \"$NEW_VERSION\"/" "$SHELL_PKG"
ok "$SHELL_PKG"

echo ""

# ── Verify ───────────────────────────────────────────────────────────

echo "Verifying..."
ERRORS=0

verify() {
  local file="$1" pattern="$2" label="$3"
  if ! grep -q "$pattern" "$file"; then
    printf '  ✗ %s: expected %s\n' "$file" "$label" >&2
    ERRORS=$((ERRORS + 1))
  else
    ok "$file"
  fi
}

verify "$WORKSPACE_CARGO"   "version = \"$NEW_VERSION\""            "workspace version"
verify "$ENGINE_CARGO"       "version = \"=$NEW_VERSION\""           "macros pin"
verify "$THREE_SYNC_CARGO"   "version = \"=$NEW_VERSION\""           "engine pin"
verify "$RUNTIME_PKG"        "\"version\": \"$NEW_VERSION\""         "runtime version"
verify "$RENDER_CORE_PKG"    "\"version\": \"$NEW_VERSION\""         "render-core version"
verify "$THREE_PKG"          "\"version\": \"$NEW_VERSION\""         "three version"
verify "$THREE_PKG"          "\"@galeon/render-core\": \"=$NEW_VERSION\"" "three render-core pin"
verify "$R3F_PKG"            "\"version\": \"$NEW_VERSION\""         "r3f version"
verify "$R3F_PKG"            "\"@galeon/render-core\": \"=$NEW_VERSION\"" "r3f render-core pin"
verify "$R3F_PKG"            "\"@galeon/three\": \"=$NEW_VERSION\""  "r3f three pin"
verify "$SHELL_PKG"          "\"version\": \"$NEW_VERSION\""         "shell version"

if [[ $ERRORS -gt 0 ]]; then
  die "$ERRORS verification(s) failed. Check the files manually."
fi

ROLLBACK_NEEDED=0

echo ""
echo "Done. All 11 locations updated to $NEW_VERSION."
echo ""
echo "Next steps:"
echo "  1. Update CHANGELOG.md (move Unreleased items under ## [$NEW_VERSION])"
echo "  2. git commit -am \"release: v$NEW_VERSION\""
echo "  3. git tag v$NEW_VERSION && git push origin master v$NEW_VERSION"
