---
id: viewer-social-im-conversations
title: IM / conversation UI + chat input
topic: viewer
status: done
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-social-panels
blocked_by: [viewer-ui-widget-scaffold, viewer-ui-text-input-widget]
refs: [viewer-chat-input-history]
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

## Done (2026-07-21)

New viewer module `conversations.rs`: a **Conversations floater** with a
**dynamic vertical tab strip** — Nearby Chat always first and un-closable, then
one tab per one-to-one IM, group chat and ad-hoc conference, spawned live as
messages / invites arrive and closable with a per-tab `×`.

- **Pure model + ECS mirror.** `ConversationModel` (unit-tested) is fed only
  from the `SlEvent` stream — `ChatReceived` (nearby), `InstantMessageReceived`
  (1:1), `GroupSessionMessage`, `ConferenceSessionMessage`, plus name harvesting
  from group membership / name / profile / invite events — and a
  `ConversationsUi` resource holds one view (tab button, pane, transcript,
  input) per entry.
- **Input per tab, per the plan.** Nearby plugs in the **local-chat-input**
  widget (its `LocalChatSubmit` → `Command::Chat`); IM / group / conference tabs
  plug in the plain **chat-input** widget, mapping `ChatInputSubmit` to
  `InstantMessage` / `SendGroupMessage` / `SendConferenceMessage` with an
  optimistic local echo (the grid does not echo those back). Nearby is skipped
  in the `ChatInputSubmit` path so a local line is never re-sent as an IM.
- **Attention, not intrusion.** Unread badges on inactive tabs; the tab (window
  open) and the toolbar Conversations button (window closed) **flash** on a new
  IM / group / conference line — the window is never auto-popped. Selecting a
  tab clears its unread.
- **Invites + typing.** A `ChatterBoxInvitation` opens the tab as a *pending
  invite* with **Accept / Decline** (`AcceptChatInvite` / `DeclineChatInvite`);
  `ChatTyping` / `ImTyping` drive an "X is typing…" line.
- **Docked to the nearby-chat bar.** A new per-floater `FloaterSpec.dock_host`
  (added to the floater manager) lets this window dock into **its own host** at
  the bottom leading corner — pinned directly above the nearby-chat bar,
  dropping into the bar's place when the bar is toggled off — instead of the
  shared top-trailing dock host, with the dock button and free-floating tear-off
  intact.
- **Resizable strip / pane divider** reusing the tab widget's
  `TabStrip` / `TabStripWidth` / `TabDivider`, so the split width persists per
  host floater via `floater_persist`. The transcript **scrolls within the
  fixed / user-resized height** rather than growing the window.
- **Wiring.** The bottom-bar **Conversations** button (second, beside the Chat
  toggle — the semantic pair) and a **Comm ▸ Conversations** menu entry
  (`Ctrl+T`, with an open-state check) both toggle it. A rejected login (e.g. an
  OpenSim stale-presence block) now logs at `error!`. `GroupKey` is now
  `pub`-exported from `sl-client-bevy` (parity with `sl-client-tokio`).

**Deferred (own tasks / follow-ups):** chat-input line history
([[viewer-chat-input-history]]); keyboard arrow-navigation *within* the tab
strip; the friends and group **lists** the reference also hangs off this floater
(separate, already-existing tasks). Verified live on OpenSim (open / type nearby
chat / dock / undock / resize-scroll); the IM / group / conference paths are
unit-tested, since an idle OpenSim login sees no peers.
