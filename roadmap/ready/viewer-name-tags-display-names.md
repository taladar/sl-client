---
id: viewer-name-tags-display-names
title: Name tags — wire GetDisplayNames caps into the tag
topic: viewer
status: ready
origin: user request (2026-07); split from viewer-name-tags
refs: [viewer-name-tags-billboard-render]
---

Context: [context/viewer.md](../context/viewer.md).

The viewer today draws a **debug-grade** name tag: `avatars.rs` spawns one
plain-white `bevy_ui` text node per avatar (`NameTag`, `spawn_label`,
`position_name_tags`) showing only the **legacy** name
(`AvatarName::legacy_name()`), resolved once per agent over UDP
`UUIDNameRequest` (`Command::RequestAvatarNames`), with an 8-character UUID
fragment until the reply lands.

Wire the display-name path into that existing tag. The `GetDisplayNames` cap is
**already fully implemented** — `Command::RequestDisplayNames` /
`Event::DisplayNames` / `Event::DisplayNameUpdate` (`api-g3`) — the viewer
simply never calls it. Request display names per agent, then show the
**display name over `@username`**, falling back to the legacy name when no
display name is available. Keep the legacy `UUIDNameRequest` path as the
fallback: OpenSim generally does not serve the cap, so both must work. A pushed
`DisplayNameUpdate` has to refresh a live tag (a resident renaming themselves).

This is the name-resolution wiring only — the styling, bubble, occlusion and
world-space rendering are [[viewer-name-tags-billboard-render]]; this task feeds
the tag whichever renderer is in place. The nearby-avatar radar reuses this same
name resolution ([[viewer-avatar-radar]]).

Reference (Firestorm, read-only): `llavatarnamecache`,
`llvoavatar::idleUpdateNameTag`.

Builds on: the existing `avatars.rs` tags, `api-g3` display-name caps, and the
`UUIDNameRequest` legacy path.
