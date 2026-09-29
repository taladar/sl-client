---
id: viewer-automation-driver
title: sl-viewer-driver — the async test API over both transports
topic: viewer
status: ready
origin: viewer automation design (2026-09-28)
points: 8
blocked_by: [viewer-automation-remote-transport, viewer-automation-inprocess-transport]
refs: [test-e2e-stage, viewer-automation-ctl-cli]
---

Context: [context/automation.md](../context/automation.md).

What a test author writes against. It must read like the intent ("click
Apply in the Build window, expect the other viewer to see the prim") and
hide which transport is underneath.

## Wanted

A `sl-viewer-driver` crate (async, tokio):

- `Viewer` handle over either transport (a `Transport` trait with the
  remote socket and the in-process App as its two implementations), with
  the identity `hello` reports.
- Locator handles built fluently: `viewer.ui().window("build").button_key(
  "build-apply")`, with `click`, `double_click`, `right_click`, `hover`,
  `fill`, `press`, `check` / `uncheck`, `select_option`, `drag_to`, `text`,
  `value`, `is_disabled`, `is_checked`, `count`, `all`.
- World handles: `viewer.world().object_named("Door")`, `.avatar(..)`,
  `.me()`, with `touch`, `open_pie`, `select`, `sit`, and readouts.
- Probes: chat / IM transcripts, notifications, agent state, event cursors,
  screenshot.
- `expect(locator).to_be_disabled()` / `.to_have_text(..)` /
  `.to_be_visible()` / `expect(chat).to_contain(..)` — each an in-viewer
  wait with a default timeout, overridable per call.
- **Failure artifacts**: on any failed action or expectation, save a
  screenshot, the semantic tree around the scope and the event tail into
  the test's artifact directory, and put their paths in the error.

New crate: expect the extraction gates (see the new-crate memory); consider
whether `sl-automation-proto` should simply be a module of this crate plus
the viewer's, if the gates make two crates costly.

Acceptance: the same test body passes through both transports against one
fake grid; a deliberately failing expectation leaves the three artifacts
and names them in its message.
