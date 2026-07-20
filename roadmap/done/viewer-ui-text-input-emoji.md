---
id: viewer-ui-text-input-emoji
title: Emoji entry on the single-line text field
topic: viewer
status: done
origin: follow-up requested during viewer-ui-text-input-widget (2026-07)
blocked_by: [viewer-ui-text-input-widget]
refs: [viewer-emoji-picker-floater, viewer-emoji-colon-autocomplete, viewer-chat-input-bar, viewer-emoji-data]
---

Context: [context/viewer.md](../context/viewer.md).

The **field-side** emoji-entry affordance for an arbitrary single-line text
field ([[viewer-ui-text-input-widget]]) — the piece the **chat input**
([[viewer-chat-input-bar]]) needs and its first consumer. Colour-emoji
*rendering* already works (from [[viewer-ui-text-foundation]]); what is missing
is a way to *select and insert* an emoji into a running text field.

This is deliberately narrower than the two standalone emoji surfaces it composes
with, and exists to draw the boundary between them:

- the grouped, searchable palette is [[viewer-emoji-picker-floater]] (a
  floater);
- the inline `:smile:` short-code completer is
  [[viewer-emoji-colon-autocomplete]].

What belongs *here* is the widget-level integration both of those rely on: an
opt-in emoji affordance on a single-line field (an emoji button beside / inside
the field that opens the picker anchored to the field), and the shared
**insert-glyph-at-the-caret** primitive that drops a chosen emoji into the
`EditableText` at the current selection and leaves the caret after it —
grapheme- and IME-correct, reusing the widget's existing edit path rather than a
raw `set_text`. A `TextInputSpec` opt-in (`emoji: bool`, off by default) turns
it on, so chat input and IM get it while a numeric or a name field does not.

Short-codes and glyph data come from [[viewer-emoji-data]] (the `sl-emoji`
crate). Keep it constructible without wiring per [[viewer-ui-widget-scaffold]]:
inserting a glyph is pure field state and reaches no session.

Reference (Firestorm, read-only): `llchatentry` (the chat line editor with its
emoji button), `llemojihelper` (the entry helper shared by the fields).

## Done (2026-07-20)

Realized as the reusable **chat-input widget** rather than a bare-field
`TextInputSpec.emoji` flag (the emoji button sits *beside* the field, which is a
composition, not a field property) — `sl-client-bevy-viewer/src/chat_input.rs`.
`spawn_chat_input` builds a bordered box around a bare, filling single-line
[[viewer-ui-text-input-widget]] field with a trailing **emoji button** that, on
press, writes `OpenEmojiPicker { field, near: press-location }` — a new message
[[viewer-emoji-picker-floater]] handles by targeting that field, anchoring the
window at the click (new `Floater::set_position`), showing and raising it. The
**insert-glyph-at-the-caret** primitive is the picker's existing
`insert_glyph_into_field` (parley `insert_or_replace_selection`, grapheme-/IME-
correct), so a chosen glyph lands in the field the button belongs to.

The widget also attaches the inline `:`-completer
([[viewer-emoji-colon-autocomplete]]) and emits a `ChatInputSubmit` on Enter; it
reaches no session (constructible without wiring), and is swept live as the
`chat-input` specimen. Its first consumers are the local-chat variant
([[viewer-chat-channel-and-commands]]) and, as follow-ups, the nearby-chat bar
([[viewer-chat-input-bar]]) and the conversations floater
([[viewer-social-im-conversations]]).
