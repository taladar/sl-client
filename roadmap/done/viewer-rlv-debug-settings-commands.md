---
id: viewer-rlv-debug-settings-commands
title: RLV — @setdebug_*/@getdebug_* allowlist and @setrot
topic: viewer
status: done
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-preferences-debug-settings-editor]
blocked_by: [viewer-rlv-restriction-state]
---

Context: [context/viewer.md](../context/viewer.md).

The RlvExtGetSet extension family (`rlvextensions.cpp`):
`@setdebug_<setting>:<value>=force` and `@getdebug_<setting>=<channel>`
expose a small allowlisted set of debug settings to scripts — AvatarSex
(read/write, pseudo-setting), AspectRatio (read, pseudo-setting),
RenderResolutionDivisor (read/write), plus read-only
RestrainedLoveForbidGiveToRLV, RestrainedLoveNoSetEnv and
WindLightUseAtmosShaders — while the `@setdebug=n` gate locks user edits
of those settings. The same module implements `@setrot:<radians>=force`,
which rotates the avatar to face a heading.

Like the environment family ([[viewer-rlv-environment-commands]]), these
are extension-prefix commands outside the behaviour dictionary: Firestorm
routes them through the `RlvExtCommandHandler` fallback, and our parser
yields `RlvBehaviour::Unknown` with the raw keyword kept. We parse the
`setdebug` gate itself but have no prefix recognition, no allowlist, no
application to our settings store, and no `@setrot`. Scope: prefix
dispatch on Unknown keywords, the exact allowlist plus the pseudo-setting
semantics, wiring reads/writes into the debug-settings registry built by
[[viewer-preferences-debug-settings-editor]], replying on the requested
channel, and the forced-rotation action.

Reference (Firestorm, read-only): `indra/newview/rlvextensions.cpp`,
`indra/newview/rlvextensions.h`.

## Done

A **sixth layer** in `sl-rlv`: `extension.rs`, the commands the behaviour
dictionary never claimed. `RlvExtCommand::classify` is the reference's whole
dispatch condition — only a `RlvBehaviour::Unknown` keyword can be one, split
on its *first* underscore, with the param kind part of the match rather than a
later check, so `@getdebug_x=force` is not a malformed read but no command at
all. `RlvState::run_extension` is where the consumer sends a command
`RlvState::apply` handed back as `NotAStateChange`, which is exactly where the
reference falls through to its `RlvExtCommandHandler` chain.

The allowlist is a table of six rows carrying name, value kind and the three
`DBG_*` flags. `RlvDebugSetting` is deliberately an **exhaustive** enum —
unusually for this crate — because a consumer has to say what every row reads
as, and a row added later should break it into saying so rather than silently
answering nothing. What the crate cannot know it asks an `RlvExtSource` for;
the parsing (C++'s prefix-stopping `operator>>`, its closed list of boolean
words), the formatting (`%d` / `%u` / `%.3f`) and every rule about who may read
or write are here and unit-tested.

Three reference quirks are kept, each pinned by a test: the pseudo `AvatarSex`
write is a lie a script tells itself (stored verbatim in the state machine,
never reaching a settings store, read back as the spelling that went in);
`@setrot` is registered for the reply dispatch too and checks neither, so
`@setrot:1.5=2222` really does turn the avatar and answers nothing; and a read
of a setting this viewer does not have still *succeeds*, with an empty answer,
because a script that asked a question must not be left waiting.

Viewer side, three seams:

- **`ViewerRlvExt`** (`sl-viewer-world-api::rlv`) is the source. The two
  `RestrainedLove*` rows are ordinary settings; `WindLightUseAtmosShaders`
  answers `1` because this viewer always renders the atmospheric sky, which is
  what a script asking the question wants to know. The two pseudo rows read
  `RlvExtFacts`, a resource each fact's **owner** publishes: the camera
  publishes the view's aspect ratio, the avatar layer publishes the worn
  Shape's sex. The sex rule itself moved into `bake_inputs::shape_is_male` and
  the appearance editor now calls it, so one avatar cannot be two sexes because
  two call sites read the `male` param differently.
- **`@setrot`** lands on `AvatarControls::forced_heading`, taken by the
  movement driver on its next frame: it replaces the tracked heading (so it
  wins over the mouselook aim) and is advertised at once instead of waiting out
  the turning throttle.
- **The `@setdebug=n` gate** drops the writable allowlisted rows from the
  debug-settings editor's list for as long as the restriction holds, and the
  restriction revision is now an input to that view exactly as the search term
  is. Hiding rather than greying is the reference's choice and the honest one:
  a greyed row would show a value the user cannot change and the object can.

The RLVa console is the one place today that speaks the family end to end: it
offers a command to the extension handlers after the state machine hands it
back, shows a `@getdebug_*` answer on the reply stream with the channel it
would have been shouted on, and says so when the channel is one no reply may
go on.

## Not done — and why

- **`RenderResolutionDivisor` reads and writes nothing.** It is the one
  allowlisted row this viewer has no setting for, because it has no
  reduced-resolution render path at all — and a setting registered for RLV to
  write that nothing looks at would be a lie in the settings editor. The source
  answers `None`, so a read is empty and a write is a bad option, which is
  precisely what the reference does when `getControl` finds nothing. The render
  path it needs is now its own task, [[viewer-render-resolution-divisor]].
- **The reply is not chatted.** `run_extension` builds the `RlvReply` a script
  would hear, truncated and channel-checked, but nothing shouts it — the same
  gap [[viewer-rlv-queries]] and [[viewer-rlv-notify]] left, and for the same
  reason: no owner-say command intake exists yet, so the console is the only
  issuer and it shows the answer instead. That door is now a task of its own,
  [[viewer-rlv-command-intake]], because it is one seam for all three
  chat-back producers rather than a loose end of each.
- **`DBG_PERSIST` is not modelled.** The reference caches per row whether the
  setting persists, so a script-written value is never saved to disk. There is
  no row here a script can write *and* that this viewer stores, so the flag
  would protect nothing; the rule it encodes belongs at the write site when one
  appears.
- **`@setrot` while seated or in flycam does nothing**, because the movement
  driver already refuses to advertise a body rotation in both (the vehicle owns
  the avatar's orientation; flycam parks the body). The reference resets the
  agent's axes regardless and lets the simulator fight it.

## Verified

`cargo test --release` over every touched crate — `sl-rlv` (232 + 11
doctests), `sl-viewer-rlv`, `sl-viewer-world-api`, `sl-viewer-preferences`,
`sl-viewer-world-view`, `sl-viewer-world-avatar`, `sl-viewer-asset-editors` and
`sl-client-bevy-viewer` (294) — all green; `cargo clippy --release
--all-targets` clean over the same set.

The new tests pin the dispatch (head *and* param kind), the case-insensitive
allowlist, each row's format, the empty-but-successful read, the unusable
reply channel, the read-only and unknown-row refusals, the typed write, the
verbatim pseudo write, the `@setdebug` holder gate both ways, the `@setrot`
offset and its option failure, and — in the editor — that a held `@setdebug`
drops the writable rows from the list and gives them back when it lifts.

One bug the tests caught rather than the eye: an extension command was bumping
the restriction revision, so a `@getdebug_*` read or a `@setrot` would have
rebuilt the Restrictions and Locks floaters for a command that changes no
restriction. Only what `RlvState::apply` accepted counts now.

Not verified live: no grid session drove the family. The console path — type
`@getdebug_restrainedlovenosetenv=2222` at the RLVa console and read the
answer, then `@setrot:0=force` and watch the avatar turn — is the interactive
check to make.
