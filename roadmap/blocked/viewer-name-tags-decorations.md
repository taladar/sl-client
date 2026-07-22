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
- the comma-joined **status line** (the 2026-07-22 nametag survey's full
  set): Away, Busy/DND, Autoresponse (own tag —
  [[viewer-do-not-disturb-away]]), Muted, "Appearance" while editing
  appearance, "Loading..." while the avatar is a cloud;
- a **typing indicator** line (setting-gated, as FS's
  `FSShowTypingStateInNameTag`), plus the rez-state debug line
  (`NameTagDebugAVRezState`);
- the **username / legacy-name** second line options
  (`NameTagShowUsernames`, `FSNameTagShowLegacyUsernames`) and show-own-tag;
- **friend / group / muted** colouring, **contact-set** colouring
  ([[viewer-contact-sets]]), and the minimap mark-colour override;
- the **client-tag** display / colouring question (`FSColorClienttags`
  family — decide how much of the tag-guessing system to carry);
- a **speaking-indicator** hook: the voice dot placement, fed by the
  per-agent voice activity [[viewer-voice-controls]] surfaces once voice
  audio lands (in scope since 2026-07-22).

The complexity (ARC) and distance lines with range colouring are split out
to [[viewer-name-tags-complexity-distance]].

Reference (Firestorm, read-only): `llhudnametag`,
`llvoavatar::idleUpdateNameTag`, `llavatarnamecache`.

Deps: [[viewer-name-tags-billboard-render]] (the tag surface these lines and
colours draw onto).
