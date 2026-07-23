---
id: viewer-r28
title: Text-field caret nearly invisible — color, blink, focus cues
topic: viewer
status: done
origin: text-caret research (2026-07-23, user report)
refs:
  [
    viewer-ui-skin-tokens,
    viewer-ui-text-caret-grapheme-motion,
    viewer-ui-settings-binding-text,
  ]
---

Context: [context/viewer.md](../context/viewer.md).

Symptom: you often cannot tell whether the caret is in a given text field
until you start typing.

## Root causes (verified in code)

- **Caret color is dark-on-dark.** All eight `EditableText` spawn sites
  attach `TextCursorStyle::default()`, and Bevy 0.19's default caret
  `color` is `SLATE_700` (#334155, `bevy_text-0.19.0/src/cursor.rs`) — a
  palette chosen for light themes. Our skins put fields on near-black
  backgrounds (e.g. `srgb(0.10, 0.12, 0.16)`), so the caret rectangle is
  effectively invisible. Nothing routes the caret through the skin: the
  bevy_flair CSS (`assets/skins/*.css`) has no caret/cursor property and
  no code sets a non-default `TextCursorStyle`.
- **Blink exists but can't help.** Bevy does blink (visible the first
  half of `EditableText.cursor_blink_period`, default 1 s, timer reset on
  focus/layout change, drawn only while the entity holds `InputFocus` —
  `bevy_ui-0.19.0/src/widget/text_input_layout.rs`), but a blinking
  invisible rectangle is still invisible.
- **No focus cue on click-focus.** The shared focus outline runs only for
  *keyboard* focus (`InputFocusVisible`, `ui.rs`), so a field focused by
  clicking shows no indication at all.

## Reference behaviour (Firestorm, read-only)

- Caret color is **always theme-derived**: the line editor draws the
  caret in the field's *text colour* (`lllineeditor.cpp` draw, "Use same
  color as text for the Cursor"); the multi-line editor uses the
  skinnable `cursor_color` param bound to `TextCursorColor` in
  `colors.xml` (`lltextbase.cpp` ~L824-845).
- Blink: solid for `CURSOR_FLASH_DELAY` (1.0 s) after the last
  keystroke/focus, then a 1 Hz square flash (0.5 s on / 0.5 s off);
  `resetCursorBlink()` on every keystroke keeps it solid while typing;
  no blink when the app is unfocused or the field read-only.
- Focused editors additionally light a keyboard-focus border highlight
  regardless of how focus arrived (`mBorder->setKeyboardFocusHighlight`).
- **Overwrite mode** (Insert key, `gKeyboard->toggleInsertMode()`,
  global `LL_KIM_INSERT`/`LL_KIM_OVERWRITE`): the caret becomes a
  **block** at least a space wide covering the next glyph, which is
  re-rendered in the inverted text colour; IME preedit overwrites
  instead of inserting (`lllineeditor.cpp` ~L2196, `lltextbase.cpp`
  ~L827-858).

## Fix scope

- One shared caret-style constructor for every editor spawn (no more
  eight bare `default()`s), with the caret colour **from the skin/theme**
  — a skin token per [[viewer-ui-skin-tokens]] (bevy_flair needs a
  custom property or a skin-constant lookup for `TextCursorStyle`), per
  the user: caret colour must be themeable, and both bundled skins need
  a visible value (reference default: the text colour).
- Selection colours from the skin too while there
  (`selection_color` / `unfocused_selection_color`).
- Match the reference blink envelope: solid ~1 s after focus/keystroke,
  then 1 Hz flash (Bevy's per-editor `cursor_blink_period` + its
  layout-change reset get close; verify the keystroke-hold).
- Show a focus cue for click-focused text fields as well (extend the
  focus outline or a focused border colour, mirroring the reference's
  always-on keyboard-focus highlight).
- **Overwrite-mode block caret**: Insert toggles a (global, like the
  reference) overwrite mode; caret renders as a block over the next
  glyph with inverted glyph colour, and editing overwrites. Bevy's
  `bevy_text` editing has no overwrite concept, so this needs either an
  upstream contribution (preferred, per the fork-upstream convention) or
  a viewer-side caret overlay + edit-behaviour wrapper.

Verify with the gallery binary (fastest UI loop) across both skins:
caret visible immediately on click into every field kind, blinks when
idle, solid while typing, block caret in overwrite mode.

## Implemented (2026-07-23)

- **Skin-routed caret + selection colours.** New reflectable shim
  `SkinTextCaret` (`src/skin.rs`, the same pattern as the logical box
  components — `TextCursorStyle` itself is not reflectable), registered as
  the `caret-color` / `selection-color` / `unfocused-selection-color` CSS
  properties. `common.css` routes them on a new `.sk-text-field` class
  through new `--caret` / `--selection` / `--selection-unfocused` role
  tokens (defined in both skins + the dark overlay; caret = the text
  colour, the reference default). `stamp_text_field_class` tags every
  `Added<EditableText>` — no per-widget wiring, like the focus-ring stamp.
- **One shared caret style, no bare defaults.** `install_caret_style`
  (`src/ui_text_input.rs`) installs the style + blink state on every
  editor spawn; the three explicit `TextCursorStyle::default()`
  attachments are gone. Unskinned fallback: the field's own `TextColor`.
- **Reference blink envelope.** Bevy's immediate-flash blink is disarmed
  (`cursor_blink_period` pinned long) and `drive_caret_blink` reproduces
  the reference: solid `CURSOR_FLASH_DELAY` (1 s) after focus arrival or
  any keystroke (also on editor-generation change, so IME commits and
  programmatic edits hold it solid too), then a 1 Hz half-on/half-off
  flash. Envelope unit-tested.
- **Focus cue for click-focus.** `.sk-text-field:focus` in `common.css`
  shows the skin's focus ring on a text field for *any* focus source
  (other widgets keep `:focus-visible`-only), mirroring the reference's
  unconditional `setKeyboardFocusHighlight` border.
- **Overwrite mode.** Insert toggles a global `OverwriteMode` — only while
  a text field holds focus, like the reference's `LLLineEditor` handling
  `KEY_INSERT` (and so Insert aimed at an embedded CEF page never flips
  it). Typing overwrites: `apply_overwrite_edits` rewrites queued
  `TextEdit::Insert`s to `Delete` + insert before `apply_text_edits`
  drains them, with a line-end budget so it never eats a newline (plain
  insert at line/text end, IME preedit and selections left alone;
  unit-tested). The caret becomes a block ~a glyph wide
  (`EditableText::cursor_width`), drawn translucent so the covered glyph
  stays legible — an approximation of the reference's inverted-glyph
  block, which `bevy_ui`'s plain-rectangle caret cannot draw. Upstream
  overwrite support in `bevy_text` remains the eventual faithful path.

Verified in the gallery (2026-07-23): caret visible on click in both
skins, focus ring on mouse focus, solid-then-blink envelope, overwrite
block caret. One channel is moot for now: `unfocused-selection-color`
never paints, because Bevy's text-input widgets drop the selection
entirely when a field loses focus (the reference dims it instead) — the
token stays wired for when upstream preserves an unfocused selection.
The verification also caught (and fixed) a pre-existing gallery-startup
panic: `browser_widget::sync_browser_focus` demanded the viewer-only
`InputContext` resource through an unused parameter, so the gallery had
crashed at launch since the CEF web-media commit.

Remaining (deliberately out of scope, small): the reference suppresses the
blink while the *application window* is unfocused and for read-only
fields; IME preedit *overwriting* (rather than inserting) needs an
IME-aware editor hook Bevy does not expose.
