---
id: viewer-chat-log-records-blocked-residents
title: The disk chat transcript records a blocked resident's lines
topic: viewer
status: done
origin: found while building [[viewer-muted-residents-text-still-shown]] (2026-09-10)
refs: [viewer-muted-residents-text-still-shown, viewer-block-list]
---

Context: [context/viewer.md](../context/viewer.md).

[[viewer-muted-residents-text-still-shown]] stopped a blocked resident's text
reaching every **display** surface — the chat overlay, the Nearby transcript,
the IM / group / conference tabs, the server backlog. The **disk** transcript
still records it: `ChatLog::observe_event` writes the nearby line and the
inbound IM whatever the mute list says, so the block-list is undone by opening
the log, and a later recall (`ChatHistoryPage` / `NearbyChatHistoryPage`) reads
those lines straight back into the pane the live filter just cleared.

The reference does not log them. `chat.mMuted` (set from
`isMuted(from_id, from_name, flagTextChat)`) gates the nearby log write —
`fsfloaternearbychat.cpp`'s `if (args["do_not_log"].asBoolean() ||
chat.mMuted) return;` — and the IM path never reaches `addMessage` for a muted
sender at all. Its truth table is explicit: MUTED ⇒ *DISPLAY No, STORE IN
HISTORY No*.

## Why it was not done with the display filter

The mute list is a **viewer** resource. `MuteModel` lives in
`sl-viewer-world-api` and is filled by `sl-viewer-people`'s `ingest_mute_list` /
`note_local_mutes`; `ChatLog` lives in `sl-client-bevy`'s **session runtime
task**, which has neither. `sl_proto::Session` parses the downloaded mute file
(`parse_mute_list`) and emits `Event::MuteList`, but keeps no model of it, so
there is nothing on that side to ask.

## The shape of the fix

The matching logic is already pure and already stated once
(`MuteModel::is_muted_aspect_named` + `chat_text_muted`). What is missing is a
copy of the *list* on the runtime side. Either:

- lower a small pure mute model into `sl-proto`'s `Session` (fed by the
  `MuteList` reply and by outgoing `Command::Mute`, exactly as
  `note_local_mutes` feeds the viewer's), and have `ChatLog::observe_event`
  consult it — which also gives every other runtime consumer the same answer; or
- push the decision up: give `ChatLog` a mute snapshot the viewer refreshes on
  each `MuteModel` revision bump.

The first keeps one owner of the rule and no cross-tier snapshot to go stale;
it is the larger change. Whichever lands, the check is `ALLOW_TEXT_CHAT` and
the object case is owner-or-object, as it is for display.

Note that lines logged *before* a block stay in the file, as they do in the
reference — a block is not retroactive over a transcript already on disk.
