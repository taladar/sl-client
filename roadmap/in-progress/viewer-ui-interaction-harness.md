---
id: viewer-ui-interaction-harness
title: Headless synthetic-pointer input for the UI test harness
topic: viewer
status: in-progress
origin: user request (2026-07) — end manual re-testing of UI interactions
points: 8
refs: [viewer-ui-test-harness]
---

Context: [context/viewer.md](../context/viewer.md),
[context/testing.md](../context/testing.md).

In progress (2026-08-30). Resolved by reading the Bevy 0.19 sources: winit
never writes `PointerInput` — `bevy_picking`'s `mouse_pick_events` derives
it from `WindowEvent` in `First` — so the driver writes the raw typed
messages *plus* their `WindowEvent` wrappers and lets the picking input
plugin derive the rest (one source of truth). A hit uses the previous
frame's layout, so every pointer step is its own `update()` and a click is
three updates. Double-click counting is wall-clock, so `click()` zeroes
`PickingSettings.multi_click_interval` around its frames. `Activate`
synthesis needs `UiWidgetsPlugins` + `InputDispatchPlugin`; `ui_picking`
needs `UiStack` and visibility propagation (`bevy::camera::CameraPlugin`).

The harness in `ui_test.rs` drives behaviour by `trigger(Activate)`, which
deliberately skips hit-testing: it cannot say whether the button is *where
the user's pointer thinks it is*, cannot distinguish left from right from
middle click, and cannot scroll, drag, or hover. Bevy's upstream UI
event-mocking PR (#17399) died unmerged, but everything needed is `pub` in
0.19 — `PickingPlugin`/`InteractionPlugin`, `bevy_ui::picking_backend::
{UiPickingPlugin, ui_picking}`, `ui_stack_system` — so this is ordinary
downstream code, the same resolution as [[viewer-ui-test-harness]]'s own
"one real unknown".

Build `ui_interact.rs`: an `InteractionTest` over `LayoutTest` adding
bevy_picking + the UI picking backend + `bevy::input::InputPlugin` + one
headless `Window`/`PrimaryWindow` entity and a mouse pointer entity; a
driver API (`hover`/`click`/`scroll`/`drag`/`key`) that writes
`PointerInput` **and** the matching raw `bevy_input` messages so observers
and `ButtonInput`/`AccumulatedMouseScroll` readers agree (the same
consistency winit provides live); target positions taken from the real
`ComputedNode` layout via `find_by_name` — so an occluded or mispositioned
control **fails**, which is the new coverage this tier buys over
`trigger(Activate)`. Add a generic message recorder generalising
`RecordedActions` to `FloaterCommand`, `SlCommand` and menu-open messages.

Prove the teeth the house way: a click on an occluded/mispositioned node
must fail. Port one `pie_menu.rs` commit-select test to the synthetic
pointer as the reference consumer.

Known risks to establish early: `PreUpdate` picking vs `PostUpdate` layout
ordering (hits use last frame's layout — the drag driver needs a per-step
`update()`); double-click synthesis (bevy_picking's `Click` carries no
count); whether `bevy_ui_widgets`' interaction systems must be added for
`Activate` synthesis from real clicks.
