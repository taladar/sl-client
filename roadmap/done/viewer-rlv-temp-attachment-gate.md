---
id: viewer-rlv-temp-attachment-gate
title: RLV — the temporary-attachment half of the owner-say gate
topic: viewer
status: done
origin: deferred from viewer-rlv-command-intake (2026-09-07) — the one clause of
  the reference's admission test this viewer could not evaluate
refs: [viewer-rlv-command-intake, viewer-rlva-floaters-toggles]
---

Context: [context/viewer.md](../context/viewer.md).

The owner-say gate ([[viewer-rlv-command-intake]]) implements all of the
reference's admission test except its trailing clause
(`llviewermessage.cpp:3142`):

```text
… ( no chatter object || !isAttachment || !isTempAttachment
    || RLVaEnableTemporaryAttachments )
```

It is an **or**-chain, so it excludes exactly one case: a *temporary*
attachment speaking while `RLVaEnableTemporaryAttachments` is off. This viewer
cannot tell that case apart, so it currently lets it through — which matches the
reference's own **default** (the setting is on), but diverges for a user who
turns it off, and that user turned it off precisely to get this.

## What is missing, and it is small

The reference's `LLViewerObject::isTempAttachment()` reads the attachment's
`AttachItemID` name-value; `sl-proto` already parses the pairs
(`Object::name_value_data("AttachItemID")`), and the viewer's `TrackedObject`
simply does not keep the value.

**Correction, made while implementing (2026-09-07).** This task originally said
the test was "attached, and my `mAttachmentItemID` is **null**". It is not —
`llviewerobject.cpp:7616` reads

```cpp
return (mID.notNull() && (mID == mAttachmentItemID));
```

the object's **own id equalling** its item id. A simulator with no inventory
item to name sends the object's id instead of nothing at all (OpenSim's
`LLClientView`: `FromItemID`, `if (fromID.IsZero()) fromID = part.UUID`), so
the null reading would have called every temp attachment ordinary — and, worse,
would have called an ordinary attachment whose update happened to carry no
name-values *temporary*. Implemented as the equality.

Scope:

- carry the attachment's item id onto `TrackedObject` (`Option<ObjectKey>` /
  `Option<Uuid>`, `None` for a temp attachment or a non-attachment), filled on
  the object-update path beside `attachment_point`;
- a predicate next to `sl_viewer_world_api::rlv::object_attachment` answering
  "is the object with this key a temporary attachment";
- fold it into `swallows_owner_say`, which is the one predicate the intake, the
  chat overlay, the Nearby transcript and the chat log all ask — so the gate and
  the swallow stay the same test, as they must (a line the gate refuses is one
  the reference *shows*, so refusing without also un-swallowing would eat it).

Watch the ordering: the whole point of the or-chain is that a *non*-temporary
attachment and a rezzed in-world prim are never excluded, whatever the setting
says. Getting that inverted would break every ordinary collar.

The same `isTempAttachment` predicate is what [[viewer-rlv-blocked-objects]]
needs, which is why that one is blocked on this.

Reference (Firestorm, read-only): `llviewermessage.cpp` (~L3142),
`llviewerobject.cpp` (`isTempAttachment`), `rlvcommon.cpp`
(`RlvSettings::getEnableTemporaryAttachments`).

## Done

The gate is now the whole of the reference's admission test, and the clause it
was missing is decided from the wire.

**The fact lives in `sl-proto`, not in the viewer.**
`Object::attachment_item_id` parses the `AttachItemID` name-value and
`Object::is_temp_attachment` is the
reference's equality — so the rule is stated once, in the pure crate, where the
runtimes and the fake grid can all reach it. `Session::is_temp_attachment` asks
it of the session's own object cache, which is what lets the **chat log** — two
tiers below any viewer mirror — take part in the gate at all.

**The viewer keeps the id, not a derived flag.** `TrackedObject.attachment_item`
is filled beside `attachment_point` on the object-update path, and
`sl_viewer_world_api::rlv::is_temp_attachment` is the predicate over it. Keeping
the raw id rather than a boolean is what [[viewer-rlv-blocked-objects]] and the
folder locks will need, and it costs nothing.

**It follows the attachment, not the update.** An update that carries no
name-values at all keeps the last item id named (a compressed update may omit
the block, and losing the id would silently un-refuse the speaker); an object
that stops being an attachment loses it, which is where the reference nulls it
too. Both rules are pinned by a test that runs the real ingest.

**One test, four surfaces.** `swallows_owner_say` grew the speaker and the
object mirror, and its three callers — the intake, the chat overlay, the Nearby
transcript — pass them. The fourth surface is the on-disk transcript, which
cannot see a viewer mirror: `ChatLogConfig` gained `obey_temp_attachments`
(pushed beside `swallow_rlv_commands` from the RLVa switches) and
`swallows_rlv_command` gained the speaker's temp-attachment verdict, which
`observe_event` resolves from the session cache. So the refusal and the swallow
stay the same test on every surface, which they must be: a line the gate refuses
is one the reference *shows*, and refusing without un-swallowing would eat it.

The push system that carried a flip of `RestrainedLove` to the log now carries
`RLVaEnableTemporaryAttachments` with it, since both live on the RLVa menu and
both change which lines the engine takes.

## Not done — and why

- **The reference does not walk the linkset here, and neither does this.** A
  child prim of a temporary attachment carries neither an attachment point nor
  an `AttachItemID` of its own, so a script speaking from one is admitted —
  there as here. `object_attachment` *does* walk (a bare `@detach=n` locks the
  attachment, not the prim); these two are deliberately different, and the
  divergence is the reference's.
- **`RlvAttachmentLocks::isLockedAttachment`'s use of the same predicate** — the
  clause that exempts a temp attachment from a *folder* lock — is not wired,
  because there are no folder locks: they need the `#RLV` folder tree
  ([[viewer-inventory-folder-tree]]). The four lock registries that do exist
  never consulted it.

## Verified

`cargo clippy --release --all-targets --workspace` clean. Two Bevy systems
crossed the seven-parameter line by gaining the object mirror and carry an
`expect` saying what each parameter is.

Unit tiers, each pinning one link of the chain:

- **sl-proto** — the wire rule: an attachment naming an inventory item is
  ordinary, one naming *itself* is temporary, one naming nothing is ordinary
  (the reference's null item id fails its own equality), and an in-world prim is
  neither. Plus the chat-log config's two cases: a refused temp attachment's
  line is logged rather than eaten, and a worn collar's is unaffected — the flag
  must not become a second master switch.
- **sl-viewer-world-objects** — the ingest, run for real through `apply_object`:
  the item id arrives, survives an update that carries no name-values, and dies
  with the attachment. And the gate's or-chain over a mirror holding both kinds:
  with the flag off exactly one of three speakers drops out, and with it on
  none do.
- **sl-viewer-rlv, fake-grid tier** — the whole thing over real UDP. The grid
  attaches a prim to the agent with its `AttachItemID` naming itself; the viewer
  obeys it under the roster default, is told to stop, refuses the next line
  *and* stops swallowing it, and — in that same refusing state — still obeys an
  ordinary speaker. Mutation-checked both ways: with the clause removed the
  refusal assertion fails, and with the clause widened to refuse every object
  the ordinary speaker's assertion times out. The tier mirrors objects into
  `ObjectState` with a test system rather than the render-owning production
  ingest, using the same `sl-proto` accessors so it stays a mirror and not a
  second implementation of the rule under test.

Not verified live: no script-attached object has spoken `@` to this viewer on a
real grid.
