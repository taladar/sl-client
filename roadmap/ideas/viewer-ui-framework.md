---
id: viewer-ui-framework
title: In-viewer UI / floater-panel-menu framework
topic: viewer
status: ideas
origin: reference-viewer feature-cluster survey (2026-07)
---

Context: [context/viewer.md](../context/viewer.md).

The shell every other panel needs: draggable / dockable windows (floaters),
panels, menus, buttons, lists, tree views, text input, tab containers, a
notifications host, and theming / skins. Today the viewer only has fixed
`bevy_ui` overlays (`chat.rs`, `diagnostics.rs`) — no widget framework, no
windowing, and **no text input anywhere in the crate**. 36 other tasks wait on
this one, which makes it the most consequential architectural choice left.

## Hard requirements

Stated up front, because they gate the choice rather than being discovered late:

- **Non-BMP Unicode.** SL chat, names and notecards carry emoji and other
  astral-plane codepoints. Rendering *and editing* (caret, selection, backspace)
  must be **grapheme-cluster** correct, not char- or UTF-16-indexed.
- **Bidi.** RTL (Arabic / Hebrew) mixed with LTR must render per the Unicode
  Bidirectional Algorithm, with correct caret and selection in editable fields.
- **IME.** Preedit / composition display and candidate-window placement (CJK).
- **i18n / l10n.** Translated strings with named substitutions, plurals and
  gender — see [[viewer-i18n-localization]].

## What the reference actually is (Firestorm XUI, surveyed 2026-07)

Worth knowing precisely, because "match XUI" is a much smaller job than it
sounds — and in two places matching it would mean shipping a bug.

- **687** XUI files in the `default/en` skin (6.6 MB); ~138 registered widget
  types, but only ~40 are generic primitives — the other ~100 are viewer-domain
  composites (inventory trees, chiclets, texture pickers, the net map) that we
  will write in any toolkit.
- **Skinning is in practice a design-token exercise.** 6 selectable skins and
  **21 themes — not one theme overrides a layout**; every one is `colors.xml`
  (447 named colours) plus named textures (939). Skins override between 0.9 %
  and 9.6 % of layout files, mostly the same handful, largely to paper over
  panels that size badly under different metrics.
- **Layout is 9,913 absolute `topleft` pixel rects** plus sibling-relative
  `top_pad` / `top_delta` offsets, against only 196 `layout_stack`s (a 1-D flex
  with `auto_resize` + `min_dim`). This is why *translation edits geometry*: the
  German `floater_inspect.xml` overrides `width="155"` so a longer label fits. A
  real layout engine (Bevy already ships taffy/flexbox) is a strict superset and
  dissolves that whole class of breakage. The XUI files cannot be imported
  anyway — the pixel coordinates *are* the design.
- **Firestorm has no bidi at all.** No HarfBuzz, no FriBidi, no ICU anywhere;
  text is laid out by walking codepoints LTR at FreeType advance widths. No RTL
  locale ships, and none could. We would exceed the reference here without
  trying — matching it means deliberately shipping broken text. (Non-BMP it
  *does* handle: UTF-32 `LLWString`, colour emoji through an OpenType-SVG
  Twemoji fallback predicated on a user setting.)
- **IME, by contrast, is real and load-bearing** — `LLPreeditor`, implemented by
  the two editor widgets, with Win32 / SDL2 / Cocoa backends. Japanese is a
  top-tier SL locale.

**Copy:** the design-token indirection (named colours / textures / fonts);
`control_name=`-style **two-way binding to a settings store** (1,293 uses — it
is why ~20 preference panels have almost no code behind them); an
**id-keyed recursive merge** for overlays; runtime-loaded, hot-reloadable UI
definitions; the declarative notification catalogue.

**Do not copy:** `LLInitParam` (2,000 lines of C++ templates reimplementing
serde); the absolute-rect layout model; whole-file skin replacement (the reason
a skin forks a 3,500-line `floater_tools.xml` and then breaks every release);
and `LLTrans::getCountString` (a hardcoded if-ladder over three languages, wrong
for Polish — which ships).

## The Rust / Bevy landscape (surveyed 2026-07)

**Bevy 0.19 replaced cosmic-text with `parley` 0.9** (Linebender: harfrust
shaping, ICU segmentation, fontique fallback). That decides most of it, because
the hard requirements are already met *inside our own dependency tree*:

- `parley::bidi` is an in-crate UAX #9 implementation; `move_left` /
  `move_right` walk clusters in **visual order**, and `selection_geometry()`
  returns multiple rects, so selections split correctly across bidi runs.
- `backdelete()` deletes the previous **cluster**, with an explicit emoji case —
  grapheme-correct editing, not codepoint-correct.
- `set_compose()` / `clear_compose()` / `ime_cursor_area()` — the IME half.
- Bevy already ships the transport (`Ime::Preedit` / `Ime::Commit`,
  `Window::ime_enabled`, `set_ime_cursor_area`), `bevy_ui_widgets`'
  `EditableText` consumes that whole loop, and `bevy_input_focus` gives focus
  and tab navigation. Both are in the default `ui` feature.

**Recommendation: `bevy_ui` + `bevy_flair` (skins) + `bevy_fluent` (i18n).**
`bevy_flair` is real CSS — selectors, pseudo-classes, `@keyframes`, `var()`,
`@font-face`, hot-reloaded `.css` — a *better* skin language than XUI and the
natural home for the token system. `bevy_fluent` puts Project Fluent `.ftl`
behind Bevy assets with runtime locale switching (named args, plurals, gender).
Both released within three days of Bevy 0.19, so the ecosystem is tracking
rather than lagging.

Rejected, each on a single killer objection: **egui** (`TextBuffer` is
char-indexed; bidi open since 2021 with no plan; the parley rewrite is a stalled
draft), **iced** (wgpu 27 against Bevy's 29; RTL caret broken; no
virtualization), **Xilem / masonry** (pins wgpu 28 — cannot even share a Device
with Bevy 0.19; no release in 9 months), **Blitz / Dioxus** (no drag-and-drop,
no `contenteditable`, no virtualization), **CEF** (meets everything; costs
300–500 MB and a Chromium security treadmill). **Slint** is the runner-up — wgpu
29 matches exactly and it has a runtime-loadable `.slint` interpreter — but it
is GPLv3-or-paid and cannot edit bidi text.

## What we would still have to build

The toolkit gives primitives, not a viewer. Missing upstream, ours to write:

1. **The floater window manager** — drag / resize / z-order / focus / minimize /
   dock / tear-off. Nothing upstream has it; every SL viewer hand-writes it.
2. **A virtualized list / tree.** Bevy's `ListBox` spawns one entity per row, so
   a 10k-item inventory ([[viewer-inventory-ui]]) means 10k taffy nodes.
   Windowed recycling is DIY and is **the main technical unknown** — no prior
   art at that scale in Bevy.
3. **A syntax-highlighted editor** for [[viewer-lsl-script-editor]]:
   `parley::PlainEditor` is *plain* (one whole-buffer style set, no per-range
   styles, no undo). That gap is in parley itself, so every parley-based toolkit
   inherits it equally — build on `RangedBuilder` +
   `editing::{Cursor,Selection}`.
4. Tab containers, toasts ([[viewer-notifications-dialogs]]), the settings-bound
   widget layer ([[viewer-preferences-ui]]), and the **IME preedit rendering**:
   winit hands us a single preedit cursor range, while the reference models
   composition as clause segments plus standout flags. Budget real time for it.

Also enable the **`system_font_discovery`** feature — we do not have it today,
and without it `parley`/`fontique` does no OS font fallback, so any chat line
with CJK, Cyrillic, Arabic or emoji renders as **tofu**. It does not solve
colour emoji: swash rasterises COLRv0 / CBDT / sbix but **not COLRv1**, so a
COLRv1-only system Noto renders blank and we must bundle a CBDT/sbix font.

## First step

Spike it before committing: one `EditableText`, `system_font_discovery` on, a
bundled colour-emoji font — then check (a) mixed Arabic/Hebrew + Latin caret
movement and selection rects, (b) backspace over an emoji ZWJ family and a
regional-indicator flag deletes one *grapheme*, (c) a live CJK IME shows preedit
and places its candidate window, (d) a CJK + emoji chat line shows no tofu. If
those pass, adopt `bevy_ui` and budget the floater / tree / editor work; if they
fail, fall back to Slint on its licence terms.

Reference (Firestorm, read-only): `indra/llui/` (`llfloater`, `llpanel`,
`llmenugl`, `lllayoutstack`, `llfolderview`), `lluictrlfactory`, `llpreeditor`,
`lltrans`, and the XUI layouts + skins under `newview/skins/`.

Builds on: the current `bevy_ui` overlays. Supersedes the MVP "no non-quit UI"
non-goal.
