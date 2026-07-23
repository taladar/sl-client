---
id: viewer-anti-spam-filter
title: Incoming-event anti-spam / flood protection
topic: viewer
status: ready
origin: debug-settings/chat-lines survey (2026-07-23)
refs:
  [viewer-block-list, viewer-dialog-offers-invites, viewer-particle-pick-mute]
---

Context: [context/viewer.md](../context/viewer.md).

Firestorm's NACL antispam is a per-source rate limiter across every
inbound channel a griefer can flood: chat, IMs, group invites, script
dialogs, sounds and sound preloads, particles, and teleport/inventory
offers. A source exceeding N events in T seconds is silenced for the
session, with an optional notification naming the throttled source.

Scope:

- A shared rate-limiter keyed by source (agent/object UUID) with
  configurable amount/time (`_NACL_AntiSpamAmount`, `_NACL_AntiSpamTime`)
  and a global-queue option (`_NACL_AntiSpamGlobalQueue`).
- Hook the dispatch points: nearby chat + IM ingest, dialog
  (`ScriptDialog`) delivery, sound trigger/preload ingest, particle
  ingest, offer/invite delivery.
- Sound/preload multipliers (`_NACL_AntiSpamSoundMulti`,
  `_NACL_AntiSpamSoundPreloadMulti`) and a newline-flood guard
  (`_NACL_AntiSpamNewlines`).
- Object inventory-offer flood throttle with a summarised "N offers from
  X throttled" notice (`FSNotifyIncomingObjectSpam`,
  `FSOfferThrottleMaxCount`).
- "Even my own objects" option (`FSUseAntiSpamMine`); master toggle
  (`UseAntiSpam`); throttle notices report to chat when enabled.

Reference (Firestorm, read-only): `NACLantispam.cpp`/`.h`, the
`_NACL_AntiSpam*` / `FSNotifyIncomingObjectSpam*` settings.

Builds on: chat/IM ingest (done); dialog, sound, and offer delivery hook
in as those systems land ([[viewer-dialog-offers-invites]]).
