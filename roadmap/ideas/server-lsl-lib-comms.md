---
id: server-lsl-lib-comms
title: Library tranche — chat, listens, dialogs and link messages
topic: server
status: ideas
origin: LSL-on-the-fake-grid audit (2026-09-20)
blocked_by: [server-lsl-vm-execution, server-world-chat-routing,
  protocol-sim-script-messages]
refs: [server-world-link-sets, test-fake-grid-lsl-offline-cases]
---

Context: [context/lsl.md](../context/lsl.md).

How scripts talk — to residents, to each other, and to their own
linkset. Small in function count, enormous in how much content it
unlocks.

- **Saying**: `llSay`, `llWhisper`, `llShout`, `llRegionSay`,
  `llRegionSayTo`, `llOwnerSay`, `llInstantMessage`. All route through
  the region fan-out ([[server-world-chat-routing]]);
  `llRegionSayTo` is the one that targets a single key and reaches an
  object as well as an avatar. `llOwnerSay` goes to the owner wherever
  they are in the region and does not appear to anyone else.
- **Listening**: `llListen`, `llListenRemove`, `llListenControl`, and
  the `listen(integer channel, string name, key id, string message)`
  event. The filter rules matter: an empty name or `NULL_KEY` matches
  anything, a non-empty one matches exactly, and there is a per-script
  handle limit (65) that content hits. A listen is removed on state
  change ([[server-lsl-state-and-events]]).
- **Dialogs**: `llDialog` and `llTextBox`, needing the
  `send_script_dialog` that [[protocol-sim-script-messages]] adds. The
  reply comes back as a `ScriptDialogReply` on the hidden negative
  channel and is delivered to the script's listens — i.e. the reply path
  is chat, not a separate mechanism, which is why this tranche needs
  listens to exist first. `llTextBox` is the same message with the
  `!!llTextBox!!` sentinel button, which `sl-proto` already models.
  Button-count and label-length limits are enforced by the simulator,
  loudly.
- **Pointing the viewer somewhere**: `llLoadURL` and `llMapDestination`,
  the other two senders in [[protocol-sim-script-messages]].
- **Within the object**: `llMessageLinked` and the
  `link_message(integer sender, integer num, string str, key id)` event,
  with `LINK_*` sentinel resolution from [[server-world-link-sets]]. A
  link message to `LINK_THIS` reaches the sending prim's *other*
  scripts, including the sender itself — a detail that turns an
  innocuous broadcast into a loop.
- **Email and XML-RPC** are deliberately elsewhere
  ([[server-lsl-lib-email-xmlrpc]]).

Acceptance: `script-dialog` and `script-permissions` become offline
conformance cases against a scripted fixture; two scripts in one region
hold a conversation over a non-zero channel with no viewer involved; a
`llDialog` shown in the Bevy viewer answers back into the script's
`listen`; and a listen registered in one state is gone after a state
change.
