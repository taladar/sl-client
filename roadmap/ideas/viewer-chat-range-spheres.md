---
id: viewer-chat-range-spheres
title: In-world whisper/say/shout chat-range spheres
topic: viewer
status: ideas
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-minimap-avatar-dots, viewer-debug-render-beacons]
---

Context: [context/viewer.md](../context/viewer.md).

Firestorm can draw translucent range spheres around the own avatar at
the whisper (10 m), say (20 m) and shout (100 m) chat radii
(`FSShowChatRangeSpheres`), so the user can see exactly who is in
earshot before speaking. We already render the equivalent 2D cue — the
minimap whisper/chat/shout rings (`MiniMap*Ring` settings, done
[[viewer-minimap-avatar-dots]]) — but have no 3D in-world
visualization. Implementation is three alpha-blended sphere shells
anchored to the avatar root, toggled by one setting; the transparent
overlay glow-mask rule applies (alpha-blend world materials must
preserve the glow mask). Rendering style can share machinery with the
marker rendering planned in [[viewer-debug-render-beacons]].

Reference (Firestorm, read-only): `FSShowChatRangeSpheres` consumers in
`indra/newview/llvoavatarself.cpp` / the pipeline chat-range-sphere
render, `indra/newview/app_settings/settings.xml`.
