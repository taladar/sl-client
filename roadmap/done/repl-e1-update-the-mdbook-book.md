---
id: repl-e1
title: Update the mdbook (book/)
topic: repl
status: done
origin: SL_REPL_ROAD_MAP.md — Phase E — docs & live verification
---

Context: [context/repl.md](../context/repl.md).

**E1. Update the mdbook (`book/`).** Added a new **Tools** section to
`book/src/SUMMARY.md` with `book/src/tools/sl-repl.md`, documenting the three
new crates, how to run them, the command grammar/placeholders/meta commands,
smoke mode, the wire-diagnostics surface, always-on logging/recording, and the
credential TOML + window-aligned MFA timing (`In this codebase` note maps it
all to modules). Updated the protocol pages for the diagnostics surface:
`comms/sessions.md` gained a **Diagnostics** section (the `Diagnostic` type vs
`Event`, the five variants, `set_diagnostics`/`poll_diagnostic`, the changed
`Client::run` signature, `SlDiagnostic`); `comms/lludp-transport.md`
(`Reader::position` → `DecodeFailed` offset, `ExpectedReplyMissing` on resend
exhaustion); `comms/messages.md` (a *When decoding goes wrong* section +
generated `message_name`); `comms/caps.md`
(`UnknownCapsEvent`/`CapsDecodeFailed`, the `report_caps_failure` sentinel →
`ExpectedReplyMissing`); `content/login.md` (TOML credentials + the
wall-clock-aligned MFA wait). Also listed the REPL crates in
`architecture.md`. `mdbook build` clean, `rumdl`/`typos` clean, all links
resolve.
