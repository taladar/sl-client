---
id: inventory-b2
title: Binary-LLSD codec in sl-llsd
topic: inventory
status: done
origin: INVENTORY_ROADMAP.md
---

Context: [context/inventory.md](../context/inventory.md).

## B2. Binary-LLSD codec in `sl-llsd` (from A3)

Standalone; the cache tasks (B5/B10) serialise through it.

- [x] Add `sl-llsd/src/binary.rs` (+ the `time` dep, for the date path):
      `Llsd::to_llsd_binary(&self) -> Vec<u8>` and
      `parse_llsd_binary(bytes: &[u8]) -> Result<Llsd, LlsdError>` over all 11
      `Llsd` variants, per the A3 tag-byte spec; export it. Honour the A3-pinned
      Firestorm wrinkles: **emit and require** the closing `]` / `}` (a missing
      terminator is an `Err`), treat the 4-byte BE count as authoritative,
      tolerate notation-style `'` / `"` strings + quoted keys on read but only
      emit length-prefixed `s` / `k`, and convert `Llsd::Date` (ISO-8601 string)
      ↔ `f64` epoch-seconds — matching Firestorm's host-endian raw `Date` write
      (`Real` stays BE via `ll_htond`).
- [x] Round-trip tests: each variant individually; a nested map/array; the cache
  map shape `{ categories: [...], items: [...] }` (note item creation dates are
  LLSD `Integer`, not `Date`, so the cache map never exercises the date path);
  and `binary → Llsd` equals `xml → Llsd` for a shared fixture (cross-check
  against the existing XML path).
- [x] A decode-robustness test (truncated / bad-tag / missing-terminator /
  count-mismatch input ⇒ `Err`, no panic; no indexing-panic — restriction-lint
  clean).
