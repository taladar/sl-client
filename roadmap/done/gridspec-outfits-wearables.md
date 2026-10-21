---
id: gridspec-outfits-wearables
title: The Current Outfit Folder and wearables on each grid
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, viewer-bake-cof-layer-order, viewer-outfit-editor,
  fake-grid-own-attachments-and-region-moves]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Done (2026-10-20)

The `outfits` census (1av, live grids; eight runs on aditi, five on OpenSim,
the last of each with every answer held) reads the Current Outfit Folder and
the legacy reply, wears a copy of A's shape by the folder (a link, the old
link removed, a bake) and by `AgentIsNowWearing`, sends a UDP link into the
folder raw, logs in again, links two copies of a clothing layer with the
reference's order tokens and swaps them, links an outfit folder, and puts the
outfit back (`book/src/gridspec/appearance.md` § Outfits). Findings:

- Second Life's AIS3 states a link's target as `linked_id`, with no
  `asset_id`; OpenSim's descendents listing of the folder carries every
  link's target item besides.
- `AgentWearablesRequest` is answered on Second Life with four fixed
  placeholder items (the ids the reference sends in its dummy
  `AgentIsNowWearing`), types 1 to 4, each naming a fixed asset, never a
  shape, whatever is worn; OpenSim answers the outfit the last
  `AgentIsNowWearing` stated, in the order sent, and keeps it across a login.
  Neither sends one unasked.
- Second Life writes the folder over AIS3 alone (one version on per write)
  and refuses a UDP link with the `AlertMessage` "Cannot create requested
  inventory."; OpenSim takes UDP links and removals. Neither tells the agent
  anything of a change to the folder.
- Second Life bakes a slot's layers in the order of the links' `@` tokens.
  Its bake version moves only with `IncrementCOFVersion` (which also moves
  the folder's), and a refused bake dumps the outfit of the last accepted
  bake.

Viewer: two real bugs, both fixed.

- `inventory_item_from_llsd` read only `asset_id`, so every AIS3 link — the
  whole Second Life COF — named no target (`current-outfit-folder` was failing
  on aditi: 43 of 43 links unresolved). It reads `linked_id` (unit-tested).
- The own avatar's wearables (`bake_inputs.rs`), which its shape is made of,
  came from `AgentWearables` alone: on Second Life the four placeholders, no
  shape. On a placeholder answer the viewer now reads the outfit from the
  folder — its links, each target fetched over AIS3 — ordered by slot and
  token, and a re-listed folder with other links is an outfit change
  (unit-tested; a full-stack test on both fake flavours,
  `the_own_shape_is_read_from_where_the_grid_keeps_the_outfit`, fails without
  the fix).

Fake grid: small parts done — the Second Life flavour answers
`AgentWearablesRequest` with the placeholders (`WearablesReply`), and its AIS3
writer states links by `linked_id`. Taking writes into the folder stays with
[[fake-grid-own-attachments-and-region-moves]], which now carries the
measured model; the census goes offline with it.

## Known already

COF green on both grids; SL rejects UDP links into it; `AgentWearablesUpdate`
full on OpenSim, a dummy on aditi; the fake grid ignores UDP COF links and
`AgentIsNowWearing`.

## Discover

`current_outfit_folder`, `wearables_request`, wearing / replacing outfits via
the live e2e; ordering conventions; unsolicited `AgentWearablesUpdate`.

## Document

`book/src/gridspec/appearance.md` § Outfits.

## Fake grid

Medium — [[fake-grid-own-attachments-and-region-moves]].

## Viewer

Outfit read from the COF on SL, wearables on OpenSim.
