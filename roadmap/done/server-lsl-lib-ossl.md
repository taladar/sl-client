---
id: server-lsl-lib-ossl
title: Scope the OSSL (os*) surface — how much, and why
topic: server
status: done
origin: LSL-on-the-fake-grid audit (2026-09-20)
blocked_by: [server-lsl-library-surface-table, gridspec-lsl-ossl]
refs: [server-lsl-lib-avatar-control, viewer-opensim-region-extras-limits,
  server-lsl-ossl-culture-strings, server-fake-grid-dynamic-textures]
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
  ([[server-fake-grid-lsl-syntax]]), because serving OpenSim's full
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

## Measured (2026-10-26, [[gridspec-lsl-ossl]])

The model to imitate, as the local OpenSim answers it
(`book/src/gridspec/lsl.md` § OSSL, probes `ossl/available` and
`ossl/errors`):

- **The permission decision** is per function: an `Allow_` line wins over
  the threat level in both directions (`osGetAgents`, threat level `None`, is
  denied by its estate-only list; `osGetScriptEngineName`, `High`, is allowed
  by `true`); a function with no line is allowed when its level is at or
  under `OSFunctionThreatLevel`. Dozens of functions check only that OSSL
  is on, and some (`osSHA256`, `osRound`, `osApproxEquals`, …) check
  nothing.
- **The three denial texts**, prefixed `OSSL Permission Error: `:
  `<fn> permission denied.  Allowed threat level is <max> but function
  threat level is <level>.`, `<fn> permission denied.`, `<fn> disabled in
  region configuration`; an argument error is `OSSL Runtime Error: <fn>: …`.
  Each is thrown: the event ends, YEngine stops the script until a reset,
  XEngine runs on; the message goes on `DEBUG_CHANNEL` to every avatar near
  it, in each engine's wrapping, and no script hears it.
- **Warnings are shouted, not thrown**: a deprecated function (`Use of
  function <fn> is deprecated. Use <new> instead.`) and some argument errors
  shout on `DEBUG_CHANNEL`, where scripts hear them, and sleep the script a
  second; the call still returns its value.
- `modSendCommand` returns a fresh key whether or not a module answers;
  `lsGetWindlightScene` answers from the region's environment.

A fake grid that flattens the tiers should still produce these texts for
whatever it refuses, since a script (or a test) that reads the debug channel
reads them.

## Done (2026-11-01)

The written scope is § *OpenSim's own functions* of the book chapter
`simulator/lsl-engine.md`; the per-function list the coverage test holds the
registry to is `sl-lsl-runtime/src/library/opensim-scope.txt` (in, later with a
named task, or out with a reason). Beyond the scoping the acceptance asked
for, the tranche implements what of it the fake grid can run today:

- **The table.** OpenSim's own served document, moved from
  `sl-fake-grid/lsl-syntax/` to `sl-lsl-runtime/opensim/` (the fake grid reads
  it from there), is the source of OpenSim's part of the library table: 236
  functions of the OSSL, LightShare and MOD families, 7 of them and
  `modSendCommand` from a supplement of what OpenSim compiles and leaves out
  (`ossl-unserved.xml`), and 139 constants only OpenSim has. Each descriptor
  carries its `Family` and its `Gate`, the permission check derived from
  `OSSL_Api.cs` (`ossl-checks.txt`, `extract_ossl_checks.py`). Overloads are
  entries of their own (`OsApproxEquals` … `OsApproxEquals6`), and the
  compiler picks one as YEngine does, by arity and then argument types.
- **The compiler gate.** Second Life's dialect refuses every OpenSim family
  and OpenSim-only constant; OpenSim's compiles them.
- **The permission model**, not flattened: `vm::ossl::Settings` (the stock
  `osslDefaultEnable.ini`, vendored, or a region's own through
  `Engine::set_ossl`), the owner's `Standing` from the host, `Allow_` and
  `Creators_` lines over the threat level, OpenSim's three denial texts thrown
  as each engine reports a script exception, `PermissionErrorToOwner`, and
  the shouted warnings with their second's sleep (`ScriptCtx::shout_error`).
- **55 functions implemented**: strings, regular expressions, hashing,
  numbers, vectors, the drawing-list builders, the engine's and simulator's
  identity, region size, ground height and day length, `osGetAgentIP`,
  `osListenRegex`, `osMessageObject` and `modSendCommand`.
- **Proof**: `ossl/available` and `ossl/errors` replayed against both
  OpenSim engines' transcripts (with what other tranches own answered as the
  grid did, and one named deviation: `(string)` of a vector writes five
  decimals here, six on OpenSim, by the value model's rule), Second Life
  refusing both; unit tests of the permission model, overload resolution,
  regex listens and object messages.

Handed on: the world-touching `os*` functions to the tranches of the `ll*`
functions they shadow (a note in each), .NET's culture-sensitive string
search and the in-place sort to [[server-lsl-ossl-culture-strings]], the
dynamic textures to [[server-fake-grid-dynamic-textures]], and the seven new
`Host` methods and the region's `[OSSL]` settings to
[[server-fake-grid-script-engine-wiring]].
