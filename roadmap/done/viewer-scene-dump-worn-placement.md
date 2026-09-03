---
id: viewer-scene-dump-worn-placement
title: A scene dump places an avatar and its attachments as the reference does
topic: viewer
status: done
origin: the first two-viewer scene-dump pair (2026-09-02)
points: 3
refs: [viewer-scene-dump, test-firestorm-crosscheck-report]
---

Context: [context/testing.md](../context/testing.md).

The first pair of scene dumps disagreed about one object in the catalogue
scene: the fixture NPC's skull box, at **27.06 m** here and **26.20 m**
there, with a rotation against an identity one. [[viewer-scene-dump]]
recorded it as a dump-semantics difference to settle before anybody read it
as a placement bug, and this is that settlement — which turned out to be
two differences of one family, only one of which the first reading saw.

**The attachment.** The reference's document reports
`LLViewerObject::getPositionRegion()`, which composes a child against its
parent **object** — for an attachment, the avatar:

```text
mPositionRegion = parent->getPositionRegion() + getPosition() * parent->getRotation()
```

while the thing is *drawn* parented to a skeleton joint, which
`LLViewerJointAttachment::setupDrawable` does by re-parenting the drawable's
own transform onto the joint. So the reference reports the wearer's
position plus the wire offset, and the drawn place is a joint's height
further up. The fixture is that arithmetic in the open: the NPC stands at
`z = 25.95` and wears the box a quarter metre above its skull point, so the
reference says 26.20 and the skull joint puts it at 27.06 — two answers to
two different questions, with the skull's 0.86 m above the avatar's own
position between them.

**The avatar under it**, which the first reading missed because the
attachment's own difference hid it. Composing the reference's way onto our
*drawn* avatar root still left 0.94 m: an avatar's wire position is the
centre of its physics capsule, and both viewers lower the skeleton from
there so the feet meet the ground (`body_root_transform`'s `root_drop`
here, `LLVOAvatar::updateCharacter`'s `root_pos.z -= …` there). The
reference's dump reports the **object's** position, undropped. Ours
reported the drawn body root — so every avatar in every scene differed by
that drop, and everything they wore with them, and no one had noticed
because nothing had looked at the `avatars` section closely.

Both are now placed the reference's way, in `avatar_placement` /
`worn_placement` / `compose_link`
(`sl-viewer-world-view/src/scene_dump.rs`): an avatar reports the position
the simulator sent for its object (composed onto its seat when it sits),
and a worn object walks its parent chain up to that avatar and composes
each link's wire pose onto it. Everything not worn is still read back from
what was drawn, which is the whole point of the document and is not given
up for a tidy rule.

Three things worth keeping:

- **The reference's quirk is reproduced deliberately.** A linked child of
  an attachment is composed against its parent's *local* rotation, so the
  wearer's turn is applied once rather than twice — that is what
  `getPositionRegion` does, and a comparison that is right about a
  grandchild while disagreeing with the document it is diffing has bought
  nothing. A test pins it, and says why in its own name.
- **HUD attachments come out right as a side effect.** One is drawn in
  screen space, where a region position means nothing at all; ours used to
  emit that screen-space pose as a region position. Composed the
  reference's way it now means what the reference means by it.
- **The drawn pose is not lost.** A worn object and an avatar each carry
  `drawn_position` / `drawn_rotation` beside the reported pair — keys this
  viewer emits and the reference does not, like `day_position`. An
  attachment on the wrong joint, or one never parented to the skeleton at
  all (which looks like raw wire coordinates in a region field), is still a
  difference a reader can see, on the side of the pair that can see it.
  [[test-firestorm-crosscheck-report]] must expect them absent on the
  reference's side rather than rank them as a divergence.

Verified against Firestorm on the fake grid's catalogue scene (2026-09-03):
the two dumps agree on both avatars and on the worn box, to millimetres,
where they were 0.94 m and 0.86 m apart; and the frames of the same run
show the box on the NPC's skull in both viewers, which is the measurement
that says the drawn placement was never the thing that differed.
