#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only OR Commercial
#
# Tests the shell logic used by .github/workflows/release.yml.
# Pure logic only - no working-tree file reads.
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
    echo "  FAIL: $label - expected '$expected', got '$actual'"
    FAIL=$((FAIL + 1))
  fi
}

# -- Tag stripping (GITHUB_REF_NAME#v) --------------------------------

echo "=== Tag version stripping ==="
for CASE in "v0.2.0|0.2.0" "v1.0.0-alpha.1|1.0.0-alpha.1" "v0.3.0-rc.2|0.3.0-rc.2" "v10.20.30|10.20.30"; do
  INPUT="${CASE%%|*}"
  EXPECTED="${CASE##*|}"
  ACTUAL="${INPUT#v}"
  assert_eq "$INPUT -> $EXPECTED" "$EXPECTED" "$ACTUAL"
done

# -- Prerelease dist-tag (gametau pattern: prerelease -> alpha) ------

echo ""
echo "=== Prerelease dist-tag ==="
for CASE in "0.2.0|latest" "1.0.0-alpha.1|alpha" "0.3.0-beta.2|alpha" "0.2.0-0.1|alpha"; do
  VER="${CASE%%|*}"
  EXPECTED="${CASE##*|}"
  if [[ "$VER" == *-* ]]; then TAG="alpha"; else TAG="latest"; fi
  assert_eq "$VER -> $EXPECTED" "$EXPECTED" "$TAG"
done

# -- GitHub Release metadata -----------------------------------------

echo ""
echo "=== GitHub Release flags ==="
for CASE in "0.2.0|false|--latest" "1.0.0-alpha.1|true|--latest=false" "0.3.0-rc.2|true|--latest=false"; do
  VER="${CASE%%|*}"
  REST="${CASE#*|}"
  EXPECTED_PRERELEASE="${REST%%|*}"
  EXPECTED_LATEST="${REST##*|}"

  if [[ "$VER" == *-* ]]; then
    PRERELEASE="true"
    LATEST_FLAG="--latest=false"
  else
    PRERELEASE="false"
    LATEST_FLAG="--latest"
  fi

  assert_eq "$VER prerelease" "$EXPECTED_PRERELEASE" "$PRERELEASE"
  assert_eq "$VER latest flag" "$EXPECTED_LATEST" "$LATEST_FLAG"
done

echo ""
echo "=== Release evidence artifact naming ==="
for CASE in "v0.2.0|release-evidence-v0.2.0|.release-evidence/release-evidence-v0.2.0.md" \
            "v1.0.0-alpha.1|release-evidence-v1.0.0-alpha.1|.release-evidence/release-evidence-v1.0.0-alpha.1.md"; do
  TAG="${CASE%%|*}"
  REST="${CASE#*|}"
  EXPECTED_ARTIFACT="${REST%%|*}"
  EXPECTED_FILE="${REST##*|}"

  ACTUAL_ARTIFACT="release-evidence-${TAG}"
  ACTUAL_FILE=".release-evidence/release-evidence-v${TAG#v}.md"

  assert_eq "$TAG artifact name" "$EXPECTED_ARTIFACT" "$ACTUAL_ARTIFACT"
  assert_eq "$TAG evidence file" "$EXPECTED_FILE" "$ACTUAL_FILE"
done

# -- cargo search propagation grep ------------------------------------

echo ""
echo "=== cargo search grep pattern ==="
SEARCH_OUTPUT='galeon-engine-macros = "0.1.0"    # Procedural macros'

FOUND="$( echo "$SEARCH_OUTPUT" | grep -q '"0.1.0"' && echo true || echo false )"
assert_eq "matches published 0.1.0" "true" "$FOUND"

FOUND="$( echo "$SEARCH_OUTPUT" | grep -q '"0.2.0"' && echo true || echo false )"
assert_eq "rejects missing 0.2.0" "false" "$FOUND"

# -- "already exists" string match ------------------------------------

echo ""
echo "=== already-exists detection ==="
OUTPUT='error: crate version `0.1.0` is already uploaded; already exists on crates.io index'
MATCH="$( [[ "$OUTPUT" == *"already exists on crates.io index"* ]] && echo true || echo false )"
assert_eq "detects already-exists" "true" "$MATCH"

OUTPUT='error: failed to select a version for the requirement `galeon-engine-macros`'
MATCH="$( [[ "$OUTPUT" == *"already exists on crates.io index"* ]] && echo true || echo false )"
assert_eq "rejects propagation error" "false" "$MATCH"

# -- Propagation retry detection --------------------------------------

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

# -- awk version extraction -------------------------------------------

echo ""
echo "=== awk version extraction ==="
TOML_LINE='version = "0.2.0"'
EXTRACTED="$(echo "$TOML_LINE" | awk -F'"' '/^version = / {print $2; exit}')"
assert_eq "extracts 0.2.0 from TOML line" "0.2.0" "$EXTRACTED"

# -- Results ----------------------------------------------------------

echo ""
echo "================================"
echo "Results: $PASS passed, $FAIL failed"
echo "================================"
[ "$FAIL" -eq 0 ] || exit 1
