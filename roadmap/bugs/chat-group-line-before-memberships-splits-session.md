---
id: chat-group-line-before-memberships-splits-session
title: A group chat line before the membership list opens a separate conference
topic: chat
status: bugs
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
