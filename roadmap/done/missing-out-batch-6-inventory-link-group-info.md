---
id: missing-out-batch-6
title: inventory link & group info
topic: missing
status: done
origin: MISSING_ROADMAP.md
---

Context: [context/missing.md](../context/missing.md).

**Out batch 6 — inventory link & group info.** `LinkInventoryItem` (create an
inventory link), `GroupTitleUpdate` (set the agent's active group title),
`UpdateGroupInfo` (edit a group's charter/settings).

Implemented as `Session::link_inventory_item(new: &NewInventoryLink)` →
`InventoryCallbackId` (mirroring `create_inventory_item`; the reply arrives as
the already-handled `Event::InventoryItemCreated`),
`Session::update_group_info(params: &UpdateGroupInfoParams)`, and
`Session::update_group_title(group_id: GroupKey, title_role_id: GroupRoleKey)`.
`NewInventoryLink` (in `types/inventory.rs`) keys the link target by the
polymorphic `InventoryItemOrFolderKey` (an item *or* folder link) and keeps
the asset/inv type codes as raw `i8`, consistent with the sibling
`NewInventoryItem`; the wire `TransactionID` is always nil for a link.
`UpdateGroupInfoParams` (in `types/group.rs`) mirrors `CreateGroupParams` but
targets an existing `GroupKey` and carries no name (a group cannot be
renamed). `GroupTitleUpdate` needs no domain struct — it is a `GroupKey` +
`GroupRoleKey` pair (the role carrying the desired title); the message's
routing is otherwise just the echoed agent/session ids. Wired as
`Command::{LinkInventoryItem, UpdateGroupInfo, UpdateGroupTitle}` through the
tokio and bevy runtimes, the `command_name` formatter, and the
`link_inventory_item` / `update_group_info` / `update_group_title` REPL tokens
(with `build_new_inventory_link` / `build_update_group_info_params` helpers,
the link builder choosing item vs folder via a `folder_link` flag). Covered by
two pack-the-wire lifecycle tests and four REPL parse tests. Group ops are
SL-testable against aditi; `LinkInventoryItem` works on OpenSim.
