---
id: viewer-conversation-pane-toolbar
title: The conversation pane's toolbar, including the participant list
topic: viewer
status: ready
origin: user observation (2026-08-21) while live-checking
  [[viewer-conference-start-ui]]
refs: [viewer-social-im-conversations, viewer-conference-start-ui,
  viewer-voice-controls, viewer-group-session-moderation]
---

Context: [context/viewer.md](../context/viewer.md).

A conversation pane carries only a close ✕ and (as of
[[viewer-conference-start-ui]]) an add-participants ✚. The reference's IM
session floater has a **toolbar row** above the transcript
(`floater_im_session.xml`, `toolbar_panel`), and the missing piece the user
noticed first is the one that shows **who is in the conversation**.

The reference's row, left to right, and what each maps to here:

- **`expand_collapse_btn`** ("Collapse/Expand this pane") — toggles the
  **participant list** beside the transcript. This is the important one: a
  group or ad-hoc conference has a roster (`Session::participants` for the
  session's `ChatSessionKind`, fed by the accept roster, `SessionAdd` /
  `SessionLeave`, and the agent-list updates) that the viewer holds and never
  shows. Rows want the same name resolution and per-avatar menu the radar's
  rows have, so this is the radar's row treatment over a different source, not
  a new list widget.
- **`add_btn`** ("Add someone to this conversation") — **done**, the ✚.
- **`close_btn`** ("End this conversation") — done, the ✕ (move it into the
  row when the row exists).
- **`gear_btn`** ("Actions on selected person", `menu_im_conversation.xml`) —
  the per-participant menu, which is the same avatar-action set the radar and
  minimap menus already dispatch; it should route to those arms rather than
  grow its own.
- **`view_options_btn`** (`menu_im_session_showmodes.xml`) — Compact /
  Expanded view, Show time, Show names in one-to-one conversations. The last
  two overlap [[viewer-chat-transcript-style-options]]; check before
  duplicating.
- **`voice_call_btn`** — belongs to [[viewer-voice-controls]], which also
  wants speaking indicators *in this very list*; leave a slot for it rather
  than building it here.
- **`tear_off_btn`** — our floater scaffold already docks / tears off at the
  floater level, so this is likely a no-op for us; confirm before adding.

A moderator's controls over that roster (mute, eject) are
[[viewer-group-session-moderation]]'s, and they hang off this list once it
exists.

Reference (Firestorm, read-only):
`indra/newview/skins/default/xui/en/floater_im_session.xml` (the
`toolbar_panel` block), `menu_im_conversation.xml`,
`menu_im_session_showmodes.xml`, `llfloaterimsession.cpp`.
