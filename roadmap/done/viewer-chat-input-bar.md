---
id: viewer-chat-input-bar
title: Chat input bar (local chat + focus)
topic: viewer
status: done
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-social-panels
blocked_by: [viewer-ui-text-input-widget, viewer-input-focus-contexts, viewer-ui-settings-store]
---

Context: [context/viewer.md](../context/viewer.md).

The core "chat focus" flow: a chat input bar built on
[[viewer-ui-text-input-widget]]. Enter focuses it (switching to the **Chat**
input context), Esc blurs back to World; sending on Enter emits local chat via
`Command::Chat` (the wire path `send_chat_from_viewer` already exists) and
drives the pre-wired `typing.rs::TypingState::set()` hook (P31.9 shipped the
typing animation explicitly waiting for a real input box).

Also the **settings-gated "typing in the world auto-starts local chat"**
behaviour (Firestorm's "letter keys start local chat"): when the setting (in
[[viewer-ui-settings-store]]) is on, a printable keypress while in the World
context opens the bar and forwards that character into it.

Chat receive already works (`chat.rs` overlay, `ChatReceived`). Channel /
whisper-shout / `/me` selection is [[viewer-chat-channel-and-commands]]; the
scrollable history panel is [[viewer-chat-history-panel]].

Reference (Firestorm, read-only): `fsfloaternearbychat` input, `llchatbar`,
`llnearbychatbar`.

Builds on: `protocol-1` local chat, `chat.rs`, `typing.rs`. Supersedes the MVP
"no chat input" non-goal.

## Update (2026-07-20): the widget exists; this is now the live wiring

The reusable **local-chat-input widget** is done
([[viewer-chat-channel-and-commands]] /
`sl-client-bevy-viewer/src/local_chat_input.rs`) — it already owns the field,
the emoji button, the `:`-completer, the whisper/say/shout select box, `/N`
channel routing, the `/command` registry and the Shift/Ctrl+Enter volume
overrides, emitting a session-free `LocalChatSubmit`. So this task is now
**narrowly the live nearby-chat bar**: place the widget in the bottom-area upper
stack (`crate::bottom_toolbar::BottomArea::upper`), map its `LocalChatSubmit` to
`Command::Chat`, drive `typing.rs::TypingState::set()`, and wire the focus flow
(**Enter** focuses the bar / **Esc** blurs to World).

The settings-gated
**"a printable keypress in the World auto-starts local chat"** behaviour is
split out to [[viewer-chat-input-world-autostart]] — it belongs to *this* live
bar only, not the widget or the conversations-floater instance. The same widget
is also plugged into the conversations floater by
[[viewer-social-im-conversations]].

## Done (2026-07-21)

`sl-client-bevy-viewer/src/nearby_chat_bar.rs` — `NearbyChatBarPlugin` places
the reusable local-chat-input widget ([[viewer-chat-channel-and-commands]]) in
the bottom area's **upper stack** (`crate::bottom_toolbar::BottomArea::upper`),
so it rides just above the button bar. All the input behaviour is the widget's;
the bar adds only the live wiring:

- **Send** — the widget's `LocalChatSubmit` mapped to `Command::Chat` (message /
  channel / chat-type straight through), filtered to the bar's own field so a
  second instance (the conversations floater) will not double-send.
- **Focus flow** — `Enter` while the **World** owns the keyboard focuses the bar
  (revealing it if hidden); `Esc` blurs back to the World (via
  [[viewer-input-focus-contexts]]).
- **Typing** — `typing.rs::TypingState` is now driven from the bar (active while
  it is focused and holds a draft). The **T-key stand-in** the typing module
  carried "until a real chat input arrives" is **removed**, and
  `drive_own_typing` is no longer gated on `world_has_keyboard` (typing happens
  in the TextEntry context, which that gate would have suppressed).

**Toggle button.** The bottom toolbar grows a **leading** `Chat` button (a new
`ToolbarTarget::NearbyChat`, first in `TOOLBAR_BUTTONS` so it mirrors ends under
RTL) that shows / hides the bar and lights while it is shown — the reference's
chat button.

**Deferred (its own task).** The settings-gated "a printable World keypress
auto-starts local chat" affordance is [[viewer-chat-input-world-autostart]] (now
ready). The bar is a live, session-touching surface, so it is verified by
running the viewer rather than in the headless harness.
