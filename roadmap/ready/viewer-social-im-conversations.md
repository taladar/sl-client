---
id: viewer-social-im-conversations
title: IM / conversation UI + chat input
topic: viewer
status: ready
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-social-panels
blocked_by: [viewer-ui-widget-scaffold, viewer-ui-text-input-widget]
---

Context: [context/viewer.md](../context/viewer.md).

The **IM / conversation** UI: a conversation list plus per-session transcript
panes for one-to-one IM, group chat, and nearby (local) chat. Notably this task
owns **chat input** — typing to local / IM / group sessions — which the MVP
viewer deferred, so it needs the text-input widget
([[viewer-ui-text-input-widget]]) as well as the floater scaffold
([[viewer-ui-widget-scaffold]]).

Local-chat receive and IM already have protocol support (`protocol-2` IM,
`chat.rs` overlay); this task adds the interactive conversation panels and the
input path, superseding the MVP "no chat input" non-goal.

Reference (Firestorm, read-only): `llimview`, `llfloaterimsession`,
`llconversationview`, `fsfloaternearbychat`.

Builds on: `protocol-2` IM and the `chat.rs` overlay. Supersedes the MVP "no
chat input" non-goal.

## Update (2026-07-20): reuse the chat-input widgets

The two chat-entry widgets are done ([[viewer-ui-text-input-emoji]] /
`crate::chat_input`, [[viewer-chat-channel-and-commands]] /
`crate::local_chat_input`), so this task does not build a text entry — it
**plugs the right widget into each pane**:

- **local (nearby) chat** pane → the **local-chat-input** widget
  (`spawn_local_chat_input`): it carries the whisper/say/shout select box, `/N`
  channel routing, the `/command` registry and the Shift/Ctrl+Enter volume
  overrides that only make sense for local chat.
- **one-to-one IM, group chat, and conference/conversation** panes → the plain
  **chat-input** widget (`spawn_chat_input`): the field + emoji button +
  `:`-completer, with no channel/volume machinery (an IM has no channel or
  shout). Map its `ChatInputSubmit` to the session's IM / group send.

The nearby-chat *bar* (the always-visible bottom-edge one, distinct from this
floater's local tab) is [[viewer-chat-input-bar]]; both consume the same
local-chat-input widget.
