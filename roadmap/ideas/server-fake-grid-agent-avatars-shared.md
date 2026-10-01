---
id: server-fake-grid-agent-avatars-shared
title: Fake grid — logged-in agents see each other's avatars
topic: server
status: ideas
origin: test-e2e-live-verify-sweep (2026-09-30)
refs: [test-e2e-sweep-two-avatars, server-world-agent-movement]
---

Context: [context/testing.md](../context/testing.md).

Two stage viewers logged into one fake region do not see each other: each
session streams the region's objects and NPCs, but never another session's
avatar, so a viewer's radar, minimap, name tags and world locators find the
catalogue NPCs and nobody else ([[test-e2e-sweep-two-avatars]] is waiting on
this). A real simulator sends every avatar in the region to every agent: the
avatar's `ObjectUpdate` (with its name values), its `AvatarAppearance`, its
`AvatarAnimation`, and a `KillObject` when it leaves. The fake grid already
publishes region writes to every session's watcher; an arriving or leaving
agent is one more such write. Where the avatar stands is the login point
until [[server-world-agent-movement]] moves it.
