---
id: viewer-group-chat-snooze
title: Per-group chat snooze and group-mute options
topic: viewer
status: ready
origin: debug-settings/chat-lines survey (2026-07-23)
refs: [viewer-social-groups]
---

Context: [context/viewer.md](../context/viewer.md).

Temporarily silence a noisy group without leaving it: Firestorm lets you
snooze a group chat session for a duration (per-group configurable),
mute all group chat wholesale, and auto-mute groups whose notices you
have disabled.

Scope:

- Snooze action on a group session: suppress its message toasts/opens
  for N minutes, with a per-group duration option
  (`FSEnablePerGroupSnoozeDuration`); the session auto-wakes after.
- Mute-all-groups master toggle (`FSMuteAllGroups`).
- Mute groups with notices disabled (`FSMuteGroupWhenNoticesDisabled`).
- Snoozed/muted state visible in the conversation list.

Reference (Firestorm, read-only): the `FSMute*`/snooze settings and
`fsfloaterim` group-session handling.

Builds on: group chat sessions and the groups UI (done,
[[viewer-social-groups]]).
