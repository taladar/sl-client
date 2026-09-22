---
id: viewer-skin-icon-set
title: A skin should be able to carry an icon set, not just colours
topic: viewer
status: ready
origin: Vintage skin fidelity audit (2026-09-20)
points: 5
refs: [viewer-ui-skin-tokens, viewer-vintage-skin, viewer-ui-status-bar-parcel-icons,
  viewer-skin-glyphs-from-content]
---

Context: [context/viewer.md](../context/viewer.md),
[context/vintage-skin.md](../context/vintage-skin.md).

Half of the reference's `textures.xml` overrides in the Vintage skin are not
widget art at all — they are the **legacy inventory icon set**: every folder
and item type re-pointed at the `legacy/inv_*.tga` glyphs, plus the legacy
parcel status icons. A resident who recognises Vintage recognises those icons
before they notice a colour.

Where we stand:

- **Inventory icons are emoji glyphs chosen in Rust.** `inventory.rs`
  `item_glyph` / `folder_icon` / `wearable_icon` map a type onto a literal
  (`"\u{1f455}"` for a shirt). No skin can reach them; the comment on
  `wearable_icon` says outright that they stand in for the reference's
  per-type textures "which we do not ship".
- **Parcel status icons are already halfway there.** They are PNG glyph masks
  under `assets/icons/parcel/`, tinted by `-bevy-image-color` through
  `.sk-parcel-icon`, and `common.css` already documents the per-glyph override
  (`.sk-parcel-icon--voice { -bevy-image: url("my-voice.png"); }`). That is
  the pattern; it just has one user.
- **Two more image families pick their art in Rust** (found 2026-09-22 while
  building the checkbox widget). Both are `ImageNode`s whose *tint* is already
  the skin's and whose *art* is not, so both are the parcel-icon pattern with
  the second half missing:
  - the **Friends list's rights columns** — `people.rs`'s `PeopleIcons`
    (eye / pin / pencil headers, and the `check_on` / `check_off` a cell
    swaps through `icons.checkbox(set)`). Drawn in code rather than shipped
    as art, which is also why they are the easiest to re-point.
  - the **link icons** in `linkified_text.rs` — `icons/link/agent.png`,
    `group.png`, `location.png`, chosen by `LinkIcon` and tinted per link.

## What to do

- Name each icon slot as a class the way the parcel icons are named, so a skin
  can re-point one glyph or all of them with `-bevy-image`. The slot list is
  the reference's: inventory item types, folder types (open and closed),
  wearable sub-types, parcel status, the floater caption glyphs, the Friends
  list's rights columns, and the link icons.
- Where the art is *text* rather than an image, the mechanism is `content` on
  a `::before` instead of `-bevy-image` —
  [[viewer-skin-glyphs-from-content]] audits those, and the two tasks should
  agree on the class names where a slot could be drawn either way (a
  checkbox's tick is the obvious one: a glyph today, a nine-slice under
  [[viewer-skin-image-backed-widgets]]).
- Give the shipped skins a real icon set for those slots so the emoji fall
  back out of the UI. Emoji were a good stand-in — they read at a glance and
  cost nothing — but they carry a colour the skin cannot change and a metric
  the row cannot control, which is why they never looked quite level.
- Keep a documented fallback: a slot with no art in the active skin falls back
  to the base set, never to nothing.

## The art question, decided up front

The reference's `legacy/inv_*.tga` files are Linden Lab / Firestorm artwork
under their own terms, not the LGPL that covers the code. **We do not copy
them.** A classic-look icon set for this project is drawn here, to the same
silhouettes-at-16-px brief, and its provenance is recorded the way
`viewer-assets/character/`'s README records the vendored body meshes'. Budget
the drawing as part of the task, not as a footnote — it is the larger half.

## Done when

Icon slots are addressable from CSS, the shipped skins supply the set, the
emoji glyph tables are gone from `inventory.rs`, and a skin that overrides a
single slot changes only that glyph.
