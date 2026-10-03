---
id: test-phase-z-deferred-04
title: script-upload on aditi — SL drops the task-inventory write.** The scri
topic: test
status: done
origin: TEST_ROADMAP.md — Phase Z — Deferred: multi-avatar Aditi work
---

Context: [context/test.md](../context/test.md).

## Done (2026-10-03) — Second Life never dropped the write

`script-upload` and `script-running` now pass on aditi, complete. Second Life
was never dropping our `RezScript`. Four separate faults were in the way,
each hiding the next:

1. **The harness scripted a stranger's prim.** Every object case took the
   first unseen `ObjectAdded` after its rez as its own. On a busy sandbox that
   is usually somebody else's (often temporary) prim, so the `RezScript`, the
   `ObjectSelect` and the cleanup derez all went to an object we did not own,
   which the grid drops silently — while our own cube sat until auto-return
   (the "objects persisted and were auto-returned" clue). The eight copies of
   the waiter are one shared `support::wait_for_own_new_object`, which takes a
   root prim we own, read from the per-viewer `OBJECT_YOU_OWNER` flag: a plain
   prim's owner id arrives **nil** on Second Life (it is sent only with sound
   or particles). A refused rez now reports the grid's alert text.
2. **No place to build.** The avatar's last location was no-build land, a
   telehub redirects logins and teleports, and Second Life ignores a rez about
   70 m from the avatar without a word. A `build_location` fixture names a
   spot (aditi: the Mauve public sandbox); the rezzing cases
   (`GridTest::rezzes_objects`) log in there, fly the avatar to the spot when
   the login landed elsewhere (Second Life ignores the `autopilot` generic
   message), point the **camera** at it — objects stream by camera position,
   and the cubes were landing all along, out of view — and rez there.
3. **The contents listing's `Xfer` named remote path `0`.** Second Life does
   not answer that; the reference names `LL_PATH_CACHE` (4) — now ours too.
4. **The listing parser required `group_owned`.** The reference writes the
   line only when it is true; absent now reads as false.

Second Life's Mono compile error format, measured:
`(4, 20) : ERROR : Syntax error`. Follow-ups filed:
[[protocol-request-task-inventory-cap]] (current viewers fetch contents over
HTTP), [[protocol-xfer-listing-parse-error-ends-session]],
[[test-conformance-object-edit-click-action-opensim]].

**`script-upload` on aditi — SL drops the task-inventory write.** The
`script-upload` case is green on OpenSim but gated to OpenSim only: on SL the
task-inventory *write* never lands (the object's contents serial stays `0` after
both [`RezScript`](Command::RezScript) and an
[`UpdateTaskInventory`](Command::UpdateTaskInventory) drop), while **rez,
agent-inventory create, and reads (`RequestTaskInventory`) all succeed on the
same authenticated session**, the avatar owns the object, and the wire encoding
matches the viewer byte-for-byte. Ruled out live on aditi: login/MFA (auth
confirmed — objects persisted and were auto-returned 15 min later), land
permission (you may edit your own objects wherever you can rez them — and the
Firestorm viewer
**successfully creates a script in an object at the same spot**), the item
checksum (ported faithfully from `LLInventoryItem::getCRC32`, with the object as
parent — `RestoreItem::for_task_drop`/`new_script`), and object selection (a
fired `ObjectSelect` did not help; it also never returned `ObjectProperties` on
SL). Since the viewer works on the same parcel, it is a client-message
difference. **Next step: packet/message capture** of the Firestorm viewer doing
object-Contents "New Script" (rez → New Script) — grab the outgoing `RezScript`
(and any preceding `ObjectSelect`) and diff the field values against ours.
Leading suspects: a required preceding selection message, or an item-block field
value. When found, flip `script-upload` back to `[both]` and run the aditi
SL-Mono error-format validation (the parser + all the upload code already exist
and are unit-tested). **`script-running` rides the same blocker:** it plants its
toggleable script with the same `RezScript` task-write, so it is gated
OpenSim-only too — the same viewer capture that unblocks `script-upload` flips
both back to `[both]` (its `GetScriptRunning`/`SetScriptRunning`/ `ScriptReset`
surface, including the CAPS `ScriptRunningReply` decode, is already
grid-agnostic).

**Ruled out 2026-09-27: the transaction id.** Firestorm's object-Contents
"New Script" (`llpanelcontents.cpp` → `LLViewerObject::saveScript`) sends a
**null** `TransactionID` in the inventory block, where `new_script` is given
`Uuid::new_v4()`. A live aditi run of `script-upload` with `Uuid::nil()`
failed exactly as before (the script never appeared). Remaining differences
from the viewer's item, read from source, not yet tried: `next_owner` is
`PERM_MOVE | NextOwnerPerms("Scripts")` there against `ALL` here;
`CreationDate` is `time_corrected()` there against `0` here (it enters the
CRC); the description is the generated `LLViewerAssetType` default there
against empty here; and the `AgentData.GroupID` is the agent's active group
there against nil here. A wire capture is not available, so the way forward is
source reading, or outgoing-message logging built into the test-harness
Firestorm branch.

---
