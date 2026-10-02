---
id: viewer-mute-entry-name-is-not-the-username
title: A blocked avatar's mute entry is named by its first name, not its username
topic: viewer
status: bugs
origin: test-e2e-sweep-two-avatars (2026-10-02)
refs: [test-e2e-sweep-two-avatars]
---

Context: [context/viewer.md](../context/viewer.md).

## Observation

Blocking the fake grid's "Catalogue Resident" from the radar's row menu sent
`Mute { id, name: "Catalogue", mute_type: Agent }`. The reference names an
avatar's mute entry by `LLAvatarName::getUserName()`
(`LLAvatarActions::toggleBlock`, `llavataractions.cpp`), which for a legacy
`Catalogue Resident` is the lower-case `catalogue` (`LLCacheName::
buildUsername`), and `first.last` for anyone else.

## To do

Check what each block path (radar, profile, people list, chat) puts in the
name and make them all the username the reference uses; a name-matched mute
(`has_by_name`, chat from a muted name) compares against it.
