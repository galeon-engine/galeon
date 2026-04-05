#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only OR Commercial
#
# bump-version.sh — Update all 6 lockstep version sources atomically.
#
# Usage: scripts/bump-version.sh X.Y.Z
#
# Validates semver format, checks current versions are consistent,
# then updates all 7 locations across 6 files. Fails fast on any
# inconsistency or missing file.

set -euo pipefail

# ── Helpers ──────────────────────────────────────────────────────────

die() { printf 'error: %s\n' "$1" >&2; exit 1; }
ok()  { printf '  ✓ %s\n' "$1"; }

# ── Args ─────────────────────────────────────────────────────────────

NEW_VERSION="${1:-}"

if [[ -z "$NEW_VERSION" ]]; then
  echo "Usage: scripts/bump-version.sh X.Y.Z"
  echo ""
  echo "Updates all 6 lockstep version sources (7 edits)."
  exit 1
fi

# Validate semver (with optional prerelease/build metadata)
if ! [[ "$NEW_VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[a-zA-Z0-9.]+)?(\+[a-zA-Z0-9.]+)?$ ]]; then
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
ENGINE_TS_PKG="packages/engine-ts/package.json"
SHELL_PKG="packages/shell/package.json"

ALL_FILES=(
  "$WORKSPACE_CARGO"
  "$ENGINE_CARGO"
  "$THREE_SYNC_CARGO"
  "$RUNTIME_PKG"
  "$ENGINE_TS_PKG"
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
CURRENT=$(sed -n '/\[workspace\.package\]/,/^\[/{s/^version *= *"\(.*\)"/\1/p}' "$WORKSPACE_CARGO")
[[ -n "$CURRENT" ]] || die "Cannot read workspace version from $WORKSPACE_CARGO"
ok "$WORKSPACE_CARGO  workspace.package.version = $CURRENT"

# 2. engine dep on macros
V_ENGINE_MACROS=$(sed -n 's/^galeon-engine-macros.*version *= *"=\([^"]*\)".*/\1/p' "$ENGINE_CARGO")
[[ -n "$V_ENGINE_MACROS" ]] || die "Cannot read galeon-engine-macros pin from $ENGINE_CARGO"
ok "$ENGINE_CARGO  galeon-engine-macros = =$V_ENGINE_MACROS"

# 3. three-sync dep on engine
V_THREE_ENGINE=$(sed -n 's/^galeon-engine.*version *= *"=\([^"]*\)".*/\1/p' "$THREE_SYNC_CARGO")
[[ -n "$V_THREE_ENGINE" ]] || die "Cannot read galeon-engine pin from $THREE_SYNC_CARGO"
ok "$THREE_SYNC_CARGO  galeon-engine = =$V_THREE_ENGINE"

# 4-6. package.json versions (using grep to extract)
read_pkg_version() {
  sed -n 's/.*"version": *"\([^"]*\)".*/\1/p' "$1" | head -1
}

V_RUNTIME=$(read_pkg_version "$RUNTIME_PKG")
[[ -n "$V_RUNTIME" ]] || die "Cannot read version from $RUNTIME_PKG"
ok "$RUNTIME_PKG  version = $V_RUNTIME"

V_ENGINE_TS=$(read_pkg_version "$ENGINE_TS_PKG")
[[ -n "$V_ENGINE_TS" ]] || die "Cannot read version from $ENGINE_TS_PKG"
ok "$ENGINE_TS_PKG  version = $V_ENGINE_TS"

# 5b. engine-ts dep on runtime
V_ENGINE_TS_RUNTIME=$(sed -n 's/.*"@galeon\/runtime": *"=\([^"]*\)".*/\1/p' "$ENGINE_TS_PKG")
[[ -n "$V_ENGINE_TS_RUNTIME" ]] || die "Cannot read @galeon/runtime pin from $ENGINE_TS_PKG"
ok "$ENGINE_TS_PKG  @galeon/runtime = =$V_ENGINE_TS_RUNTIME"

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
  "$V_ENGINE_TS"
  "$V_ENGINE_TS_RUNTIME"
  "$V_SHELL"
)

for v in "${ALL_VERSIONS[@]}"; do
  if [[ "$v" != "$CURRENT" ]]; then
    die "Version mismatch! Expected all to be '$CURRENT' but found '$v'. Fix manually before bumping."
  fi
done
ok "All 7 locations currently at $CURRENT"
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

# 5. engine-ts package.json (version + runtime dep)
sed -i "s/\"version\": \"$OLD_ESC\"/\"version\": \"$NEW_VERSION\"/" "$ENGINE_TS_PKG"
sed -i "s/\"@galeon\/runtime\": \"=$OLD_ESC\"/\"@galeon\/runtime\": \"=$NEW_VERSION\"/" "$ENGINE_TS_PKG"
ok "$ENGINE_TS_PKG"

# 6. shell package.json
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
verify "$ENGINE_TS_PKG"      "\"version\": \"$NEW_VERSION\""         "engine-ts version"
verify "$ENGINE_TS_PKG"      "\"@galeon/runtime\": \"=$NEW_VERSION\"" "runtime pin"
verify "$SHELL_PKG"          "\"version\": \"$NEW_VERSION\""         "shell version"

if [[ $ERRORS -gt 0 ]]; then
  die "$ERRORS verification(s) failed. Check the files manually."
fi

echo ""
echo "Done. All 7 locations updated to $NEW_VERSION."
echo ""
echo "Next steps:"
echo "  1. Update CHANGELOG.md (move Unreleased items under ## [$NEW_VERSION])"
echo "  2. git commit -am \"release: v$NEW_VERSION\""
echo "  3. git tag v$NEW_VERSION && git push origin master v$NEW_VERSION"
