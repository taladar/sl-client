---
id: viewer-chat-spellcheck
title: Spellcheck in text inputs
topic: viewer
status: ready
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-ui-text-input-widget]
---

Context: [context/viewer.md](../context/viewer.md).

Spellchecking in the viewer's text inputs — chat first, but implemented at the
text-input-widget layer so IM, notecard and profile editors inherit it:
misspelled words get an underline decoration, and the context menu on such a
word offers suggestions, "add to dictionary" and "ignore".

Scope: a spellcheck service around a Rust hunspell-compatible checker
(evaluate `spellbook` — pure Rust, reads hunspell `.dic`/`.aff` — before
binding the C hunspell), dictionary discovery + a per-language download /
import story like the reference's, a user dictionary in the account dirs, the
underline render in the text widget, the suggestion context menu, and the
enable/language settings (surfaced later via the preferences cluster).

Reference (Firestorm, read-only): `llspellcheck`, `floater_spellcheck.xml`,
`floater_spellcheck_import.xml`.

Builds on: [[viewer-ui-text-input-widget]] and the settings store.
