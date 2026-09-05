---
id: viewer-nonblocking-overlay-steals-focus
title: A see-through container in front of a field clears its focus, at random
topic: viewer
status: done
origin: found while fixing [[viewer-testkit-click-focus-resource-sensitive]]
  (2026-09-05)
points: 3
refs: [viewer-testkit-click-focus-resource-sensitive,
  viewer-widget-any-mouse-button-activates]
---

Context: [context/viewer.md](../context/viewer.md).

A node carrying `Pickable { should_block_lower: false, is_hoverable: true }`
that sits **in front of** a focusable node which is not its own descendant made
clicking that node focus it only about half the time. The other half the click
landed and did nothing: the field took focus and lost it again in the same
frame.

## Why

`bevy_picking` walks the hits front to back and stops at the first blocker, so a
non-blocking-but-hoverable node in front leaves **two** entries in the hover map
— the overlay and the control under it. `bevy_input_focus`' `click_to_focus`
then raised one `AcquireFocus` per hit (it fires once per hit entity, gated on
`press.entity == press.original_event_target()`, which only suppresses the
*bubbling copies* of one event, not several distinct targets):

- the control's found its `TabIndex` and focused it;
- the overlay's found none, bubbled all the way to the window, and
  `acquire_focus`'s window arm called `focus.clear()`.

Whichever landed last won, and the order was the hover map's `EntityHashMap`
iteration order — a function of entity ids, so a coin flip that is stable within
a run and moves whenever anything upstream spawns a different number of
entities. The same clearing arm produced
[[viewer-testkit-click-focus-resource-sensitive]]; that one was a harness
fixture shaped like nothing the viewer builds, this one is a shape the viewer
does build.

## What landed

### The general shape, upstream

Fixed in the `taladar/bevy` fork this workspace already pins, at rev
`0a41f66f4` (`bevy_input_focus/src/tab_navigation.rs`): **only the hit a press
stopped on decides focus**.

The entry filed this as "only the topmost hit has any business deciding focus".
Writing it showed that is the wrong end: the topmost hit *is* the transparent
overlay, and focusing from it would clear the focus every time instead of half
the time. The right rule falls out of what `build_hover_map` actually produces —
a run of non-blocking hits front to back, terminated by **at most one** blocking
hit. Everything in front of the blocker declared itself transparent to the pick,
so the blocker is what the user clicked *through to*, and it is the hit whose
`AcquireFocus` describes what happened. When nothing blocks at all (every hit
opted out and window picking is off) the hindmost hit stands in, tie-broken by
entity so the answer does not depend on hash order either.

Two escape hatches keep it from taking click-to-focus away from anyone:
`Option<Res<HoverMap>>`, because having the `bevy_picking` *feature* on does not
mean `PickingPlugin` was added; and a pointer with no entry in the hover map (a
synthesised press) keeps the old unconditional behaviour. The rule itself is
split out as `focus_deciding_hit` and tested for the property that actually
broke — that it answers the same whichever order it reads the hits in.

The fork is the fix — deliberately not sent to `bevyengine/bevy`, the same way
[[viewer-widget-any-mouse-button-activates]] shipped. Nothing here is waiting on
an upstream decision, so a bump to a later Bevy re-applies this diff along with
the others the pin already carries.

### The two live containers with that shape

`grep` for `should_block_lower: false` found fifteen sites. Most are
**ancestors** of their own focusables (`ui-root`, `nearby-chat-bar`,
`volume-cluster`, `parcel-audio-bar`, `quick-prefs-button-wrapper`,
`floater-dock-host`, five in `bottom_toolbar.rs`), so they render *behind* what
they contain and their own children block them out of the hover map — that is
what the non-blocking `Pickable` is for, and they keep it. The minimap's
mouselook transparency already pairs it with `is_hoverable: false`.

Two were genuinely exposed, both lifted in front of other panels by a
`GlobalZIndex` while owning no focusable of their own, and both now
`Pickable::IGNORE`:

- **`notification-channel`** (`notification_host.rs`) — `TOAST_CHANNEL_Z` puts
  it above every floater and bar, and its box covers the toasts *and the gaps
  between them*. Its hover observers are on the toast cards, which are children
  and pick for themselves, so the container never wanted a hover event.
- **`conversations-dock-host`** (`conversations.rs`) — `DOCK_HOST_Z` is one
  above the bottom bar and the host carries a 4 px padding rim around whatever
  is docked, so that rim sits in front of the bottom bar's buttons. Nothing
  observes the host; only `position_conversations_dock_host` reads it.

Neither changes what `pointer_over_blocking_ui` answers: it counts *blocking*
hits, and both were already non-blocking.

`texture-picker-row` is non-blocking on purpose (the stable tree container
beneath it is what the world-pick's UI-block test sees) and its hoverability is
load-bearing — `on_row_hover` / `on_row_unhover` are on the row. It stays as it
is; `Button` does not require `TabIndex`, so a row is not focusable and the new
rule costs it nothing.

### The tooth

`a_click_focuses_a_field_under_a_nonblocking_overlay`, beside
`a_click_focuses_the_field_at_any_entity_id` in `sl-viewer-testkit`'s
`interact.rs`: a focusable field with a `GlobalZIndex`-lifted, non-blocking,
hoverable overlay over it, swept across eight entity ids. It asserts the hover
map really held **both** entities before checking the focus, so the fixture
cannot pass by failing to build the shape it is about.

A `crate::orphan_root_violations`-style check was considered for the third half
and dropped: with the upstream rule fixed, a non-blocking hoverable node in
front of a focusable one is no longer a defect, and a violation list that
reports a non-problem is worse than none.

## How it was verified

A/B'd across the pin, which is the only way to tell a fix from a lucky run here.
With the pin held at the **old** rev `76dc042f8` and nothing else changed, the
new test fails and names the ids it fails at:

```text
a click must focus the field under a non-blocking overlay whatever the entity
ids are, and it did not at: ["1 entities ahead of it: focus is None",
                            "5 entities ahead of it: focus is None"]
```

Two ids in eight, `InputFocus` empty — the coin flip, in the live shape. The two
sibling tests (`a_click_focuses_the_field_it_lands_on`,
`a_click_focuses_the_field_at_any_entity_id`) pass on that same old rev, which
is the point of adding a third: the harness was already sound and neither of
them can see this, because neither builds an overlay. On the new rev all eight
ids focus the field.

Also: the `bevy_input_focus` suite (34 tests) passes with `--features
bevy_picking`, and the whole workspace (181 suites) passes on the bumped pin,
which is what carries the two container changes through the existing interaction
sweeps.

Reference (Firestorm, read-only): none — this is `bevy_picking` /
`bevy_input_focus` behaviour, not a viewer-protocol question.
