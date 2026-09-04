---
id: viewer-widget-any-mouse-button-activates
title: A secondary or middle click presses every button in the viewer
topic: viewer
status: bugs
origin: viewer-ui-interaction-contracts sweep (2026-09-04)
points: 3
refs: [viewer-ui-interaction-contracts]
---

Context: [context/viewer.md](../context/viewer.md).

Found by the first run of the interaction-contract sweep, which drives the whole
input alphabet at every interactive node: **62 controls emit their action on a
`PointerButton::Middle` click and 62 on a `PointerButton::Secondary` click**,
identically to `Primary`. Save, Cancel, Decline, Block, Join, "Allow access",
every toast's close button — the context click anywhere on one commits it.

(The vocabulary matters, so: the sweep presses `MouseButton::Right`, which
`bevy_picking`'s input plugin maps to `PointerButton::Secondary` through a fixed
table — `Left`→`Primary`, `Right`→`Secondary`, `Middle`→`Middle`. Which physical
button that is belongs to the user's pointer settings, which is exactly why the
widget guards in this workspace are all written against `PointerButton` and why
this entry is too.)

The cause is upstream and one place. `bevy_ui_widgets`' button observers never
read which button was pressed
(`bevy_ui_widgets/src/button.rs`, the fork's `43aaa0d`):

```text
fn button_on_pointer_click(
    mut click: On<Pointer<Click>>,
    mut q_state: Query<
        (Has<Pressed>, Has<InteractionDisabled>, Has<ActivateOnPress>),
        With<Button>,
    >,
    mut commands: Commands,
) {
    let entity = click.entity;
    if let Ok((pressed, disabled, on_press)) = q_state.get_mut(entity) {
        click.propagate(false);
        if pressed && !disabled && !on_press {
            commands.trigger(Activate { entity });
        }
    }
}
```

The gates are `Pressed`, `InteractionDisabled` and `ActivateOnPress` — never
`click.button`. `bevy_picking` does carry it (`Click { button: PointerButton }`,
mapped from all three of Left/Right/Middle in its input plugin), so the
information exists right up to the `Pointer<Click>` → `Activate` boundary and is
thrown away there: `Activate` has only an `entity` field. **No downstream
observer can filter even if it wanted to**, which is why all 89 `On<Activate>`
observers in this workspace are equally affected, including the generic
`spawn_button` wiring in `sl-viewer-ui-core/src/ui_element.rs`.

`ActivateOnPress` (the menu buttons) makes it fire on the *down* edge of any
button, so a menu opens on a secondary click too.

Everywhere the viewer writes its **own** pointer observer it already filters —
`if press.button != PointerButton::Primary { return; }` in `ui_combo`, `menu`,
`ui_tab`, `ui_table`, `ui_search`, `floater`, `ui_color_picker`,
`virtual_list`. So the convention is settled and only the `Activate` path
escapes it. The same widget can hold both halves: a combo anchor carries
`Button` *and* a `Primary`-filtered `toggle_combo_popover`, so a `Secondary`
click on it fires the action but does not open the dropdown.

**Fix**: in the `taladar/bevy` fork this workspace already pins, gate
`button_on_pointer_down` / `_up` / `_click` on `PointerButton::Primary` and give
`Activate` a `button` field so a consumer that *wants* `Secondary` (the
inventory and radar context menus) can still have it. One place, against 89 call
sites for any downstream workaround. Worth an upstream PR.

Until then the behaviour is pinned as-is in `ui_contract::contracts`, with the
offending rows commented against this id — so the fix arrives as a table diff
that deletes them.
