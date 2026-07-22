---
id: viewer-inspector-popups
title: Avatar / object inspector popups from clickable chat names
topic: viewer
status: blocked
origin: user request (2026-07-22)
blocked_by: [viewer-url-linkification]
refs: [viewer-hover-tooltips, viewer-social-profiles]
---

Context: [context/viewer.md](../context/viewer.md).

The reference's **inspectors**: the small self-dismissing mini-profile
popup that opens when a name is clicked — an avatar's name in chat / a
transcript (the `secondlife:///app/agent/<id>/inspect` links the
linkification layer renders clickable) shows the avatar inspector
(name, a profile-text snippet, and the quick actions: view profile,
add friend, IM, teleport offer); an object name (e.g. the speaker of
object chat, `app/object/<id>/inspect`) shows the object inspector
(name, owner, description, touch / sit / buy affordances via
`RequestObjectPropertiesFamily`). The popup anchors near the click and
**dismisses itself** on mouse-leave / focus loss / a short timeout —
the same lightweight surface the hover tips need
([[viewer-hover-tooltips]]), so the two should share it. Full profile
opening stays with [[viewer-social-profiles]].

Reference (Firestorm, read-only): `llinspectavatar.cpp`,
`llinspectobject.cpp`, `llinspectremoteobject.cpp`,
`llurlentry` (`.../inspect` actions).
