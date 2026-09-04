---
id: test-fake-grid-asset-round-trip
title: An asset id the grid hands out should name bytes the grid can serve
topic: test
status: ready
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
