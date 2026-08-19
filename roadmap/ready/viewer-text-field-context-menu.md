---
id: viewer-text-field-context-menu
title: Text-field edit context menu (cut / copy / paste / select all)
topic: viewer
status: ready
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-ui-text-input-widget, viewer-ui-context-menu,
  viewer-chat-spellcheck]
---

Context: [context/viewer.md](../context/viewer.md).

The reference offers a right-click menu on every text input and text
editor: Cut / Copy / Paste / Delete / Select All (menu_text_editor.xml;
the same verbs also live on the viewer Edit menu, menu_edit.xml), with
spellcheck entries prepended when the click lands on a misspelled word —
that spellcheck half belongs to [[viewer-chat-spellcheck]].

Our text widget (`sl-client-bevy-viewer/src/ui_text_input.rs`,
[[viewer-ui-text-input-widget]] done) supports the keyboard shortcuts
but has no context menu at all, so mouse-first users cannot paste into
the chat bar or a search field. Scope: one shared MenuDef over the
line-menu widget ([[viewer-ui-context-menu]] done), targeting the
focused/clicked editable text, with enable states derived from the
current selection and clipboard content (`clipboard.rs`).

Reference (Firestorm, read-only):
`indra/newview/skins/default/xui/en/menu_text_editor.xml`,
`menu_edit.xml`; `indra/llui/lllineeditor.cpp`,
`indra/llui/lltexteditor.cpp` (`createDefaultContextMenu`).
