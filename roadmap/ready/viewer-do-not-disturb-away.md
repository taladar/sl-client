---
id: viewer-do-not-disturb-away
title: Away / auto-AFK / Do-Not-Disturb modes + autoresponse
topic: viewer
status: ready
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-chat-input-bar]
refs: [viewer-name-tags-decorations]
---

Context: [context/viewer.md](../context/viewer.md).

The presence modes and their behaviours:

- **Away** — manual toggle plus auto-AFK after N minutes of no input
  (configurable timeout); plays `ANIM_AGENT_AWAY`, sets the away agent flag,
  shows "(Away)" in the tag ([[viewer-name-tags-decorations]]), and clears on
  input.
- **Do Not Disturb (Busy)** — manual mode: suppress IM/inventory-offer
  toasts (queue them for later), send the configurable busy auto-response to
  IM senders, decline teleport offers, play `ANIM_AGENT_DO_NOT_DISTURB`.
- **FS autoresponse** — the Firestorm extension: an auto-reply mode separate
  from DND (respond but keep receiving), with per-mode reply texts and an
  "only to non-friends" option; shown in the own tag.

Scope: the mode state machine, the agent-flag / animation wire writes, the
IM-side auto-reply + toast queueing, the timeout setting, and the menu /
status entries to switch modes. The reply texts and timeouts persist in the
settings store per account.

Reference (Firestorm, read-only): `llagent` (busy/away), `fsautoresponse`
settings, `llimview` (DND queueing).

Builds on: the IM/chat session layer and the settings store.
