---
id: permission-b4
title: The query command, snapshot type, reply event and runtime wiring
topic: permission
status: done
origin: PERMISSION_ROADMAP.md
---

Context: [context/permission.md](../context/permission.md).

**B4 (from A7). The query command, snapshot type, reply event and runtime
    wiring.** The mirror's read-out path; the new `Command` and `Event` force
    the runtime arms (findings 1–2), so the sl-proto types and the
    wiring land together. **Done 2026-06-26** — added the public
    `ScriptPermissionState` struct (`types/script.rs`, fields
    `grants: Vec<ScriptGrantInfo>` / `controls: ScriptControlsInfo`,
    re-exported via `types.rs` / `lib.rs`), the unit
    `Command::QueryScriptPermissions` (`command.rs`), the synthesized
    `Event::ScriptPermissionState(ScriptPermissionState)` variant
    (`types/event.rs`, documented as the crate's first locally-built reply
    event), and the `Session::script_permission_state()` accessor collecting
    `script_grants()` + `script_controls()`. Wired all three runtimes at
    parity: the tokio command arm pushes the snapshot onto the events
    `mpsc::Sender`; the bevy `advance_running` arm writes
    `SlEvent(Event::ScriptPermissionState(…))`; `format.rs` gained the
    `event_name` + `command_name` arms (`format_event` renders via the generic
    `Debug` path, so no bespoke arm needed) and the REPL `CommandSpec`
    `query_script_permissions` (no args). The compiler-forced exhaustive
    matches were updated: the `sl-survey` `handle_event` ignore arm and the
    three login/survey examples. One `lifecycle.rs` test
    (`script_permission_state_bundles_grants_and_controls`) records a grant +
    takes a control and asserts the snapshot reflects both stores and agrees
    with the individual accessors. `SessionContext::apply_event` caching was
    left out (the plan marked it optional; it has a `_` arm). Builds,
    clippy-clean (restriction lints), `cargo test --workspace` green, fmt
    clean. No deviations from the planned scope.
    - **sl-proto.** Add the public `ScriptPermissionState` struct (fields
    `grants: Vec<ScriptGrantInfo>` and `controls: ScriptControlsInfo`, as in
    the § API-surface & exposure reference; `ScriptGrantInfo` already carries
    the B2.5 denied status, so the snapshot conveys denials too); the
    `Command::QueryScriptPermissions` **unit** variant (`command.rs`, modelled
    on `ReleaseScriptControls`); the
    `Event::ScriptPermissionState(ScriptPermissionState)` variant
    (`types/event.rs`), documented as a locally-*synthesized* query reply, not
    a wire event (the crate's first such `Event`); and the
    `Session::script_permission_state() -> ScriptPermissionState` accessor
    collecting `script_grants()` + `script_controls()`. **No `Session::poll`
    change** — the event is emitted by the runtimes, not the session.
    - **Runtimes** (all three at parity): `sl-client-tokio/src/lib.rs` — a
    `Command::QueryScriptPermissions` arm pushes
    `session.script_permission_state()` onto the events `mpsc::Sender` (its
    `Command::RevokePermissions` arm is already added in B2);
    `sl-client-bevy`'s `drive` match — the same arm, writing
    `SlEvent(Event::ScriptPermissionState(session.script_permission_state()))`
    (compiler-forced). For the new `Event`, add arms in
    `sl-repl/src/format.rs::event_name` (the exhaustive `const fn` —
    compiler-forced) and `format_event` (print grants + controls), and
    ignore arm in `sl-survey/src/bin/sl-survey.rs::handle_event` (exhaustive
    — compiler-forced). Add the REPL `CommandSpec` `query_script_permissions`
    (no args). Optionally cache the snapshot in `SessionContext::apply_event`
    (it has a `_` arm, so this is optional).
    Depends on B2 (`ScriptGrantInfo` / `script_grants`) and B3
    (`ScriptControlsInfo` / `script_controls`). Test: build a `Session` with a
    recorded grant + a taken control, call `script_permission_state()`, assert
    both stores are reflected. Smoke in the REPL: `revoke_permissions`
    then `query_script_permissions`, confirm the printed snapshot reflects the
    change (test-avatar setup).
