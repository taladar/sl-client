---
id: viewer-script-dialog-options
title: Script-dialog stacking, position & safety options
topic: viewer
status: ideas
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-dialog-lldialog, viewer-permission-request-dialog,
       viewer-anti-spam-filter, viewer-notification-toast-tuning]
---

Context: [context/viewer.md](../context/viewer.md).

Policy and presentation knobs around llDialog script dialogs — our
dialog floater (done [[viewer-dialog-lldialog]]) shows one dialog style
with no options. Stacking limits (`ScriptDialogLimitations`): keep only
one open dialog per object, per channel, per-channel-for-attachments,
and HUD variants — a griefing/spam control adjacent to
[[viewer-anti-spam-filter]]. Screen position
(`ScriptDialogsPosition`): docked toast flow vs pinned to one of the
four screen corners. Layout: visible button rows per dialog
(`FSRowsPerScriptDialog`), V1-style animated slide-in
(`FSAnimatedScriptDialogs`), opaque background
(`FSScriptDialogNoTransparency`), and removing the Block button from
the dialog row (`FSRemoveScriptBlockButton`).

Safety-relevant and worth pulling forward even alone: LSL *debit*
permission dialogs (the money-taking permission,
[[viewer-permission-request-dialog]] is done) defaulting their focused
button to Deny (`FSPermissionDebitDefaultDeny`) so an accidental Enter
cannot grant a scripted object the right to take L$.

Reference (Firestorm, read-only):
`indra/newview/fstoastscripttextbox.cpp`,
`indra/newview/llnotificationscripthandler.cpp`,
`indra/newview/llscriptfloater.cpp`,
`indra/newview/skins/default/xui/en/panel_preferences_UI.xml`.
