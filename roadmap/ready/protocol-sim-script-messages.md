---
id: protocol-sim-script-messages
title: The simulator-side script messages SimSession neither sends nor decodes
topic: protocol
status: ready
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
- `send_money_balance_reply` — the balance push after a
  `llGiveMoney` / object payment, and the `description` field the viewer
  shows as the transaction line.

Each addition is the same small shape the ~130 existing senders share,
and each gets the round-trip test the crate already applies to a
server-direction message: build it, decode it with the client's own
parser, assert the record.

Acceptance: the three grab messages, `ScriptDialogReply` and
`MoneyTransferRequest` arrive as typed `ServerEvent`s carrying their
full payloads; the four senders exist and round-trip through the client
parser; and no existing `ClientMessage` consumer regresses (the
catch-all stops seeing these, which is the point).
