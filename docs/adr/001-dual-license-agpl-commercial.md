# ADR-001: Dual License AGPL-3.0 + Commercial

**Status:** accepted
**Date:** 2026-03-18
**Issue:** #1

## Context

The engine needs a license that keeps contributions flowing back to the open
source project while allowing commercial game studios to ship proprietary games
without copyleft obligations. MIT/Apache-2.0 was initially considered but
provides no copyleft protection — anyone can fork and close-source without
contributing back.

## Decision

Dual license: **AGPL-3.0-only** (open source default) **OR Commercial** (paid,
with tiered royalties based on gross revenue).

Commercial royalty tiers:
- Up to $100K: free
- $100K–$500K: 1% above $100K
- $500K–$1M: 3% above $500K
- Above $1M: 5% above $1M

Every source file carries `// SPDX-License-Identifier: AGPL-3.0-only OR Commercial`.

## Consequences

- **Easier:** Contributions flow back under AGPL. Revenue from commercial games
  funds development. Small indie studios pay nothing.
- **Harder:** `license` field in Cargo.toml is not a valid SPDX expression for
  crates.io — will need `license-file` if we ever publish. Dual licensing adds
  legal complexity. Some potential contributors may avoid AGPL-licensed projects.

Precedent: Qt, MongoDB, MariaDB use similar dual-license models.
