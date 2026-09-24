---
id: viewer-skin-glyphs-from-content
title: A skin can choose the glyph, not just its colour — audit where that applies
topic: viewer
status: done
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

## Progress (2026-09-24)

**The open question is settled, and inheritance goes further than the font.**
The `946f8a2` fork fix copies the host's font into the pseudo-element, and
`color: inherit` on a `::before` follows the host *live*
(`a_glyph_takes_its_colour_from_its_host`). So one rule,
`.sk-glyph::before { content: ""; color: inherit; }`, serves every slot: a
glyph greys with its host and no slot needs a colour rule.

**The vocabulary** is `sl-viewer-ui-core/src/glyph.rs`: `GLYPH_CLASS`, 35
slot classes (`sk-glyph-close`, `-reload`, `-disclosure`, …), the state
classes they read (`sk-loading`, `sk-expanded`, `sk-sort-ascending`, …), a
`glyph_host` bundle, and `UiLabel::Glyph(slot)` for a button whose caption is
a mark. `common.css` gives every slot its shipped character; the stepping
marks and a closed disclosure triangle mirror under `dir="rtl"`.

**Converted**, the Rust constant deleted in each case:

- widgets: floater close / minimize↔restore / dock↔tear-off / resize grip;
  combo arrow; search magnifier and clear; menu tick (`:checked` on the row)
  and sub-menu arrow; table sort arrows.
- panels: People presence dot and sort arrows; Groups' active marker (its own
  `sk-current`, since `.sk-active` paints a selection background);
  Conversations close / add; the seven toast and dialog close boxes
  (`DISMISS`); the toast queue's "N more ▸" (a `::after`, the one slot with
  text of its own); the permission toasts' bullets (now a hanging-indent row;
  the gallery specimens also passed pre-bulleted lines, so they drew two);
  media bar play↔pause, back, forward, home, reload↔stop, mute, zoom,
  external, padlock; web browser back / forward / reload↔stop / external /
  padlock; parcel audio ♫, play↔stop, mute; volume panel mute and ▲;
  inventory disclosure (a leaf drops the slot); inventory gallery back /
  forward / up; Build Tools link-part stepper; quick-prefs preset stepper and
  gear; Search's Events day stepper; chat bar emoji button; the asset
  blacklist's Permanent ✔; the radar's region dot (`sk-precise`); debug
  settings' changed `*`.
- The pie menu has no sub-menu marker to convert: sub-pies are ruled out by
  construction.

**Tests.** `every_glyph_slot_takes_its_mark_from_the_skin` pins all 52
slot × state cases in both shipped skins, and fails if a slot in
`glyph::SLOTS` has no case. `a_scratch_skin_redraws_every_mark` loads
`tests/assets/glyph-scratch.css`, which gives every case its own mark, and so
proves the "done when" by CSS alone. `a_glyph_state_is_taken_back` covers the
never-revert trap, and `stepping_marks_mirror_under_rtl` the mirroring. The
book's skin chapter has a *glyph slots* section and the state classes, which
the vocabulary test holds it to.

## Left as they are, deliberately

- **Glyphs inside translated strings**: `search-prev` / `search-next`
  ("‹ Prev"), `experiences-page-previous` / `-next`, `menu-inventory-gear` (⚙),
  `radar-col-region` (●), the stream-metadata notification's ♫. They are
  `.ftl` text, which a translator owns. Making them slots means a glyph-plus-key
  label, and for the gear a menu-bar label that can be a glyph.
- **Type icons** (inventory items and folders, notice attachments, notecard
  embeds, offer kinds) belong to [[viewer-skin-icon-set]].
- The radar's status letters (T / S / A) are abbreviations, not marks, and its
  gallery specimen still composes "● T S" as one sample string.
- The four slots that already worked this way (checkbox tick, radio pip,
  tab-scroll and scrollbar arrows) keep their own classes and colour rules.
- The gallery header's own chips ("Skin ▸").

## Closed (2026-09-24)

The user checked the gallery: the floater chrome glyphs, the combo arrow and
the search box marks render and toggle. Closed on the user's word with the
menu tick / sub-menu arrow and the toast overflow unseen live (both pinned by
the skin tests; the overflow has no specimen, which
[[viewer-gallery-floaters-are-mostly-stubs]] now tracks). The same look
filed [[viewer-floater-minimize-caps-follow-no-pattern]] and
[[viewer-minimized-floaters-move-to-a-shelf]].
