---
id: viewer-name-tags-decorations
title: Name tags — title / state / colouring decorations
topic: viewer
status: blocked
origin: user request (2026-07); split from viewer-name-tags
blocked_by: [viewer-name-tags-billboard-render]
---

Context: [context/viewer.md](../context/viewer.md).

Add the reference viewer's tag **decorations** on top of the world-space
renderer ([[viewer-name-tags-billboard-render]]):

- a **group title** line above the name;
- **"(Away)" / "(Busy)"** status text;
- a **typing indicator**;
- **friend / group / muted** colouring;
- the **client-tag** style question.

A *speaking* indicator is explicitly out of scope — it needs decoded voice
([[viewer-voice-audio]]) and this project scopes voice to signalling only.

Reference (Firestorm, read-only): `llhudnametag`,
`llvoavatar::idleUpdateNameTag`, `llavatarnamecache`.

Deps: [[viewer-name-tags-billboard-render]] (the tag surface these lines and
colours draw onto).
