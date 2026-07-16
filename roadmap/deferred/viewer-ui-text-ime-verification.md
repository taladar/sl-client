---
id: viewer-ui-text-ime-verification
title: Verify IME preedit & candidate placement on an IME-capable host
topic: viewer
status: deferred
origin: unverified check from viewer-ui-text-foundation (2026-07)
refs: [viewer-ui-text-foundation, viewer-ui-text-input-widget]
---

Context: [context/viewer.md](../context/viewer.md).

Requirement 3 of [[viewer-ui-text-foundation]] — a live CJK IME shows preedit
and places its candidate window — is **not verified**. The dev machine has no
input method configured, so the check could not be performed; the other three
requirements were checked there. Deferred rather than blocked: the barrier is an
environment, not another task.

The code path is wired and believed correct, but unproven end to end:
`bevy_ui_widgets::EditableTextInputPlugin` (in `DefaultPlugins`) runs
`on_ime_input`, translating `Ime::Preedit` → `TextEdit::ImeSetCompose` (with a
`PreeditCursor`) and `Ime::Commit` → `TextEdit::ImeCommit`;
`listen_for_ime_input_when_text_input_focused` sets `Window::ime_enabled` while
an `EditableText` has focus; and `update_ime_position` drives
`Window::ime_position` from `PlainEditor::ime_cursor_area()`.

Do, on a host with an IME (e.g. fcitx5 / ibus + mozc or pinyin):

- Run the viewer, press `F4`, click the demo editor, and compose CJK text.
- Confirm preedit text appears underlined at the caret, the candidate window is
  positioned at the caret (not at the window origin), committing inserts the
  text, and `Escape` / focus loss clears composition without inserting.
- Confirm the preedit is excluded from `EditableText::value()` until commit.

Known limitation to confirm, not fix here: winit exposes a **single** preedit
cursor range, while the reference viewer (`llpreeditor`) models composition as
clause segments with standout flags — so a multi-clause Japanese conversion
cannot be rendered with per-clause emphasis. [[viewer-ui-text-input-widget]]
already budgets for this; record what the IME actually delivers so that task can
size the work.
