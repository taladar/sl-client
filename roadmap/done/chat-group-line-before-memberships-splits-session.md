---
id: chat-group-line-before-memberships-splits-session
title: A group chat line before the membership list opens a separate conference
topic: chat
status: done
origin: mixed-channel race audit (2026-11-23)
---

Context: [context/chat.md](../context/chat.md).

Confirmed by reading the code. Not reproduced live. Applies to Second Life,
which sends these lines with `from_group` clear.

## Bad order

A group line arrives by UDP `ImprovedInstantMessage` or event-queue
`ChatterBoxInvitation` before the event-queue `AgentGroupDataUpdate`, typically
just after login.

## Cause

`reads_as_group` (`sl-proto/src/session/methods.rs`) is false while
`member_groups` is empty, so the line opens a Conference session.
`note_memberships` replaces the list but never moves that session.

## Effect

One group conversation is split across a Conference tab and a Group tab for the
rest of the login.

## Fix

When a membership list arrives, re-key matching Conference sessions to Group
and emit an event so the UI moves the tab.

## Fix (2026-11-24)

Fixed as planned.

- When `note_memberships` takes a membership list or a successful join, it
  moves a `Conference` session under a group's id onto that group's session.
  A group session already open absorbs it, its lines first, adding its roster,
  typers and unread count.
- `Event::ChatSessionMovedToGroup` is queued before the membership event, and
  the viewer's Conversations floater moves the tab with `rekey`.
- `rekey` now puts the moved tab's lines in front when it merges into an
  existing tab. That is also the right order for a conference we started
  whose invitation beat the start reply.

Transcript lines already written to disk stay in the conference's file.

Tests: `a_group_line_before_the_memberships_moves_to_the_group` in
`sl-proto/tests/lifecycle.rs`, and the strengthened merge check in
`a_started_conference_follows_the_grid_s_session_id` in
`sl-viewer-people/src/conversations.rs`.
