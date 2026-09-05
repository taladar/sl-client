---
id: viewer-widget-any-mouse-button-activates
title: A secondary or middle click presses every button in the viewer
topic: viewer
status: done
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

**Fixed** in the `taladar/bevy` fork this workspace already pins, at rev
`76dc042f8`: every widget in `bevy_ui_widgets` now acts on
`PointerButton::Primary` alone, and `Activate` carries
`button: Option<PointerButton>` (`None` for a keyboard activation) so the button
is no longer discarded at the boundary.

The gate went wider than the button, because the button was not alone in the
defect — a middle or secondary click equally ticked a checkbox, picked a radio,
selected a list row, chose a menu item and started a slider or scrollbar drag.
`text_input` was already the exception, and its `if press.button !=
PointerButton::Primary { return; }` is the idiom the other seven files now
follow: `button.rs`, `checkbox.rs`, `radio.rs`, `list.rs`, `menu.rs`,
`slider.rs`, `scrollbar.rs`.

The guard returns *before* the handler stops propagation, matching
`text_input`'s. A widget that has decided to ignore a button must not swallow it
too, or an ancestor that does want the secondary click — a context menu on the
panel behind a caption — would never see it.

`Pointer<Cancel>` carries no button, so the cancel handlers are unchanged: they
only clear a `Pressed` that a primary press put there.

It arrived here as exactly what the pin promised. The 124 rows in
`ui_contract::contracts` that recorded the wrong emission failed together with
"emitted [], the contract wants […]", and the correction is the table diff that
deletes them; middle and secondary clicks are inert now, which needs no row at
all. `Row::emits_wrongly` and `Row::bug` went with the last of their rows —
`LayoutClaim::KnownBroken` remains for a *layout* pin, and the census a bug id
in the table bought is recoverable from this commit.

The other half — that the button survives the boundary at all — could not be
pinned by absence, so it has a test of its own:
`an_activation_says_which_pointer_button_raised_it` asserts a primary click
raises `Some(PointerButton::Primary)`, `Enter` raises `None`, and the other two
buttons raise nothing.
