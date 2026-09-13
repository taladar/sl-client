---
id: viewer-audit-asset-editor-scaffold
title: The wearable editor reports a save that has not happened, and claims other editors' results
topic: viewer
status: done
origin: static code audit (2026-08-26)
points: 5
---

Context: [context/viewer.md](../context/viewer.md).

Three editors, three copies of one window, and the copy that had drifted
furthest carried the bugs.

**A Save As announced a copy that did not exist yet.** The appearance editor
wrote "Saved a copy to inventory." in the same breath as queueing
`Command::UploadAsset` — before the bytes had left, let alone been kept. A
refused upload left that sentence on screen as the only thing the resident was
ever told. The copy is minted by the CAPS uploader, whose reply carries no
correlation id at all, so there was nothing to report *from*: the viewer's one
ordered creation queue (`PendingItemCreations`) knew the reply had landed and
told nobody. It now hands out a `ItemCreationTicket` on `enqueue` and publishes
an `ItemCreationFinished` on every reply it consumes, success or failure — the
generic-upload sibling of the settings editors' `SettingsItemCreated`. The
editor holds its ticket and says "Saving a copy…", then "Saved a copy to
inventory." or "Saving a copy failed." A ticket rather than a count because the
queue is shared: the inventory's New Clothes creator puts wearable creations
through it too, and two creations of one slot are otherwise indistinguishable.

**An in-place Save claimed whichever save finished first.** The reporting system
matched any `InventoryAssetSaved` arriving while it was saving, because the
event carried nothing to match *on*: `AssetUploadComplete` names the stored
asset (`combine(transaction_id, secure_session_id)`) and not the item, and the
session threw away the transaction that predicted it. So an earlier save of its
own that had timed out, or any other legacy save, decided what the resident was
told about theirs. `Session::save_inventory_asset` now registers each save under
the id its completion will name, and `Event::InventoryAssetSaved` carries the
`transaction_id` back — the client-side correlation token, and the only one on
this path. The editor mints one per Save and reports only its own.

That registry closes a second hole for free: an **inlined** save (the common
case — anything under the single-packet limit) has no `Xfer` offer to expire, so
a simulator that simply never answered left the editor on "Saving…" for the rest
of the session. Registrations now expire after `INVENTORY_SAVE_TIMEOUT`,
surfacing the failed save. Deliberately longer than `XFER_OFFER_TIMEOUT`, so an
oversized save that was never pulled is still reported by its offer expiring,
which is the more specific of the two answers.

**Closing a notecard or a script threw away what you had typed, silently.**
Both windows are `closable`, and a keyed floater is *despawned* by the close
pass — so by the time anything could have asked, the text was gone. The floater
manager now takes a `ConfirmBeforeClose` component: while it is armed a
`FloaterOp::Close` becomes a `FloaterCloseRequested` and the window is left
untouched, which is the reference's `LLFloater::canClose` drawn at the same
place. The editors' shared scaffold arms it from an `UnsavedWork` component and
answers the request with the reference's own `SaveChanges` prompt — Save / Don't
Save / Cancel. **Save** holds the close until the save actually lands, so a
refused one leaves the window up with the failure on screen instead of closing
over work that was never stored.

"Don't Save" is spelled as a deliberate discard rather than as "the work was
saved after all": the buffer has not changed, the tracker measures it as unsaved
again on the very next frame, and the guard that re-armed would turn the
resident's own answer into the same question again, for ever.

The appearance editor's window is guarded too — its edit previews on the avatar
and is stored nowhere until a Save — and closing it now *ends* the edit:
`set_preview_asset` substitutes the edit into the worn outfit, so a window that
went away leaving its preview behind kept the avatar wearing an unsaved edit
with nothing left to save, revert or even see it in, and the next open read that
preview back as though it were what is worn.

Dirtiness is measured, not flagged. A text editor compares its buffer against
the text it loaded or last saved, so typing a character and taking it back
leaves nothing to ask about; and what a save makes the new baseline is the text
that *went out*, held on the window, not whatever is in the field when the reply
arrives — a resident who kept typing during the upload has not saved that.

The duplication that produced all of this is gone. `asset_editor` now owns the
chrome the windows are made of — the status line, the read-only note, the body
field, the Save button, the tear-down, and the colours they were each declaring
privately — and the save channel: a Save button writes `SaveEditorWindow` for
the floater it sits in, and so does the confirmation's "Save", so the two cannot
come to save different things. Each editor module is left with what is genuinely
its own: how its asset decodes, what its body looks like, and which capability
its save goes out over.

What is deliberately *not* shared is a generic `AssetEditor<T>` over the window
state machines. The two text editors' remaining clones are their `open_*` and
`ingest_*` passes, and a type parameter over those would have to carry a
`K::Extra` for everything they do not share — the notecard's decode baseline and
view toggle against the script's compile target, diagnostics list, Running
toggle and run-state query — which reads worse than the two passes do and buys
nothing this bug came out of. The parts that *did* diverge (the chrome, the save
channel, the in-flight correlation, the unsaved-work guard) have one
implementation each, and there is no third copy of any of them left.
