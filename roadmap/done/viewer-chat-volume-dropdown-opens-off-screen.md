---
id: viewer-chat-volume-dropdown-opens-off-screen
title: The chat volume select opens upward into nothing
topic: viewer
status: done
origin: viewer-ui-interaction-contracts sweep (2026-09-04)
points: 2
refs: [viewer-ui-interaction-contracts]
---

Context: [context/viewer.md](../context/viewer.md).

Found by the interaction-contract sweep: clicking `local-chat-volume-button`
lays the whisper/say/shout dropdown out **above the top edge of the window** —
`local-chat-volume-dropdown` at `[120, -57]..[192, 9]` in a 1600×1200 viewport,
with all six of its rows at negative Y. Three of the four options are
unreachable and the fourth is half a row.

The cause is in `sl-viewer-chat/src/local_chat_input.rs`, `build_volume_select`:
the panel is hand-positioned, and unconditionally upward.

```text
Node {
    display: Display::None,
    position_type: PositionType::Absolute,
    bottom: Val::Percent(100.0),
    right: Val::Px(0.0),
    ..column(Val::Px(0.0))
}
```

`bottom: 100%` means "sit entirely above my anchor" with no fallback and no
window margin. It is right in the running viewer only because the chat bar
happens to sit at the bottom of the screen; the same widget in a panel anywhere
else — the gallery card, a floater, a docked chat at the top — opens into
nothing.

The viewer already has the machinery this wants. `ui_combo`'s dropdown declares

```text
Popover {
    positions: vec![
        PopoverPlacement {
            side: PopoverSide::Bottom,
            align: PopoverAlign::Start,
            gap: 0.0,
        },
        PopoverPlacement {
            side: PopoverSide::Top,
            align: PopoverAlign::Start,
            gap: 0.0,
        },
    ],
    window_margin: 4.0,
}
```

— an ordered list of placements the positioner falls through when the first has
no room, plus a margin it will not cross. **Fix**: give the volume dropdown the
same, with `Top` first (its preferred side) and `Bottom` as the fallback, and
drop the hand-written `bottom`/`right` insets. As a bonus it then also declares
itself a floating layer, which is what the layout checks read to know a
drop-down is *allowed* to leave its parent's box — see `popover_ancestors` in
`sl-viewer-testkit`.

Pinned meanwhile as a `LayoutClaim::KnownBroken` row in
`ui_contract::contracts` naming this id. That row is a **canary**: it asserts
the breakage is still there, so whoever fixes this is told to delete it.

## Done

The panel is a `Popover` now, with `Top` (its preferred side, and the only one
the old inset could express) first and `Bottom` as the fallback, both aligned
`End` so the panel's right edge still lines up with the button's — what
`right: 0` used to say. The hand-written `bottom` / `right` insets are gone; the
positioner owns the placement and keeps a 4 px window margin, the same one
`ui_combo` keeps.

Two placement tests in `local_chat_input`, driven at **both** ends of an 800×600
window, because one case cannot tell a fallback from a hard-coded side: parked
at the bottom the panel must open upward, parked at the top it must open
downward, and in both cases lie inside the viewport. Their teeth were checked by
deleting the `Bottom` placement — the top-of-window case then reproduced the
reported geometry exactly (`[120, -56]..[192, 10]`, the button at y 9).

The side assertions allow one pixel, which is the button's border rather than
slack: absolute positioning is measured against the anchor's **padding** box, so
the panel starts inside the border line.

One thing the entry did not anticipate. This widget builds its panel once and
toggles `Display::None`, where a menu or a combo spawns its popover on open and
despawns it on close — so it is the first popover that is *resident while
closed*, and the testkit's exemptions were written for the other shape. Left
alone, `popover_ancestors` would have retired the overflow check for the chat
bar and everything above it, in every cell of the sweep, forever. Both
exemptions now go through `sl_viewer_testkit::open_popovers`, which counts only
popovers that are laid out — the same zero-size test `viewport_violations`
already uses to skip a closed panel.

The `KnownBroken` canary rows are deleted, as the pin asked. That left the
mechanism with no user, so it has teeth instead: `judge_layout` is split out of
`judge` and `a_known_broken_pin_fails_the_day_it_is_fixed` drives both
directions of the inverted claim — a backwards check nothing exercises is the
easiest kind to have silently inverted.
