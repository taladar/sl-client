---
id: viewer-skin-glyphs-from-content
title: A skin can choose the glyph, not just its colour — audit where that applies
topic: viewer
status: ready
origin: raised while building viewer-skin-checkbox-radio-shape (2026-09-22)
points: 5
refs: [viewer-skin-checkbox-radio-shape, viewer-skin-icon-set,
  viewer-skin-widget-state-classes, viewer-vintage-skin]
---

Context: [context/viewer.md](../context/viewer.md),
[context/vintage-skin.md](../context/vintage-skin.md).

The skin work so far assumed **CSS cannot change text**, and said so in a
module doc, a `common.css` comment and a test message. That is wrong, and
wrong about browsers too: `::before` / `::after` with `content` is how a
stylesheet has inserted text for twenty years.

`bevy_flair` supports it. `PseudoElementsSupport` (opt in per entity) spawns a
child `TextSpan` for a text entity — a hidden `Node` for a block one — and
`bevy_flair_core` maps `"content" => TextSpan[".0"]`. Its own example
stylesheet does `a::before { content: "🔗"; }`.

So a decision this codebase has been making in Rust — *which mark* a widget
wears — belongs to the skin wherever the mark is decoration rather than data.
[[viewer-skin-checkbox-radio-shape]]'s checkbox already works this way: it
spawns an empty text node and the skin writes

```css
.sk-checkbox:checked .sk-checkbox-tick::before {
  content: "\2713";
  color: var(--check-tick);
}
```

which lets a skin pick among U+2713 `✓`, U+2714 `✔`, U+2717 `✗`, U+00D7 `×`, a
dot or a filled square without a Rust change. This task is the audit for
everywhere else.

## What the census found (2026-09-22)

**103 glyph / arrow / ellipsis constants across 51 files.** Two kinds, and
they want different treatment:

### State-driven swaps — a system picks the text from a state

These are the strong candidates: the state is already a selector
([[viewer-skin-widget-state-classes]] put it there), so the glyph can follow
the same rule and the system disappears.

| what | where | state |
| --- | --- | --- |
| checked / unchecked | 99 uses across the hand-rolled checkboxes | `:checked` |
| sort direction ▲ ▼ | `ui_table` (10 uses) | a class the header sets |
| muted 🔊 / 🔇 | `volume_panel`, `parcel_audio`, `media_controls` | a class |
| reload ⟳ / stop ✕ | `media_controls`, `web_floater` | a class |
| folder open / closed | `inventory`'s disclosure arrow | a class |
| active group ● | `groups.rs` | `.sk-active` |
| presence ● / ○ | `people.rs` | `.sk-presence-*`, already classed |

### Fixed decorations — the glyph never changes, but is still Rust's

A combo's drop arrow, the scroll arrows on a tab strip, the floater close /
dock glyphs, the search magnifier, the pie menu's sub-menu marker. Nothing
about them varies at runtime, so they are pure decoration a skin should own —
and a `::before` with no Rust node at all may be simpler than the text node
they use today.

## Proven, not assumed

`a_checkbox_takes_all_four_of_its_looks_from_the_skin` in the viewer's skin
test asserts the `::before`'s `content` and colour against the real shipped
stylesheet, so the mechanism is known to work end to end here — a checked box
carries `\u{2713}`, an unchecked one carries nothing, and a refused checked
one greys. Copy that test's shape when converting anything else: the
pseudo-element is found **by position** (first child), because
`bevy_flair`'s `PseudoElement` enum is private.

## Open question to settle first

**Does the pseudo-element's `TextSpan` inherit its parent's `TextFont`?**
`bevy_flair`'s own example sets `font-family` and `font-size` on the
`::before` explicitly, which suggests not — and a glyph in a face that lacks
it renders as tofu, which no headless test can see. Settle this with one live
check before converting anything beyond the checkbox, because the answer
decides whether every such rule must also name a font.

## Deliberately out of scope

- **Anything carrying data.** A name tag's text, a chat line, a row's value.
  `content` replacing those would be a skin rewriting the world.
- **Image-backed art**, which is [[viewer-skin-image-backed-widgets]]'s, and
  **icon sets**, which are [[viewer-skin-icon-set]]'s. This task is only about
  glyphs that are *text* today.
- The emoji picker's cells: those are the content.

## Done when

No widget chooses a decorative glyph in Rust where a selector already
describes the state, the fixed decorations are `content` rules, and a scratch
skin can change every mark in the chrome — tick, arrows, mute, reload — by
editing CSS alone.
