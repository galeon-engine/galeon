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

# ── Helper: mirrors the workflow's prerelease extraction logic ───────

npm_tag_for() {
  local VER="$1"
  if [[ "$VER" == *-* ]]; then
    local PRE_PART="${VER#*-}"
    local PRE_SEG="${PRE_PART%%[.-]*}"
    if [[ "$PRE_SEG" =~ ^[a-zA-Z] ]]; then
      echo "$PRE_SEG"
    else
      echo "pre"
    fi
  else
    echo "latest"
  fi
}

# ── Helper: mirrors the workflow's tag stripping logic ───────────────

strip_tag() {
  echo "${1#refs/tags/v}"
}

# ── Helper: mirrors the workflow's is_tag detection ──────────────────

is_tag_ref() {
  if [[ "$1" == refs/tags/v* ]]; then echo "true"; else echo "false"; fi
}

# ── Helper: mirrors the workflow's pin mismatch check ────────────────

pins_match() {
  local pin="$1" version="$2"
  if [ "$pin" = "=$version" ]; then echo "match"; else echo "mismatch"; fi
}

# ── Tag stripping ────────────────────────────────────────────────────

echo "=== Tag version stripping ==="
assert_eq "v0.2.0"         "0.2.0"           "$(strip_tag refs/tags/v0.2.0)"
assert_eq "v1.0.0-alpha.1" "1.0.0-alpha.1"   "$(strip_tag refs/tags/v1.0.0-alpha.1)"
assert_eq "v0.3.0-rc.2"    "0.3.0-rc.2"      "$(strip_tag refs/tags/v0.3.0-rc.2)"
assert_eq "v10.20.30"      "10.20.30"         "$(strip_tag refs/tags/v10.20.30)"

# ── is_tag detection ─────────────────────────────────────────────────

echo ""
echo "=== is_tag detection ==="
assert_eq "tag push"        "true"  "$(is_tag_ref refs/tags/v0.2.0)"
assert_eq "master push"     "false" "$(is_tag_ref refs/heads/master)"
assert_eq "PR merge"        "false" "$(is_tag_ref refs/pull/42/merge)"
assert_eq "dispatch"        "false" "$(is_tag_ref refs/heads/main)"

# ── Tag-version mismatch detection ───────────────────────────────────

echo ""
echo "=== Tag-version mismatch detection ==="
assert_eq "match 0.1.0=0.1.0"     "false" "$( [ '0.1.0' != '0.1.0' ] && echo true || echo false )"
assert_eq "mismatch 0.2.0≠0.1.0"  "true"  "$( [ '0.2.0' != '0.1.0' ] && echo true || echo false )"
assert_eq "mismatch alpha≠stable"  "true"  "$( [ '0.2.0-alpha.1' != '0.2.0' ] && echo true || echo false )"

# ── Prerelease npm dist-tag derivation ───────────────────────────────

echo ""
echo "=== Prerelease npm dist-tag ==="
assert_eq "stable → latest"          "latest"   "$(npm_tag_for 0.2.0)"
assert_eq "alpha.1 → alpha"          "alpha"    "$(npm_tag_for 1.0.0-alpha.1)"
assert_eq "beta.2 → beta"            "beta"     "$(npm_tag_for 0.3.0-beta.2)"
assert_eq "rc.1 → rc"                "rc"       "$(npm_tag_for 2.0.0-rc.1)"
assert_eq "alpha-canary.1 → alpha"   "alpha"    "$(npm_tag_for 0.2.0-alpha-canary.1)"
assert_eq "numeric 0.1 → pre"        "pre"      "$(npm_tag_for 0.2.0-0.1)"
assert_eq "SNAPSHOT → SNAPSHOT"       "SNAPSHOT" "$(npm_tag_for 1.0.0-SNAPSHOT)"
assert_eq "dev.0 → dev"              "dev"      "$(npm_tag_for 3.0.0-dev.0)"
assert_eq "next.3 → next"            "next"     "$(npm_tag_for 1.0.0-next.3)"

# ── cargo search grep pattern ────────────────────────────────────────

echo ""
echo "=== cargo search grep pattern ==="
SEARCH_OUTPUT='galeon-engine-macros = "0.1.0"    # Procedural macros'

FOUND="$( echo "$SEARCH_OUTPUT" | grep -q '"0.1.0"' && echo true || echo false )"
assert_eq "matches published 0.1.0" "true" "$FOUND"

FOUND="$( echo "$SEARCH_OUTPUT" | grep -q '"0.2.0"' && echo true || echo false )"
assert_eq "rejects missing 0.2.0" "false" "$FOUND"

# ── Lockstep pin comparison logic ────────────────────────────────────

echo ""
echo "=== Lockstep pin comparison ==="
assert_eq "=0.1.0 vs 0.1.0 → match"      "match"    "$(pins_match '=0.1.0' '0.1.0')"
assert_eq "=0.1.0 vs 0.2.0 → mismatch"   "mismatch" "$(pins_match '=0.1.0' '0.2.0')"
assert_eq "=0.2.0 vs 0.2.0 → match"      "match"    "$(pins_match '=0.2.0' '0.2.0')"
assert_eq "=0.1.0 vs 1.0.0 → mismatch"   "mismatch" "$(pins_match '=0.1.0' '1.0.0')"

# ── Results ──────────────────────────────────────────────────────────

echo ""
echo "════════════════════════════════"
echo "Results: $PASS passed, $FAIL failed"
echo "════════════════════════════════"
[ "$FAIL" -eq 0 ] || exit 1
