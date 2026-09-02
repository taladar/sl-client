---
id: test-fake-grid-red-patch-on-own-avatar-chest
title: A red shard sits on the own avatar's chest in every fake-grid capture
topic: test
status: bugs
origin: Firestorm capture run (2026-09-02)
points: 2
refs: [test-fake-grid-self-avatar-baked-textures-rejected]
---

Context: [context/testing.md](../context/testing.md).

Once the own avatar de-clouded and stood up
([[test-fake-grid-self-avatar-baked-textures-rejected]]), every Firestorm
capture of it shows a small **dark-red, sharp-edged shard** at chest /
shoulder height, clipping through the body. It is in the same place in
every frame of every run, and it is the only thing on the avatar that is
not the green the fake grid bakes it.

What it is not, each checked against the same run's scene dump:

- **Not another object.** The only object within 4 m of the avatar is the
  region's water plane (256×256 at z = 20).
- **Not a fixture texture bleeding in.** No catalogue prim is nearer than
  8 m, and the catalogue NPC and its checker-box attachment are 24 m away.
- **Not a wearable-layer stand-in.** None of `wearable_layer_rgba`'s
  colours is red — the skin tones are `222,184,155` / `214,176,148`, hair
  is `78,54,38`, iris `96,118,132`.
- **Not an unbaked slot showing the sentinel.** The `IMG_DEFAULT_AVATAR`
  stand-in is mid-grey.

So it is part of the avatar's own geometry, drawn in a colour nothing in
the fixture set supplies — which makes an unwritten buffer or an
inside-out face the first two things to look at. Note the reference
viewer draws the *system* avatar mesh here, so this is as likely to be
something the grid tells it (a texture entry face this workspace does not
mean to set, a stray visual param) as anything about the bake.

Reproduce with `scripts/fake-grid.sh --port 9100 --scenario catalogue`
and the Firestorm capture harness aimed at the arrival point; the shard
is visible in a 5× crop of the avatar in any frame.
