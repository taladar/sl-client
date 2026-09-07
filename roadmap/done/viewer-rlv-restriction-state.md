---
id: viewer-rlv-restriction-state
title: RLV — the restriction state machine
topic: viewer
status: done
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

## Done

`sl-rlv` grew the state machine, plus the two table changes the addendum asks
for. The crate is still pure — `thiserror` and `uuid`, no Bevy, no I/O.

**A behaviour is no longer a keyword.** The old table conflated the two, so
`@touchfar=n` and `@fartouch=n` were separate restrictions and `@attachthis=n`
did not share a reference count with `@attachallthis=n`. `behaviour.rs` now has
the reference's *two* tables: `RlvBehaviour` is `ERlvBehaviour`, the
reference-counting slot; `RlvEntry` is one dictionary row —
`(keyword, param kind) -> behaviour + flags` — which is the key
`m_String2InfoMap` is built on. Synonym rows fold onto the canonical behaviour,
so the deprecated camera shims reach `@setcam_*`, and every force-wear spelling
reaches the one `ForceWear` the reference calls `RLV_CMD_FORCEWEAR`.
`RlvBehaviourFlags` carries STRICT / SYNONYM / EXTENDED / EXPERIMENTAL /
DEPRECATED per row, and `RlvCommand` now hands the row it resolved through to
its consumer.

**Restriction rules** (`restriction.rs`) tabulate, per restriction, what its
option means and whether either form reference-counts. This is the half of the
reference's `RLV_TYPE_ADDREM` handlers that is not viewer work, and it is what
keeps an exception from reading back as a restriction: `@sendim:<uuid>=add`
grants an exception and counts nothing, while `@recvimfrom:<uuid>=n` both
restricts and names its target. `@detach` and `@setoverlay_touch` never count;
`@addattach` counts bare but not per-point; `@camzoommin` counts *as*
`@setcam_fovmin`; `@setcam`/`@setdebug`/`@setenv` admit one holder and
`@setsphere` six.

**Modifiers** (`modifier.rs`) are the 21 `ERlvBehaviourModifier` slots with
typed values (float / int / vec3 / vec4 / UUID), a per-slot default, an
add-default-on-empty flag, primary-object tracking and the most-restrictive-wins
comparators. `RlvModifierValue` is `Eq`+`Hash` by float bit pattern, which is
the same test as the reference's `operator==` for finding the exact value an
object contributed, only reflexive.

**`RlvState`** (`state.rs`) holds it: objects to restrictions and back,
reference counts, exceptions, modifier slots. `apply` takes any decoded command
and owns the `=n` / `=y` / `@clear` / local-modifier-`=force` ones, answering
`NotAStateChange` for the actions and queries that are the consumer's.
`clear_object` is the detach transition, reached the way the reference reaches
it — by feeding the object a synthetic `@clear`, so the counts come down exactly
as if it had lifted each restriction itself. `is_exception` implements the
`@permissive` / `_sec` rule including the detail at `rlvhandler.cpp:287`: when
`@permissive` is in force the strict sweep collects *non*-strict holders too.
`@setcam` reproduces its toggle handler — the holder becomes the primary object
of all eleven camera slots and `@setcam_unlock`'s count is rewritten.
`version.rs` answers the handshake (3.4.3, 2.9.28 floor, RLVa 2.4.2 / impl 13)
and `known_commands` answers `@getcommand`, honouring the experimental-command
switch the reference implements by not registering those rows at all.

Deliberate deviations, all documented at their site: numeric options reject
trailing garbage where `std::stof` stops at the first bad character; `@notify`'s
channel is validated here though its filter stays opaque; the holder limit is
checked before the duplicate check, which matches the three exclusive
restrictions and differs from `@setsphere` only in which success/failure code an
already-holding object gets for a no-op; and where the reference declares two
owning rows for one behaviour and kind (the `@attachthis` / `@attachallthis`
folder-lock pairs) `RlvEntry::canonical` answers with the first rather than
nothing.

One real bug found by the crate's own hostile-input test: `@recvim:10;bad=n`
wrote the parsed minimum distance into its slot before discovering the maximum
was malformed, then unwound the held command — stranding a value nothing could
ever take away again. Both halves are parsed before either is written.

## Verified

`cargo test -p sl-rlv` — 78 tests plus 4 doctests, green.

The tables were cross-checked mechanically rather than by eye, as the parser's
were. Two throwaway scripts parsed `RlvBehaviourDictionary`'s constructor and
diffed it against the Rust tables:

- 191 dictionary rows agree on keyword, param kind, canonical behaviour *and*
  flag set; the only Rust row without a counterpart is `clear`, which the
  reference handles as a param type rather than an entry;
- all 21 global modifier slots agree on owning behaviour, display name, default
  value, add-default flag and comparator; all 13 local modifiers agree on
  behaviour, name and value type.

The state machine itself is covered by 30 tests in `state.rs`: reference
counting across two objects, duplicate adds, detach, filtered and unfiltered
`@clear`, synonyms sharing a count, exception-versus-restriction, strict
exceptions needing every holder, `@permissive`, the never-counted behaviours,
holder limits, every option-refusal path, most-restrictive-wins in both
directions, squared IM ranges, `@setcam` exclusivity and unlock recount, local
modifiers, `@getstatus` / `@getstatusall` text, `@getcommand`, and a hostile
command stream after which every count, object, exception and modifier slot is
empty again.

Not verified live: still no consumer in the workspace. The enforcement families
this unblocks ([[viewer-rlv-enforce-send-side]] and its eleven siblings) are
what will drive it against a real scripted object.

Reference (Firestorm, read-only): `indra/newview/rlvhandler.cpp` (`RlvHandler`,
`processCommand`, `processAddRemCommand`, `processClearCommand`, `onDetach`,
`addException` / `isException` / `isPermissive`, the `RLV_TYPE_ADDREM`
handlers), `indra/newview/rlvhelper.cpp` (`RlvObject`, `RlvBehaviourModifier`,
`RlvBehaviourDictionary`), `indra/newview/rlvmodifiers.h` (the comparators),
`indra/newview/rlvdefines.h` (`ERlvBehaviourModifier`, `ERlvCmdRet`,
`ERlvExceptionCheck`, the version constants), `indra/newview/rlvcommon.cpp`
(`RlvStrings::getVersion`).
