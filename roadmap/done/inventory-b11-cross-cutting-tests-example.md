---
id: inventory-b11
title: Cross-cutting tests + example
topic: inventory
status: done
origin: INVENTORY_ROADMAP.md
---

Context: [context/inventory.md](../context/inventory.md).

## B11. Cross-cutting tests + example (from A11) — DONE

- [x] Extended `sl-proto/tests/lifecycle.rs`: inventory **survives teleport**
      (`teleport_preserves_inventory` / `local_teleport_preserves_inventory` —
      `seed_loaded_inventory` / `assert_inventory_intact` mirror the chat
      persistence pair across the same handover); a cache-merge relogin path
      (`relogin_merge_skips_version_matching_folders`) round-trips a `Loaded`
      tree through the cache bytes and asserts the skeleton merge skips refetch
      of version-matching folders, queuing only the version-bumped one. The
      verbatim-dir (`<agent-uuid>.inv.llsd.gz` written **directly** under the
      cache dir, Firestorm-style) is asserted by the B10 runtime cache-shell
      tests and confirmed by the live verify below.
- [x] Added `sl-client-tokio/examples/inventory_cache.rs`: two sequential logins
      sharing one cache dir — first-login fetch-and-write, then
      second-login-load-and-skip — observable via the per-login
      `InventoryDescendents`-reply count it logs.
- [x] Live verify on OpenSim (`opensim.service`, test avatar `Avatar Tester`):
      cold login fetched **68** folder-contents replies (80 sub-folders, 157
      items) and wrote `<uuid>.inv.llsd.gz` + `.lib.inv.llsd.gz` (version header
      `0x00000005`); warm login fetched **0** — the cache loaded and skipped all
      68 version-matching folders.
- [x] Gate: `cargo fmt --all`, full clippy (restriction lints), `rumdl` on this
      file (80-col), on the current branch.
