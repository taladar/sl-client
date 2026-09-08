---
id: viewer-rlv-blocked-behaviours
title: RLV — blocked behaviours, and RestrainedLoveNoSetEnv as the one that uses them
topic: viewer
status: ready
origin: split from viewer-rlv-environment-commands (2026-09-08) — the setting that task could not honour
refs: [viewer-rlv-environment-commands, viewer-rlv-command-intake, viewer-rlv-blocked-objects, viewer-preferences-debug-settings-editor]
---

Context: [context/viewer.md](../context/viewer.md).

`BHVR_BLOCKED` (`rlvhelper.h:52`) is a per-behaviour flag that takes one
keyword out of the language: `RlvHandler::processCommand` checks
`rlvCmd.isBlocked()` before anything else it would do with the command and
answers `RLV_RET_FAILED_DISABLED` (`rlvhandler.cpp:472`). It is **not** the
blocked-*object* list — that is [[viewer-rlv-blocked-objects]], a different
refusal (`RLV_RET_FAILED_BLOCKED`) with a different producer.

The reference has exactly one user of it, and it is the point of this task:
**`RestrainedLoveNoSetEnv`** blocks `setenv` for `RLV_TYPE_ADDREM`
(`RlvBehaviourDictionary::RlvBehaviourDictionary`, `rlvhelper.cpp:358`), so a
worn object cannot take control of the wearer's environment away from them. Note
what it does *not* do, because the obvious reading is wrong: the `@setenv_*`
**force** commands keep working. All it stops is `@setenv=n` — the restriction
that would grey out the user's own environment menu and lock every other object
out of the sky.

## Why this is not a one-line change

`sl-rlv`'s behaviour table is a `const` — one static row per keyword, its flags
part of the declaration. The reference *mutates* its dictionary at
construction, from a setting read once, which is why the setting needs a relog
there. Here the flag has to be **runtime state** beside the table rather than a
property of it:

- somewhere to hold the blocked set (`RlvState`, or a registry the state
  borrows), and a way for the consumer to set it from the settings store;
- the check in `RlvState::apply`, ahead of everything else it does with the
  command, in the reference's own position;
- a new `RlvOutcome` variant for `RLV_RET_FAILED_DISABLED`, which this crate
  has no spelling for yet — and which the console's report line and
  `outcome_stream` then have to say something sensible about;
- the setting itself, live rather than needing a relog (the same divergence the
  RLVa strings already take), which means the blocked set moves when the user
  moves it — and a `@setenv=n` already in force when they turn it on has to be
  released, or the setting would only bite objects that had not asked yet.

The setting exists and is registered today
([[viewer-preferences-debug-settings-editor]] lists it, and `@getdebug_*`
reads it); nothing reads it for anything else.

## Verify

The check is client-side and needs no grid: a unit test that `@setenv=n` is
refused with the new outcome while the setting is on, that `@setenv_ambient`
still applies, and that turning the setting on releases a restriction already
held. The interactive check is the RLVa console — type `@setenv=n` with the
setting on and read the report line.

Reference (Firestorm, read-only): `rlvhelper.h` (`BHVR_BLOCKED`, `isBlocked`),
`rlvhelper.cpp:358` and `RlvBehaviourDictionary::toggleBehaviourFlag`,
`rlvhandler.cpp:472`, `rlvdefines.h` (`RLV_RET_FAILED_DISABLED`),
`rlvcommon.cpp` (`RlvSettings::getNoSetEnv`).
