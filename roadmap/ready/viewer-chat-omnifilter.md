---
id: viewer-chat-omnifilter
title: Omnifilter chat-filtering floater
topic: viewer
status: ready
origin: main-menu survey (2026-07-23)
refs:
  [
    viewer-chat-keyword-alerts,
    viewer-chat-autoreplace,
    viewer-chat-history-panel,
  ]
---

Context: [context/viewer.md](../context/viewer.md).

Firestorm's Omnifilter (Comm ▸ Omnifilter) is a rule-based chat filter:
a user-managed list of match rules (plain text or regex) with an action
per rule — hide the line, highlight it, or rewrite it — applied to the
nearby-chat and IM pipelines. Stronger than keyword alerts
([[viewer-chat-keyword-alerts]], notify-only) and than autoreplace
([[viewer-chat-autoreplace]], outbound rewriting).

Scope:

- Rule model: match (text/regex, case toggle, scope: nearby/IM/group) +
  action (hide, highlight colour, replace text), persisted per account.
- Apply rules on the inbound chat pipeline before transcript insert;
  hidden lines are dropped from display (optionally still logged).
- A management floater: rule list with add/edit/remove/reorder and
  enable checkboxes.

Reference (Firestorm, read-only): the `omnifilter` floater
(`Floater.Toggle omnifilter`, `menu_viewer.xml` Comm section) and its
implementation under `indra/newview/fs*`.

Builds on: the chat ingest pipeline and transcript UI (done).
