---
id: viewer-rlv-restriction-state
title: RLV — the restriction state machine
topic: viewer
status: ready
origin: user request (2026-07); split from viewer-rlva-parsing
blocked_by: [viewer-rlv-command-parser]
---

Context: [context/viewer.md](../context/viewer.md).

Given the typed command stream of [[viewer-rlv-command-parser]], hold the
**restriction/exception state machine** keyed by issuing object. This is the
heart of the model — every enforcement family asks it "is behaviour X in force?"
at its choke point rather than re-deriving it.

Restrictions are **per issuing object** and reference-counted across objects
(`rlvhandler.cpp`):

- a behaviour stays in force while *any* object holds it, and every restriction
  an object placed is dropped when it detaches (the reference clears on detach);
- the bookkeeping is bidirectional — object → behaviours and
  behaviour → objects — plus per-behaviour **exceptions**
  (`@sendim:<uuid>=add`, an avatar allowed through an otherwise-blanket block);
- `@clear` (with an optional filter) drops an object's restrictions.

Everything this state machine decides is data the viewer (and any bot on
`sl-client`) can then act on. Like the parser it lives in the pure `sl-rlv`
crate with no I/O, and it can answer the queries that need no viewer state
(the version handshake reports 3.4.3 with a 2.9.28 compatibility floor). The
clear-on-detach transition is what makes the enforcement layer correct, so model
it explicitly here rather than in each consumer.

Reference (Firestorm, read-only): `rlvhandler.cpp` (per-object restriction map,
reference counting, clear-on-detach), `rlvdefines.h`.

## Parity-audit addendum (2026-08-19)

The full-parity audit of `rlvhelper.cpp` / `rlvdefines.h` /
`rlvmodifiers.h` adds cross-cutting machinery this task owns beyond
per-object refcounting and clear-on-detach:

- The behaviour-modifier value system: 21 `ERlvBehaviourModifier` slots
  with typed values (float / vector / UUID), a per-slot default,
  primary-object tracking, and most-restrictive-wins comparators
  (`RlvBehaviourModifierCompMin` / `CompMax`; the `addModifier` rows in
  the rlvhelper.cpp dictionary constructor). The owning enforcement
  families consume the slots (e.g. FARTOUCHDIST, RECVIMDISTMIN/MAX,
  the SETCAM_* group, SHOWNAMETAGSDIST, SITTPDIST, TPLOCALDIST), but the
  typed storage and selection logic live here.
- The 13 `ERlvLocalBhvrModifier` per-object local modifiers addressed
  through the command option (`@setsphere:mode;1=n` style) — used by the
  @setsphere/@setoverlay effect families.
- `@permissive` plus the `_sec` strict-command semantics: a `_sec`
  command (and everything, under @permissive) ignores exceptions issued
  by *other* objects; only the restricting object's own exceptions
  apply.
- Synonym canonicalisation (the BHVR_SYNONYM dictionary rows):
  touchfar→fartouch; camavdist/camdistmin/camdistmax/camtextures/
  camunlock (and camzoommin/camzoommax)→setcam_*; addoutfit*=force→
  attach*; attach*overorreplace→attach*.
- Behaviour-flag metadata (BHVR_EXPERIMENTAL / BHVR_EXTENDED /
  BHVR_DEPRECATED) carried per dictionary entry — `@getcommand` filters
  on these flags, so the state machine must retain them.
