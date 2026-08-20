---
id: viewer-muted-residents-text-still-shown
title: A blocked resident's chat and IMs are still shown
topic: viewer
status: bugs
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
