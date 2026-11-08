---
id: fake-grid-own-attachments-and-region-moves
title: Fake grid — the agent's own attachments, the COF, and region moves
topic: test
status: done
origin: gap found while fixing viewer-inventory-worn-before-attach-confirmed (2026-09-27)
refs: [viewer-inventory-worn-before-attach-confirmed, viewer-object-moved-region-reuse,
  viewer-menu-touch-object, server-fake-grid-agent-avatars-shared,
  server-fake-grid-attachment-resources]
blocked_by: [gridspec-outfits-wearables, gridspec-attachments]
---

Context: [context/test.md](../context/test.md).

Everything [[viewer-inventory-worn-before-attach-confirmed]] and
[[viewer-menu-touch-object]] fixed was only reproducible on the live
OpenSim, because `sl-fake-grid` models none of it. The survey of
2026-09-27:

- **Wearing and detaching from inventory is decoded and ignored.**
  `SimSession` turns `RezSingleAttachmentFromInv` /
  `RezMultipleAttachmentsFromInv`, `ObjectAttach`, `ObjectDetach` and
  `DetachAttachmentIntoInv` into server events, and `world.rs`'s catch-all
  drops every one. There is no attach at login from the appearance either.
  Only timeline actions (`Action::Attach` / `Detach`) and NPC fixtures
  (`PrimFixture::attached_to`) put an attachment on an avatar.
- **The COF is seeded but never written.** Body-part links exist at login.
  `LinkInventoryItem` is decoded and ignored, and `RemoveInventoryItem` /
  `RemoveInventoryObjects` are not decoded server-side at all, so the
  viewer's COF maintenance goes nowhere.
- **Region moves do not carry attachments.** A crossing or neighbour
  teleport kills nothing on the old circuit, which matches OpenSim's
  `MakeChildAgent`. But the destination rezzes only the avatar, not its
  attachments. Every region also gives the avatar local id 1, where real
  grids assign each region its own, so the "same full id, new local id"
  move is never exercised.
- **`OwnerID` is always the real owner.** OpenSim sends it zeroed unless the
  object has an attached sound (it is the sound's owner). The viewer's first
  inventory-Touch lookup matched on it, and it passed everything the fake
  grid could throw at it.

## Scope

- Handle the attach events: rez the item's object on the agent's avatar
  with its attachment-point `state`, `AttachItemID` and parent, replacing or
  adding per the mode. A refused attach (a missing item, or a scenario
  switch) sends nothing, so the "requested but never arrived" case is
  testable. Re-attach the appearance's attachments at login.
- Handle the detach events: `KillObject` for the attachment, and the item
  back in inventory.
- Accept COF writes: `LinkInventoryItem` into the COF and
  `RemoveInventoryItem` / `RemoveInventoryObjects` out of it, with the
  folder version bumped so the viewer's refetch sees them.
- Carry the avatar's attachments to the destination of a crossing or
  neighbour teleport under the same full ids and **new** local ids. Give
  each region its own avatar local id. Make the old region's kill a
  scenario switch: none (OpenSim), or kills before the arrival (Second Life,
  the order [[viewer-object-moved-region-reuse]] has to survive).
- An `OwnerID` mode matching OpenSim (zeroed without a sound), defaulting to
  it.

## Measured (gridspec-attachments, 2026-10-20)

`book/src/gridspec/appearance.md` § Attachments has the model to build to, and
the `attachments` census (live only for now) is the conformance case to put on
both offline flavours when this lands. What the scope above misses or has to
change:

- An attach kills nothing in the world: `ObjectAttach` (and a script's
  `llAttachToAvatar[Temp]`) sends the same object again, attached, under the
  same ids. A drop is the same in reverse, and removes the item.
- `OwnerID` is nil on an attachment and a dropped object on **both** grids;
  Second Life gives a viewer that is not the owner `OBJECT_OWNER_MODIFY`
  (`0x10000000`), OpenSim does not.
- *Replace* takes off one attachment of the point on Second Life, all of them
  on OpenSim. A worn item worn again, or an unknown item, is ignored; Second
  Life alerts `RezAttemptFailed` for the unknown one.
- The inventory messages: an `ObjectDetach` restates the item (event-queue
  `BulkUpdateInventory` on Second Life, UDP `UpdateCreateInventoryItem` on
  OpenSim); `DetachAttachmentIntoInv` restates nothing; an attach from the
  world announces a new item over UDP on both.
- A HUD goes to its wearer only; Second Life still sends other viewers the
  kill when it comes off.
- The limit is 38, the 39th refused silently.
- Across a teleport the attachment keeps its full id under a new local id.
  Second Life's region kills the avatar (for the agent and for viewers left
  behind) and never its attachments; OpenSim's kills nothing. Both grids
  re-wear what was worn at the next login.

## Measured (gridspec-outfits-wearables, 2026-10-20)

`book/src/gridspec/appearance.md` § Outfits, from the `outfits` census (live
only for now; put it on both offline flavours when this lands). What the COF
half of the scope has to be:

- Second Life writes the folder over AIS3 alone: a link created (`201`) or
  removed (`200`) states `_updated_category_versions` with the folder one
  version on, and a link is stated with `linked_id`, never `asset_id` (the
  fake grid's AIS3 writer does that since). A UDP `LinkInventoryItem` into
  the folder is refused with the `AlertMessage` "Cannot create requested
  inventory." and nothing made.
- OpenSim takes UDP `LinkInventoryItem` (an `UpdateCreateInventoryItem`
  back) and `RemoveInventoryItem` (nothing back) into the folder, one version
  on each; its descendents listing of the folder carries every link's target
  item too. An item link's description, an outfit folder link
  (`AT_LINK_FOLDER`) and a description rewritten later are kept on both.
- Neither grid says anything unasked about a change of the folder: no
  `AgentWearablesUpdate`, no appearance.
- `AgentWearablesRequest`: OpenSim answers the outfit it holds, which the
  last `AgentIsNowWearing` set (in the order sent) and a new login keeps;
  Second Life four fixed placeholders whatever is worn — already imitated
  (`WearablesReply`). But the Second Life flavour's bake service still bakes
  the outfit `AgentIsNowWearing` set, where Second Life bakes the folder's:
  read the outfit from the COF there once the COF takes writes.
- Second Life bakes a slot's layers in the order of their links' `@` tokens;
  its bake version moves only by `IncrementCOFVersion`, not with the
  folder's own version.

## Verify

End-to-end tests against the unmodified client: wear a HUD from inventory
and see it arrive with its COF link; detach it from the pie menu and from
inventory and see the kill and the link drop; teleport to a neighbour and
back with it worn, and see one copy, still worn, still linked; a refused
attach never reads as worn.

## From server-world-sit-and-attach (2026-11-02)

A worn object is now the region's state (`sl-fake-grid/src/seating.rs`): its
root carries `AttachedTo { agent, point, at }`, the store indexes what each
avatar wears, `ObjectAttach` and a script's `llAttachToAvatar` re-send it worn
under the same ids and `ObjectDrop` puts it back, each raising `attach`, and a
worn object is sent to its wearer's viewer alone (`seating::shown_to`, every
fake residents not being shown to each other). Each avatar of a region has
its own local id now (`SceneFixtures::arrive_avatar`), a new one on its
return, which takes "give each region its own avatar local id" off this
task's scope; a teleport or crossing still carries no attachment, which is
the "same full id, new local id" move left to exercise. Left for this task,
on top of
its own scope:

- the item an attach from the world makes and a drop removes; `ObjectDetach`
  restating the worn item (`world::take_off` files a new one today) and
  `DetachAttachmentIntoInv`, which is ignored;
- the `attach(NULL_KEY)` a detach into the inventory raises before the object
  leaves, which needs the object kept in the region for a tick;
- a teleport away or a logout takes the worn objects out of the region
  (`seating::leave`), scripts and all; carrying them, with their scripts'
  state, is the region move this task already names.

## From server-world-changed-raisers (2026-11-03)

`events/changed-worn`: a worn object's script hears `CHANGED_REGION |
CHANGED_TELEPORT` on arriving in each region of a teleport there and back on
Second Life, and nothing else; on OpenSim YEngine first hears its own 32768 as
it leaves, and **XEngine runs `state_entry` again** in the worn object on every
arrival, its globals lost. Once attachments travel, their scripts travel with
them and raise these (`book/src/gridspec/lsl.md` § `changed`: what a viewer's
change raises).

## Done (2026-11-07)

Built to `book/src/gridspec/appearance.md` § Attachments and § Outfits, with
both censuses running offline on both flavours.

- **Wearing** (`sl-fake-grid/src/attachments.rs`, `AttachmentPolicy`): a
  `RezSingleAttachmentFromInv` / `RezMultipleAttachmentsFromInv` rezzes the
  item's linkset onto the avatar under its `AttachItemID`; *replace* takes
  off the point's first (Second Life) or all (OpenSim), restating their
  items; an item worn already and the 39th are ignored, an unknown one is
  refused with Second Life's `RezAttemptFailed`. An `ObjectAttach` and a
  script's `llAttachToAvatar` file a new item, announced over UDP (the
  script's id minted in the region, `FileWorn` filing it in the wearer's
  session); `llAttachToAvatarTemp` (new in `sl-lsl-runtime`) files none.
  Second Life states the appearance again after a wear of one non-HUD item,
  listing what is worn.
- **Taking off**: `ObjectDetach` and a script's `llDetachFromAvatar` kill for
  every viewer (a HUD for the others on Second Life alone) and restate the
  item — one per message for a viewer's — on the event queue or over UDP;
  `DetachAttachmentIntoInv` restates nothing; a drop keeps the ids, clears the
  name-values and removes the item; what comes off is saved into its item.
- **On the wire**: an update states the owner of a sound alone
  (`sl_proto::UpdateOwner`), so a worn and a dropped object say nil; any
  prim's flags say empty contents (`FLAGS_INVENTORY_EMPTY`, kept by the
  store), and a stranger is told it may transfer it and, on Second Life, that
  its owner may modify it.
- **Region moves**: a teleport and a crossing pack what is worn out of the
  region left and unpack it at the arrival, same full ids, new local ids — on
  OpenSim the id it had in a region it comes back to — its scripts carried
  with what they held (`Engine::travel_saved`: `CHANGED_REGION`, with
  `CHANGED_TELEPORT` for a teleport, YEngine's own 32768 as it leaves,
  XEngine's `state_entry` again). Second Life's region left kills the avatar
  for the agent and the viewers left behind as the finish goes out, OpenSim's
  nothing. Each region mints local ids from a range of its own.
- **Logins**: what an account wears is put back on (`WornOutfits`), and its
  inventory outlives the session (`inventory::Inventories`): kept at a logout
  or the kick a second login gives the last session, with the outfit last
  stated; handed over across a teleport or a crossing. Each session mints ids
  from a range of its own (`SimSession::seed_serials`).
- **The folder**: the session already took the COF writes; AIS3 now states a
  link with no flags, permissions or sale, and a link just made with
  `_broken` and `_embedded`. `FakeSl`'s bake service bakes the folder's outfit
  in token order. `AccountConfig::dressed` dresses an account in pants and
  shoes; the offline conformance accounts are dressed.
- **Tests**: `client_end_to_end` — a worn item shown to the other after the
  body and never a HUD, and put back on at the next login; taken off, by the
  item, dropped, refused; carried next door and back. `sl-lsl-runtime` —
  what a travelling script hears per engine. Conformance: `attachments` and
  `outfits` offline on both flavours; `attachments`' two answers that turn on
  aditi's telehub (`move_back_b_arrived`, `move_back_same_local_id`) held live
  only.

Also built after review: a take-off's scripts hear `attach(NULL_KEY)` — the
object, killed for every viewer, waits out the region's next tick before it
is saved into its item and leaves (`attachments::Leaving`); and the
`AttachmentResources` answer lists what is worn
([[server-fake-grid-attachment-resources]]).
