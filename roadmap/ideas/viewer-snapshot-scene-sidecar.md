---
id: viewer-snapshot-scene-sidecar
title: Say what a snapshot is a picture of
topic: viewer
status: ideas
origin: spun out of the UI-scene discussion (2026-09-21)
points: 5
refs: [viewer-snapshot-floater, test-firestorm-fake-grid-crosscheck,
       viewer-name-tags-complexity-distance]
---

Context: [context/viewer.md](../context/viewer.md).

We already build a structured description of what the viewer was showing when
it took a frame — `scene_dump.rs` writes `scene.json` beside every cross-check
capture, and the patched Firestorm writes the same document. It exists for
machine comparison, but the *information* in it is the thing a person often
wants about their own photographs: where was this, when, and who is in it.

So: an option on the snapshot floater ([[viewer-snapshot-floater]]) to write a
description beside a saved snapshot. Off by default, and **narrower than the
harness dump** — that one is exhaustive by design (per-face textures, LODs,
materials) and nobody wants a megabyte of it next to a photo.

A useful default is small:

- **Where** — region name, region coordinates, parcel name, and the camera's
  position and aim;
- **When** — the local timestamp and the region's time of day / environment,
  since "what did the sky look like" is most of why a shot looks how it does;
- **Who** — the avatars in frame.

## Why anyone would want it

Photographers and machinima people credit the residents in a shot and
routinely cannot remember who was there. Bloggers want the place name and a
SLURL that goes back to the exact camera position, not the parcel centre.
Anyone who has taken a few thousand snapshots has a directory where the only
way to find one is to open them all.

## The part that needs deciding, not just building

**"Who is in frame" is other people's data.** Names are visible in world, so
this records nothing a bystander could not read off the screen — but a
machine-readable list beside an image is different in kind from a name tag
that was on screen for a moment: it is durable, greppable, and travels with a
file that may be published. That is a real difference, and it argues for:

- the avatar section **off unless switched on**, separately from the rest;
- display names recorded as *shown*, with the choice of whether to include
  agent ids treated as its own decision (an id is a stable identifier across
  every photograph, a display name is not);
- deriving "in frame" honestly — it means *drawn in this image*, not "within
  draw distance", or the file claims a presence the picture does not show.

Worth checking what the reference viewer and the grid's terms say about
publishing resident lists before this ships, rather than after.

## Shape

Two options, not exclusive:

- a **sidecar** `shot_001.json` beside `shot_001.png` — easy to write, easy to
  parse, trivially lost when the image is moved or uploaded;
- **embedded metadata** in the image itself (PNG `tEXt`/`iTXt`, or EXIF for
  JPEG) — survives being moved, and is what an image cataloguer would read.
  Also what makes it travel to places the user did not necessarily intend,
  which is the privacy point again.

Let the same code build both from one structure, and reuse
`scene_dump.rs`'s serialisation rather than growing a second description of
the world that can disagree with the first.

## Done when

A snapshot saved to disk can carry a description of where, when and — if
asked — who, in a form a script can read; the defaults record nothing about
other residents; and the description is generated from the same place the
harness dump is.
