---
id: inventory-a11
title: Test & verification strategy
topic: inventory
status: done
origin: INVENTORY_ROADMAP.md
---

Context: [context/inventory.md](../context/inventory.md).

**A11. Test & verification strategy.** Extend the existing inventory tests
    in `sl-proto/tests/lifecycle.rs` (and the `inventory_edit` example) rather
    than duplicating: skeleton seeds roots + `Unknown`; a descendents reply
    flips a folder to `Loaded` and fills the index; binary-LLSD round-trip
    (every variant + the cache map); merge keeps a version-matching folder (no
    refetch) and drops a stale one; pagination walks a large folder; the model
    **survives teleport** (mirror the chat persistence test); the library tree
    is held under its own root; the chat-log verbatim-dir retrofit. Runtime: a
    caller-supplied temp dir round-trips save → gunzip → header `5` → load →
    merge → model equality. Live: OpenSim relogin reuses unchanged folders.

## Test & verification reference (from A11)

Extend `sl-proto/tests/lifecycle.rs` (reuse `established` + the inbound
builders): skeleton seeds roots + `Unknown`; a descendents reply flips the
folder `Loaded` and the index lists its children; binary-LLSD round-trip (each
variant + the cache map, cross-checked against the XML path); `merge_skeleton`
keeps a version-matching folder (absent from the returned fetch list) and drops
a stale one (present in it); a pagination cursor walks a large folder; inventory
**survives teleport** (mirror the chat persistence test — seed a `Loaded` tree,
drive the handover, assert intact); the library tree sits under its own root.
Runtime (tokio): a caller temp dir round-trips save → gunzip → 4-byte header `5`
→ load → `merge_skeleton` → model equality, plus the chat-log verbatim-dir
change. Live (OpenSim, second test avatar): first login fetches + writes the
cache; second login loads it and skips refetch of version-matching folders
(observed via diagnostics).

**Verified against the code (anchors for B11).** The harness every new test
extends already exists and is the right host — `sl-proto/tests/lifecycle.rs`
(17,762 lines) with the `established` session helper (`:312`) and the inbound
builders (`inbound_im` `:1211`, `inbound_im_from` `:2542`, `inbound_offer_im`
`:15001`, `inbound_group_im` `:16202`). The four A11 sl-proto extensions each
have a **live precedent test** to grow from, not greenfield scaffolding:

- **Skeleton seeds roots + `Unknown`** extends
  `login_skeleton_emits_inventory_skeleton` (`:8503`) — today it only asserts
  the emitted `Event::InventorySkeleton`; B11 additionally asserts the seeded
  `FolderState` (roots known, sub-folders `Unknown`) once B3 lands the model.
- **A descendents reply flips `Loaded` + fills the index** grows
  `inventory_descendents_surfaces_event` (`:8663`) and its CAPS twin
  `caps_inventory_response_surfaces_event` (`:8733`) — both drive a reply
  through the fold; B11 adds the `Loaded { version }` + `inventory_children`
  index assertions.
- **Inventory survives teleport** mirrors `teleport_preserves_chat_and_presence`
  (`:17612`) **exactly** — its `seed_chat_and_presence` (`:17536`) /
  `assert_chat_and_presence_intact` (`:17579`) pair and its
  `TeleportFinish`-driven handover are the template; B11 adds a
  `seed_loaded_inventory` / `assert_inventory_intact` pair across the same
  handover (the intra-region `local_teleport_preserves_chat_and_presence`
  `:17694` is the second site to cover).
- **Pagination walks a large folder** mirrors
  `history_page_pages_newest_first_through_older_windows` (`:17347`) and
  `history_page_on_unopened_session_is_empty` (`:17387`) — the `MessageCursor`
  paging precedent the `InventoryCursor` test copies.

Binary-LLSD round-trip (B2) cross-checks against the **existing XML path**:
`sl-wire/tests/llsd.rs` (258 lines) already round-trips every scalar
(`serializes_every_scalar_and_round_trips` `:155`), nested arrays/maps
(`serializes_nested_arrays_and_maps` `:189`), and the cache-shaped fetch body
(`builds_fetch_inventory_request` `:128`); the inline
`field_accessors_reject_wrong_kind_but_tolerate_absent` (`sl-wire/src/llsd.rs`
`:1285`) is the malformed-input precedent for B2's decode-robustness test. These
LLSD tests **move to `sl-llsd` with B1** (the A3 split), so B2's binary tests
land beside them in the new crate, not in sl-wire.

The example to grow is `sl-client-tokio/examples/inventory_edit.rs` (237 lines —
the create→update→move→delete mutation walk under #30); B11 either extends it or
adds a sibling `inventory_cache` example (grep-confirmed it does **not** exist
yet) for the first-login-write → second-login-load-and-skip flow. The two
existing inventory **unit** tests
(`inventory_keys_round_trip_uuid_bit_identically`
`sl-proto/src/types/inventory.rs:214`, `inventory_folder_ids_survive_round_trip`
`:234`) confirm the typed keys reused as model payloads (A1) already round-trip.

Phase-B-deliverable check (grep-confirmed **absent**, so every B11 assertion
waits on its producing task): `merge_skeleton`, `inventory_folder_page`,
`to_llsd_binary`, `parse_llsd_binary`, `next_inventory_fetch_batch`,
`InventoryCursor`, `FolderState`, and the `FolderType` enum (`FolderType`
appears only in doc comments today). This pins the B11 dependency order — the
teleport/merge/pagination/binary tests each compile only **after** B3/B4/B5/B2
respectively, matching the "every field lands with its writer, reader, and
tests" rule, so B11's cross-cutting tests are split across the B-tasks that
introduce their subjects and the residue (the teleport-survival test, the
relogin example, the live OpenSim verify) is what B11 itself owns.
