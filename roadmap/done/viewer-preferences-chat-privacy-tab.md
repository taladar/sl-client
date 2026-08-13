---
id: viewer-preferences-chat-privacy-tab
title: Preferences — chat / IM + privacy tab
topic: viewer
status: done
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-preferences-ui
blocked_by: [viewer-preferences-floater]
---

Context: [context/viewer.md](../context/viewer.md).

The **chat / IM** and **privacy** tab of the preferences floater
([[viewer-preferences-floater]]): chat font / colours / timestamps, the local-
chat and IM logging options, busy / auto-response text, and the privacy toggles
(online-status visibility, script-info / permission prompts, do-not-disturb) —
each control bound to the typed settings store through the floater's binding.

Reference (Firestorm, read-only): `llfloaterpreference*` (the chat / privacy
panels).

Builds on: [[viewer-preferences-floater]].

## Done

New viewer module **`src/preferences_chat.rs`** (`PreferencesChatPlugin`, tab
id `chat`, after the audio tab), with wire / runtime support:

- **Chat display** (global scope, applied live): the font-size step combo
  (small / medium / large — overlay 13/15/17 px, transcript 11/13/15 px; a
  step change restyles already-floating overlay lines and forces a
  transcript rebuild), the nearby-toast lifetime slider (3–60 s; the
  overlay's hold = lifetime − 3 s fade), and the overlay line-cap slider.
- **Chat & IM logging** (account scope): nearby / IM-group-conference
  enables, log timestamps (+date, +seconds), filename date suffix, legacy
  filenames, the `conversation.log` index and its retention days. A pure
  `chat_log_config_from_settings` builds the `ChatLogConfig`; the new
  runtime command **`Command::SetChatLogConfig`** swaps it onto the live
  logger (`ChatLog::set_config` in BOTH runtimes, loading / dropping the
  conversation index on an off↔on flip) — pushed once when the account
  scope loads at login and again on any OK that changed it. The log *path*
  stays with [[viewer-preferences-network-cache-tab]].
- **Automatic replies**: the busy / autorespond / autorespond-non-friends
  texts moved here from the general tab (same keys, no migration); still
  consumed by [[viewer-do-not-disturb-away]].
- **Privacy** (server state, transient settings): "only friends and groups
  know I'm online" and "email me IMs when offline" mirror the grid's
  `UserInfo` pair — requested each floater open, seeded from
  `Event::UserInfo`, written back on OK only when changed and only after a
  grid echo was seen. New **`UserInfo` capability** support (sl-wire
  `user_info.rs` builders/parsers both directions, `CAP_USER_INFO`
  requested from the seed, a caps arm feeding the existing
  `Event::UserInfo`); both runtimes now route `RequestUserInfo` /
  `UpdateUserInfo` cap-first with the legacy UDP fallback (OpenSim has no
  such cap and keeps using UDP).

Hand-offs kept out deliberately: chat colours →
[[viewer-preferences-colors-skins-tab]]; keyword alerts →
[[viewer-chat-keyword-alerts]]; chat bubbles → [[viewer-chat-bubbles]];
per-line *display* timestamps → [[viewer-chat-timestamps]]; the DND/away
mode runtime → [[viewer-do-not-disturb-away]]; look-at privacy →
[[viewer-lookat-faithful]]; script-permission prompt suppression already
lives in the alerts tab's notification table; "only friends can call/IM
me" has no honest client-side consumer yet.

Verified by unit tests at every layer (sl-wire round-trips, the sl-proto
caps-arm lifecycle test, `set_config` tests in both runtime shells, the
tab's config-mapping / apply-diff / seed-gate tests, the chat overlay
resolver tests) and live on the local OpenSim grid (UserInfo UDP
round-trip, logging toggles steering the on-disk transcripts, display
settings applied in the running viewer).
