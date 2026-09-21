---
id: viewer-audit-checkbox-box-widget
title: The settings checkbox box is spawned and repainted by hand in seven places
topic: viewer
status: ready
origin: viewer-audit-preferences-hub-decoupling (2026-09-20)
points: 3
refs: [viewer-audit-preferences-hub-decoupling, viewer-audit-skin-token-coverage]
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-ui-widgets`' `settings_binding::bound_checkbox` gives a checkbox its
*behaviour* — the store binding, the `Checked` state, the account guard — and
stops there. Every caller then hand-spells the same box around it:

```rust
    commands.spawn((
        bound_checkbox(binding),
        Node {
            width: Val::Px(CHECK_SIZE),
            height: Val::Px(CHECK_SIZE),
            border: UiRect::all(Val::Px(2.0)),
            flex_shrink: 0.0,
            ..default()
        },
        BorderColor::all(CONTROL_BORDER),
        BackgroundColor(CHECK_OFF),
        TabIndex(0),
        SomeLocalCheckboxBox,
        ChildOf(row),
    ));
```

and pairs it with a marker component and a system that is, five times out of
six, character-for-character this:

```rust
    for (mut fill, checked) in &mut boxes {
        let target = if checked { CHECK_ON } else { CHECK_OFF };
        if fill.0 != target {
            fill.0 = target;
        }
    }
```

The sites: `sl-viewer-preferences` four times (`preferences.rs:386`,
`phototools.rs:1305`, `quick_preferences.rs:950`, `debug_settings.rs:459` and
`:612`, the last two reusing `preferences.rs`'s constants and marker), plus
`sl-viewer-notices/src/experiences_floater.rs:1160`,
`sl-viewer-people/src/radar.rs:1016` and `sl-viewer-search/src/search.rs:1882`.
Markers: `PrefCheckboxBox`, `PhotoCheckboxBox`, `QuickPrefCheckboxBox`,
`NotifyCheckboxBox`, `RadarLimitCheckbox`, `SearchCheckboxBox`. Only
`drive_pref_checkbox_visual` differs, and only by a third `CHECK_DISABLED`
branch for the account guard.

Inside `sl-viewer-preferences` the *palette* is triplicated as well:
`preferences.rs`, `phototools.rs` and `quick_preferences.rs` each declare their
own `CONTROL_BORDER` = `srgb(0.40, 0.50, 0.62)`, `CHECK_OFF` =
`srgb(0.12, 0.14, 0.18)` and `CHECK_ON` = `srgb(0.30, 0.70, 0.45)`. Only the
size genuinely differs (18 px in the floater, 16 px in the other two, 13 px in
`sl-viewer-environment`'s `my_environments`, 14 px in `experiences_floater`).

The shape that removes all of it: in `sl-viewer-ui-widgets`, next to
`bound_checkbox`, a `checkbox_box(binding, size, paint)` bundle carrying one
`CheckboxBox` marker and a `CheckboxPaint { on, off, disabled }` component, and
one system over `(&mut BackgroundColor, Has<Checked>, Has<InteractionDisabled>,
&CheckboxPaint)`. A per-entity paint keeps every caller's current colours
exactly as they are, so nothing changes on screen; the callers keep their own
`Name` and any extra markers they query for other reasons.

Sequencing note: the colours are hard-coded `Color`s today and
[[viewer-audit-skin-token-coverage]] will want them as skin tokens. One
`CheckboxPaint` per widget is the cheaper thing to re-point at a token later
than seven scattered constant sets, so this is worth doing first — but it does
mean the two tasks touch the same lines, and they should not run in parallel.
