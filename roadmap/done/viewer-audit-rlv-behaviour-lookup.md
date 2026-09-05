---
id: viewer-audit-rlv-behaviour-lookup
title: RLV behaviour lookup is param-type-blind and has no modifier fallback
topic: viewer
status: done
origin: static code audit (2026-08-26)
points: 3
refs: [viewer-rlv-restriction-state, viewer-audit-rlv-behaviour-table-test]
---

Context: [context/viewer.md](../context/viewer.md).

`sl-rlv` parses untrusted in-world chat, and its behaviour lookup is wider than
the reference's:

- `sl-rlv/src/command.rs:146` — `resolve_behaviour` looks up the keyword alone.
  Firestorm keys `m_String2InfoMap` on `(behaviour, paramType)`
  (`rlvhelper.cpp:446`), so a force-only keyword used as a restriction yields
  `RLV_BHVR_UNKNOWN`. Here `@sit=n` classifies as `Sit`/`Add`. The
  `rlv_behaviours!` table (`behaviour.rs:17`) has no param-type column, so this
  cannot currently be expressed.
- `sl-rlv/src/command.rs:163` — no **local behaviour-modifier** fallback. The
  reference's `getBehaviourInfo` retries `<base>_<modifier>` against the base
  behaviour for FORCE commands (`rlvhelper.cpp:447-453`, e.g.
  `@setcam_fov=force`); every such keyword maps to `Unknown` here.

Verified correct and not to be touched: the lowercase-whole-message,
drop-empty-comma-tokens, `clear`-without-param and `:`-only-after-`=` rules
(`command.rs:97-155`) match `rlvhelper.cpp:760-790` and
`llviewermessage.cpp:3145` exactly.

Note the crate has **no consumer anywhere in the workspace**, which is expected
— [[viewer-rlv-restriction-state]] is the pending enforcement layer. Fixing the
table now means that layer inherits a correct classifier. Pair with
[[viewer-audit-rlv-behaviour-table-test]].

## Fixed

The table now carries the param-kind column the reference is keyed on, and the
lookup uses it.

- `RlvParamKind` (`command.rs`) is `ERlvParamType` collapsed the way
  `m_String2InfoMap` keys it — `AddRem` / `Force` / `Reply` / `Clear`, with add
  and remove folded together as `getBehaviourInfo` does
  (`(eParamType & RLV_TYPE_ADDREM) ? RLV_TYPE_ADDREM : eParamType`).
  `RlvParam::kind()` projects a decoded param onto it.
- Every `rlv_behaviours!` row gained a `params [...]` column holding the set of
  kinds the reference declares an entry for, so a keyword registered twice —
  `@sit=n` the restriction and `@sit=force` the action — lists both.
  `RlvBehaviour::accepts` asks one axis, `RlvBehaviour::param_kinds` reports the
  row, and `RlvBehaviour::ALL` enumerates the table (which is what the
  reference's `getCommands` walks for `@getcommand`).
- `RlvBehaviour::resolve(keyword, kind)` replaces the keyword-only
  `resolve_behaviour`: strip `_sec`, look the base up **for this kind**, apply
  the strict gate to the row that was found, then fall through to the modifier
  retry. `parse_field` classifies the param first and resolves last, because the
  behaviour is not knowable before the kind is.
- The local behaviour-modifier fallback is `RlvLocalModifier`, a second small
  table of the thirteen named knobs the reference registers on `@setoverlay`
  (alpha, texture, tint) and `@setsphere` (mode, origin, color, distmin,
  distmax, distextend, param, tween, valuemin, valuemax). A `=force` keyword
  that resolves to nothing is split at its last `_` and retried against the
  **restriction** rows only — which is where the reference registers modifiers —
  and the command comes back as the base behaviour with
  `RlvCommand::modifier` set, matching `RlvCommand::getBehaviourType()` plus
  `getBehaviourModifier()`. Never for a strict keyword, never for a non-force
  kind, never when the last part is empty.

What changed on the wire-facing behaviour: `@tpto=n`, `@version=n`,
`@showloc=force`, `@getstatus=force` and every other keyword used with a kind it
was not declared for now resolve to `Unknown` instead of silently classifying as
a restriction on that keyword. `@clear=n` does too, and that is the reference's
answer as well — `n` wins the param precedence, so the lookup asks for a `clear`
*restriction*, which does not exist. The old test asserting `Clear` there was
pinning our own deviation; it now pins the reference. `@setsphere_mode=force`
and its twelve siblings stop being `Unknown`.

## Verified

`cargo test -p sl-rlv` — 26 tests plus the doctest, green. New coverage:
`param_kind_gates_the_behaviour_lookup` (force-only, reply-only,
restriction-only, and a keyword declared for both), `local_modifier_fallback`
(both bases, the `@setoverlay_tween` behaviour that only looks like a modifier,
and five negatives), `local_modifier_table_roundtrips`, and the three
table-driven sweeps from [[viewer-audit-rlv-behaviour-table-test]].

The table itself was cross-checked mechanically against the reference rather
than by eye: a throwaway script parsed `RlvBehaviourDictionary`'s constructor
(mapping `RlvBehaviourProcessor`/`RlvBehaviourGenericProcessor`/
`RlvBehaviourToggleProcessor` to ADDREM, `RlvForceProcessor` to FORCE,
`RlvReplyProcessor` to REPLY, and reading the explicit `RLV_TYPE_*` on plain
`RlvBehaviourInfo` rows) and diffed it against the Rust table. All 176 shared
keywords agree on their param-kind set, all 19 `BHVR_STRICT` flags agree, and
both local-modifier lists agree. The only row with no counterpart is `clear`,
which the reference handles as a param type rather than a dictionary entry; it
is declared here for the `Clear` kind alone, which is what makes `@clear=n`
resolve to `Unknown` the way the reference does.

Not verified live: the crate still has no consumer —
[[viewer-rlv-restriction-state]] is the enforcement layer that will exercise it
against an in-world scripted object.

Reference (Firestorm, read-only): `indra/newview/rlvhelper.cpp`
(`RlvBehaviourDictionary::RlvBehaviourDictionary`, `addEntry`,
`getBehaviourInfo`, `getBehaviourFromString`, `RlvBehaviourInfo::addModifier` /
`lookupBehaviourModifier`, `RlvCommand::RlvCommand`),
`indra/newview/rlvhelper.h`
(the processor aliases that carry each entry's param-type mask),
`indra/newview/rlvdefines.h` (`ERlvParamType`, `ERlvLocalBhvrModifier`).
