---
id: test-e2e-sweep-rlv
title: End-to-end tests for the RLVa console and windows
topic: test
status: done
origin: test-e2e-live-verify-sweep (2026-09-30)
points: 5
refs: [test-e2e-live-verify-sweep, viewer-rlv-receive-side-consumers,
  viewer-rlv-send-side-consumers]
---

Context: [context/automation.md](../context/automation.md).

The RLVa console runs commands typed by hand exactly as a worn object's
`@` commands would, so these are single-viewer fake-grid tests: fill the
console, press Enter, read the reply line and the state it changed.

- [[viewer-rlv-environment-commands]]: `@getenv_ambient=2222`, then
  `@setenv_ambient:1/0/0=force` turns the sky red, then Use Shared
  Environment restores it.
- [[viewer-rlv-debug-settings-commands]]: `@getdebug_restrainedlovenosetenv=2222`,
  then `@setrot:0=force` turns the avatar.
- [[viewer-rlv-blocked-behaviours]]: with `RestrainedLoveNoSetEnv` on,
  `@setenv=n` reports as blocked.
- [[viewer-rlva-floaters-toggles]]: the content of the four RLVa windows
  and the menu greying under restrictions.

The same commands from a worn scripted object on a real grid are
[[test-e2e-sweep-live-grid]]'s.

## Done

`sl-client-bevy-viewer/tests/e2e_rlv.rs`, four stage tests on the fake grid,
both backends, driven through the RLVa console and nothing else:

- **The menu and the windows**: every RLVa entry below the master switch is
  greyed until the switch is on (and the switch ticks); the console reports
  each command of a line on its own line, `INFO:` or `ERR: … (unknown
  command)`; the Restrictions window lists `fly`, `remattach:chest` and
  `addoutfit:gloves` with its count, the Locks window the attachment-point and
  wearable-layer locks they make with its count; and closing the console lifts
  everything it held.
- **Strings**: an edit is kept across picks and Restore default puts the
  reference's wording back.
- **Environment and heading**: `@getenv_ambient`, a red `@setenv_ambient`
  read back both ways, Use Shared Environment taking it away, and `@setrot`
  turning the avatar north and then east.
- **`@setenv=n` and `RestrainedLoveNoSetEnv`**: the held restriction greys
  Use Shared Environment; the setting turned on in the debug-settings editor
  releases it, says so and gives the menu back; the next `@setenv=n` reports
  `(turned off in your settings)`; `@getdebug_*` reads it; `@setenv_ambient`
  still applies.

Two state readouts were added for it, since neither a sky nor a heading has a
widget to read: `Probe::Environment` (`EnvironmentReadout`: the drawn sky's
name and ambient, and whether the local sky stands in for the shared one, from
`RlvEnvironmentSlot`) with the driver's `environment()` and
`sl-viewer-ctl environment`, and `AgentReadout::heading`
(`AvatarControls::held_heading`, also printed by `sl-viewer-ctl agent`).

The drive found three bugs, each fixed with a regression test that fails
without the fix:

- **The console showed every line one submit late.** `bind_console_rows`
  dresses a row the pool grew through its own commands, so the frame the text
  node lands the row's `VirtualRow` no longer reads as changed, and the line
  stayed blank until the next one was typed. It now also binds a row whose
  text holder just arrived (`a_logged_line_shows_without_waiting_for_the_next`).
  The other virtual lists dress rows in a separate system ordered before the
  binder, which Bevy's automatic sync point covers.
- **The Restrictions and Locks counts ignored plurals** ("1 restrictions, 0
  exceptions, from 1 objects", "1 locks in force"); both are Fluent selects
  now, and the test asserts the singular and the plural.
- **A parked pooled row kept its last item's name** in the semantic model:
  most binders skip a row with no index rather than blank it, so the
  debug-settings list filtered to `RestrainedLoveNoSetEnv` held two rows of
  that name and the locator was ambiguous. The model names a parked row
  nothing (`a_parked_pool_row_has_no_name`), which fixes it for every list at
  once and for AccessKit too.

Not testable yet, and filed: the Strings window's `blocked_recvim` texts are
shown nowhere, because no receive path asks the RLV façade at all — `@recvim`,
`@recvchat`, `@accepttp` and the rest are held and listed and change nothing
([[viewer-rlv-receive-side-consumers]], the twin of
[[viewer-rlv-send-side-consumers]]).
