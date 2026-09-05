---
id: test-fake-grid-asset-round-trip
title: An asset id the grid hands out should name bytes the grid can serve
topic: test
status: done
origin: noticed doing test-fake-grid-object-write-path (2026-09-05)
points: 5
refs:
  [
    test-fake-grid-object-write-path,
    test-asset-save-mutation-survey,
    test-assets-object-asset-codec,
    test-assets-remaining-class-audit,
    test-shared-test-assets,
    viewer-task-inventory-open-and-save-back,
  ]
---

Context: [context/testing.md](../context/testing.md).

The fake grid hands out asset ids from three stores and backs almost none of
them with bytes. An id a viewer is given is an id it will eventually fetch —
to open a notecard, play a gesture, wear a shirt, read a script out of a
prim, or check that the thing it just saved is really what the grid now
holds — and today most of those fetches 404.

The reason it matters is the viewer more than the grid: **a save is only
observable as a re-fetch, and an inventory item is only openable if its
asset resolves.** The notecard editor, the LSL editor, the appearance and
material editors and the EEP editors all end in an upload, and nothing below
a live grid can currently tell "the save reached the service" from "the save
was swallowed and the editor still shows what it had in memory".

## The three stores

**The agent (and library) inventory.** `scenario::stock_item` mints
`asset_id` as `item_id + 0x1000` — a number, not an asset. The "Party Hat"
in the agent tree and the "Library Texture" in the library tree both point at
nothing, and their `item_type` / `inv_type` are both left `0` (texture)
whatever the item is called. The four `DEFAULT_BODY_PARTS` are the exception
and show the shape of the fix: `sl_test_assets::builtin::library_wearables`
writes real `LLWearable` bodies and the ids in the item, the worn set and the
asset store are the same ids by construction. Every other seeded item wants
the same treatment — a real body of its declared class, or no item.

**The task (prim) inventory.** `UdpAssetFixtures::task_item_assets` is a
`(task, item)` map of bytes stated up front, per session, while the live task
inventories are now the region's (`SceneFixtures::task_inventories`). An item
**dropped into a prim** is minted a *fresh* task item id, so no fixture could
have stated its bytes: the `TransferRequest` for it is refused with
`UnknownSource`, and the item a test just watched the contents serial advance
for is one whose asset cannot be read back. A script uploaded into a prim
over `UpdateScriptTask` lands in the same place. The task-item transfer
should resolve through the item's own `asset_id` against the grid-wide store,
the way every other asset fetch already does, with the stated map folded into
that store at start-up and dropped — one place an asset id means something,
rather than two that can disagree about the same item.

**Uploads.** `ServerEvent::CapsAssetUploaded` and
`ServerEvent::AssetUploaded` both carry the complete bytes, and both go past
the driver's flush unread; `GridAssets` is written once at start-up from the
region fixtures and never again. Three minting sites:

- the two-stage CAPS uploader (`SimSession::complete_caps_upload`) —
  `NewFileAgentInventory`, `UploadBakedTexture` and every
  `Update{Gesture,Notecard,Script,Settings,Material}{Agent,Task}Inventory`.
  `CapsUploadMetadata` already names the target: a new file, an agent item,
  or a task item **and its holding object** — so persisting the bytes and
  repointing the named item's `asset_id` is one edit covering both
  inventories;
- the legacy UDP transaction upload (`AssetUploadRequested` →
  `AssetUploaded`), which is how a *wearable* save reaches a grid, there
  being no cap for it. The stored id is `combine(transaction_id,
  secure_session_id)` — deterministic, so a fetch works the moment the bytes
  are kept;
- a take (`world::taken_item`, from [[test-fake-grid-object-write-path]]),
  which mints an id for the serialised object and leaves it unbacked because
  nothing serialises an object. This is the one half that waits: it needs
  [[test-assets-object-asset-codec]].

## What "round trip" means is a finding, not an assumption

The obvious acceptance — the bytes come back as they went in — is a guess,
and probably wrong for several classes. A simulator that parses an LLSD
settings asset or a GLTF material in order to validate it also re-serialises
it; a notecard carrying embedded inventory has its embeds' ids and
permissions rewritten on save; a script save produces a second asset (the
compiled bytecode) that the completion reply never names; a take *authors* an
object asset rather than echoing one. Nothing here has ever measured it — the
closest is [[test-notecard-create-update]], which re-fetches the body it just
wrote, compares the **length**, and records the result as a metric rather
than asserting it.

[[test-asset-save-mutation-survey]] is that measurement, and this task takes
its shape from it. It is deliberately not a `blocked_by`: most of the work
below is "the id resolves to an asset of the declared class at all", which
holds whether or not the grid rewrote the bytes, and only the comparison at
the end waits. Where the survey finds a mutation the fake grid should
**reproduce** it rather than echo — an echoing grid cannot fail a viewer that
trusts its own in-memory copy after a save, which is the bug the round trip
exists to catch.

## What the workspace can already write

`sl-test-assets` writes settings, sounds, rigged meshes, GLTF materials,
textures and wearables; `sl-notecard`, `sl-lsl` and `sl-anim` cover the rest
of what the editors touch. `AssetType::Object` is the one class with no codec
at all, and [[test-assets-remaining-class-audit]] is where the classes nobody
saves get their recorded "no" — this task should not invent a fixture for a
class that audit decides is vestigial.

Wanted:

- every seeded inventory item — agent, library, task — carrying a real asset
  of its declared class, with the id in the item and the id in the store the
  same id by construction, as the body parts already are;
- a driver arm folding every completed upload into `GridAssets` and
  repointing the item the metadata names, advancing the object's contents
  serial where it is a task item, because a changed asset is a changed
  listing;
- the task-item `TransferRequest` answered from the item's `asset_id`
  against that store;
- an offline conformance case per savable class, or one that walks them,
  asserting the re-fetch against whatever [[test-asset-save-mutation-survey]]
  found a real grid returns — and one that opens a seeded item of each class,
  which is the read half and is cheaper;
- a viewer-tier check that at least one editor's Save is observable this way,
  since that is the failure the round trip exists to expose.

Acceptance: every asset id the fake grid puts in an inventory item — agent,
library or task — resolves to bytes of that item's declared class; a
notecard, a script, a settings asset and a material saved over their caps,
and a wearable saved over the UDP transaction path, are each readable back
from the id the grid returned and match what a real grid returns for the same
save; the same holds for an item saved into a prim's task inventory; and
every `AssetType` a viewer can save has either a round trip or a recorded
reason it has none.

## What landed

**The fixture table.** `sl_test_assets::inventory` is one real asset body per
class the workspace can write one for — texture, sound, landmark, clothing,
body part, notecard, script, animation, gesture, mesh, settings, material —
carrying the id the item declares, the bytes it resolves to, and a **second**
body of the same class. Two bodies rather than one because a round trip that
re-fetches the id it was handed proves nothing if the bytes never changed: a
grid that swallowed the save and one that stored it answer identically. Each
body is read back through the decoder that owns its format in the crate's own
tests, which is what lets a consumer assert "of the declared class" by
comparing bytes alone.

Two classes have no body and the absence is the finding, recorded in
`inventory::unsupported_classes()` (plus seven more that are legacy or
reserved). A crate test fails if any `AssetType` has both a body and a recorded
reason, or neither, so a new variant cannot slip through undecided.

**The stock scenario** seeds one agent item per entry, filed in the system
folder its class belongs in, and gives the library item a real library body
part. That replaced a "Party Hat" and a "Library Texture" whose asset ids were
their item ids plus `0x1000` — pointing at nothing — and whose declared class
was `texture` whatever their names said.

**`sl-fake-grid/src/uploads.rs`** folds every completed save into `GridAssets`
and repoints the item that named it: the two-stage CAPS uploader (creating an
item for `NewFileAgentInventory`, repointing one for every `Update*` family,
advancing the holding object's contents serial for a task item), the legacy UDP
transaction upload, and the `UpdateInventoryItem` that binds it.

**The task-item transfer** resolves through the item's own `asset_id` against
that one store (`udp_assets::task_item_asset`); `UdpAssetFixtures
::task_item_assets` is gone. The request's own `asset_id` field is deliberately
not trusted.

**`sl-conformance`'s `asset-round-trip`** (offline) walks every seeded class
through a fetch, every savable one through a save and a re-fetch of the
returned id, and the prim's task inventory through a rez, a drop, a listing, a
UDP read and a task save.

## What it changed on the way

- **`UpdateInventoryItem` is typed** (`ServerEvent::UpdateAgentInventoryItems`,
  `UpdatedInventoryItem`), out of `RAW_FORWARDED`. It carries `bound_asset` —
  `combine(transaction_id, secure_session_id)` when the block's transaction is
  non-nil — because that derivation is the *only* thing correlating a wearable
  save's bytes with the item they belong to, and doing it once beside the
  `AssetUploadRequest` arm beats doing it in every driver.
- **A completion's `new_inventory_item` is the item it replaced**, for every
  `Update*` family, not a freshly minted id. OpenSim's `ItemUpdater` answers
  `uploadComplete.new_inventory_item = m_inventoryItemID`
  (`BunchOfCaps/UpdateItemAsset.cs:326`), and it has to: handing a client an id
  nothing holds would have it file a second copy of a notecard it only edited.
- **`SimSession::send_inventory_items_created`** echoes each item's callback id;
  the old method kept writing the "no callback" zero, which is right for a
  server-side creation nobody asked for and wrong for a reply.
- `AssetType::Object`'s doc comment now records why it has no fixture.

## What is still open

- **The object asset.** A take still mints an unbacked id, because nothing
  serialises an object — [[test-assets-object-asset-codec]].
- **Whether an echo is right.** The fake grid stores what it was given. A real
  grid very probably re-serialises a settings or material asset, rewrites a
  notecard's embedded ids, and produces a second (bytecode) asset for a script
  save that the completion never names. [[test-asset-save-mutation-survey]]
  measures it; where it finds a mutation the fake grid should **reproduce** it,
  and this case's assertion tightens from "the bytes came back" to "the bytes a
  real grid returns came back".
- **The viewer tier.** No editor's Save has been observed through this yet;
  [[viewer-task-inventory-open-and-save-back]] is the first that should be.
