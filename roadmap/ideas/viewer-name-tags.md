---
id: viewer-name-tags
title: Avatar name tags (the full reference-viewer feature)
topic: viewer
status: ideas
origin: user request (2026-07)
---

Context: [context/viewer.md](../context/viewer.md).

The viewer today draws a **debug-grade** name tag, not the SL one. `avatars.rs`
spawns one plain-white `bevy_ui` text node per avatar (`NameTag`, `spawn_label`,
`position_name_tags`; `NAME_TAG_FONT_SIZE` 16, `NAME_TAG_GAP` 0.3), projected
with `Camera::world_to_viewport` and hidden when off-screen. It shows only the
**legacy** name (`AvatarName::legacy_name()`), resolved once per agent over UDP
`UUIDNameRequest` (`Command::RequestAvatarNames`), with an 8-character UUID
fragment until the reply lands. There is no styling, no culling, no occlusion,
and no state beyond the name.

Promote it to the reference feature:

- **Names.** Display name over `@username`, falling back to the legacy name.
  The `GetDisplayNames` cap is **already fully implemented** —
  `Command::RequestDisplayNames` / `Event::DisplayNames` /
  `Event::DisplayNameUpdate` (`api-g3`) — the viewer simply never calls it.
  Keep the legacy path as the fallback: OpenSim generally does not serve the
  cap, so both must work, and a pushed `DisplayNameUpdate` has to refresh a
  live tag.
- **Decorations.** Group title line, "(Away)" / "(Busy)", a typing indicator,
  friend / group / muted colouring, and the client-tag style question.
  A *speaking* indicator is explicitly out of scope — it needs decoded voice
  ([[viewer-voice-audio]]) and this project scopes voice to signalling only.
- **Behaviour.** Distance-based fade and a hide-beyond-N-metres cut-off,
  on-screen size clamping (so a distant tag stays legible without dominating),
  occlusion / depth behaviour against world geometry, a backdrop bubble and
  outline so tags read against any background, and a click target that selects
  the avatar ([[viewer-object-selection]]).
- **Preferences.** Show tags / show own tag / show display names / distance
  limit ([[viewer-preferences-ui]]).

Whether tags stay `bevy_ui` nodes or become world-space billboarded text is the
first architectural decision: the reference draws them in-world
(`LLHUDNameTag`), which is what makes occlusion, depth and size clamping
natural — screen-space nodes make each of those a special case.

**Standing hazard** (from `viewer-name-tags-lost-to-probe-cameras`): every
camera query in this code must stay qualified `With<FlyCamera>`, or the P33.2
reflection-probe cameras make `Query::single()` fail every frame and the tags
vanish.

Reference (Firestorm, read-only): `llhudnametag`, `llhudtext`,
`llvoavatar::idleUpdateNameTag`, `llavatarnamecache`.

Builds on: the existing `avatars.rs` tags, `api-g3` display-name caps, and the
`UUIDNameRequest` legacy path.
