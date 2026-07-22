---
id: viewer-name-tags-complexity-distance
title: Name tags — complexity & distance lines
topic: viewer
status: blocked
origin: Vintage-parity coverage audit (2026-07-22); nametag feature survey
blocked_by: [viewer-name-tags-billboard-render, viewer-avatar-complexity-limit]
refs: [viewer-name-tags-decorations]
---

Context: [context/viewer.md](../context/viewer.md).

The Firestorm complexity / distance tag additions (the survey's `FSTag*`
family), on top of the tag renderer and the complexity computation:

- **Complexity (ARC) line** — the avatar's render cost in the tag, with
  the reference's three modes: own tag only (`FSTagShowOwnARW`), every
  avatar (`FSTagShowARW`), or only too-complex/jellied avatars
  (`FSTagShowTooComplexOnlyARW`); coloured green→red against the
  complexity limit, plus the red texture-area line when attachment
  surface area is the jelly reason.
- **Distance line** — "N.NN m" to the avatar (`FSTagShowDistance`).
- **Distance-range colouring** — tint the whole name by chat reach:
  whisper / say / shout / beyond-shout bands
  (`FSTagShowDistanceColors` + the four `NameTag*DistanceColor` tokens),
  so who-can-hear-me is visible at a glance.

The identity/status decorations live in
[[viewer-name-tags-decorations]]; this task is the two numeric lines and
the range colouring, all settings-gated.

Reference (Firestorm, read-only): `llvoavatar::idleUpdateNameTagText`
(`FSTagShow*`), `llhudnametag`.

Deps: [[viewer-name-tags-billboard-render]] (tag surface),
[[viewer-avatar-complexity-limit]] (the ARC numbers + jelly reasons).
