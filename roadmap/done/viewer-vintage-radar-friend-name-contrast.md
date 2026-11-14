---
id: viewer-vintage-radar-friend-name-contrast
title: A friend's name in the radar vanishes on Vintage's selected row
topic: viewer
status: done
origin: split out of viewer-vintage-radar-range-colours (2026-11-13)
refs: [viewer-vintage-radar-range-colours, viewer-vintage-skin]
---

Context: [context/viewer.md](../context/viewer.md),
[context/vintage-skin.md](../context/vintage-skin.md).

## Observation

In the Vintage skin a friend's name in the radar is drawn in the name-tag
friend colour (`NameTagColorFriend`, `#bfeb7d`), which all but vanishes on the
periwinkle (`#8d90c2`) selected row and is weak on the light sage list face.

## Why

`radar.rs` takes the friend and muted name tints from the name-tag palette, so
the list agrees with the tags over the avatars' heads — a deliberate link (a
user who retunes `NameTagColorFriend` sees it in both places). The reference
radar does not colour a friend's name at all by default: it draws every name
in `AvatarListItemIconDefaultColor` and makes a friend's **bold** (and a muted
avatar's italic) unless `FSLegacyRadarFriendColoring` is set; a colour only
comes from a contact set (`fsradar.cpp`, `LGGContactSets::colorize`).

## To decide

Whether to follow the reference (bold friend, italic muted, colour only from
contact sets) or keep the tint and give the radar's name colours skin roles
the way the range bands got theirs.

## Done (2026-11-13)

Followed the reference: a radar name is drawn in the list's own text colour
(`LABEL_COLOR`, which carries the `.sk-text` role, as the reference's
`AvatarListItemIconDefaultColor` is Vintage's black), a friend's is **bold**
and a muted avatar's *italic* (both for a muted friend), with no tint from the
name-tag friend / muted settings. The range cell is bold while its avatar is
drawn (a known distance that is not coarse-only), the reference's
`range_style`. `radar.rs` `RowStyles` holds both, `set_row_styles` writes them
on the cells' `TextFont`, and the specimen applies them too. Contact-set
colours stay out until contact sets exist.

Test: `a_friend_is_bold_a_muted_avatar_italic_and_a_drawn_range_bold`.
