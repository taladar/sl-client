---
id: viewer-ui-text-foundation
title: UI text & font foundation (bevy_ui + parley bring-up)
topic: viewer
status: ready
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-ui-framework
---

Context: [context/viewer.md](../context/viewer.md).

Stand up the committed `bevy_ui` + `parley` text stack and prove it meets the
hard text requirements before the widget layer is built on top. This is the root
of the whole UI cluster: every panel, chat line, name and notecard flows through
it, so it is fixed first and everything else `blocked_by` it (transitively).

**`bevy_ui` is the toolkit — this is not a go/no-go.** Bevy 0.19 replaced
cosmic-text with `parley` 0.9 (harfrust shaping, ICU segmentation, fontique
fallback), and `bevy_ui_widgets`' `EditableText` + `bevy_input_focus` are in the
default `ui` feature. egui was rejected (char-indexed `TextBuffer`, no bidi) and
Slint is paid-or-GPLv3, so there is no alternative to fall back to. If a check
below fails, we file a follow-up and fix it *within* the bevy stack.

Do:

- Enable the **`system_font_discovery`** feature (we do not have it today —
  without it `parley`/`fontique` does no OS font fallback and any CJK / Cyrillic
  / Arabic / emoji line renders as **tofu**).
- Bundle a **CBDT/sbix colour-emoji font** (swash rasterises COLRv0 / CBDT /
  sbix but **not** COLRv1, so a COLRv1-only system Noto renders blank).
- Bring up one `EditableText` and confirm the four hard requirements
  render/edit correctly:
  1. **Bidi** — mixed Arabic/Hebrew + Latin: caret moves in visual order and
     selection geometry splits correctly across runs
     (`parley::bidi`, `move_left`/`move_right`, `selection_geometry()`).
  2. **Grapheme editing** — backspace over an emoji ZWJ family and a
     regional-indicator flag deletes exactly one *grapheme* (`backdelete()`).
  3. **IME** — a live CJK IME shows preedit and places its candidate window
     (`set_compose`/`clear_compose`/`ime_cursor_area`; Bevy transports
     `Ime::Preedit`/`Ime::Commit`, `Window::ime_enabled`,
     `set_ime_cursor_area`).
  4. **No tofu** — a CJK + emoji chat line renders fully with colour emoji.

File a follow-up task for any gap surfaced (e.g. richer IME preedit than winit's
single cursor range).

Reference (Firestorm, read-only): `indra/llui/` text widgets, `llpreeditor`
(the IME model). Builds on the current `bevy_ui` overlays (`chat.rs`,
`diagnostics.rs`).
