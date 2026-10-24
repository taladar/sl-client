---
id: server-lsl-lib-comms
title: Library tranche — chat, listens, dialogs and link messages
topic: server
status: ideas
origin: LSL-on-the-fake-grid audit (2026-09-20)
blocked_by: [server-lsl-vm-execution, server-world-chat-routing,
  protocol-sim-script-messages, gridspec-lsl-comms]
refs: [server-world-link-sets, test-fake-grid-lsl-offline-cases]
---

Context: [context/lsl.md](../context/lsl.md).

**Measured (2026-10-13, [[gridspec-local-chat]],
`book/src/gridspec/chat.md` § A script's lines):** what each say function's
line looks like to an avatar on each grid — type, owner, name, place, reach
and where a long text is cut. Build the `Line` each function says to that
table per flavour: OpenSim's `llShout` and its debug-channel `llShout` /
`llRegionSay` name nobody as owner, and its `llOwnerSay` names the prim as
owner and the region's middle as the place; Second Life's debug lines stop
within a say's reach; `llRegionSay` on channel 0 says nothing on either.

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

## Measured (2026-10-24, [[gridspec-lsl-comms]])

`book/src/gridspec/lsl.md` § Chat, listens, dialogs and link messages has
every row; the ones a flavour must split on:

- **Listen limit**: Second Life 65 per script, the 66th a run-time error
  (`Too Many Listens`) that stops the script; OpenSim 64, the 65th and every
  later one a non-positive handle, silently, the script running on.
- **Dialog refusals** (each a `DEBUG_CHANNEL` line, no dialog, unless
  said): over 12 buttons (both refuse, different words); a label over 24
  characters (Second Life refuses, OpenSim shows every label cut to 23); a
  blank label (both refuse); a non-string button (Second Life refuses,
  OpenSim shows it as text); an empty message (Second Life refuses, OpenSim
  complains and shows it); a message of 512 or more (Second Life refuses;
  OpenSim shows it cut to 511, complaining only past 512). The empty button
  list is `OK` on Second Life, `Ok` on OpenSim. A reply is never checked
  against the buttons.
- **Text box refusals**: an empty message and one over 511 are refused on
  both, in each grid's words.
- **A long say** reaches a listener cut to 1024 on Second Life, 1023 on
  OpenSim.
- **`llInstantMessage`**: 2 s on both; `FromAgentID` the owner, the message
  id the prim; cut to 1023; `NULL_KEY` complains on both.
- **The owner in another region**: Second Life delivers only the instant
  message; OpenSim (a neighbour, a child agent) the instant message and
  `llRegionSayTo`, and still answers `OBJECT_POS` for the owner.
- **Link messages in a lone prim**: `LINK_THIS`, `LINK_SET` and 0 reach
  every script, the sender included, on both; `LINK_ROOT` (1) reaches them
  on OpenSim and nobody on Second Life.
- What is the same on both: the ranges (10 / 20 / 100 m, 3-D), a prim never
  hearing itself, `llRegionSayTo` reaching a prim at any distance and
  nothing for its own key or `NULL_KEY`, `llRegionSay` refused on 0.

`SimSession::send_script_dialog` and the decoded `ScriptDialogReply` exist
now, so the dialog leg of this task's acceptance needs no protocol work.
