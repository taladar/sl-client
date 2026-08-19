---
id: viewer-group-session-moderation
title: Group-chat moderator options
topic: viewer
status: ready
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-avatar-context-menu, chat-b3, viewer-social-group-extras]
---

Context: [context/viewer.md](../context/viewer.md).

The reference shows a Moderator Options block on group-session
participants (menu_participant_list.xml, menu_conversation.xml, and the
agent URL menu): **Allow/Forbid text chat**, **Mute / Unmute this
participant**, **Mute/Unmute everyone**, **Eject from Group**, and
a **Ban member** entry — visible only when the agent holds moderator
powers in the session.

The server side is the ChatSessionRequest CAPS ("mute update" on the
text and voice channels) plus the group eject/ban paths we already
speak (test-group-admin is done; group bans belong to
[[viewer-social-group-extras]]). Our viewer has no moderation UI
anywhere — no moderation code in the viewer sources, and the avatar pie
ships the moderation powers only as greyed placeholders per
[[viewer-avatar-context-menu]].

Scope: track the moderator flag from session agent-updates (participant
tracking exists from [[chat-b3]]), add the participant context entries
in the conversations panel (`sl-client-bevy-viewer/src/conversations.rs`)
gated on that flag, and send the ChatSessionRequest mute update for the
per-participant and everyone variants; eject/ban route through the
existing group-admin commands.

Reference (Firestorm, read-only):
`indra/newview/skins/default/xui/en/menu_participant_list.xml`,
`menu_conversation.xml`, `indra/newview/llfloaterimsessiontab.cpp`,
`indra/newview/llspeakers.cpp` (moderation), ChatSessionRequest in
`indra/newview/llimview.cpp`.
