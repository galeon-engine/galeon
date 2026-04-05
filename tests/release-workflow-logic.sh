#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only OR Commercial
#
# Validates the shell logic used by .github/workflows/release.yml:
#   - Version extraction from Cargo.toml
#   - Tag stripping and tag-Cargo.toml mismatch detection
#   - Prerelease detection and npm dist-tag derivation
#   - Lockstep dependency pin validation (positive + negative)
#   - Workflow structure (workflow_call, needs gates, evidence job)
#
# Run from repo root: bash tests/release-workflow-logic.sh

set -euo pipefail

PASS=0
FAIL=0

assert_eq() {
  local label="$1" expected="$2" actual="$3"
  if [ "$expected" = "$actual" ]; then
    echo "  PASS: $label"
    PASS=$((PASS + 1))
  else
    echo "  FAIL: $label — expected '$expected', got '$actual'"
    FAIL=$((FAIL + 1))
  fi
}

# ── Version extraction ───────────────────────────────────────────────

echo "=== Version extraction from Cargo.toml ==="
VERSION=$(grep -m1 '^version' Cargo.toml | sed 's/.*"\(.*\)"/\1/')
assert_eq "workspace version is semver" "0.1.0" "$VERSION"

# ── Tag stripping ────────────────────────────────────────────────────

echo ""
echo "=== Tag version stripping ==="
for TAG_REF in "refs/tags/v0.2.0" "refs/tags/v1.0.0-alpha.1" "refs/tags/v0.3.0-rc.2"; do
  TAG_VER="${TAG_REF#refs/tags/v}"
  case "$TAG_REF" in
    refs/tags/v0.2.0)         assert_eq "v0.2.0 → 0.2.0" "0.2.0" "$TAG_VER" ;;
    refs/tags/v1.0.0-alpha.1) assert_eq "v1.0.0-alpha.1 → 1.0.0-alpha.1" "1.0.0-alpha.1" "$TAG_VER" ;;
    refs/tags/v0.3.0-rc.2)    assert_eq "v0.3.0-rc.2 → 0.3.0-rc.2" "0.3.0-rc.2" "$TAG_VER" ;;
  esac
done

# ── Tag-Cargo.toml mismatch detection ────────────────────────────────

echo ""
echo "=== Tag-Cargo.toml mismatch detection ==="
CARGO_VER="0.1.0"

TAG_VER="0.2.0"
MISMATCH="false"
if [ "$TAG_VER" != "$CARGO_VER" ]; then MISMATCH="true"; fi
assert_eq "detects v0.2.0 vs 0.1.0 mismatch" "true" "$MISMATCH"

TAG_VER="0.1.0"
MISMATCH="false"
if [ "$TAG_VER" != "$CARGO_VER" ]; then MISMATCH="true"; fi
assert_eq "no mismatch for v0.1.0 vs 0.1.0" "false" "$MISMATCH"

# ── Prerelease detection + npm dist-tag ──────────────────────────────

echo ""
echo "=== Prerelease detection + npm dist-tag ==="
for CASE in \
  "0.2.0|latest" \
  "1.0.0-alpha.1|alpha" \
  "0.3.0-beta.2|beta" \
  "2.0.0-rc.1|rc" \
  "0.2.0-alpha-canary.1|alpha" \
  "0.2.0-0.1|pre" \
  "1.0.0-SNAPSHOT|SNAPSHOT" \
  "3.0.0-dev.0|dev" \
  "1.0.0-next.3|next"; do
  VER="${CASE%%|*}"
  EXPECTED="${CASE##*|}"
  if [[ "$VER" == *-* ]]; then
    PRE_PART="${VER#*-}"
    PRE_SEG="${PRE_PART%%[.-]*}"
    if [[ "$PRE_SEG" =~ ^[a-zA-Z] ]]; then
      NPM_TAG="$PRE_SEG"
    else
      NPM_TAG="pre"
    fi
  else
    NPM_TAG="latest"
  fi
  assert_eq "$VER → $EXPECTED" "$EXPECTED" "$NPM_TAG"
done

# ── cargo search grep pattern ────────────────────────────────────────

echo ""
echo "=== cargo search grep pattern ==="
SEARCH_OUTPUT='galeon-engine-macros = "0.1.0"    # Procedural macros'

FOUND="false"
if echo "$SEARCH_OUTPUT" | grep -q '"0.1.0"'; then FOUND="true"; fi
assert_eq "grep matches existing 0.1.0" "true" "$FOUND"

FOUND="false"
if echo "$SEARCH_OUTPUT" | grep -q '"0.2.0"'; then FOUND="true"; fi
assert_eq "grep rejects missing 0.2.0" "false" "$FOUND"

# ── workflow_dispatch vs tag path ────────────────────────────────────

echo ""
echo "=== workflow_dispatch vs tag path ==="
GITHUB_REF="refs/heads/master"
IS_TAG="false"
if [[ "$GITHUB_REF" == refs/tags/v* ]]; then IS_TAG="true"; fi
assert_eq "master push → is_tag=false" "false" "$IS_TAG"

GITHUB_REF="refs/tags/v0.2.0"
IS_TAG="false"
if [[ "$GITHUB_REF" == refs/tags/v* ]]; then IS_TAG="true"; fi
assert_eq "tag push → is_tag=true" "true" "$IS_TAG"

# ── Lockstep pin validation (positive) ───────────────────────────────

echo ""
echo "=== Lockstep pin validation (positive — pins match) ==="

MACROS_PIN=$(grep 'galeon-engine-macros' crates/engine/Cargo.toml | grep -o '"=[^"]*"' | tr -d '"')
assert_eq "engine→macros pin =$VERSION" "=$VERSION" "$MACROS_PIN"

ENGINE_PIN=$(grep 'galeon-engine ' crates/engine-three-sync/Cargo.toml | grep -o '"=[^"]*"' | tr -d '"')
assert_eq "three-sync→engine pin =$VERSION" "=$VERSION" "$ENGINE_PIN"

RUNTIME_PIN=$(node -e "import{readFileSync}from'fs';const p=JSON.parse(readFileSync('./packages/engine-ts/package.json','utf8'));console.log(p.dependencies['@galeon/runtime']||'')" --input-type=module)
assert_eq "engine-ts→runtime pin =$VERSION" "=$VERSION" "$RUNTIME_PIN"

# ── Lockstep pin validation (negative) ───────────────────────────────

echo ""
echo "=== Lockstep pin validation (negative — mismatch detection) ==="
FAKE="9.9.9"

ERRORS=0
if [ "$MACROS_PIN" != "=$FAKE" ]; then ERRORS=$((ERRORS + 1)); fi
if [ "$ENGINE_PIN" != "=$FAKE" ]; then ERRORS=$((ERRORS + 1)); fi
if [ "$RUNTIME_PIN" != "=$FAKE" ]; then ERRORS=$((ERRORS + 1)); fi
assert_eq "all 3 pins mismatch $FAKE" "3" "$ERRORS"

ERRORS=0
GOOD="=$FAKE"
STALE="=$VERSION"
if [ "$GOOD" != "=$FAKE" ]; then ERRORS=$((ERRORS + 1)); fi   # matches — 0
if [ "$STALE" != "=$FAKE" ]; then ERRORS=$((ERRORS + 1)); fi  # stale — 1
if [ "$STALE" != "=$FAKE" ]; then ERRORS=$((ERRORS + 1)); fi  # stale — 2
assert_eq "2 of 3 pins stale detected" "2" "$ERRORS"

# ── Workflow structure checks ────────────────────────────────────────

echo ""
echo "=== Workflow structure ==="

HAS="false"
if grep -q "version.workspace = true" crates/engine-macros/Cargo.toml; then HAS="true"; fi
assert_eq "engine-macros uses version.workspace" "true" "$HAS"

HAS="false"
if grep -q "version.workspace = true" crates/engine/Cargo.toml; then HAS="true"; fi
assert_eq "engine uses version.workspace" "true" "$HAS"

HAS="false"
if grep -q "version.workspace = true" crates/engine-three-sync/Cargo.toml; then HAS="true"; fi
assert_eq "engine-three-sync uses version.workspace" "true" "$HAS"

HAS="false"
if grep -q '^version = ' Cargo.toml; then HAS="true"; fi
assert_eq "workspace root declares version" "true" "$HAS"

HAS="false"
if grep -q "workflow_call:" .github/workflows/ci.yml; then HAS="true"; fi
assert_eq "ci.yml has workflow_call" "true" "$HAS"

HAS="false"
if grep -q "uses: ./.github/workflows/ci.yml" .github/workflows/release.yml; then HAS="true"; fi
assert_eq "release.yml invokes ci.yml" "true" "$HAS"

HAS="false"
if grep -A3 "publish-crates:" .github/workflows/release.yml | grep -q "needs:.*ci"; then HAS="true"; fi
assert_eq "publish-crates needs ci" "true" "$HAS"

HAS="false"
if grep -A3 "publish-npm:" .github/workflows/release.yml | grep -q "needs:.*ci"; then HAS="true"; fi
assert_eq "publish-npm needs ci" "true" "$HAS"

HAS="false"
if grep -q "sleep 45" .github/workflows/release.yml; then HAS="true"; fi
assert_eq "no sleep 45 in release.yml" "false" "$HAS"

HAS="false"
if grep -q "tags: \['v\*'\]" .github/workflows/release.yml; then HAS="true"; fi
assert_eq "release.yml triggers on v* tags" "true" "$HAS"

HAS="false"
if grep -q "verify-publish:" .github/workflows/release.yml; then HAS="true"; fi
assert_eq "verify-publish job exists" "true" "$HAS"

HAS="false"
if grep -q "release-evidence:" .github/workflows/release.yml; then HAS="true"; fi
assert_eq "release-evidence job exists" "true" "$HAS"

HAS="false"
if grep -A3 "release-evidence:" .github/workflows/release.yml | grep -q "needs:.*ci"; then HAS="true"; fi
assert_eq "release-evidence needs ci" "true" "$HAS"

for CRATE in galeon-cli protocol-consumer-test protocol-rename-test; do
  HAS="false"
  if grep -q "publish = false" "crates/$CRATE/Cargo.toml"; then HAS="true"; fi
  assert_eq "$CRATE has publish=false" "true" "$HAS"
done

# ── Results ──────────────────────────────────────────────────────────

echo ""
echo "════════════════════════════════"
echo "Results: $PASS passed, $FAIL failed"
echo "════════════════════════════════"
[ "$FAIL" -eq 0 ] || exit 1
