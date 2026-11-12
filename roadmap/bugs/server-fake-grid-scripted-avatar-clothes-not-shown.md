---
id: server-fake-grid-scripted-avatar-clothes-not-shown
title: A dressed scripted avatar is drawn without its clothes
topic: server
status: bugs
origin: server-fake-grid-scripted-avatars (2026-11-12)
refs: [server-fake-grid-scripted-avatars]
---

Context: [context/lsl.md](../context/lsl.md).

## Observation

The `scripted-avatar` scene's `sitter()` is `dressed()` (its account wears
pants and shoes over its body parts), yet an `sl-crosscheck` capture of the
scene showed it with the bare body. The scripted avatar is an
`sl-client-tokio` client that does nothing about its appearance after login:
it sends no `AgentSetAppearance` and no bake request, so whether the grid
bakes its outfit, and what other viewers are told, is unexamined.

## Wanted

Find whether the grid's central bake of a dressed account needs the client's
trigger, or the viewer is not drawing a baked outfit it was sent; a dressed
scripted avatar shows its clothes to another viewer.
