---
id: viewer-mute-entry-name-is-not-the-username
title: A blocked avatar's mute entry is named by its first name, not its username
topic: viewer
status: done
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

## Done (2026-10-16, with [[gridspec-mute-list]])

The observation was the reference's own name: `LLAvatarName::getUserName()`
defaults to `lowercase = false`, so a `Catalogue Resident` is muted as
`Catalogue`, not `catalogue` (that form is `buildUsername`, which the
reference uses only to *compare* names). What remained was the other block
paths: the one guarded handler (`sl-viewer-people`'s `wire_name`) now names
every resident mute by the user name the grid resolved for the id (else the
given name with `Resident` dropped), an update keeps the entry's own name, and
`RequestUnblock` removes an entry under the name it is filed under — which
OpenSim requires. Name-matched tests compare in account form
(`MuteList::agent_named`).
