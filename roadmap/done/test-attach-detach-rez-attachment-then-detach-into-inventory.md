---
id: test-attach-detach
title: rez attachment, then detach into inventory
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 14 — Appearance, attachments & animations `[both]`
---

Context: [context/test.md](../context/test.md).

`attach-detach` — rez attachment, then detach into inventory. `1av`.
Like `object-rez-derez`, the case needs a wearable object *item* the avatar is
not guaranteed to own, so it manufactures one: [`Command::RezObject`]
(`ObjectAdd`) creates a throwaway cube a metre above a region reference
primitive, then [`Command::DerezObjects`]
([`DeRezDestination::TakeIntoAgentInventory`]) takes it into the Objects
folder, materialising the [`Event::InventoryItemCreated`] item. It then wears
that item with [`Command::RezAttachment`] (`RezSingleAttachmentFromInv`) at a
concrete point (right hand) and detaches it with
[`Command::DetachAttachmentIntoInventory`] (`DetachAttachmentIntoInv`). The
attach is proved by matching the rezzed [`Event::ObjectAdded`]'s
`AttachItemID` name-value ([`Object::name_value_data`]) back to the item — not
merely that *some* attachment appeared — and by its `state` byte carrying a
valid attachment point ([`Object::attachment_point`], id 6 = right hand); the
detach by the [`Event::ObjectRemoved`] (`KillObject`) for that object's
region-local id (OpenSim's `DetachSingleAttachmentToInv` → `DeleteSceneObject`
with `silent = false`). Net-new was three attachment re-exports missing from
the runtimes (`RezAttachment` / `AttachmentPoint` / `AttachmentMode` — a
pre-existing parity gap: `sl-client-bevy` also lacked `RezAttachment`) plus
the case. **Green/complete on OpenSim** (attach RTT ~76 ms, detach RTT
~45 ms). The aditi run is deferred with `object-rez-derez` and the batch:
the manufacture-a-cube step needs rez permission, which Second Life gates per
region on an uncontrolled landing region, so the object lifecycle is exercised
on the local grid. `[both]`.
