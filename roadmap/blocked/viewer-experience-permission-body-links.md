---
id: viewer-experience-permission-body-links
title: Experience card name — clickable experience-profile SLURL
topic: viewer
status: blocked
origin: deferred from viewer-experience-permission-dialog (2026-07-29) — the
  experience-name link parked like the sibling toasts', pending the linkification
  layer
blocked_by: [viewer-url-linkification]
refs: [viewer-experience-permission-dialog, viewer-slurl-parse-dispatch]
---

Context: [context/viewer.md](../context/viewer.md).

The experience-acceptance toast ([[viewer-experience-permission-dialog]])
renders the experience name as **plain text**. The reference
`ScriptQuestionExperience` notification sets `[EXPERIENCE]` to
`secondlife:///app/experience/<id>/profile` — a clickable SLURL to the
experience profile. Once the shared linkification layer
([[viewer-url-linkification]]) lands, feed the experience name through it so the
name renders as a link to the experience profile, exactly as the sibling toasts
will ([[viewer-group-notice-body-links]], [[viewer-script-dialog-body-links]],
[[viewer-load-url-body-links]]) — the click dispatch for the SLURL is
[[viewer-slurl-parse-dispatch]]'s job, not this one's.

This is the same deferral chat's links carry: the card's name node becomes a
link-decorated text context instead of a bare `Text`. (The permission lines and
the object / owner names stay plain text — only the experience name is a link.)
