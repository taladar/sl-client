---
id: viewer-reattached-rigged-mesh-renders-static-mislocated
title: Re-attached rigged mesh renders static and mislocated (fine at login)
topic: viewer
status: bugs
origin: user report (2026-07-31, own avatar on aditi)
refs: [viewer-p17-2, viewer-p16-1, viewer-p16-2]
---

Context: [context/viewer.md](../context/viewer.md).

## Symptom

Rigged mesh clothing attachments that rendered **correctly at login** (rigged to
the skeleton, in place) come back **static (unrigged) and in the wrong
location** after being **detached and re-added at runtime** ("add to current
outfit"). The login-time attach path works; the runtime re-attach path does not.

## Where to look

The rigged-attachment build is deferred: a decoded rigged mesh is held as
`RiggedAttachmentPending` (`objects.rs:477-500`) until
`apply_rigged_attachments` can resolve the avatar's joint entities, then built
as a `SkinnedMesh`
(`bevy::mesh::skinning`). At login the joints already exist when the attachment
decodes, so the skinned build succeeds and the attachment-point transform seats
it. On a runtime **re-attach**:

- Is the new `ObjectUpdate` still recognised as a **rigged attachment** (does it
  carry / do we still read the `attachment_point` and the skin/joint data), and
  does it re-enter the `RiggedAttachmentPending` → `apply_rigged_attachments`
  path — or is it spawned as a plain **static** object (no `SkinnedMesh`)?
- The **wrong location** points at the attachment-point parenting / transform
  ([[viewer-p16-1]] detect-parent, [[viewer-p16-2]] attachment-transform) not
  being applied, so it renders at its raw region/local position instead of
  parented to the attachment point / joint.
- Whether a prior **detach** left stale state (a half-torn-down entity, a
  consumed `RiggedAttachmentPending`, or a cached "already built" marker) that
  makes the re-attach skip the rigged build. Detach teardown is at
  `objects.rs:~3706`.

## Verify

Live on aditi: detach a rigged mesh attachment, then re-add it (add to current
outfit), and confirm it rebuilds as a `SkinnedMesh` and seats at its attachment
point — matching the login result.
