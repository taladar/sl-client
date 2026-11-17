---
id: protocol-variable-block-lists-over-255
title: A request listing more than 255 objects fails to encode and ends the session
topic: protocol
status: done
origin: gridspec-circuit (2026-10-06)
refs: [gridspec-circuit]
---

Context: [context/protocol.md](../context/protocol.md).

## Observation

A `Variable` block list's count is one byte on the wire, so a message can
carry at most 255 entries. `Command::RequestObjects` with 300 ids failed in
`RequestMultipleObjects::encode_body` with "variable-length value of 300
bytes exceeds the 255-byte capacity" (the wording is the encoder's; it is
counting blocks), and `sl-client-tokio`'s run loop returned the error — the
whole session ended over one request. Found by `throttle-set` on aditi.

`Circuit::send_request_multiple_objects` now splits its ids across messages
(`MAX_VARIABLE_BLOCKS`). Every other request that takes a caller's list of
objects is still unsplit: `ObjectSelect`, `ObjectDeselect`, and the edit,
link, delete, return and permission messages built the same way in
`sl-proto/src/session/circuit.rs`. A viewer selecting a large linkset or
box-selecting a build reaches them.

## Wanted

- Split every caller-sized block list at 255, as the reference viewer does
  (it starts a new message when the current one is full).
- Decide what a command that fails to encode should cost. Today the tokio
  driver ends the session and the Bevy driver reports the command failed and
  carries on, which is not parity, and
  one bad request should not be the end of a login.
- The encoder's error text should say blocks, not bytes, when it is counting
  blocks.

## Also on the simulator's side (2026-10-11)

`SimSession::send_kill_object` fails the same way with more than 255 ids:
the fake grid's Second-Life flavour names every prim of a derezzed linkset
in one `KillObject`, and a set of 256 prims derezzed beside another object
was never killed on the client (found by `object-link-delink`). The fake
grid splits the list itself now (`MOST_KILLS_A_MESSAGE` in `world.rs`); the
senders in `sim_session.rs` that take a caller's list still do not.

## A second case: a long dialog reply (2026-10-24)

[[gridspec-lsl-comms]] answered an `llTextBox` with 300 characters through
`Command::ReplyScriptDialog`; `ScriptDialogReply`'s `ButtonLabel` is a
one-byte-length field, the encode failed, and the tokio run loop ended the
session — the error only showed at the logout. The reply itself is now cut
to the 254 bytes the field carries at a character boundary
(`ScriptDialog::fit_reply`, used by `Session::reply_script_dialog`), as the
reference viewer's `LLMessageSystem::addString` cuts it (its text box also
caps the input at 250 characters). The general question above — what a
command that fails to encode should cost each driver — is still open, and
every other caller-sized string field reaches it the same way.

## Resolution (2026-11-17)

- `sl-proto`'s new `split` module fills messages the way the reference's
  `isSendFull` does: at most 255 of the block (or a caller's smaller limit)
  and no more than the 1200-byte `MTUBYTES`, header blocks repeated in each.
  The simulator's object-update packing now uses it too.
- Client senders: the selection lists (select, deselect, duplicate,
  duplicate-on-ray, delete, owner, group, permissions, delink, undo, redo,
  detach, drop) at the reference's `MAX_OBJECTS_PER_PACKET` of 254; a link
  repeats its root at the head of every message, as `sendListToRegions` does
  with `linkset_root`; a `DeRezObject` take goes 250 roots to a packet with
  the packet count and number filled in, and a take past 254 packets is
  refused whole (the reference's "AcquireErrorTooManyObjects"). Inventory
  moves and removals (`RemoveInventoryObjects`' two lists counted together),
  gestures, group invites, ejections, role edits, animations, viewer effects
  and parcel selections split at 255 / the MTU. `RequestMultipleObjects` keeps
  its flat 255, which is the reference's own count for it.
- Simulator senders: kills, online/offline notifications, sound preloads,
  object-owner tallies, directory and picker replies, rights changes,
  inventory removals, mean-collision alerts and the map block / item / layer
  replies. `ForceObjectSelect` resets only in its first message. The fake
  grid's own kill chunking is gone.
- An empty list sends nothing where it would say nothing, and still one
  message where it means something (a parcel selection of "every object", an
  empty directory reply, a forced selection that only clears).
- Messages whose blocks are not independent are left unsplit and fail to
  encode past the limit: the parameter lists of `GenericMessage`,
  `EstateOwnerMessage` and `GodlikeMessage`, `AgentSetAppearance`'s visual
  parameters, `CoarseLocationUpdate` (its `You`/`Prey` indices point into the
  list) and `AgentWearablesUpdate`.
- A failed command now costs only itself in both drivers: the tokio run loop
  reports it as the new `Event::CommandFailed { command, reason }` and keeps
  going, as the Bevy driver already did with `SlCommandFailed`.
- The encoder's block-count error is the new `WireError::TooManyBlocks`,
  naming the block; `VariableTooLong` is left for byte-length fields.
