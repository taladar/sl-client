---
id: viewer-chat-autoreplace
title: Chat auto-replace rules
topic: viewer
status: ready
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-ui-widget-scaffold]
refs: [viewer-chat-spellcheck]
---

Context: [context/viewer.md](../context/viewer.md).

The auto-replace engine: as-you-type word substitution in chat/IM inputs from
user-editable rule lists (classic uses: abbreviation expansion, typo fixing,
accent insertion). Replacement fires on a word boundary, and an immediate
undo (backspace) restores the literal text — matching the reference so it
never fights the user.

Scope: the rule model (ordered lists of `keyword → replacement`, each list
individually on/off), persistence in the account dirs, import/export of the
reference's XML list format (existing FS users bring their lists), the
replacement hook in the text-input widget, and the settings floater to edit
lists and rules.

Reference (Firestorm, read-only): `llautoreplace`, `floater_autoreplace.xml`.

Builds on: the text-input widget and the settings store.
