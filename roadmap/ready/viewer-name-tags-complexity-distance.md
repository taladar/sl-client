---
id: viewer-name-tags-complexity-distance
title: Name tags — complexity (ARC) lines
topic: viewer
status: ready
origin: Vintage-parity coverage audit (2026-07-22); nametag feature survey
blocked_by: [viewer-avatar-complexity-limit]
refs: [viewer-name-tags-decorations, viewer-name-tags-billboard-render]
---

Context: [context/viewer.md](../context/viewer.md).

The Firestorm complexity tag additions (the survey's `FSTag*` family), on
top of the tag renderer and the complexity computation:

- **Complexity (ARC) line** — the avatar's render cost in the tag, with
  the reference's three modes: own tag only (`FSTagShowOwnARW`), every
  avatar (`FSTagShowARW`), or only too-complex/jellied avatars
  (`FSTagShowTooComplexOnlyARW`); coloured green→red against the
  complexity limit, plus the red texture-area line when attachment
  surface area is the jelly reason.

The **distance line and range colouring** originally scoped here shipped
with [[viewer-name-tags-billboard-render]] (2026-08-05): "N.NN m" measured
from the **own avatar** (the camera-based distances govern only the
fade/cut-off — a deliberate split), tinted by the whisper / say / shout /
beyond bands, with the whole-tag range tint behind the `ColorByDistance`
setting (default off, like the reference).

Reference (Firestorm, read-only): `llvoavatar::idleUpdateNameTagText`
(`FSTagShow*`), `llhudnametag`.

Deps: [[viewer-avatar-complexity-limit]] (the ARC numbers + jelly
reasons); the tag surface ([[viewer-name-tags-billboard-render]]) is done.
