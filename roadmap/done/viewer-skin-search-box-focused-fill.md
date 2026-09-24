---
id: viewer-skin-search-box-focused-fill
title: A search box cannot show the focused-field fill, because focus is on its child
topic: viewer
status: done
origin: viewer-skin-light-surface-roles (2026-09-23)
points: 3
refs: [viewer-skin-light-surface-roles, viewer-ui-search-field, viewer-vintage-skin]
---

Context: [context/viewer.md](../context/viewer.md),
[context/vintage-skin.md](../context/vintage-skin.md).

[[viewer-skin-light-surface-roles]] added `--field-bg-focused` — the reference's
`UIControlBGLightFocused`, the brighter face the editor you are typing in wears
against `TextBgWriteableColor`. Every **decorated** field gets it for free,
because `spawn_text_input` puts `.sk-field` and `.sk-text-field` on the *same*
entity as the editor, so `.sk-field:focus` matches the thing that has focus.

The **search box** cannot. `spawn_search_field` builds a bordered container
holding a leading glyph, a *bare* field and a trailing clear button — the
container carries `.sk-search-field` and paints `--field-bg`, and the editor
inside it is what takes focus. There is no selector from the focused child back
up to the box:

- `bevy_flair` parses six non-tree-structural pseudo-classes and
  `:focus-within` is not among them (`css_selector/mod.rs`
  `parse_non_ts_pseudo_class`: hover, active, focus, focus-visible, disabled,
  checked).
- `:has()` *does* parse, and `.parent:has(#child)` matches in the engine's own
  tests — but matching is not the problem. The style pass re-resolves the
  entities it is told have changed, and a descendant's focus state moving does
  not invalidate its ancestors, so `.sk-search-field:has(.sk-text-field:focus)`
  would match only whenever something else happened to dirty the box. A rule
  that is right in a unit test and intermittent in the viewer is worse than no
  rule.

So the box needs a **class**, the way every other state the engine cannot see
gets one ([[viewer-skin-widget-state-classes]]).

## What to do

- A system in `ui_search.rs` that mirrors "my field has focus" onto the
  container — `InputFocus` is a resource, so this is one lookup, and
  `set_state_class` already exists to keep the write change-guarded (an
  unguarded `ClassList` deref every frame costs more than the paint it
  replaces).
- The class compounds with `.sk-search-field` in `common.css` and reads
  `--field-bg-focused`, beside the `.sk-field:focus` rule it is the sibling of.
- Consider whether the **focus ring** should follow: `.sk-text-field:focus`
  rings the editor, which inside a box means a ring drawn *inside* the box's
  border. The reference lights the box's own border highlight
  (`mBorder->setKeyboardFocusHighlight`), so the ring probably belongs on the
  container too — which is the same mechanism and the same one system.

## Watch out

The **scaffold already stamps** `.sk-focusable` and `.sk-text-field` on the
editor (`skin.rs` `stamp_focus_ring_class` / `stamp_text_field_class`), so
whatever this adds has to not double-ring: one of the two rules wins, and which
is a cascade question to pin in a test rather than to discover live.

## Done when

Focusing the inventory filter or the menu-bar search brightens the **box**, not
just the caret; the flat skins are unchanged because they give `--field-bg` and
`--field-bg-focused` one value; and a test asserts the class arrives and leaves
with focus, plus one in `skin_palette_resolves` that the compound rule resolves
to the light value under the `light-field.css` fixture.

## Done (2026-09-24)

`sk-focus-within` — named for CSS's pseudo-class, because that is exactly what
it stands in for — lives in `skin.rs` with the rest of the state vocabulary, and
`ui_search.rs`'s `reflect_search_box_focus` puts it on the box while the field
inside it holds focus. The box had to be told which field that is
(`SearchFieldBox`, inserted once the field exists), and the write goes through
`set_state_class`, so a settled box never wakes the style engine. One system,
one class, fifteen consumers: every search box in the viewer is the widget.

**The ring moved with the fill, and that is the half that needed a test.** Two
rules already ring every focused editor — `.sk-text-field:focus` on any focus
and the scaffold's `.sk-focusable:focus-visible` on Tab — and a search box's
field is *bare* and fills only the middle, between the glyph and the clear
button. Left alone, the box lights and a second ring is drawn inside it around
part of it. `.sk-search-field .sk-text-field:focus` and its `:focus-visible`
sibling take it off at (0,3,0) against their (0,2,0), which puts the ring where
the reference draws it — on the box's own border
(`mBorder->setKeyboardFocusHighlight`) — and is the sort of claim no reading of
the CSS text can settle, so
`a_focused_search_box_rings_the_box_and_not_the_editor` asserts it through the
engine with both of those rules live and `InputFocusVisible` true.

The resting `.sk-search-field` rule gained an `outline-width: 0px` baseline for
the reason `.sk-focusable`'s has one: `bevy_flair` has no value to revert a
property to, so a ring written only by the focused rule would be given once and
never taken away.

### Not done

- `:focus-within` itself. The class is the stand-in; if `bevy_flair` ever grows
  the pseudo-class (and the invalidation to go with it), these two rules and one
  system collapse into a selector.
- No other container wears the class today. It is named generically because the
  state is, not because a second wearer is planned — a combo box's editor and a
  spinner's would be the candidates.
