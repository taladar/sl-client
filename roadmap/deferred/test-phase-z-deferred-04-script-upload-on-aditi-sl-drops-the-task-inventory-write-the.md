---
id: test-phase-z-deferred-04
title: script-upload on aditi — SL drops the task-inventory write.** The scri
topic: test
status: deferred
origin: TEST_ROADMAP.md — Phase Z — Deferred: multi-avatar Aditi work
---

Context: [context/test.md](../context/test.md).

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

---
