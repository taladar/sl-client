---
id: viewer-vintage-radar-range-colours
title: The radar's range column is unreadable on a light list
topic: viewer
status: bugs
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
