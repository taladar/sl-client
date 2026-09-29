---
id: viewer-automation-remote-transport
title: Remote transport — automation over a private Unix socket
topic: viewer
status: done
origin: viewer automation design (2026-09-28)
points: 5
blocked_by: [viewer-automation-executor]
refs: [viewer-automation-driver, viewer-automation-ctl-cli]
---

Context: [context/automation.md](../context/automation.md).

## Done (2026-09-29)

- **Protocol** (`sl-automation-proto`): `hello` → `Hello { protocol,
  viewer: ViewerIdentity }` (`PROTOCOL_VERSION` 1; viewer, version, pid,
  grid, agent name, agent id once logged in); `subscribe { cursor?, streams }`
  → `Subscribed { cursor }`, then `Notification::Log { subscription, page }`
  lines; `unsubscribe { subscription }` → `Unsubscribed`. A line on the wire
  is a `ViewerMessage` (a `Response`, or a `Notification` tagged by
  `notification`); `Notification::Rejected` answers a line with no id.
- **Executor**: answers `hello` from an `AutomationIdentity` resource (the
  viewer's assembly inserts it) plus the agent probe; keeps the event-log
  subscriptions and queues their notifications in `AutomationQueue`
  (`take_notifications`), at most `NOTIFICATION_ENTRIES` a frame each.
- **Transport** (`sl-viewer-automation/src/remote.rs`): `RemoteEndpoint::open`
  binds the socket before the App exists (a failure is `Error::AutomationSocket`
  and stops start-up), in a private staging directory, chmod `0600`, then
  hard-linked into place; a stale socket (connection refused) is replaced, a
  live one or a non-socket is refused; the file is removed on drop if it is
  still ours. The listener runs on `sl_client_bevy::shared_runtime()`;
  `RemoteAutomationPlugin` moves lines between it and the queue in `Last`
  around the executor, renumbering requests from `REMOTE_ID_BASE` so two
  clients may share ids. A half-closed client still gets its answers; a
  departed one has its subscriptions ended.
- **Viewer**: `ViewerAppOptions::automation` is now `Automation { Off,
  InProcess, Socket(path) }`; `--automation-socket [PATH]` (bare = the
  default `$XDG_RUNTIME_DIR/sl-client-bevy-viewer/automation-<pid>.sock`),
  logged at start-up.
- **Tests**: the transport's teeth (`remote/tests.rs`: private file, stale /
  live / non-socket, hello, shared ids, subscription stream and its end, a
  departed client, bad lines, a duplicate id in flight, half-close); the
  full-stack `a_session_is_driven_over_the_automation_socket` (login wait,
  hello with the agent, a wait in flight answered after an `open_floater`);
  the CLI shapes of the switch.
- **Live** (2026-09-29): two `--headless` viewers against one fake grid, one
  with an explicit path and one with the default, each answered `hello` with
  its own agent, opened the inventory with a wait for it in flight, streamed a
  toolbar click's UI action, refused a bogus method; after `SIGTERM` both
  logged out and both socket files were gone.

Not here: a windowed viewer has no synthetic input, so over its socket the
requests that play input answer `Unavailable` (reads, waits, `open_floater`
and screenshots work).

The real binary is driven from outside its process: by `sl-e2e`, by the
CLI, by an agent. The transport carries `sl-automation-proto` requests to
the executor and responses and event streams back.

## Why not `bevy_remote`

It was the first candidate (it is in the Bevy fork, with custom and watching
methods) and was rejected on review (2026-09-28):

- `RemotePlugin::empty()` is private, so `Default` always registers the
  built-in world-writing methods (insert / remove / spawn / despawn) on an
  **unauthenticated TCP port** — any local process could rewrite the
  viewer's world;
- its HTTP server starts a second listener for the render sub-app on the
  **fixed port 15703**, so two viewer processes collide, and bind errors are
  lost in a detached task (`http.rs`), which hides an error;
- bound on port 0 it never reports the real port.

Each is fixable in the fork, but a small transport of our own is less work
than carrying those patches and has a better security story.

## Wanted

- `--automation-socket <path>` (runtime switch, off by default — **not** a
  Cargo feature): a Unix domain socket, created mode `0600`, default under
  `$XDG_RUNTIME_DIR`; an existing socket file is an error unless stale.
- Line-delimited JSON-RPC carrying `Request` / `Response`; subscriptions
  for the event logs stream notifications on the same connection.
- The listener runs on the shared async runtime and feeds the executor's
  queue over a channel; responses are written as the executor answers, so
  several requests may be in flight.
- A `hello` returning the protocol version and viewer identity (agent,
  grid, pid).
- Bind or accept failures are logged as errors and fail start-up when the
  switch was given — never silently.

Acceptance: a launched viewer answers `hello` on its socket; a request opens
a floater and the response arrives when done; two viewer processes run side
by side with their own sockets; no socket exists without the switch.
