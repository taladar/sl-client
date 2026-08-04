---
id: viewer-name-tags-decorations
title: Name tags — remaining decorations
topic: viewer
status: ready
origin: user request (2026-07); split from viewer-name-tags
refs: [viewer-name-tags-billboard-render]
---

Context: [context/viewer.md](../context/viewer.md).

Most of the original decoration set shipped with the world-space renderer
([[viewer-name-tags-billboard-render]], 2026-08-05): the group-title line,
the Away / Blocked / Typing status line, the username line, the distance
line with range colours, and friend / muted / Linden / display-name-state
colouring. What remains, on top of that renderer:

- **contact-set** colouring ([[viewer-contact-sets]]) and the minimap
  mark-colour override;
- the **client-tag** display / colouring question (`FSColorClienttags`
  family — decide how much of the tag-guessing system to carry);
- the own tag's **Unavailable / Auto-Response** status entries, fed by
  the local do-not-disturb state ([[viewer-do-not-disturb-away]]);
- the **"Loading..."** cloud line and the rez-state debug line
  (`NameTagDebugAVRezState`);
- the `(Editing Appearance)` status entry (the CUSTOMIZE signalled
  animation);
- a **speaking-indicator** hook: the voice dot placement, fed by the
  per-agent voice activity [[viewer-voice-controls]] surfaces once voice
  audio lands;
- the reference's screen-coverage **LOD** (drop lines as cumulative tag
  coverage grows) and single-line **ellipsis truncation** at the 298 px
  width (word-wrap ships today).

The complexity (ARC) lines are split out to
[[viewer-name-tags-complexity-distance]].

Reference (Firestorm, read-only): `llhudnametag`,
`llvoavatar::idleUpdateNameTag`, `llavatarnamecache`.
