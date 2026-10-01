---
id: test-e2e-sweep-rlv
title: End-to-end tests for the RLVa console and windows
topic: test
status: ready
origin: test-e2e-live-verify-sweep (2026-09-30)
points: 5
refs: [test-e2e-live-verify-sweep]
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
