---
id: viewer-vintage-radar-range-colours
title: The radar's range column is unreadable on a light list
topic: viewer
status: done
origin: viewer-vintage-skin second pass (2026-09-26)
refs: [viewer-vintage-skin]
---

Context: [context/viewer.md](../context/viewer.md),
[context/vintage-skin.md](../context/vintage-skin.md).

## Observation

In the Vintage skin the radar's **Range** column draws its values in the
name-tag distance colours (green within chat range, yellow within shout
range) on the light sage list face, where both all but vanish. A friend's
name (`#bfeb7d`) on the periwinkle selected row has the same problem.

## Why

`radar.rs` takes the band colours from the user-tunable name-tag distance
settings (`NameTagDistanceChat` / `…Shout`), which are tuned for text over the
world, not for a list. The reference colours radar rows with its own
`AvatarListItemChatRange` / `…ShoutRange` / `…BeyondShoutRange` — in Vintage
`#000000`, `#00000080`, `#66000066`, dark on the light list — recorded in the
context file's *World-facing* table.

## Done when

The radar's range bands come from skin roles of their own (defaulting to the
current colours in the flat skins), and Vintage sets the reference's values.

## Done (2026-11-13)

The range cell wears `.sk-radar-range` plus one band class
(`-chat` / `-shout` / `-beyond` / `-unknown`, `radar.rs` `range_classes`), and
`common.css` paints each from a token of its own, `--radar-range-*`. CSS
classes rather than palette settings because Vintage's shout and beyond carry
alpha (`#00000080`, `#66000066`), which the RGB-only colour palette would
drop, turning shout into chat. The rules are (0,3,0) and sit after the selected
row's text rule, so a selected row keeps its band's colour, as the reference's
cell with its own colour does.

- Flat skins keep what they drew: chat and shout the name-tag distance green /
  yellow (Azure its own two), beyond `var(--field-text)` and unknown
  `var(--field-placeholder)` — the list text and muted text the `.sk-text` /
  `.sk-title` roles gave them, by reference so a theme still moves them.
- Vintage: `#000000`, `#00000080`, `#66000066`, and `#66000066` for an unknown
  range too, which the reference paints with `AvatarListItemBeyondShoutRange`.
- The radar no longer reads the name-tag distance settings at all, so a user's
  `NameTagDistanceColorChat` / `…Shout` override no longer reaches it — the
  reference keeps the two separate too.

Tests: `a_range_cell_wears_exactly_its_band` (the class swap),
`the_radar_range_bands_are_the_skins` (Vintage and Graphite through the real
engine, resting and selected rows), the radar specimen's class row, and the
skins chapter's token / class tables.

The friend name on Vintage's selected row, the other half of the observation,
was split out as [[viewer-vintage-radar-friend-name-contrast]] and fixed in the
same commit.
