---
id: protocol-sim-script-messages
title: The simulator-side script messages SimSession neither sends nor decodes
topic: protocol
status: done
origin: LSL-on-the-fake-grid audit (2026-09-20)
points: 5
refs: [server-world-touch-and-grab, server-lsl-lib-comms,
  server-lsl-lib-money-permissions]
---

Context: [context/lsl.md](../context/lsl.md).

The server direction of the wire is in good shape — `SimSession` has
~130 `send_*` / `enqueue_*` methods and decodes most of what a client
says. The script-facing gaps are few and each is small, but every one of
them is load-bearing for [[server-script-engine]], and they are worth
closing as one protocol-topic task before the engine work starts, so the
engine is written against a complete surface.

**Not decoded** (they fall through to the catch-all
`ServerEvent::ClientMessage(Box<AnyMessage>)`, i.e. the raw message with
no typed event):

- `ObjectGrab`, `ObjectGrabUpdate`, `ObjectDeGrab` — a touch and a drag.
  Without these a script can never receive `touch_start` / `touch` /
  `touch_end`, which is the single most common way content is used. The
  surface data (`grab_offset`, and the `SurfaceInfo` blocks carrying UV,
  normal, binormal, position and **face index**) is what `llDetectedUV`,
  `llDetectedTouchFace` and friends read; `sl-proto`'s client side
  already models it (`types/editing.rs`).
- `ScriptDialogReply` — the answer to an `llDialog`. The client already
  sends it (`Session::reply_script_dialog`); the simulator side has
  nowhere for it to arrive.
- `MoneyTransferRequest` — paying an object, which is what raises the
  `money` event.
- `ScriptSensorRequest` (and its `ScriptSensorReply`) — the viewer-side
  sensor sweep. Lower priority than the rest; include it or record why
  not.

**No sender:**

- `send_script_dialog` — `llDialog` / `llTextBox`. `sl-proto` already
  *parses* a `ScriptDialog` client side, with the
  `!!llTextBox!!` sentinel modelled; the inverse does not exist.
- `send_load_url` — `llLoadURL`. Again, parsed client side
  (`LoadUrlRequest`), never built.
- `send_script_teleport_request` — `llMapDestination`.

`send_money_balance_reply` was on this list and exists now
(`sl-proto/src/sim_session.rs`, with the transaction block and its item
description; the fake grid's groups use it) — audit 2026-10-15.

Each addition is the same small shape the ~130 existing senders share,
and each gets the round-trip test the crate already applies to a
server-direction message: build it, decode it with the client's own
parser, assert the record.

Note one consumer of the raw grabs now exists: `sl-fake-grid/src/grabs.rs` drags
a prim from the `ObjectGrabUpdate`s it matches out of
`ServerEvent::ClientMessage` (spins already arrive typed); it has to move to the
typed events with this change.

Acceptance: the three grab messages, `ScriptDialogReply` and
`MoneyTransferRequest` arrive as typed `ServerEvent`s carrying their
full payloads; the three senders exist and round-trip through the client
parser; and no existing `ClientMessage` consumer regresses (the
catch-all stops seeing these, which is the point).

## From gridspec-lsl-comms (2026-10-24)

The dialog pair is done: `SimSession::send_script_dialog` puts a
`ScriptDialog` (an `llTextBox` is its one `!!llTextBox!!` button, the owner
block only when an owner id is given), and a `ScriptDialogReply` decodes to
`ServerEvent::ScriptDialogReply`, recorded as the object's last reply
(`SimSession::script_dialog_reply`), with the round-trip test
`script_dialog_and_reply_round_trip`. `e2e_script_dialog` holds the viewer's
card to both fake flavours with it. The rest of the list stands: the grabs,
`MoneyTransferRequest`, the sensor pair, `send_load_url` and
`send_script_teleport_request`.

## Done (2026-10-31)

The rest of the list is closed:

- The grabs decode to `ServerEvent::ObjectGrabbed`, `ObjectGrabMoved` and
  `ObjectReleased`, each carrying **every** `SurfaceInfo` block the client
  sent (as `SurfaceInfo`, the client's own type — OpenSim also keeps them
  all and hands a script the first) and the drag's `TimeSinceLast` as a
  `Duration` of the reference viewer's milliseconds.
- `MoneyTransferRequest` decodes to `ServerEvent::MoneyTransferRequested`
  (`MoneyTransferOrder`): source, destination, the flags byte as the new
  `TransactionFlags` (which parties are groups, `lltransactionflags.h`), the
  amount as the wire's signed 32 bits so a negative ask reaches the money
  module to be refused, the two aggregate-permission bytes, the
  `MoneyTransactionType` and the description. Whether the source is the agent
  itself (OpenSim drops the request otherwise) is the money module's check.
- `SimSession::send_load_url` and `send_script_teleport_request` build from
  the client's own `LoadUrlRequest` and `ScriptTeleportRequest`, so the
  round-trip test compares the record the simulator sent with the one the
  client parsed (an agent and a group owner; a region and none).
- **The sensor pair is not wrapped**: `ScriptSensorRequest` /
  `ScriptSensorReply` are `Trusted` simulator↔simulator messages a viewer
  never sends or receives (neither Firestorm nor OpenSim's `LLClientView`
  handles them), as `book/src/content/scripts.md` already records. A script
  engine answers `llSensor` from the scene store, not from the wire.

The fake grid's drag (`grabs.rs`) and the viewer's touch e2e stages
(`e2e_pilot.rs`) read the typed events; the symmetry test's raw-forward
ledger lost the grab family and `MoneyTransferRequest`.
