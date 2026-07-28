---
id: viewer-chat-clickable-sender-names
title: Clickable sender names in the chat / conversations transcript
topic: viewer
status: ready
origin: split from [[viewer-clickable-name-widgets]] (2026-07-28) — the reusable
  name-link widget landed and the owner floaters migrated, but the chat
  transcript needs a per-line restructure before a name can be made clickable
---

Context: [context/viewer.md](../context/viewer.md).

The reusable [`NameLink`](../done/viewer-clickable-name-widgets.md) widget
(`ui_name_link.rs`) is in place and the About Region / About Land owner links
use it. Chat sender names should be clickable too — click a resident's name in
the transcript to open their profile — but that is a **restructure**, not a
drop-in, which is why it is split out here.

Why it does not drop in:

- The conversations-floater transcript (`conversations.rs`) is rendered as a
  **single flowed `Text` blob** by `format_transcript` (all lines joined into
  one string), and the transient chat overlay (`chat.rs`) is one `Text` line
  per message (`"{from_name}: {message}"`). **bevy_ui cannot make individual
  text spans within one `Text` node clickable** — picking hits the whole node.
  So each line must become a small row of separate nodes,
  `[name-link] : [body]`, with the name a `spawn_name_link` node.
- `TranscriptLine` currently stores only the speaker's **display name string**,
  not the speaker's `AgentKey`. To bind a `NameLink` it must carry the id:
  thread `from_agent_id` through the IM / direct paths, and accept that
  **nearby chat may not have a typed id** (`push_nearby` has none) — those lines
  stay plain, non-clickable (a `Loading`/`Unset`-style plain label), which the
  widget already models.

Scope:

- Restructure the transcript render into per-line rows (recall + live lines
  alike), the sender name a `NameLink` bound to `Set(agent)` when the id is
  known and a plain label otherwise; keep the "You" label for own lines
  non-clickable. Preserve the scroll / bounded-height behaviour.
- Decide whether the transient overlay (`chat.rs`) also gets clickable names or
  stays plain (it fades, so clicking is awkward — likely leave it plain and do
  only the persistent conversations transcript).
- Group-chat / IM participant names and the "X is typing…" line are candidates
  for the same treatment once the per-line row exists.

Reference (Firestorm, read-only): `LLChatHistory` / the name-link segments in
the chat log, and the click-name → profile behaviour.
