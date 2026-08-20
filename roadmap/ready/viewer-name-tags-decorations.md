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
colouring. The `(Editing Appearance)` status entry (the CUSTOMIZE
signalled animation) shipped separately (2026-08-05). What remains, on
top of that renderer:

- **contact-set** colouring ([[viewer-contact-sets]]) and the minimap
  mark-colour override;
- the **client-tag** display / colouring question (`FSColorClienttags`
  family — decide how much of the tag-guessing system to carry);
- the **"Loading..."** cloud line and the rez-state debug line
  (`NameTagDebugAVRezState`);
- a **speaking-indicator** hook: the voice dot placement, fed by the
  per-agent voice activity [[viewer-voice-controls]] surfaces once voice
  audio lands;
- the reference's screen-coverage **LOD** (drop lines as cumulative tag
  coverage grows) and single-line **ellipsis truncation** at the 298 px
  width (word-wrap ships today).

The complexity (ARC) lines are split out to
[[viewer-name-tags-complexity-distance]].

The **Unavailable / Auto-Response** status entries shipped with
[[viewer-do-not-disturb-away]] (2026-08-20), which owns the state behind
them: `Unavailable` comes off the do-not-disturb signalled animation like
`Away` does, and `Auto-Response` is own-tag-only behind
`ShowAutorespondInNameTag` (default off, as the reference has it).

Reference (Firestorm, read-only): `llhudnametag`,
`llvoavatar::idleUpdateNameTag`, `llavatarnamecache`.

## Parity-audit addendum (2026-08-19)

Additional tag knobs from the parity audit: the third name-tag display
mode — "show briefly" (the `AvatarNameTagMode` radio's timed state,
duration `RenderNameShowTime`; our `ShowNameTags` in `avatars.rs` is a
plain bool), suppressing the group title on the *own* tag only
(`RenderHideGroupTitle`), tag background opacity (`ChatBubbleOpacity`
doubles as the name-tag alpha in the reference), a user Z-offset
correction to nudge tag height (`FSNameTagZOffsetCorrection`), and the
legacy fixed-at-avatar-position tag mode (`FSLegacyNametagPosition`,
tag pinned to the avatar instead of floating above by height).
