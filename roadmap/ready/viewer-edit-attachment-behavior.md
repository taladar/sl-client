---
id: viewer-edit-attachment-behavior
title: Edit tools & widgets on worn attachments
topic: viewer
status: ready
origin: prim-linking follow-up (2026-07-24)
blocked_by: [viewer-object-selection-core, viewer-transform-gizmos]
---

Context: [context/viewer.md](../context/viewer.md).

Verify — and where they diverge, fix — how the existing build-mode surfaces
behave when the selection is a **worn attachment** rather than an in-world
object, so they match the reference viewer.

Today the selection core deliberately excludes worn attachments: the click
pick treats a `hit.summary.attachment` as empty world (`edit_selection.rs`),
and the rubber-band sweep skips `motion.attachment`. So the transform gizmos,
the numeric position / rotation / size fields, and the parameter tabs are
never exercised against an attachment at all. The reference viewer instead
lets you select and edit **your own** attachments in build mode:

- Move / rotate manipulate the attachment **relative to its attach point**
  (the offset the avatar carries), not a region-local pose; the numeric
  fields show that attachment-local position / rotation, and an edit sends an
  `ObjectUpdate` that re-homes the attachment offset (no
  `MultipleObjectUpdate` region move). Reference: `LLManipTranslate` /
  `LLManipRotate` attachment-frame handling, `LLSelectMgr` attachment updates.
- Scale, parameter edits (name / description / flags / shape / material /
  light), and the permission surfaces apply to the attachment's prims the
  same way as an in-world object where permitted.
- Someone **else's** attachment is not editable (only inspectable) — the pick
  must keep excluding it.

Scope: decide whether the selection core should admit own-attachment picks
(gated on wearer == self), then audit each edit surface — gizmos
([[viewer-transform-gizmos]]), the numeric fields / parameter tabs
([[viewer-object-edit-floater-shell]],
[[viewer-prim-parameter-editing]]) — against the reference's attachment
behaviour and fix the frame / message-path differences. Client-side unit
tests where the transform math is testable; live-verify the wire path on a
worn test attachment. Pairs with the attachment alignment helper
([[viewer-attachment-align]]).

Reference (Firestorm, read-only): `llmaniptranslate` / `llmaniprotate`
attachment frame, `llselectmgr` (`gAllowSelectAvatar`, attachment select /
update), `llviewerobject::isAttachment`.
