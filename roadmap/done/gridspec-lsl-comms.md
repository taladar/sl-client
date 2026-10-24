---
id: gridspec-lsl-comms
title: LSL library behaviour on each grid — chat, listens, dialogs and link messages
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, server-lsl-lib-comms]
blocked_by: [gridspec-lsl-live-differential-runner]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

What an *avatar* is sent of each say function was measured by
[[gridspec-local-chat]] (`book/src/gridspec/chat.md` § A script's lines):
types, owner, name, place, reach, the debug channel, an empty and a long
text. What a *listen* hears is still this task's.

No aditi or OpenSim measurements for this tranche beyond the library table's
documented values.

## Discover

Probes for: Listen limit, `llDialog` button / label errors, `llTextBox`,
`llRegionSayTo` to objects, `llOwnerSay` off-region, `llInstantMessage` delay.
Run on OpenSim YEngine / XEngine and aditi.

## Document

`book/src/gridspec/lsl.md` § Chat, listens, dialogs and link messages.

## Fake grid

Large — [[server-lsl-lib-comms]].

## Viewer

Dialog and textbox floaters, error display.

## From gridspec-lsl-live-differential-runner (2026-10-24)

A probe can now ask for a line of chat on a channel or a touch, from the
owner or a second avatar (`rec=waiting drive=chat|touch by=other`;
`book/src/tools/lsl-probes.md`). `events/other-avatar` measured on all three
engines: an open listener on 7 hears the owner and the other avatar alike
(the name is `llKey2Name`'s), a listener filtered to the owner does not hear
the other, and the other's touch reads `llDetectedType` 3 on Second Life and
1 on OpenSim; `llDetectedGroup` is the object's group against the toucher's
active group (both unset on aditi: 1).

## Done (2026-10-24)

- **Discover.** Seven probes, `sl-lsl-runtime/probes/comms/`:
  `listen-limit` (a second script watching the one that fills its listens),
  `dialog` (buttons, the empty list, a forged reply, the 12/13-button,
  24/25-character, blank, integer, empty-message and 511/512/513 limits, a
  dialog to `NULL_KEY` and to the other avatar), `textbox` (typed, empty,
  300 characters, the message limits, the other avatar), `instant-message`
  (timed, to the owner, the other, `NULL_KEY`, nobody's key, 1100
  characters), `owner-away` (what reaches an owner in another region),
  `object-chat` (listeners just inside and outside each chat range,
  `llRegionSayTo` other prims, the own prim, channel 0, a long line, a
  message filter) and `link-messages` (every `LINK_*` target in a lone prim).
  All complete on aditi, YEngine and XEngine; the two engines agree.
- **Runner.** Two new drives — `dialog reply="…"` (the newest dialog's
  button, any label, or a text box's text) and `away seconds=<n>` (the owner
  teleports out and back, the prims renewed as the region streams them) —
  a `height` header for a prim above prim A, and notes of dialogs, object
  instant messages and heard chat. A refused rez now derezzes the prims
  already rezzed; the other avatar touches a prim under its own circuit's
  scope (the owner's circuits renumber after a trip).
- **Document.** `book/src/gridspec/lsl.md` § Chat, listens, dialogs and link
  messages; the drives and notes in `book/src/tools/lsl-probes.md`; a
  pointer from `gridspec/chat.md` § A script's lines.
- **Fake grid.** Large, as planned: [[server-lsl-lib-comms]] carries the
  rows a flavour splits on. The wire half it needs for dialogs is done here:
  `SimSession::send_script_dialog` and a decoded `ScriptDialogReply`
  (`ServerEvent::ScriptDialogReply`, `SimSession::script_dialog_reply`) —
  the dialog pair of [[protocol-sim-script-messages]].
- **Viewer.** `e2e_script_dialog` holds the dialog card and the text box to
  both fake flavours (a button's index and label, the empty list's `OK` /
  `Ok`, a text box's text on index 0). A text box reply longer than
  `ScriptDialogReply`'s field failed to encode and ended the tokio session;
  `Session::reply_script_dialog` now cuts it to 254 bytes at a character
  (`ScriptDialog::fit_reply`), as the reference's message system cuts it —
  the general question is [[protocol-variable-block-lists-over-255]]'s. The
  refusals are `DEBUG_CHANNEL` chat, [[viewer-script-warning-window]]'s.
