---
id: server-lsl-lib-comms
title: Library tranche — chat, listens, dialogs and link messages
topic: server
status: done
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

## From gridspec-lsl-events (2026-10-25)

`events/state-drops` and `events/merging` (`book/src/gridspec/lsl.md`
§ Events): a listen opened in `state_exit` is dropped with the others at the
state change, on every grid; five lines said in one handler, the same or
different, are five `listen` events in order, and five identical link
messages five `link_message`s — nothing merges either, anywhere.

## Measured (gridspec-lsl-throttles, 2026-10-28)

`book/src/gridspec/lsl.md` § Forced delays and throttles: neither grid
throttles `llRegionSay`, `llSay` or `llRegionSayTo` on a channel other than 0
(600 each, a burst of 200 and ten waves of 40, all heard), and Second Life
does not throttle channel 0 either (25 in a third of a second). OpenSim
sleeps a script 2 s before its twelfth `llSay` / `llShout` on channel 0
within a second. `llInstantMessage` sleeps 2 s and `llDialog` 1 s on both;
`llTextBox` sleeps 1 s on OpenSim only (0 on Second Life). The delays
themselves belong to [[server-lsl-forced-delays-per-flavour]].

## Done (2026-10-31)

All 15 functions of the tranche are implemented in `sl-lsl-runtime`, each
checking and cutting as the region's flavour does: `llWhisper`, `llSay`,
`llShout`, `llRegionSay`, `llRegionSayTo`, `llOwnerSay`, `llInstantMessage`,
`llListen`, `llListenRemove`, `llListenControl`, `llDialog`, `llTextBox`,
`llLoadURL`, `llMapDestination` and `llMessageLinked`; coverage is 145
implemented of 524.

- **Listens are the instance's** (`vm::comms::Listens`), not the host's as the
  book's first sketch had them: handles are the lowest free number; a repeat
  filter answers the same handle (on OpenSim, any active listen that would
  hear the new filter's fields, as `GetListeners` decides); the limit is 65
  and the run-time error `Too Many Listens` on Second Life, 64 and `-1` on
  OpenSim; a state change and a reset drop them. The host decides who is in
  earshot and calls `Engine::hear`, which raises `listen` once per admitting
  active listen (one per listen as OpenSim does; not measured on Second Life).
- **What the host is handed is finished**: a `Speech` cut to 1024 bytes
  (Second Life) or 1023 (OpenSim, `StringToBytesNoTerm`), `llRegionSayTo`'s
  target with nobody, a non-key and the caller's own prim dropped, a `Dialog`
  with each grid's refusals in its words (OpenSim showing an empty or long
  message, cut to 511 bytes, labels cut to 23, an integer button as text),
  an `InstantMessage` cut to 1023 bytes, `llLoadURL`'s URL checked as
  OpenSim's `new Uri` does (Second Life unmeasured, passed on).
  `llMapDestination` goes to the toucher, or outside a touch to the wearer
  (`Host::worn_by`).
- **Link messages** are resolved from `Host::links` by the flavour's rule and
  posted with store changes in **one ordered list** of what a slice raised
  (`vm::Raised`): on Second Life both are queued at the call, so the Leader's
  own "filling" message is ahead of the fill's `linkset_data` events in its
  queue. That fixed the linkset probe's one deviation: `fill-events` is 63, as
  on aditi.

**Proof** (`vm/tests/comms.rs`): the probes run in a small test region that
routes chat by range and puts dialogs to two answering avatars —
`comms/listen-limit`, `comms/link-messages`, `comms/object-chat`,
`comms/dialog` and `comms/textbox`, every `PROBE` line plus each dialog put
and each `DEBUG_CHANNEL` line the runner noted, on all three grids, with no
named deviation. `link-messages`' `got` lines are compared as a set: on aditi
the Receiver ran while the Sender was still in its handler. `data/linkset-data`
now runs on the real link messages and chat. Unit tests hold the acceptance's
conversation of two scripts on channel 7, a state change dropping every
listen (one opened in `state_exit` included), `LINK_THIS` reaching the
sender unless it resets, and the say, instant-message, map and URL rules.
`comms/instant-message` (timed with `llGetTime`) and `comms/owner-away`
(`llGetAgentSize`, `llGetObjectDetails`, a teleport) are not replayed; their
rows are unit tests or the host's.

**Not done here**: the offline `script-dialog` / `script-permissions` cases
are [[test-fake-grid-lsl-offline-cases]]'s, and the viewer-dialog leg of the
acceptance is [[server-fake-grid-script-engine-wiring]]'s — both need the
fake grid to run scripts. The forced delays stay the keyword file's
(`llTextBox` 0 on Second Life, OpenSim's error second, its channel-0
throttle): [[server-lsl-forced-delays-per-flavour]].
