---
id: viewer-ui-text-input-widget
title: Reusable text-input widget (EditableText + IME preedit)
topic: viewer
status: done
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-ui-framework
blocked_by: [viewer-ui-widget-scaffold]
---

Context: [context/viewer.md](../context/viewer.md).

A reusable single- and multi-line text-input widget wrapping `bevy_ui_widgets`'
`EditableText`, with **IME preedit rendering** — winit hands us a single preedit
cursor range, while the reference models composition as clause segments plus
standout flags, so budget real time for the display. This is the widget every
text-entry surface consumes: chat input, IM, search fields, the rebinding
editor, notecard/script editors.

The caret and selection API must be expressed in **logical** terms
(`forward`/`backward`, not left/right) and be bidi-correct via `parley`'s
visual-order cluster movement and multi-rect `selection_geometry()` — the
groundwork proven in [[viewer-ui-text-foundation]]. Grapheme-correct editing
(`backdelete()`) is inherited from parley.

Out of scope here (its own task): the syntax-highlighted, per-range-styled,
undo-capable editor for [[viewer-lsl-editor-widget]] — `parley::PlainEditor` is
*plain* (one whole-buffer style set), a gap in parley itself, so that editor
builds on `RangedBuilder` + `editing::{Cursor,Selection}` separately.

Reference (Firestorm, read-only): `lllineeditor`, `lltexteditor`, `llpreeditor`.

## Done (2026-07-20)

`sl-client-bevy-viewer/src/ui_text_input.rs` — `spawn_text_input` builds a
bordered, keyboard-reachable field over `bevy_text`'s `EditableText`, selected
by `TextInputKind`: a single-line `Line`, a wrapping/scrolling `Multiline`, and
the three numeric single-line variants `Float` (signed decimal), `Integer`
(signed), and `NonNegativeInteger` (digits only, the sign key rejected — the
"positive integer" field).

**What is built vs inherited.** Bidi (visual-order caret, split selection
geometry), grapheme-correct backspace, and the baseline IME (winit's single
preedit range, transported and rendered by `bevy_ui_widgets`) are **inherited**
from [[viewer-ui-text-foundation]] and parley — the caret/selection API is
already logical, so an RTL field needs nothing here. What this task adds is the
field **chrome**, the single-/multi-line split, and **numeric validation**.

**Numeric validation is two layers**, because a number is not a per-character
property (`1.2.3` is all legal float characters). `EditableTextFilter` (a
`bevy_text` per-character filter) blocks a disallowed *character* as typed
(flicker-free); the whole-string *arrangement* (one `.`, a `-` only at the
front) is enforced by `enforce_numeric_intermediate`, which reverts a field to
its last valid value when an edit makes it structurally invalid. The revert runs
after `EditableTextSystems` but before the editable-glyph layout
(`UiSystems::PostLayout`), so the rejected keystroke never reaches the screen.
Validators admit the intermediate states a number is typed *through* (`""`, `-`,
`1.`); `TextInputKind::parse` reads a committed value and returns `None` for
those.

Constructible without wiring (`crate::ui_element`): five specimens registered in
`ELEMENTS` (swept by `crate::ui_test` across every script, direction, scale and
font size), and an `F8` demo panel (`SL_VIEWER_TEXT_INPUT_DEMO`) with a live
`parse` read-out per numeric row for by-hand typing / IME / rejection checks.

**Deferred, not skipped.** The *richer* clause-segmented IME preedit the
reference viewer draws (`llpreeditor`) is blocked on winit exposing more than a
single cursor range and on an IME-capable host, and is tracked by
[[viewer-ui-text-ime-verification]] (which already names this task as its
budget-holder). Caret *motion* stepping one codepoint rather than one grapheme
is the pre-existing upstream issue [[viewer-ui-text-caret-grapheme-motion]];
nothing here depends on it.
