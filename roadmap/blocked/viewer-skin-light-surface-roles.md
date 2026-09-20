---
id: viewer-skin-light-surface-roles
title: Role tokens for a light data surface on dark chrome
topic: viewer
status: blocked
origin: Vintage skin fidelity audit (2026-09-20)
points: 5
refs: [viewer-ui-skin-tokens, viewer-vintage-skin]
blocked_by: [viewer-audit-skin-token-coverage]
---

Context: [context/viewer.md](../context/viewer.md),
[context/vintage-skin.md](../context/vintage-skin.md).

Our role vocabulary has exactly one surface-and-text family:
`--control-bg` / `--control-border` / `--text-primary`, plus `--surface-bg`
for a panel. Both shipped skins make every one of them dark, so "a control"
and "a place where data lives" are the same thing, and `--text-primary` is
near-white everywhere.

The reference's Vintage skin splits them apart as far as two families can go.
Chrome — floaters, menu bar, toolbars, buttons — is dark grey `#3e3e3e` with
`#dcdcdc` labels. **Data surfaces** — every text editor, scroll list, combo
list, notecard, script editor — are light sage grey (`#c8cfcc` focused,
`#bac3be` at rest, `#b9c2bd` unfocused) carrying **black** text with a
**black** caret. This is not a stylistic flourish; it is the whole visual
argument of the skin, and no assignment of values to the tokens we have today
can express it, because one token pair has to be both.

## What to add

A second family, named for what it *is* rather than for its colour, since
another skin may well make it dark again:

| Token | Reference source | Vintage |
| --- | --- | --- |
| `--field-bg` | `TextBgWriteableColor` | `#bac3be` |
| `--field-bg-focused` | `UIControlBGLightFocused` | `#c8cfcc` |
| `--field-bg-readonly` | `TextBgReadOnlyColor` | `#3e3e3e` |
| `--field-text` | `TextFgColor` | `#000000` |
| `--field-text-disabled` | `TextFgDisabledColor` | `#00000040` |
| `--field-text-readonly` | `TextFgReadOnlyColor` | `#e6e6e6` |
| `--field-placeholder` | `TextFgTentativeColor` | `#666666` |
| `--list-bg` | `ScrollBgWriteableColor` | `#c8cfcc` |
| `--menu-bg` | `MenuDefaultBgColor` | `#000000` |

`--caret` and `--selection` already exist but are currently *derived from*
`--text-primary` in both skins ("the caret matches the primary text"); under
this split they belong to the field family instead — a white caret on a
`#c8cfcc` field is invisible, which is exactly the class of bug the caret rule
was added to fix in the first place.

Note `--field-bg-readonly`: Vintage deliberately sends a **read-only** editor
back to chrome grey with light text, so "you cannot type here" is carried by
the surface, not by a greyed glyph colour. That is a real usability idea worth
keeping whichever skin is worn, and it needs the token to exist.

## Why it is blocked

[[viewer-audit-skin-token-coverage]] is widening the role vocabulary and
touching every widget crate to do it. Adding a second surface family on top of
a moving vocabulary would collide head-on. Land that first, then this is an
additive change: new tokens, and the field / list widgets pointing at them
instead of at the control family.

## Done when

Both shipped skins define the family (dark values, so nothing visibly moves),
every text input, scroll list, combo list and editor reads it, and a test skin
that sets the field family light while leaving chrome dark renders legibly —
black text, black caret, visible selection — with no Rust change.
