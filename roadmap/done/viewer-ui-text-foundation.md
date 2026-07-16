---
id: viewer-ui-text-foundation
title: UI text & font foundation (bevy_ui + parley bring-up)
topic: viewer
status: done
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

## Outcome (2026-07-16)

Stood up in `sl-client-bevy-viewer/src/ui_text.rs`: `system_font_discovery`
enabled, the `CBDT` build of Noto Color Emoji bundled (`assets/fonts/`, OFL) and
bound as the `Emoji` generic, and one prefilled multi-line `EditableText` behind
the `F4` key / `SL_VIEWER_TEXT_DEMO`.

Results against the four checks — **two pass, one fails, one unverified**:

1. **Bidi** — passes. Verified headless
   (`caret_moves_in_visual_order_across_a_bidi_boundary`): a rightward caret
   visits byte offsets non-monotonically through an RTL run, i.e. it moves in
   visual order. Selection *geometry* splitting is still only eyeballed.
2. **Grapheme editing** — **fails**, and the premise that it is "inherited from
   parley" is false: `backdelete` deletes one *codepoint* except for hard line
   breaks and single emoji clusters. Follow-up:
   [[viewer-ui-text-grapheme-backdelete]] (tripwire test records the measured
   counts).
3. **IME** — **unverified**: the dev machine has no input method configured.
   Deferred to [[viewer-ui-text-ime-verification]].
4. **No tofu / colour emoji** — passes, confirmed live. CJK renders via system
   fallback; emoji render in colour.

Two further gaps surfaced, both filed:
[[viewer-ui-text-font-family-selection]] (a `FontSource` **generic** primary
pulls in the host's COLRv1 emoji font, which shadows the bundled colour font and
renders blank — this is why the demo keeps the default single-font primary) and
[[viewer-ui-text-emoji-presentation]] (VS16 does not override a text font's own
glyph, so `❤️` stays monochrome).
