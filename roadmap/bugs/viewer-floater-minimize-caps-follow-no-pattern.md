---
id: viewer-floater-minimize-caps-follow-no-pattern
title: Which floaters have a minimize button follows no pattern, and most that lack one should have it
topic: viewer
status: bugs
origin: the user opening the gallery's floaters while checking viewer-skin-glyphs-from-content (2026-09-24)
refs: [viewer-skin-glyphs-from-content]
---

Context: [context/viewer.md](../context/viewer.md).

## Observation

Some floaters have a minimize box and some do not, and nothing about them
explains which. The minimize box itself works where it exists: it collapses
the window and becomes the restore box.

`FloaterCaps::minimizable` is set per spawn site, by hand. As of 2026-09-24,
22 floaters pass `false`, among them About Land, About Region, About Landmark,
the radar, the minimap, the avatar / group / texture / settings / experience
pickers, the avatar and group profiles, item properties, the inventory
filters, the asset blacklist, the RLVa floaters, the colour picker, the
snapshot floater, About and the panorama. About 30 pass `true`. The `true`
side includes Preferences, Debug Settings, Search, Inventory and the editors,
which is no clearer a rule. Probably each file copied whichever neighbour it
was written from.

## What the reference does

`LLFloater`'s `can_minimize` **defaults to true** (`llfloater.cpp`,
`can_minimize("can_minimize", true)`). 65 of the 258 `floater_*.xml` files in
the default skin turn it off. Of the reference files for the floaters listed
above, only the texture picker (`floater_texture_ctrl.xml`) does. About Land,
Region / Estate, the radar, the avatar picker, About, the snapshot floater and
item properties are all minimizable there.

## What to do

- Take the default from the reference: `minimizable: true` unless the
  reference's XUI for that floater says `can_minimize="false"`. Check each of
  the 22 against its reference file, and the `true` ones too.
- Consider making the reference default the `FloaterCaps` default, so a new
  floater gets the reference behaviour without choosing, and one that differs
  has to say so and why.
- Put a guard in the floater registry (`sl-client-bevy-viewer/src/floaters.rs`)
  so a registered floater's caps are checked against a table naming each
  reference file's `can_minimize`, and the next drift fails a test.

## Done when

Every floater's minimize box matches the reference floater it ports, and the
rule is written down where the next floater author will meet it.
