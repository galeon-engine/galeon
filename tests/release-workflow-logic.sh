#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only OR Commercial
#
# Tests the shell logic used by .github/workflows/release.yml.
# Pure logic only — no working-tree file reads.
#
# Run from anywhere: bash tests/release-workflow-logic.sh

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

# ── Tag stripping (GITHUB_REF_NAME#v) ───────────────────────────────

echo "=== Tag version stripping ==="
for CASE in "v0.2.0|0.2.0" "v1.0.0-alpha.1|1.0.0-alpha.1" "v0.3.0-rc.2|0.3.0-rc.2" "v10.20.30|10.20.30"; do
  INPUT="${CASE%%|*}"
  EXPECTED="${CASE##*|}"
  ACTUAL="${INPUT#v}"
  assert_eq "$INPUT → $EXPECTED" "$EXPECTED" "$ACTUAL"
done

# ── Prerelease dist-tag (gametau pattern: prerelease → alpha) ────────

echo ""
echo "=== Prerelease dist-tag ==="
for CASE in "0.2.0|latest" "1.0.0-alpha.1|alpha" "0.3.0-beta.2|alpha" "0.2.0-0.1|alpha"; do
  VER="${CASE%%|*}"
  EXPECTED="${CASE##*|}"
  if [[ "$VER" == *-* ]]; then TAG="alpha"; else TAG="latest"; fi
  assert_eq "$VER → $EXPECTED" "$EXPECTED" "$TAG"
done

# ── cargo search propagation grep ────────────────────────────────────

echo ""
echo "=== cargo search grep pattern ==="
SEARCH_OUTPUT='galeon-engine-macros = "0.1.0"    # Procedural macros'

FOUND="$( echo "$SEARCH_OUTPUT" | grep -q '"0.1.0"' && echo true || echo false )"
assert_eq "matches published 0.1.0" "true" "$FOUND"

FOUND="$( echo "$SEARCH_OUTPUT" | grep -q '"0.2.0"' && echo true || echo false )"
assert_eq "rejects missing 0.2.0" "false" "$FOUND"

# ── "already exists" string match ────────────────────────────────────

echo ""
echo "=== already-exists detection ==="
OUTPUT='error: crate version `0.1.0` is already uploaded; already exists on crates.io index'
MATCH="$( [[ "$OUTPUT" == *"already exists on crates.io index"* ]] && echo true || echo false )"
assert_eq "detects already-exists" "true" "$MATCH"

OUTPUT='error: failed to select a version for the requirement `galeon-engine-macros`'
MATCH="$( [[ "$OUTPUT" == *"already exists on crates.io index"* ]] && echo true || echo false )"
assert_eq "rejects propagation error" "false" "$MATCH"

# ── Propagation retry detection ──────────────────────────────────────

echo ""
echo "=== propagation retry detection ==="
OUTPUT='error: failed to select a version for the requirement `galeon-engine-macros = "^0.2.0"`'
RETRY="false"
if [[ "$OUTPUT" == *"failed to select a version"* ]] && [[ "$OUTPUT" == *"galeon-engine-macros"* ]]; then
  RETRY="true"
fi
assert_eq "retries on macros propagation" "true" "$RETRY"

OUTPUT='error: some other error'
RETRY="false"
if [[ "$OUTPUT" == *"failed to select a version"* ]] && [[ "$OUTPUT" == *"galeon-engine-macros"* ]]; then
  RETRY="true"
fi
assert_eq "does not retry on unrelated error" "false" "$RETRY"

# ── awk version extraction ───────────────────────────────────────────

echo ""
echo "=== awk version extraction ==="
TOML_LINE='version = "0.2.0"'
EXTRACTED="$(echo "$TOML_LINE" | awk -F'"' '/^version = / {print $2; exit}')"
assert_eq "extracts 0.2.0 from TOML line" "0.2.0" "$EXTRACTED"

# ── Results ──────────────────────────────────────────────────────────

echo ""
echo "════════════════════════════════"
echo "Results: $PASS passed, $FAIL failed"
echo "════════════════════════════════"
[ "$FAIL" -eq 0 ] || exit 1
