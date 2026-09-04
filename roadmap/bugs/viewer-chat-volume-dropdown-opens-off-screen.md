---
id: viewer-chat-volume-dropdown-opens-off-screen
title: The chat volume select opens upward into nothing
topic: viewer
status: bugs
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
