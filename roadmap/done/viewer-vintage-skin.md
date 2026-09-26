---
id: viewer-vintage-skin
title: Ship a Vintage-alike skin
topic: viewer
status: done
origin: Vintage skin fidelity audit (2026-09-20)
points: 8
refs: [viewer-ui-skin-tokens, viewer-vintage-bottom-bar,
       viewer-preferences-colors-skins-tab, viewer-skin-text-shadow-role,
       viewer-skin-list-row-striping, viewer-skin-icon-set,
       viewer-vintage-ui-chrome-crosscheck]
blocked_by: [viewer-skin-light-surface-roles, viewer-skin-image-backed-widgets]
---

Context: [context/viewer.md](../context/viewer.md),
[context/vintage-skin.md](../context/vintage-skin.md).

The capstone: a third shipped skin whose palette and widget shapes are the
reference's Vintage, rather than a third set of dark-on-dark values. The
viewer's feature set has been audited against Vintage twice — the
coverage audit of 2026-07-22 and the full-parity audit of 2026-08-19 — and
between them they produced the classic bottom bar, the utility cluster, the
radar and the contact sets. The one thing that has never been Vintage is how
it *looks*.

Everything it needs to say is already measured, in
[context/vintage-skin.md](../context/vintage-skin.md): the resolved palette
(merged over the reference's default skin, not the 100-entry override file
alone), the measured widget art, and the widget-default deltas. This task does
not re-derive any of it.

## What shipping it means

- `assets/skins/vintage/skin.css` — the role tokens set to the measured
  palette, square corners throughout (`--surface-radius: 0`,
  `--control-radius: 0`), the light field / list family from
  [[viewer-skin-light-surface-roles]], and `--text-shadow` set
  ([[viewer-skin-text-shadow-role]]).
- Authored nine-slice art for the widget surfaces that carry a shape — push
  button (idle / pressed / disabled), text field (idle / focused / disabled),
  tab, scrollbar parts, slider, progress bar, floater frame and header, the
  tooltip plate — to the measured geometry, drawn here and not copied
  ([[viewer-skin-image-backed-widgets]] and the licensing note in the context
  file).
- Registration: `SKINS` in `sl-viewer-ui-core/src/skin.rs`, the `--skin` flag's
  help, and the preferences Colors & Skins tab
  ([[viewer-preferences-colors-skins-tab]]) pick it up from there.
- The user-tunable palette (`skin_colors.rs`) gets Vintage's values as its
  skin-supplied defaults: the chat family, the name tags — note Vintage
  collapses match and mismatch onto one orange — and the minimap dots, which
  are already these values in both existing skins.

## Two deliberate deviations to record in the skin's own header comment

- **We do not reproduce `UIControlBGSelectedCompensate`.** It exists in the
  reference only to pre-compensate a hard-coded 0.7 alpha applied in code. We
  have no such multiplier; one selection colour does both jobs.
- **We do not fork floater layouts.** Vintage ships whole-file XUI forks of
  `floater_tools.xml` and friends; [[viewer-ui-skin-tokens]] ruled that out on
  purpose, and it is the reason a reference skin breaks every release. Shape
  and colour only.

## Done when

`--skin vintage` gives a viewer that reads as Vintage at a glance — dark grey
chrome, light black-texted fields and lists, periwinkle selection, bevelled
buttons, shadowed steel-blue labels, square corners — the chrome cross-check
([[viewer-vintage-ui-chrome-crosscheck]]) puts it beside the real thing, and
switching back to `graphite` leaves nothing behind.

## Implemented (2026-09-26)

`--skin vintage` ships: `assets/skins/vintage/skin.css` sets every role to
the merged reference palette and dresses every widget whose Vintage look is a
shape in nine-sliced art of our own — push button (rest / hover / pressed or
toggled in gold / refused), the bottom toolbar's buttons likewise, text field
and search box (rest / focused / read-only / refused), tab (rest / open),
scroll arrows (rest / hover / held), slider groove and handle (and a refused
handle), floater body and the front-most header strip, and the tooltip plate.
Nineteen 12x12 files at 4 px insets, drawn by `sl-viewer-skin-art`, which now
draws per-skin sets with a per-side frame and writes each file's nearest
`.meta` itself (Graphite's four files regenerate byte-identical). Registered in
`SKINS`, labelled in `en/main.ftl`, named in the `--skin` help and the book.

### Corners and button states, measured rather than assumed

The context file said every Vintage corner is square; the art's alpha says
otherwise (now corrected there). The push button — and so every bottom-bar
button, which names the same `PushButton_*` art — is rounded on ~6 px, which is
what makes the reference's bottom bar a row of pills. The generator now cuts
per-corner radii with anti-aliased coverage, the frame and bevel following the
curve: push buttons 6 px all round (a 20x20 set with 7 px insets, since the
curve must sit in the 1:1 corner), tabs 2 px along the top, the slider handle
3 px and groove 2 px. Fields, tooltip, scroll arrows and floaters stay square,
as there.

The state model follows `LLButton::draw`: held, toggled, and held-while-toggled
are **one** gold file in Vintage (`_Press`, `_Selected`, `_Selected_Press` all
map to it); hover has no image of its own and adds a glow to whatever the
button wears, so a lit button under the pointer gets its own lifted-gold file
(`:checked:hover`, with `:checked:active` / `:checked:disabled` restated after
it at (0,3,0)); and a caption changes with the toggle, never with a press — a
lit toolbar caption is `ButtonLabelSelectedColor` (white), through a new
`--button-text-selected` role where it used to be `--accent` (gold in Vintage).
Toolbar buttons also hover now: `.sk-toolbar-button:hover` reads a new
`--toolbar-button-bg-hover` role (the resting face in the flat skins, so
nothing moves there) and the class joined `HOVER_CLASSES`, without which a
skin's `:hover` rule on it could never fire.

### Second pass, from the first review (2026-09-26)

Rendered the reference's art enlarged instead of reading pixels one at a time,
which corrected several shapes. Note that the reference resolves art **by file
name, the skin's folder first**, so files like `DropDown_*.png` override the
default's even though Vintage's `textures.xml` never names them.

- **The push button is a stadium** (radius about half its height), with a
  vertical gradient inside a dark outline; pressed is a gold double ring. Now
  a 28 px file with a 12 px radius and 13 px insets — Bevy's UI slicer scales
  corners down on a shorter node (`min(target/image, max_corner_scale)`), so
  compact buttons stay pill-shaped.
- **Tabs**: shaded mauve, 2 px leading / 5 px trailing top corners, gold
  baseline; open tab in a gold frame.
- **Combos are dark grey** (`DropDown_*`), not blue: `--control-bg` is
  `#4a4d51`; the blue belongs only to the push-button art.
- **Checkbox and radio have orange rings** (`#ef9c00` / `#f5a000`).
- **Scroll arrows are dark grey with a light glyph** (it was inverted), and
  the tab strip's overflow buttons wear them too; the **scroll thumb** is a
  stadium and the **slider handle** a shaded disc.
- **Accent** is the selection periwinkle, not tab gold — it drew the tab
  divider's grip bright yellow.
- **Notecard items** have no box in the reference: the label is
  `TextEmbeddedItemColor` (dark brown) in an editable notecard and
  `TextEmbeddedItemReadOnlyColor` in a read-only one. New
  `.sk-inline-item-text` + `--inline-item-text[-readonly]`; the box floats in
  the rich text's overlay, outside the field, so the notecard stamps
  `.sk-read-only` on it itself.
- **Action buttons** (People/Groups columns, radar, parcel audio) wear the
  push-button art; they gained a `:hover` role (`--action-button-bg-hover`,
  the resting face in the flat skins) and joined `HOVER_CLASSES`. The radar's
  Profile/IM were a hand-painted box no skin could reach; now `spawn_button`.
- **Emoji grid** sits on the image-well scrim, not a list surface (colour
  emoji on light sage vanished).
- **Text in a field well** (`.sk-field .sk-text`) takes the field text — the
  field-grid specimen's numbers were label-blue on sage.
- **Tab overflow buttons were transparent in every skin**, so they looked like
  whatever panel they sat on: `--tab-scroll-bg` is now each skin's opaque
  control face, with a test. The gallery's overflow card also wraps its two
  demos (both were shrunk to share a 760 px card, clipping the few-tabs one's
  last tab so its arrows showed, and spilling the many-tabs one past the edge).
- **Experiences specimen**: rating cells showed raw Fluent keys — it resolved
  them at spawn, before the locale loaded. The cells now carry `Translated`.

Checked in the running gallery, paged with `niri msg action send-into-window`
and `screenshot-window`.

Not changed, noticed: the radar's range column is green/yellow on the light
list (Vintage uses dark `AvatarListItem*` colours there), and the parcel audio
cluster's backing is still a hard-coded dark strip.

### Roles the skin could not be said without

- **`--label-text`** — `.sk-text` reads it. Vintage's labels are steel blue
  (`LabelTextColor`) while its buttons, menus and tabs stay LtGray; one
  `--text-primary` could not be both. Button and action-button captions
  restate `--text-primary`.
- **`--floater-glyph` / `--floater-grip`** — the window icons and the resize
  grip. The reference draws them as art; Vintage's are orange.

The flat skins give all three their existing values, so nothing moves there.

### The chat bar was outside the skin

The first cross-check (`sl-crosscheck --capture-ui --sl-client-skin vintage
--firestorm-skin vintage`) showed the chat input still dark navy beside the
reference's light sage field: its box and its volume selector were painted
from Rust constants no skin could reach. The box now wears `.sk-field-box` (the
search box's well, focused face and ring, with `.sk-focus-within` mirrored from
its field by `reflect_chat_box_focus`), and the volume selector is a real combo
(`.sk-combo` / `.sk-combo-list` / `.sk-combo-option`). In Graphite that gives
the selector a control face where it had a bare outline.

### Checks

- `image_backed_widgets`: every Vintage file decodes, fits its insets and
  samples nearest, and the sheet and `widgets/` agree on which files exist;
  every shaped widget wears its art in every state, sliced, border-box, with no
  flat paint under it; a header loses its art when its window goes behind;
  **switching Vintage → Graphite leaves face, frame, corners and image exactly
  as a Graphite-only app has them.**
- `skin_palette_resolves::the_vintage_skin_reads_as_vintage`: steel-blue
  shadowed labels, LtGray captions, periwinkle selected row on a light list,
  orange window icons, DkGray chrome and black data text in the palette — and
  in both flat skins a label and a caption still resolve to one colour.
- `sl-viewer-skin-art` unit tests: bevel direction, Vintage's gold press, the
  light sunken field, the frame's diagonal split, band widths vs insets.

### Not done, and why

- **Progress bar art.** No progress-bar widget exists in the viewer, so there
  is nothing for the art to dress.
- **Scrollbar thumb and groove are colours, not art** — the reference's are
  flat (`#3c4c7c`, `#999999`), so an image would draw nothing a colour does not.
- **Floater rendering is not in a cross-check**: the harness closes and blocks
  floaters for a run, so the floater frame, tabs, sliders and tooltip are
  pinned by the tests above and need an eye in the gallery.

## Closed (2026-09-26)

Reviewed by the user in the running gallery after the second pass and called
done. Further Vintage fidelity gaps are filed as their own tasks as they are
noticed — the first two: [[viewer-vintage-radar-range-colours]] and
[[viewer-parcel-audio-bar-backing-unskinned]].
