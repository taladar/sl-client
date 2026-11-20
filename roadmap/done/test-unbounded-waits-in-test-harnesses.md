---
id: test-unbounded-waits-in-test-harnesses
title: Test harnesses still hold waits that nothing bounds
topic: test
status: done
origin: gridspec-object-rez-derez (2026-10-10) — a fake-grid test sat for two hours
refs: [gridspec-object-rez-derez]
---

A fake-grid test waited for a `DeRezAck` the grid had stopped sending, and
sat for two hours: its wait put a timeout on each event, and a grid that
keeps sending pings never reaches it. That was fixed where it was found —
every such loop in `sl-fake-grid/tests/` now has one deadline for the whole
wait, and `.config/nextest.toml` ends any test of the default profile after
half an hour — together with `FakeGridHarness::control_for`
(`sl-conformance`), `perform` (`sl-viewer-testkit`) and the blocking
receives of `sl-lsl-lsp/tests/server.rs`.

An audit of the rest of the workspace's test code the same day named these,
none of them read closely or changed. The default profile's kill covers
them; the `live` profile's end-to-end tests are never killed, on purpose,
so there a wait without a bound is a run somebody has to notice.

## The end-to-end stage and the in-process host

- `sl-e2e/src/stage.rs`, `StageBuilder::run_on`: `block_on(body)` has no
  deadline of its own, nor have `Stage::start` and `shutdown` around it.
- `Stage::shutdown`: `spawn_blocking(|| host.stop())` joins every viewer
  thread without a timeout, also right after the `LOGOUT` timeout said a
  viewer would not exit.
- `ask_to_log_out` and `start_in_process`: `host.with_app(..)` and
  `host.host(..)` are bare oneshots a viewer thread stuck in a frame or a
  build never answers (`sl-viewer-automation/src/in_process_host.rs`,
  `host`, `with_app`; `exited` is wrapped by its callers). The same in
  `sl-client-bevy-viewer/src/automation_driver.rs`, `log_out`.
- `start_in_process`: the second-factor loop hosts a new App on every
  challenge with no cap on restarts, where the process path has
  `LOGIN_RESTARTS`; and `acquire_mfa` runs the MFA command with no timeout.

A deadline here is a decision about the harness — what a closure given to
`with_app` may take, what a stage does with a viewer that will not stop —
which is why it is not a line added in passing.

## End-to-end bodies

- `e2e_login.rs`, `log_in_from_elsewhere`: the drain and `run.await` after
  the logout have no timeout, and the diagnostics channel has a capacity of
  one with its receiver kept and never read, so a second diagnostic blocks
  the client's run loop.
- `e2e_two_avatars.rs`, the typing test: a loop of `grid_hears`, each with
  its own `WAIT`, that a repeated animation event restarts.
- `e2e_two_avatars.rs`, `accept_every_offer`: the outer loop has no
  deadline, only the wait inside it.

## Conformance cases' drains

Loops that read until a stream has been quiet for a while, with no window
for the whole of it; `object_asset_format.rs` documents the trap and has a
`SETTLE_WINDOW`, these have not. Offline they stop at the case's
fifteen-minute limit.

- `asset_round_trip.rs`, `rez_container`
- `object_asset_format.rs`, the folder walk's `InventoryBulkUpdate` drain
- `estate_info.rs` and `estate_access.rs`, `drain_access_lists`
- `parcel_access_list.rs`, the list read
- `teleport_trace.rs`, `drain_map_blocks`
- `fake.rs`, `run_case`: the grid's start, the logins and the logout loop
  run outside `run_isolated`

## Smaller

- `sl-viewer-automation/src/in_process_host/tests.rs` and
  `sl-viewer-driver/src/tests.rs`: bare awaits on a scripted task.
- `sl-viewer-automation/src/remote/tests.rs`, the subscription test: a
  loop capped a call at a time.
- `sl-client-tokio/src/http.rs`, the three `get_llsd` tests: a `reqwest`
  client with no timeout.
- `sl-lsl/tests/differential.rs` and
  `sl-lsl-runtime/tests/compile_corpus.rs`: the external `tailslide` is run
  with no timeout.
- `sl-fake-grid/src/timeline.rs`, `an_event_wait_has_no_deadline_of_its_own`:
  a bare join under a paused clock.

## A drop's source is aimed at too early

Not a wait, but found the same day and in the same harness: a
`drop_from` resolves where its source row is, then frames the ground and
verifies the pick there over some frames, then presses at the point it
resolved first (`sl-viewer-automation/src/executor/world.rs`, `GroundAct`
and `WorldAct`). A list that re-lays itself out in between — the inventory
window narrows to a search 0.15 s after it is typed — leaves the press on
nothing, no drag begins, and the action reports success. `e2e_objects`
searches its item away before it searches for it, so that the row can only
show once the list has narrowed; the gesture should resolve its source
last, or check that a drag began.

## Resolution (2026-11-20)

**The in-process host** (`sl-viewer-automation/src/in_process_host.rs`) bounds
every wait on a viewer's thread itself, so the stage, the viewer's own
`automation_driver` test and every other caller get it: `host` waits
`BUILD_TIMEOUT` (300 s) for the build, `with_app` `TASK_TIMEOUT` (60 s) for its
closure, and `stop` (and the host's drop) `STOP_TIMEOUT` (180 s) for every
thread. An overrun is `HostError::TimedOut` / `HostError::NotStopped` naming
the viewer; the thread is left detached rather than waited for. Tests cover a
build that never finishes, a task on a viewer stuck in a frame, and a stop
that leaves exactly the stuck thread behind.

**The stage** (`sl-e2e/src/stage.rs`): a body still running after 30 minutes
is cancelled and fails as a body failure, then the stage is taken down as
usual — the `live` profile's only limit on a body. The end of a backend's run
shuts its runtime down with a 10 s timeout, so a blocking task left running no
longer holds the runtime's drop. The wait for the exit after a closed link
during a login is bounded by `LOGOUT`. `Stage::start`'s other steps were
already bounded (driver requests by their deadline plus grace, logins by
`LOGIN`, sockets by `CONNECT`).

**The second factor** moved inside the App since the audit: the viewer
answers each challenge itself and `LOGIN` bounds the stage's wait, so there is
no restart loop left to cap. The MFA command itself is bounded in
`sl_repl::acquire_mfa_token`: it is killed after two minutes
(`AuthError::MfaTimedOut`), its output not read then, since a process it
started may hold the pipe.

**End-to-end bodies**: `log_in_from_elsewhere` starts the second session
through `sl_fake_grid::run_client` (unbounded forwarders, nothing to drain) and
waits for its run loop at most `WAIT`; the typing test's start-and-animation
loop and `accept_every_offer` each have one deadline for the whole loop.

**Conformance drains**: `support::DrainWindow` caps each wait of a
read-until-quiet drain to what is left of an overall window and ends the
drain when it is spent. It is used by every drain named above (`rez_container`
10 s, the AIS3 folder fetch 20 s, both `drain_access_lists` 30 s / 60 s, the
parcel list read 30 s, `drain_map_blocks` 60 s), by the two `drain_terrain`s of
`modify_land` and `terrain_raw_upload` the audit had not named (30 s), and by
the three settle loops that already had a window of their own
(`drain_scene`, `object_asset_format::settle_scene`). `Session::logout` now
waits for its run loop at most 30 s after closing its commands and aborts it
then, and `fake::run_case` bounds the grid's start and the logins, which run
outside the case's own timeout, at 300 s.

**Smaller**: the host's and the driver's tests wait on their scripted tasks,
links and streams with a deadline; the remote subscription test counts its
pages; the `get_llsd` tests' client has a 30 s request timeout, except the
paused-clock one, where that timer would be the next one due whenever the
runtime waits on the socket and every request would time out (its bound is
the retry budget it asserts); both tailslide oracles kill a `--lint` run after
60 s; the timeline test's join is bounded, which on the paused clock fails at
once rather than holding.

**The drop's source**: a `drop_from` on an object or the ground finds its
source row again once the target is aimed at, just before the press, and
drags from where the row is then. With the inventory list's busy marker during
its search debounce, that holds a drop until the list has narrowed. Both
`e2e_objects` workarounds that searched an item away before searching for it
are gone.

Verified: the new host, MFA, driver and remote tests and all 402
`sl-conformance` tests pass. `e2e_objects`' take-and-rez on both fake
flavours, with its workaround gone, passed once unloaded and once
throttled to 15% CPU. On the local OpenSim, `a_worn_prim_follows_the_avatar`
passed its whole body in process. The process backend then failed earlier,
at the world right-click on the freshly rezzed prim: a phantom prim another
test account had left at the shared build location, behind a bush, took the
pick. That is grid debris, before any of the code changed here.
