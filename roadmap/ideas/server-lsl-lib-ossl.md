---
id: server-lsl-lib-ossl
title: Scope the OSSL (os*) surface — how much, and why
topic: server
status: ideas
origin: LSL-on-the-fake-grid audit (2026-09-20)
blocked_by: [server-lsl-library-surface-table]
refs: [server-lsl-lib-avatar-control, viewer-opensim-region-extras-limits]
---

Context: [context/lsl.md](../context/lsl.md).

`~/devel/3rdparty/opensim/bin/ScriptSyntax.xml` carries **224 `os*`
functions** alongside the 425 `ll*` ones, and OSSL is why several things
in this workspace exist at all — the OpenSim test grid's scripted
fixtures use `osNpc*` to stand avatars up, and the viewer's syntax
highlighting is grid-served precisely so it colours OSSL without a code
change.

This task is a scoping decision, not an implementation. It should
produce a short document and a filter in the surface table, answering:

- **Which OSSL functions does the fake grid actually need?** The honest
  answer is probably a handful: the `osNpc*` family (which is how a
  scripted scenario animates a crowd without N logins — and the fake
  grid already models NPCs as fixtures), `osTeleportAgent`, `osMessageObject`,
  `osSetDynamicTextureData` (the source of every in-world sign), and
  `osGetAvatarList`.
- **Which are actively dangerous or meaningless here?** Anything
  god-level, anything touching the ROBUST services, `osGetGridName`-style
  identity readers whose honest answer is "this is not OpenSim", and the
  whole `osDB`/console surface.
- **What does the grid tell the viewer?** Whatever we implement should
  be what the `LSLSyntax` document advertises
  ([[protocol-sim-lsl-syntax-document]]), because serving OpenSim's full
  list for an engine that implements five of them makes the editor's
  autocomplete lie.
- **The threat-level model.** OSSL's per-function permission tiers
  (`osslEnable*`, the `Severity` levels) are a real part of its
  semantics; a fake grid can flatten them, but should say so rather than
  omit them.

Acceptance: a written scope — the list of `os*` functions in, the list
explicitly out, and the reason — plus the surface table marking the rest
`out of scope` rather than `missing`, so the coverage harness stops
counting 224 functions as a gap.
