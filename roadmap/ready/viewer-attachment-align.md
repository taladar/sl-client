---
id: viewer-attachment-align
title: Attachment alignment tool (avatar align)
topic: viewer
status: ready
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-object-edit-floater-shell]
---

Context: [context/viewer.md](../context/viewer.md).

Firestorm's "Avatar Align" helper: while editing a worn attachment, a
compact floater with nudge buttons (position, rotation, scale in small
steps along each axis) — far easier than gizmo-dragging something attached
to your own head. It edits the selected attachment through the same
object-update path the edit floater uses; the mini variant docks beside
the build tools.

Reference (Firestorm, read-only): `floater_avatar_align.xml`,
`floater_avatar_align_mini.xml`.

Builds on: attachment transforms (P16) and the object edit path.

Deps: [[viewer-object-edit-floater-shell]] (an attachment selection to
act on).

## Parity-audit addendum (2026-08-19)

CORRECTION: this task's Firestorm reference points at
`floater_avatar_align.xml`, but that file is the own-avatar *rotation*
nudge floater (±1/±10/±45/±90°, face-nearest-avatar) — i.e. the
reference for [[viewer-avatar-alignment-tools]], not for attachment
alignment. Fix the reference to the actual worn-attachment editing
paths instead (attachment nudge editing while worn; see also
viewer-edit-attachment-behavior).
