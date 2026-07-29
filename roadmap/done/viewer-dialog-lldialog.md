---
id: viewer-dialog-lldialog
title: llDialog script dialogs + textbox prompts
topic: viewer
status: done
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-notifications-dialogs
blocked_by: [viewer-ui-notification-host]
refs: [viewer-script-dialog-body-links]
---

Context: [context/viewer.md](../context/viewer.md).

The `llDialog` script dialogs and `llTextBox` textbox prompts a scripted object
throws at the user: a message plus up to twelve buttons (or a text-entry field),
each button reply sent back on the script's listen channel. Rendered as a toast
on the notification host ([[viewer-ui-notification-host]]) with the buttons /
input wired to the reply path.

Much of the underlying protocol is already handled; this task is the specific
dialog panel and its button / textbox reply wiring on top of the notification
host.

Reference (Firestorm, read-only): `lltoastscriptquestion`, `llscriptfloater`,
`llui/llnotifications`.

Builds on: the script-dialog protocol already done.

Done (2026-07-29): `src/script_dialog.rs` — a bespoke card adopted into the
shared toast channel (object / owner title, message, a three-column button grid
filled bottom-up like the reference, or an `llTextBox` field + Submit), plus the
built-in Block (mute) / Ignore actions. A button / Submit sends
`Command::ReplyScriptDialog` on the hidden chat channel; Ignore / × dismisses
with no reply. Two gallery specimens (buttons + text box) and unit tests for the
grid ordering, owner display and form classification. **Links in the message are
deferred** like chat's — follow-up [[viewer-script-dialog-body-links]].
