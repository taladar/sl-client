---
id: viewer-chat-input-history
title: Chat-input line history (Ctrl+Up / Ctrl+Down recall)
topic: viewer
status: ready
origin: follow-up requested during viewer-social-im-conversations (2026-07)
refs: [viewer-ui-text-input-widget, viewer-ui-text-input-emoji, viewer-chat-channel-and-commands, viewer-chat-input-bar, viewer-social-im-conversations]
---

Context: [context/viewer.md](../context/viewer.md).

Add a **recall history** to the chat-input widgets: pressing **`Ctrl+Up`** walks
back through the lines previously submitted from that field (most-recent first),
and **`Ctrl+Down`** walks forward again toward the (empty) draft, replacing the
field's current text as it goes — the reference viewer's chat-bar history
(`LLChatEntry` / `LLLineEditor` up/down recall, gated so it does not fight
multi-line caret movement).

Where it lives: the base **`chat_input`** widget
([[viewer-ui-text-input-emoji]]), so **every** consumer gets it for free — the
always-visible nearby-chat bar ([[viewer-chat-input-bar]]), the local-chat
variant ([[viewer-chat-channel-and-commands]]), and each Conversations-floater
tab (nearby / IM / group / conference, [[viewer-social-im-conversations]]).

Design notes / scope:

- **Per-field history**, bounded (a few dozen lines), oldest evicted — keyed by
  the field entity so each conversation and the bar keep their own recall stack,
  mirroring the per-field `ChatInputSubmit` routing already in place. A line is
  pushed on submit (the point `chat_input::send_chat_input` already clears the
  field), so history and the sent line never drift.
- **`Ctrl`-modified** arrows so a bare `Up`/`Down` still moves the caret in a
  multi-line field; on a single-line field the reference uses bare up/down, but
  gating on `Ctrl` keeps one rule across both and avoids stealing arrows from
  the emoji `:`-completer's popup navigation.
- Recall replaces the whole field text and parks the caret at the end; stepping
  forward past the newest entry restores the **in-progress draft** that was
  being typed when recall started (so an accidental `Ctrl+Up` is undoable),
  matching the reference's "line 0 is the live edit" behaviour.
- Client-side only — a headless test over the history ring (push / clamp /
  up-down walk / draft restore) covers it; no grid needed.
