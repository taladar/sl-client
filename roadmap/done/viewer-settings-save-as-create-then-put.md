---
id: viewer-settings-save-as-create-then-put
title: A settings Save As uploads through a cap that does not take settings
topic: viewer
status: done
origin: Live aditi / OpenSim run of the My Environments window (2026-09-09)
refs: [viewer-environment-fixed-editor, viewer-environment-my-environments]
---

Context: [context/viewer.md](../context/viewer.md).

The settings editors' **Save As** mints its copy with
`Command::UploadAsset` — `NewFileAgentInventory` — carrying
`asset_type: "settings"`. **No grid takes that**, and both fail silently:

- OpenSim's `UploadCompleteHandler`
  (`OpenSim/Region/ClientStack/Linden/Caps/BunchOfCaps/BunchOfCaps.cs`) declares
  `sbyte assType = 0; sbyte inType = 0;` and matches the type string against
  *sound, snapshot, animation, animset, wearable, object* only. Nothing sets
  them for `settings`, so the item is created and filed as a **Texture** —
  invisible to every settings surface, since they all filter on
  `AssetType::Settings`.
- Second Life creates nothing at all.

Neither reports an error, which is why this survived: the viewer's own
`AssetUploaded` handling sees a plausible reply.

The **creation** half of this was fixed with
[[viewer-environment-my-environments]]: `new_settings_item` now sends
`CreateInventoryItem` and lets the simulator author the default asset and stamp
the subtype byte, which is what `LLSettingsVOBase::createNewInventoryItem` does.
Save As is the other half, and it needs the *second* branch of the reference's
flow, because it has a body to store rather than a default to accept:

1. `create_inventory_settings` — the same `CreateInventoryItem` (nil transaction
   id, `AT_SETTINGS` / `IT_SETTINGS`, subtype byte in the wearable-type field);
2. then, once `UpdateCreateInventoryItem` names the fresh item,
   `LLSettingsVOBase::updateInventoryItem` PUTs the encoded frame to
   `UpdateSettingsAgentInventory` — which is `Command::UpdateInventoryAsset`
   with `AssetUpdateLocation::AgentInventory`, exactly what an **in-place** Save
   already uses.

So the pieces all exist; what is missing is the correlation between step 1's
reply and the body waiting to be written. An in-place Save needs none of it (the
reply names the item it wrote), and `PendingItemCreations` is the wrong queue
here — it exists to stamp flags after an *upload* mints an item, and this path
does not upload and does not need a stamp.

Reference (Firestorm, read-only): `llsettingsvo.cpp`
(`createNewInventoryItem`, `createInventoryItem`, `onInventoryItemCreated`,
`updateInventoryItem`), `llviewerinventory.cpp` (`create_inventory_settings`).

## Done

`new_settings_item` is now the whole creation for both halves, and the body is
carried across the gap by **one** queue for the viewer,
`PendingSettingsCreations`, with its consumer in the inventory beside the upload
kind's. Three surfaces mint settings items — the inventory's create menu, the My
Environments add row, the editors' Save As — into one untagged reply stream, so
two queues would each pop on the other's creation and a Save As would write its
frame onto somebody else's fresh item. That single ordered queue is also what
makes each *consumer's* own count sound: "the next one published is mine" is
true only because there is exactly one queue behind it. A test pins the ordering
by interleaving a bodyless creation with an authored one.

The reference's permission widening came with it: `onInventoryItemCreated`
forces the everyone mask to `PERM_COPY` on every settings item it makes, which
nothing here did.

## What running it found

**A save's completion did not name the item it saved.** The editor reported
"Saving…" forever on a save that had in fact succeeded, and so never cleared its
unsaved-changes flag. Not a UI bug: an update capability's completion is only
obliged to carry the new *asset* — the item already exists and the client is the
one that named it — so a grid may echo it, may send a nil (which parses as
`None`, exactly as `UploadBakedTexture`'s genuinely item-less completion does),
or may omit it. Our own sim server echoes it, which is why nothing caught it;
aditi does not. The runtime now fills in the item an agent-inventory update was
*about* when the grid does not, which is simply true and works whatever the grid
echoes. `rebind_saved_asset` matches on the same field, so a saved notecard or
script would have re-read its pre-save asset on such a grid too.

**Save As kept editing the original.** Deliberate, and wrong: the reference's
`onInventoryCreated` clears the dirty flag and `loadInventoryItem`s the copy. If
the window stays on the original, everything just done lives in the copy while
the next plain Save writes to the original. It follows the copy now — re-pointed
rather than re-fetched, since the bytes are the ones just uploaded. A landed
save also re-baselines what Revert goes back to.

## Also fixed while verifying, and why they belong here

- **Opening a second item discarded unsaved work silently.** These windows are
  singletons because the frame is *previewed*, and two previews of one track
  cannot both be what the user is standing under — so an open replaces. The
  reference guards that with `checkAndConfirmSettingsLoss`; the
  `SettingsConfirmLoss` template was already in the catalogue. A test pins the
  template name and the button name the confirm arm routes on, since both are
  plain strings on either side and a rename would silently restore the bug.
- **The editors reopened themselves at every login**, restoring chrome and knobs
  over no session at all. `FloaterPersistExempt` was the only opt-out and drops
  geometry too, which is worth keeping — a window is arranged once. Hence
  `FloaterOpenExempt`, the narrower sibling: the visible key is neither
  declared, restored nor written, and everything else persists. Applied to the
  two editors, the settings picker and Quick Preferences.

## Not done

- **Closing an editor still discards silently.** The reference confirms that too
  (`checkAndConfirmSettingsLoss([this](){ closeFloater(); … })`), but refusing a
  close needs the floater chrome to support vetoing one, which it does not.

  **Since done** (2026-09-12, with
  [[viewer-audit-picker-requester-identity]]): the chrome grew
  `FloaterCloseGuard` / `FloaterCloseRequested` / `FloaterOp::CloseNow`, and
  both settings editors arm the guard from `session.modified` and raise the
  same `SettingsConfirmLoss` on a held-back close.

## Verified

Live on aditi: two in-place saves reported landing, each naming its own item,
and the confirmation stayed quiet after them; a Save As produced a copy carrying
the edit and the window followed it.

`cargo clippy` clean on every touched crate;
`cargo test --release -p sl-viewer-ui-widgets --lib` 205 green including the new
open-exemption round trip, `-p sl-viewer-environment --lib` 38, and
`-p sl-viewer-inventory --lib` 81 including the queue-ordering test.

## Left for later

`onInventoryCreated` also copies the *source* item's permissions onto the copy a
Save As makes, which is a different act from the `PERM_COPY` widening above and
is not done here.
