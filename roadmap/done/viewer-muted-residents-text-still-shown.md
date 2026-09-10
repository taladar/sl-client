---
id: viewer-muted-residents-text-still-shown
title: A blocked resident's chat and IMs are still shown
topic: viewer
status: done
origin: found while building [[viewer-do-not-disturb-away]] (2026-08-20)
refs: [viewer-block-list, viewer-do-not-disturb-away]
---

Context: [context/viewer.md](../context/viewer.md).

Blocking a resident ([[viewer-block-list]]) puts them on the server mute list
and the viewer honours that in several places — the name tag reads `Blocked`,
the radar marks them, world sounds are dropped
(`MuteFlags::ALLOW_OBJECT_SOUNDS`) — but **their text is not filtered**. A
blocked resident's nearby chat still appears in the overlay and the Nearby
transcript, and their IMs still open / append to a conversation tab.

The reference drops both at ingest, gated on the per-entry text aspect: a mute
with `MuteFlags::ALLOW_TEXT_CHAT` excepted still shows text, everything else is
swallowed (`LLMuteList::isMuted(id, name, LLMute::flagTextChat)` in
`LLIMProcessing::processNewMessage` and the nearby-chat path).

Scope: honour `MuteModel::is_muted_aspect(id, MuteFlags::ALLOW_TEXT_CHAT)` in
`chat.rs`'s overlay ingest and `conversations.rs`'s
`ingest_conversation_events` — nearby chat, direct IMs, and the group /
conference session lines from a blocked speaker. Objects' chat is muted by the
owner *or* the object id, exactly as `world_sounds.rs` already does for sound.

Noticed because [[viewer-do-not-disturb-away]] added the reference's opt-in
"you are blocked" auto-reply (`SendMutedAvatarResponse`): that reply is correct
on its own, but the sender's message being displayed anyway makes the pair
read oddly.

## Built

One question, asked once. `MuteModel::is_muted_aspect_named` is
`is_muted_aspect` widened with the reference's **by-name** fallback, and
`chat_text_muted` is the whole nearby-chat rule (speaker, or an object's owner)
over one `ChatMessage`. Both live in `sl-viewer-world-api` beside the model, so
the overlay and the conversations floater ask the same question rather than each
spelling out its own.

The by-name fallback is not decoration. `is_muted_aspect` matched on the id
alone, so a *Block object by name…* entry — the one lever there is against a
spammy object one cannot click, and whose rezzer hands out a fresh id per
object — did nothing to its chat. `is_muted_aspect` also lost a latent bug on
the way: a **nil** id used to match every by-name entry at once, so an object
sound whose owner was unknown was silenced by any by-name block at all.

Filtered at ingest, in both surfaces:

- `chat.rs`'s `update_chat_overlay` — a blocked speaker's say never becomes a
  floating line.
- `conversations.rs`'s `ingest_conversation_events` — nearby chat, the direct
  IM, the group line, the conference line, and the **server backlog** (the one
  path by which something said before the block still arrives; the reference
  filters it in `LLFloaterIMSessionTab`'s server-history walk).

Two arrivals carry no text and would have announced a blocked resident anyway,
so they are filtered too:

- **Typing notices.** A one-to-one typing notice *opens the tab*
  (`ConversationModel::set_typing`), so letting one through puts a blocked
  resident's name on screen with nothing said.
- **Conference invitations.** A group session a blocked resident starts opens
  no tab but is *not* declined — the group is one the user chose to be in and
  everyone else in it is still worth hearing, which is the reference's own
  reasoning. An ad-hoc conference *is* the person who opened it, so it takes
  the existing ignore-conferences exit and is declined on the wire.

A missing mute list (a headless world that registered none) reads as "nothing
is blocked" everywhere: a filter must never be the reason chat stops.

Unit-verified, no grid: the aspect/by-name matrix and the object-or-owner rule
(`sl-viewer-world-api`), the overlay drop with an unblocked control
(`sl-viewer-chat`), and an ingest app that runs only
`ingest_conversation_events` over all six arrival kinds, once unblocked and once
blocked, plus the server-backlog filter (`sl-viewer-people`).

Not carried, and split out rather than silently dropped: the **disk chat
transcript** still records a blocked resident's lines. The reference does not
(`chat.mMuted` gates the nearby log write), but our `ChatLog` lives in the
session runtime, which holds no mute list at all — see
[[viewer-chat-log-records-blocked-residents]].
