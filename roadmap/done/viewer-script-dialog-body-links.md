---
id: viewer-script-dialog-body-links
title: Script dialog body — clickable URLs / SLURLs
topic: viewer
status: done
origin: deferred from viewer-dialog-lldialog (2026-07-29) — links parked like
  chat's, pending the linkification layer
blocked_by: [viewer-url-linkification]
refs: [viewer-dialog-lldialog, viewer-slurl-parse-dispatch]
---

Context: [context/viewer.md](../context/viewer.md).

The script-dialog toast ([[viewer-dialog-lldialog]]) renders the `llDialog` /
`llTextBox` message as **plain text**. The reference `LLToastNotifyPanel` sets
its message text box up to parse URLs, so a real dialog's `http(s)` URLs and
`secondlife:///app/...` SLURLs are clickable. Once the shared linkification
layer ([[viewer-url-linkification]]) lands, feed the dialog message through it
so those runs render as links, exactly as nearby chat / IM and the group notice
([[viewer-group-notice-body-links]]) will — the click dispatch for a SLURL is
[[viewer-slurl-parse-dispatch]]'s job, not this one's.

This is the same deferral chat's links carry: the toast's message node becomes a
link-decorated text context instead of a bare `Text`.

## Outcome (2026-08-09)

Wired: the script-dialog message now renders through the shared linkification
widget (`crate::linkified_text::spawn_linkified_text`) instead of a bare `Text`,
so its `http(s)` URLs / SLURLs are clickable (hover shows the URL). Followed the
`spawn_bounded_linked_text` pattern beside the existing bounded-text helper.
