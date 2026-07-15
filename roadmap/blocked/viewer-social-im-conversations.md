---
id: viewer-social-im-conversations
title: IM / conversation UI + chat input
topic: viewer
status: blocked
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
