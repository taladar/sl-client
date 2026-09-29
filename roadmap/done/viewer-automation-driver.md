---
id: viewer-automation-driver
title: sl-viewer-driver — the async test API over both transports
topic: viewer
status: done
origin: viewer automation design (2026-09-28)
points: 8
blocked_by: [viewer-automation-remote-transport, viewer-automation-inprocess-transport]
refs: [test-e2e-stage, viewer-automation-ctl-cli]
---

Context: [context/automation.md](../context/automation.md).

## Done (2026-09-29)

- **`sl-viewer-driver`** (new crate; protocol, tokio, serde_json, fs-err —
  no Bevy): `Viewer` over either transport. **The transport boundary is a
  channel pair, not a `Transport` trait** — requests out, `ViewerMessage`s
  in. `Viewer::connect(socket)` bridges the automation socket to one;
  `Viewer::over_link(requests, messages)` takes an in-process host's. The
  connection (`connection.rs`) numbers requests, routes answers by id and
  subscription pages by subscription, and fails what waits when the
  viewer's side closes (`Closed`) or stops answering (`NoAnswer` after the
  request's own timeout plus a 30 s grace). Every handle checks the
  protocol version in its hello and keeps the identity.
- **Locators** (`ui.rs`): `viewer.ui().window("build").button_key(..)`,
  `test_id`, `role`, `button`, `key`, `get`, `named`, `name_containing`,
  `nth`, `timeout`; `click`, `double_click`, `right_click`, `hover`, `fill`,
  `press` (click to focus, then keys), `check` / `uncheck` (click unless
  already so, then wait for the state), `select_option`, `drag_to`; reads
  `node`, `text`, `value`, `is_disabled`, `is_checked`, `is_visible`,
  `nodes`, `count`, `all`. A read waits for `Attached`, then reads through
  `Snapshot { within }`, so strictness is the viewer's own.
- **World handles** (`world.rs`): `object_named`, `avatar`, `me`,
  `locator`; `touch`, `open_pie`, `hover`, `select`, `sit` (the pie's
  *Sit Here*, or *Sit Down* for an avatar), `drop_from`, `without_reveal`;
  `node`, `nodes`, `count`.
- **Probes** (`viewer.rs`, `events.rs`): `agent`, `status`,
  `conversations`, `notifications`, `selection`, `inventory`, `quiescence`,
  `wait_until_quiet`, `wait_for_state`, `read_log`, `events` (an
  `EventCursor` with `read` and `wait_for(kind)`), `subscribe` (an
  `EventStream`, ended when dropped), `diagnostics`, `screenshot`,
  `snapshot`, `hello`, `press`, `menu_path`, `pie_slice`, `open_floater`.
- **Expectations** (`expect.rs`): `expect(&locator)` —
  `to_be_{attached,detached,visible,hidden,enabled,disabled,checked,
  unchecked}`, `to_have_text`, `to_contain_text`; `expect_world` —
  attached / detached; `expect_state(probe).at(pointer)` — `to_equal`,
  `to_include`, `to_be_present`, `to_be_absent`; `expect_chat().from(..)
  .to_contain(..)`; `expect_notification().to_contain(..)` / `to_show`.
  Each is one wait request, default 10 s, `.timeout(..)` per call; the
  deadline is wall-clock only (see context).
- **Failure artifacts** (`artifacts.rs`): with
  `ViewerOptions::with_artifacts(dir)`, a failed action or expectation saves
  `<dir>/<NNN>-<action>/{screenshot.png,tree.txt,events.txt}` and names each
  in `DriverError::Failed`'s message (or why it could not be saved).
- **Protocol additions** (`sl-automation-proto`, executor): `Click` takes
  `button` (`PointerButton::{Left,Right}`) and `double`; new `DragTo
  { source, target }` — both nodes pursued for actionability, then a
  left-button drag resting two frames on the target (the drop gesture now
  shared with the world's `DropFrom`).
- **`InProcessHost`** (`sl-viewer-automation/src/in_process_host.rs`): the
  in-process transport on its own thread, stepping its viewers
  continuously; `host(label, build)` → `(ViewerHandle, ViewerLink)`,
  `with_app`, `exited`, `stop`. `InProcessTransport` gained `take_messages`,
  `has_exited` and `label`.
- **Bug fixed on the way**: a world aim never landed on a live viewer whose
  camera follows the own avatar — its stability check demanded the eye hold
  within 0.1 mm while the follow camera drifts ~1 mm a frame, and every
  multi-frame GPU probe restarted. Stability is now the target's projected
  box corners within 5 % of the box's projected size, at least half a pixel
  (`Pose::of`, see context) — a fixed half pixel still failed under the
  commit hook's parallel load, where the head's idle sway is ~0.6 px a poll.
- **Tests**: driver teeth against a scripted viewer (`src/tests.rs`: answers
  out of order, closed viewer, grace, protocol mismatch, subscription,
  artifacts saved and named, none without a directory, the socket); the
  host's (`in_process_host/tests.rs`); the executor's right/double click and
  drag; the pose tolerance; and **the acceptance**
  (`sl-client-bevy-viewer/src/automation_driver.rs`): one fake grid, two
  headless viewers on one host — `Link` over its in-process link, `Socket`
  over its automation socket — each running the same async body side by
  side (login, quiet, toolbar click → inventory visible, the click in the
  event log, close → hidden, the stock box's pie opened by a pick-verified
  right click and shut by Escape), then an expectation on a missing button
  leaving its three artifacts, named in the message, on both.

Deviations from the wording above: no `Transport` trait (the channel pair
is the whole boundary, and a trait over it would be a forward-looking
abstraction); `sl-automation-proto` stays its own crate (the viewer needs
it without the driver's tokio client). The process backend is not in this
test: both viewers are Apps in the test process, one of them served over
its socket exactly as a viewer process serves it — launching the binary is
[[test-e2e-viewer-process-launch]].

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
