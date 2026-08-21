---
id: viewer-minimap-menu-multi-avatar
title: Minimap context menu — multi-avatar entries (dynamic labels)
topic: viewer
status: done
origin: split from viewer-minimap-interactions (2026-07-23)
blocked_by: [viewer-contact-sets]
refs:
  [
    viewer-minimap-interactions,
    viewer-minimap-avatar-dots,
    viewer-radar-multi-select,
    viewer-people-lists-multi-select,
  ]
---

Context: [context/viewer.md](../context/viewer.md).

When several avatar dots sit within the minimap's pick radius, the
reference context menu grows multi-avatar variants: a **View Profiles**
submenu with one entry *per avatar under the cursor* (labelled by
resolved display name, filled asynchronously as names arrive) and
**Add to Set Multiple** ([[viewer-contact-sets]]). The mark actions
already apply to every avatar in the pick radius.

Blocker in our stack: `MenuDef` / `MenuCommand` labels are
`&'static str` — a menu is a compile-time static. Dynamic per-avatar
entries need a menu-widget extension (runtime-labelled entries or a
dynamic submenu builder), which should be designed once for every
consumer (the minimap here, later the world map and radar), not
special-cased.

Deps: [[viewer-contact-sets]] for the set actions; the dynamic-label
widget work has no task yet and belongs to this one.

## Built (2026-08-21)

The widget half first, because it is the part every later consumer wants:
`MenuItemDef::DynamicSubmenu { label, slot }` is a submenu whose lines are
**not** authored. Its labels come from a named slot of the `MenuDynamicSlots`
resource, and the line is absent while the slot is empty — so "one line per
avatar under the cursor" is a menu the domain fills rather than a menu the
domain rewrites.

The interesting decision is what a dynamic line *carries*. Nothing, is the
answer: a line is a label, and a pick reports `(slot, index)` through
`MenuDynamicPick` instead of an action string. That is what let the whole
declaration stay `&'static` — a runtime action string would have had to travel
through `UiAction` and every menu consumer in the tree. Who the third line
means is the snapshot the opener kept, which is the model the minimap's mark
actions already used for the pick radius.

The other half is the asynchronous one. A popup is built once, at open, so a
name that resolves a moment later would leave its line reading "(loading)"
until the user closed the menu. `SetMenuDynamicLabels` re-labels the open lines
in place (and remembers them for the next open) — the reference does exactly
this from its name-cache callback (`setAvatarProfileLabel`). Only the text is
rewritten: the line count is fixed at open, because a menu that grows a line
under the pointer moves what the user is about to click.

In the minimap the avatar group now has the reference's two shapes. One dot
under the cursor is the old menu (View Profile, More Options ▸ …). Several is
the list: a **View Profiles** submenu, one line per avatar in pick-radius
order, and an **Add to Set** that files all of them at once. The marks were
already whole-radius. A name nobody knows yet is asked for and drawn as
`(loading)` rather than as a UUID — a key says nothing about who is under the
cursor — and `refresh_minimap_menu_names` runs only until the last answer
lands.

`OpenAddToContactSet` became a list of residents (with `::one` / `::many` /
`.moving_from`), so the add-to-set floater files one or many under the one set
the user picks, and says so the reference's two ways: one resident is named,
several are counted (`AddToContactSetMultipleSuccess`, which the notification
catalogue already carried). The avatar pie, the profile floater and the panel's
Move to Set… go through the same list, single-element.

Tests: the widget's three (an empty slot drops its line, a list reports the
picked index and writes no `UiAction`, a late label rewrites the open line),
the minimap's two (the avatar entries split by how many are under the cursor —
the reference's `size() == 1` rule — and a profile pick opens the avatar at
that index), and the panel's success-notification split.

Not done here, and split out instead: multi-*selection* (as opposed to this
implicit pick-radius multi-targeting) in the radar and the People panel's
lists — [[viewer-radar-multi-select]], [[viewer-people-lists-multi-select]],
[[viewer-avatar-picker-multi-pick]]. RLV name-hiding stays with
[[viewer-rlv-enforce-info-hiding]].
