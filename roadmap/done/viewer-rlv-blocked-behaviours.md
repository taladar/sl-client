---
id: viewer-rlv-blocked-behaviours
title: RLV — blocked behaviours, and RestrainedLoveNoSetEnv as the one that uses them
topic: viewer
status: done
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

## Done

**The blocked set is state beside the table**, not a property of it:
`RlvState::set_behaviour_blocked` / `is_behaviour_blocked` /
`blocked_behaviours`, keyed by `(keyword, param kind)` — the reference's own
dictionary key, so `("setenv", AddRem)` is `@setenv=n` and `@setenv=y` and
nothing else. A keyword the dictionary does not declare for that kind answers
`false` rather than blocking nothing quietly, and `@getcommand` still **lists**
a blocked keyword, because `getCommands` reads no flags either: the row exists
and the dictionary says so — what it will not do is obey it.

**`RlvOutcome::FailedDisabled`**, checked in `RlvState::apply` ahead of
everything else, in the reference's position. It is reported on the console as
`(turned off in your settings)`: a keyword the *user* turned off must not read
as one this viewer never had.

**A blocked command tells `@notify` nothing.** Every other refusal is announced
— the reference reports a failed command just as it reports one that took — but
this one returns *above* its notify hook (`rlvhandler.cpp:472` against `:585`),
and that is right: a keyword the viewer does not speak says nothing about what
the wearer is holding.

**Blocking releases what is already held.** The reference reads the setting once
while building its dictionary, so there it needs a relog and there can never be
a held `@setenv` to reconcile; here it is live, like the master switch and the
RLVa strings. Without the release the setting would only bite the objects that
had not asked yet, and the collar that got in first would keep the sky. The
lifting goes through the same `=y` path an object's own would, so the counts,
exceptions and modifier slots come down identically — and it is silent, because
no object did it.

**The bridge is `apply_blocked_behaviours`** in the intake plugin, beside the
master-switch watcher and ahead of the intake, so a command arriving in the
frame the user blocked its keyword is already refused. `observe_master_switch`'s
three-way look (initial / unchanged / moved) is now `observe_flag` underneath,
shared by both: the setting the user logged in with is applied silently, and
only a real move writes the console line.

**`RlvSession::release_all` carries the blocked set across.** Switching RLV off
resets the state machine, but the user's blocked keywords are not the state
machine's to reset — without this, off-and-on-again would quietly hand `@setenv`
back to the next device.

The setting needed no new UI: the reference exposes it as a debug setting only
(no menu entry), and the debug-settings editor already lists it.

## Verified

`cargo test --release` clean over `sl-rlv` (265), `sl-viewer-rlv` (57) and
`sl-viewer-world-api` (36); `cargo clippy --release --all-targets` clean over
the three.

Twelve new tests. In the engine: that both `=n` and `=y` are refused (the flag
is on the ADDREM row); that blocking releases a held `@setenv=n` and leaves the
same object's other restrictions alone, forgetting an object that held nothing
else; that neither the release nor the refusal reaches `@notify`; that
unblocking gives the keyword back; that `@setenv_ambient=force` still reaches
the consumer that carries it out; that a row the dictionary does not declare
blocks nothing; and that `@getcommand` lists a blocked keyword anyway. Over the
bridge, in a bare app with that one system: that the setting the user logged in
with is applied, that turning it on mid-session releases the collar's hold,
bumps the revision the floaters watch and writes the console line, that turning
it off gives the keyword back, and that the blocked set survives `release_all`.

Not verified live: no grid session drove it. The interactive check is the RLVa
console — turn `RestrainedLoveNoSetEnv` on in the debug-settings editor, type
`@setenv=n`, and read the report line.
