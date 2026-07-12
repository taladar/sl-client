---
id: permission-a7
title: Specify the API-surface delta & driver/REPL exposure
topic: permission
status: done
origin: PERMISSION_ROADMAP.md
---

Context: [context/permission.md](../context/permission.md).

**A7. Specify the API-surface delta & driver/REPL exposure.** Enumerate
the new/changed `Command`s (`RevokePermissions`, an optional grant
convenience), any `Event` changes (inbound likely unchanged), and the new
`Session` accessors; and how `sl-client-tokio`, `sl-client-bevy`, and the REPL
expose the commands and a way to query the granted state, at feature parity.
Draw the boundary: what is sl-proto `Session` state versus what stays
application policy. **Done — see § API-surface & exposure reference
(from A7) + task B4 in § Phase B.** Decided: two new `Command`s
(`RevokePermissions` from A3, and a new unit `QueryScriptPermissions`); the
*optional grant convenience* is **dropped** (a grant only ever *answers* a
pending request — `AnswerScriptPermissions` is that path). **Key discovery:**
no runtime can call a live `Session` accessor (`tokio`'s `run` loop owns the
`Session`; `bevy` boxes it privately in `SlState`; the REPL only caches event
bindings), and a `Command` cannot carry a `oneshot` reply (it is `Clone` /
REPL-parsed), so read-only state can reach an application **only as an
`Event`**. Therefore the grant mirror is exposed by a **query command answered
by a reply event** (`QueryScriptPermissions` → a new *synthesized*
`Event::ScriptPermissionState(ScriptPermissionState)`, the crate's first
local-reply event), keeping all three runtimes at parity via the same
command-in / event-out path they already use. The **inbound** event surface is
unchanged (as predicted). New `Session` accessors:
`granted_permissions` / `script_grants` / `script_controls` (A3/A6) plus an
A7 `script_permission_state()` snapshot convenience.

## API-surface & exposure reference (from A7)

The complete public API delta the permission system adds, and how each of the
three runtimes (`sl-client-tokio`, `sl-client-bevy`, the REPL) surfaces it at
feature parity. The boundary: everything that *records, resets, and computes*
the mirror is sl-proto `Session` state; everything that *decides* (cooperate?
revoke? display?) is application/driver policy. The runtimes stay pure conduits
— command-in, event-out — and add no policy.

**The exposure constraint (discovered here, the crux of A7).** None of the three
runtimes can call a *live* `Session` accessor:

- `sl-client-tokio`: `Client::run` takes `self`, and the driver loop then
  **owns** the `Session` (`sl-client-tokio/src/lib.rs` — no `Arc<Mutex>`, no
  snapshot channel, no shared state). The pre-`run` accessors (`agent_id`,
  `session_id`, …) are unreachable once running.
- `sl-client-bevy`: the `Session` is boxed **privately** inside the `SlState`
  resource (`SlInner::Running { session: Box<Session>, … }`); there is no
  `Res<Session>` and no public read accessor on the resource.
- The REPL: `SessionContext` (`sl-repl/src/context.rs`) caches only bindings it
  scrapes from the event stream in `apply_event`; it never reads the `Session`.

So today read-only state reaches an application **only as an `Event`**. A
`Command` cannot carry a `oneshot` reply channel either — `Command` is
`Clone`/`Debug` and is *parsed* by the REPL (`registry.rs`), so it must stay a
plain data enum. **Therefore the only parity-preserving way to expose the grant
mirror is a query `Command` answered by a reply `Event` over the existing events
channel.** That is the design below; it adds no `Arc<Mutex>` to tokio and no
`Res<Session>` to bevy.

**New / changed `Command`s** (`sl-proto/src/command.rs`):

1. `RevokePermissions { object_id: ObjectKey, permissions: ScriptPermissions }`
   — the granular revoke (A3/B2), object-scoped; a struct variant modelled on
   `AnswerScriptPermissions`.
2. `QueryScriptPermissions` — a **unit** variant (modelled on
   `ReleaseScriptControls`) requesting a snapshot of the whole mirror.
   Fire-and-forget *to the session*; the reply rides back as
   `Event::ScriptPermissionState`.
3. **No grant-convenience command.** A1/A4 confirm a grant only ever *answers* a
   pending `llRequestPermissions`; `AnswerScriptPermissions` (gaining
   `experience_id`, B2) is that path. Nothing is granted out of band, so the
   "optional grant convenience" A7 floated is **dropped**.
4. `AnswerScriptPermissions` is unchanged *on the wire*, but its driver dispatch
   gains the `experience_id` argument plumbed from the `ScriptPermissionRequest`
   (B2) — a runtime-wiring change, not a new command.

**`Event` changes** (`sl-proto/src/types/event.rs`):

- **Inbound surface unchanged** (as A7/A3/A6 predicted):
  `ScriptPermissionRequest` (already carries `experience_id`),
  `ScriptControlChange`, `ScriptTeleport`, and the follow-cam events
  (`SetFollowCamProperties` / `ClearFollowCamProperties`) stay exactly as
  they are. No grant/deny/revoke recording emits an event (A3).
- **One new *synthesized* event:**
  `ScriptPermissionState(ScriptPermissionState)` — the reply to
  `QueryScriptPermissions`. It is **not** produced by `Session::poll` from the
  wire; each runtime's command dispatch builds it by reading the session and
  pushes it onto its event sink. This is the crate's **first** synthesized /
  local-reply event — note the precedent it sets (an `Event` that does not
  originate from an inbound message). `ScriptPermissionState` is a new public
  struct:

      #[derive(Debug, Clone)]
      pub struct ScriptPermissionState {
          pub grants: Vec<ScriptGrantInfo>,        // the A3/B2 view
          pub controls: ScriptControlsInfo,        // the A6/B3 view
      }

**New `Session` accessors** (`sl-proto/src/session/methods.rs`), all read-only,
following the `seat()` precedent (public signature, public return types, the
private internal state stays private):

- `granted_permissions(task_id, item_id) -> ScriptPermissions` (A3/B2) — one
  script's granted subset (empty when absent).
- `script_grants() -> impl Iterator<Item = ScriptGrantInfo>` (A3/B2) — every
  current grant.
- `script_controls() -> ScriptControlsInfo` (A6/B3) — the taken-controls union.
- `script_permission_state() -> ScriptPermissionState` (A7) — a convenience that
  collects the two stores into one snapshot (`grants` from `script_grants`,
  `controls` from `script_controls`); the value each runtime wraps in
  `Event::ScriptPermissionState` to answer the query.

**Runtime exposure, at parity** — three identical command-in / event-out shapes:

| Concern | sl-client-tokio | sl-client-bevy | REPL |
|---------|-----------------|----------------|------|
| Send `RevokePermissions` | `match` arm → `session.revoke_permissions(…)`, beside the `AnswerScriptPermissions` arm | `drive` `match` arm → `session.revoke_permissions(…)` | `CommandSpec { name: "revoke_permissions", usage: "<object_id> <permissions-i32>" }`, beside `release_script_controls` |
| Send `QueryScriptPermissions` | `match` arm → push `session.script_permission_state()` onto the events `mpsc` | `drive` `match` arm → `write(SlEvent(Event::ScriptPermissionState(session.script_permission_state())))` | `CommandSpec { name: "query_script_permissions", usage: "" }` |
| Receive the snapshot | the `ScriptPermissionState` event on the events `mpsc::Receiver` | `SlEvent(Event::ScriptPermissionState(..))` Bevy event | a `format_event` arm prints the grants + controls |

- Each runtime already forwards every sl-proto `Event` (tokio over
  `mpsc::Sender<Event>`; bevy as `SlEvent(SessionEvent)`; the REPL via
  `format_event`), so the snapshot reply travels the same path and needs
  **no new transport**. The per-variant wiring is **not** symmetric, though, so
  the B-tasks spell each touch-point out (consolidated findings 1–2): a new
  `Command` variant is a **compile error** in bevy (its `drive` match is
  exhaustive, no wildcard), is **silently swallowed** by tokio (its
  `Some(Command::Logout) | None` catch-all), and is invisible to the REPL
  (manual `CommandSpec` list) — so the tokio + REPL arms are parity-only, added
  by hand. A new `Event` variant likewise forces arms in
  `sl-repl/src/format.rs::event_name` (an exhaustive `const fn`) **and**
  `sl-survey/src/bin/sl-survey.rs::handle_event` (an exhaustive union), beside
  the REPL `format_event` body; tokio/bevy forward events without an exhaustive
  match, so they need no per-variant edit there.
- No runtime exposes a live accessor directly (the constraint above); they
  all go command-in / event-out, which keeps them at parity without
  bevy needing a `Res<Session>` or tokio an `Arc<Mutex<Session>>`.

**The boundary — sl-proto `Session` state vs. application policy.**

- **sl-proto `Session` owns:** the grant registry + the taken-controls tracker;
  all recording, revoke-mirroring and region-leave resets (B2) and
  inbound control folds (B3); the four accessors and the
  `script_permission_state` snapshot. It is the single source of mirror truth
  and stays sans-IO; the simulator remains authoritative (the mirror is a
  convenience, never a security boundary).
- **The application / driver owns:** every *decision* — whether to answer a
  request and with which bits, whether/when to `revoke_permissions` or
  `release_script_controls`, whether to cooperate with a `TAKE_CONTROLS` /
  camera grant (route the avatar's inputs, apply the follow-cam params), and
  when to `QueryScriptPermissions` and how to display the snapshot. The session
  **never auto-acts** (A4); the runtimes add **no** policy, only transport.
