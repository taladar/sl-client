---
id: protocol-8
title: Script dialogs & permissions
topic: protocol
status: done
origin: ROADMAP.md
---

Context: [context/protocol.md](../context/protocol.md).

**8. Script dialogs & permissions · 3 pts. ✅ Done.** A vendor / scripted-object
interaction bot. Incoming scripted prompts surface as events:
`Event::ScriptDialog` (`llDialog`/`llTextBox` — object, owner, message, buttons,
hidden chat channel; `ScriptDialog::is_text_box` detects the `llTextBox` magic
button), `Event::ScriptPermissionRequest` (`llRequestPermissions`, with a
`ScriptPermissions` bitfield mirroring the LSL `PERMISSION_*` constants),
`Event::LoadUrl` (`llLoadURL`), and `Event::ScriptTeleport`
(`ScriptTeleportRequest`/`llMapDestination`). Replies:
`Session::reply_script_dialog` (`ScriptDialogReply` — chosen button on the
dialog's channel, also used to return `llTextBox` text) and
`Session::answer_script_permissions` (`ScriptAnswerYes` — grant a subset, or
`ScriptPermissions::default` to deny). Wired as
`Command::{ReplyScriptDialog, AnswerScriptPermissions}` through both runtimes.
Verified live against the local OpenSim (XEngine/YEngine enabled): an OAR-loaded
scripted prim fired `llDialog`, `llRequestPermissions(PERMISSION_DEBIT)` and
`llLoadURL` at the test avatar, which received all three events and replied to
the dialog. *Test: local OpenSim with the script engine enabled and a scripted
object (no headless rez path — a scripted prim must be loaded via an OAR or a
viewer).*
