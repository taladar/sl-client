---
id: viewer-skin-light-surface-roles
title: Role tokens for a light data surface on dark chrome
topic: viewer
status: in-progress
origin: Vintage skin fidelity audit (2026-09-20)
points: 5
refs: [viewer-ui-skin-tokens, viewer-vintage-skin]
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

## What the blocker landed (2026-09-21)

[[viewer-audit-skin-token-coverage]] is done: the vocabulary has stopped
moving at 30 roles, so this is the additive change it was waiting to be.

Part of the first step is already in place — `--field-bg` and `--field-text`
exist as their own roles (a field is a *well*, not a control), and the text
input, the shared search box and the `.sk-field` class read them. What is left
is the rest of the family and the rest of its consumers: the scroll lists, the
combo list, the editors, and the border / selection / caret roles that have to
move with a light face for it to stay legible.

## Done when

Both shipped skins define the family (dark values, so nothing visibly moves),
every text input, scroll list, combo list and editor reads it, and a test skin
that sets the field family light while leaving chrome dark renders legibly —
black text, black caret, visible selection — with no Rust change.

## Done (2026-09-23)

Nine tokens, three of them wired to states that had no role at all:
`--field-bg-focused`, `--field-bg-readonly`, `--field-text-disabled`,
`--field-text-readonly`, `--field-placeholder`, `--list-bg`, `--menu-bg`,
beside the `--field-bg` / `--field-text` pair that already existed. Every value
in both flat skins is another role's, deliberately: an unskinned world is
dark-on-dark, so "the field you are typing in", "a read-only one" and "a scroll
list" all resolve to what they already looked like, and the split is there for
the skin that does not put data on chrome.

**A read-only field and a refused one stopped being the same look.** They shared
one grey on the argument that *you cannot type here* is what the user needs at a
glance — but the reference says it with the **surface** (a read-only editor goes
back to chrome grey with light text) rather than by dimming glyphs, and that is
a real usability idea worth having a role for. So
`reflect_uneditable_text_color` became `reflect_read_only_field` and marks only
the stance no pseudo-class can see; the disabled one is `:disabled` over the
`InteractionDisabled` the consumer already sets and needs **no system at all**.
A decorated field's box follows its
own editor's state now — `.sk-field:focus` / `.sk-field.sk-read-only` /
`.sk-field:disabled` — which works because `spawn_text_input` puts both classes
on one entity.

**A list surface, and 22 panels that each owned one.** `const LIST_BACKGROUND:
Color = rgba(0, 0, 0, 0.25)` appeared 22 times, plus seven inline copies of the
same literal, and every one of them was painted on a virtualised list's viewport
— a colour reachable by nothing. `.sk-list-surface` is one class on that node,
and the **table widget** carries it, so a table gets its face without its
consumer saying so.

That class also **re-roots the generic text roles**: `.sk-list-surface .sk-text`
and `.sk-list-surface .sk-title` resolve to the field family, because a role is
only meaningful against a surface and a row is not on chrome. Descendant
selectors, so they beat the single-class resting rules wherever those sit — and
the three state pairs that must still win (`.sk-active-text`,
`.sk-highlighted-text`, `.sk-disabled-text`) are restated at the same
specificity in the state block, which is the whole reason that block is last. A
**button** inside a row is the exception and gets its chrome caption back, since
it brings its own surface with it.

**A combo's drop-down is a list, not a menu.** It used to share `.sk-menu`; the
reference names the two separately (`ComboListBgColor` against
`MenuDefaultBgColor`, which in Vintage is opaque black) and they cannot be one
token once data surfaces go light. `.sk-combo-list` takes `--list-bg` and its
rows' labels take `--field-text`; the shape, the hover and the separator stay a
menu's.

**The caret moved family too**, which is the bug the caret rule was written to
fix, one level up: both skins derived `--caret` from `--text-primary` ("the
caret matches the text"), and a caret taken from a label colour is invisible the
moment a skin makes the field's face light. It follows `--field-text` now.

**The check** is `a_light_field_family_stays_legible_over_dark_chrome`, against
a test-only fixture (`sl-client-bevy-viewer/tests/assets/light-field.css`) that
sets the family to the measured Vintage values over the embedded fallback's dark
chrome and touches nothing else. It pins the field's face, its text, its caret,
the list's face, a row's text, a button caption inside a row, and a label
*outside* the list — the last two because the re-rooting has to be scoped, and
nothing static about the CSS text can answer a specificity question. It needed
`register_caret_properties` made `pub`, for the same reason
`register_palette_properties` is.

### Deliberate visual changes, small and worth naming

- The four table **wrappers** that painted the scrim (About Land, About Region
  ×2, Top Objects, Telehub) covered the header row as well; the scrim is on the
  viewport now, so a header strip reads as the panel behind it. That is what the
  reference does — the header is chrome, the rows are data.
- A handful of tables that had **no** scrim gain one, because the widget now
  owns the face. Consistency, at the cost of a 25%-black wash where there was
  none.

### Not done

- **The search box's focused fill.** `--field-bg-focused` reaches every
  decorated field through `.sk-field:focus`, but a search box is a *container*
  around a bare editor, and `bevy_flair` has no `:focus-within` (`:has()` parses
  but nothing invalidates the ancestor when a descendant's focus moves). It
  wants a class stamped from Rust, which is a widget change rather than a token
  one: [[viewer-skin-search-box-focused-fill]].
- `.sk-heading` is **not** re-rooted inside a list surface. A heading-coloured
  cell is rare, and re-rooting it would erase the distinction rather than
  restate it; a skin retunes `--text-heading` instead.
- A list row's **hover** still takes the chrome `--control-bg-hover` — a
  drop-down's too. [[viewer-skin-list-row-striping]] owns that for every list at
  once, along with striping and the selected row's text.
- **The live look.** Nothing here can be judged headlessly beyond legibility,
  and the two things worth an eye are the header strips above and whether a
  graphite viewer looks unchanged.
