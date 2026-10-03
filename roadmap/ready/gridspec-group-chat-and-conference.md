---
id: gridspec-group-chat-and-conference
title: Group chat sessions and ad-hoc conferences on each grid
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, chat-group-history-server-side,
  viewer-group-session-moderation,
  test-conference-roster,
  viewer-conference-start-ui]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

OpenSim has no `ChatSessionRequest` cap and no ad-hoc conferences, delivers
group messages inline to non-joined members; SL drops messages into a fresh
group until membership propagates, keeps server history, re-keys conferences.

## Discover

The group-session cases with a propagated pre-made group on aditi;
`test-conference-roster` (3 avatars); moderation; agent-list transitions;
history size.

## Document

`book/src/gridspec/chat.md` § Group and conference sessions.

## Fake grid

Large — [[server-fake-grid-groups]] (sessions) and [[server-fake-grid-im-relay]]
(conferences).

## Viewer

History fetch degrades on OpenSim; inline delivery without invitation; the SL
re-key.
