---
id: viewer-automation-ctl-cli
title: sl-viewer-ctl — drive a running viewer from the shell
topic: viewer
status: ready
origin: viewer automation design (2026-09-28)
points: 5
blocked_by: [viewer-automation-driver]
refs: [test-e2e-stage, viewer-automation-mcp-server]
---

Context: [context/automation.md](../context/automation.md).

The same driver, interactively: for a developer poking at a live viewer,
and for an agent that would otherwise ask the user to "log in and check".

## Wanted

- `sl-viewer-ctl launch` (a viewer with automation, headless or `--watch`,
  against a grid), `stage` (a fake grid plus viewers from a small TOML),
  and `attach <socket>`.
- Verbs: `tree [<selector>]`, `find`, `click`, `fill`, `press`, `wait`,
  `world find`, `world touch`, `chat`, `notifications`, `agent`,
  `screenshot <png>`, `events --follow`.
- The **string selector grammar** lands here in `sl-automation-proto`
  (parse and print, round-trip tested), e.g.
  `window[test_id=build] >> button[name_key=build-apply]`, and failure
  messages print locators in it.
- Human output by default, `--json` for scripts.

Acceptance: from a shell, launch a headless viewer on the fake grid, open
the Build floater by locator, read a disabled button's state and save a
screenshot; the grammar round-trips every locator shape.
