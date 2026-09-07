---
id: viewer-rlv-temp-attachment-gate
title: RLV — the temporary-attachment half of the owner-say gate
topic: viewer
status: ready
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

The reference's `LLViewerObject::isTempAttachment()` is "attached, and my
`mAttachmentItemID` is null" — a temp attachment is rezzed by a script rather
than worn from inventory, so the simulator sends it with **no `AttachItemID`
name-value**. `sl-proto` already parses the name-value pairs
(`ObjectData::name_value_data("AttachItemID")`); the viewer's `TrackedObject`
simply does not keep the value.

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
