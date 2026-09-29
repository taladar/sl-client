---
id: viewer-automation-inprocess-transport
title: In-process transport — the same requests against Apps in the test process
topic: viewer
status: done
origin: viewer automation design (2026-09-28)
points: 5
blocked_by: [viewer-automation-executor, viewer-automation-per-app-state,
  viewer-automation-windowless-mode]
refs: [viewer-automation-driver]
---

Context: [context/automation.md](../context/automation.md).

## Done (2026-09-29)

- **Transport** (`sl-viewer-automation/src/in_process.rs`):
  `InProcessTransport<A: HostedApp>` hosts any number of viewer Apps
  (`host(label, app)` → `ViewerHandle`; an App without the executor is
  refused), puts each `Request` straight into the viewer's `AutomationQueue`
  (`send`), and while a caller waits (`receive`, `response(id)`, `request`)
  steps every live App a frame per round with `FRAME_PAUSE` (2 ms) between
  rounds — a wait on one viewer never pauses the others, whose answers wait
  in their inboxes. Answers come back as the same `ViewerMessage`s the socket
  carries, under the caller's ids. A wait fails with `Exited` once its viewer
  exits and with `NoAnswer` after the transport's patience (5 min default,
  `with_patience`), never hangs. A dropped transport steps each App until no
  pipeline is compiling (`PipelineStatus`).
- **`HostedApp`**: a plain `App` steps with `update`; the viewer's
  `ViewerApp` implements it to step inside its `viewer{name}` span.
- **`Relay`** (`sl-viewer-automation/src/relay.rs`): the client bookkeeping
  both transports now share — renumbering from a base
  (`IN_PROCESS_ID_BASE` = 2⁴⁷, below `REMOTE_ID_BASE`), refusing a duplicate
  id in flight, subscriptions renamed to the client's id and ended with it.
  Delivery now follows the order the executor answered in
  (`AutomationQueue::take_responses`); before, two answers of one frame
  reached a socket client in hash-map order.
- **Tests**: the transport's teeth (`in_process/tests.rs`: two Apps under the
  same ids, a wait on one stepping the other, answers by id leaving the rest in
  order, a duplicate refused, a subscription under the caller's id, an exited
  viewer, the patience, an App without the executor); the executor's acceptance
  split into request sequences (`wait_for_login`, `drive_the_inventory`) run
  unchanged through the harness queue and through the transport
  (`the_executor_acceptance_passes_through_the_in_process_transport`); and
  `two_viewers_on_one_grid_answer_through_one_transport` — two headless viewers
  log into one fake grid as two accounts, both logins waited for at once under
  one id, each hello naming its own agent, a wait in flight on one untouched by
  the other opening its inventory, then holding once the first opens its own,
  and both logged out by their own flags.

Not here: an async face. A Bevy `App` is not `Send`, so the transport steps
its Apps on the caller's thread; the driver ([[viewer-automation-driver]])
decides whether its in-process `Transport` runs one on a stepping thread
(Apps built there) or steps inside its futures.

The second backend: viewers as Apps inside the test process, built by
`ViewerAppBuilder` windowless, driven by the same `Request`s. Faster to
start, debuggable in one process, and runnable under a paused clock.

## Wanted

- A transport that owns one or more Apps and steps them (round-robin, with
  the `FRAME_PAUSE` courtesy the full-stack tier uses so network threads
  progress) while requests are outstanding.
- Same `Request` / `Response` values as the remote transport, handed to the
  executor directly.
- Several viewers in one process, each its own App with its own login.

Starting point: `per_app_test` (`sl-client-bevy-viewer`) already builds two
Apps with `Storage::Directories(ViewerPaths::under(..))`, a `log_label` each
and `TerminationFlag::own()`, and steps them round-robin with
`ViewerApp::update` (which runs inside the viewer's log span) — the shape
this transport owns.

Acceptance: the executor's end-to-end test passes unchanged through this
transport; two Apps log into one fake grid and both answer requests.
