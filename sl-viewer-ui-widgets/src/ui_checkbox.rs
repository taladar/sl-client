//! The shared **checkbox** widget (`viewer-skin-checkbox-radio-shape`).
//!
//! There was no checkbox widget. `sl-viewer-ui-widgets` had `ui_radio`,
//! `ui_combo`, `ui_slider`, `ui_table`, `ui_tab` and `ui_text_input`, and every
//! checkbox in the viewer was whatever the panel that needed one drew: a text
//! glyph in most, a filled square `Node` in the phototools and environment
//! windows, each with that panel's own constants. Nineteen files declared their
//! own `CHECKED_GLYPH`. They did not agree with each other, and none of them
//! could be reached by a skin.
//!
//! # The state model is the selector one, and there is no paint system
//!
//! A checkbox has exactly the two states the engine already tracks —
//! [`Checked`] and [`InteractionDisabled`], which `bevy_flair` syncs to
//! `:checked` and `:disabled` — so every one of its four looks is a rule in
//! `common.css` and **nothing here paints**. That is not tidiness: the
//! reference expresses checked / unchecked / pressed / disabled as six separate
//! textures, so a per-frame fill could never become the Vintage look, while a
//! selector can ([[viewer-skin-image-backed-widgets]] swaps the rule's
//! background for a nine-slice and the Rust side never learns).
//!
//! # The tick is the skin's glyph, and the widget never names one
//!
//! `bevy_flair` supports `::before` / `::after` (opt in with
//! [`PseudoElementsSupport`]) and maps `content` onto the pseudo-element's
//! `TextSpan`, so **which mark a checkbox wears is a stylesheet decision**.
//! Unicode offers several — U+2713 `✓`, U+2714 `✔`, U+2717 `✗`, U+00D7 `×` —
//! and a skin that wants a cross, a dot or a filled square writes it in
//! `common.css` rather than asking for a Rust change.
//!
//! So the widget spawns a text node that names no glyph and stops. An
//! unchecked box has no rule, so its span stays empty; there is no glyph to
//! rewrite and no system to rewrite it. The node is not quite *empty* — it
//! holds one zero-width space, for a measurement reason `spawn_checkbox`
//! explains at the line that puts it there.
//!
//! # What it is not
//!
//! Not a *binding*. [`crate::settings_binding::bound_checkbox`] pairs the
//! headless [`Checkbox`] with a `SettingBinding` and owns the two-way sync with
//! the settings store; this module draws one. A panel that wants a bound
//! checkbox spawns this and adds that bundle to the returned
//! [`SpawnedCheckbox::checkbox`].

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::text::LineHeight;
use bevy::ui::{Checked, InteractionDisabled};
use bevy::ui_widgets::{Checkbox, checkbox_self_update};
use bevy_flair::style::components::{ClassList, PseudoElementsSupport};
use sl_viewer_ui_core::i18n::Translated;
use sl_viewer_ui_core::skin::{
    CHECKBOX_BOX_CLASS, CHECKBOX_CLASS, CHECKBOX_TICK_CLASS, TEXT_CLASS,
};
use sl_viewer_ui_core::ui_font::UiFont;

/// The box's edge length, in logical pixels. The reference's is 15×15 at its
/// own scale; this is the size the viewer's 13 px chrome font sits beside.
const BOX_SIZE: f32 = 14.0;

/// The gap between the box and its caption, in logical pixels.
const LABEL_GAP: f32 = 6.0;

/// The tick's font size, as a fraction of the box it sits in.
///
/// Not the caption's size, which is the caller's and says nothing about the
/// square the mark has to fit: a 20 px caption beside a 14 px box would put a
/// 20 px glyph inside it. A skin that wants a different mark sets `font-size`
/// on `.sk-checkbox-tick` along with its `content`.
const TICK_FONT_SCALE: f32 = 0.8;

/// A checkbox to spawn.
#[derive(Clone, Debug)]
pub struct CheckboxSpec {
    /// The element id the checkbox reports in its `UiAction`, and the prefix of
    /// its nodes' [`Name`]s.
    pub element: &'static str,
    /// The caption beside the box.
    pub label: String,
    /// Its focus stop — the checkbox, not the box or the caption.
    pub tab_index: i32,
    /// The caption's font size, in logical pixels.
    pub font_size: f32,
    /// Whether [`label`](Self::label) is a Fluent **key** to translate
    /// (re-resolved on a locale change) rather than literal display text. Use
    /// it for real UI; `false` for the gallery and tests, whose labels are
    /// fixed sample text.
    pub translate_label: bool,
}

/// What [`spawn_checkbox`] built: the three nodes a caller may need to reach.
#[derive(Clone, Copy, Debug)]
pub struct SpawnedCheckbox {
    /// The checkbox itself — the focus stop, the [`Checkbox`], and what a
    /// caller adds a `SettingBinding` or an observer to.
    pub checkbox: Entity,
    /// The box node, for a caller that wants to size or place it.
    pub box_node: Entity,
    /// The caption, for a caller that rewrites its text.
    pub label: Entity,
}

/// Spawn a checkbox under `parent`.
///
/// The returned [`SpawnedCheckbox::checkbox`] carries the headless
/// [`Checkbox`], so a caller wires behaviour by adding to it — a
/// `SettingBinding` through [`crate::settings_binding::bound_checkbox`], or its
/// own `ValueChange<bool>` observer.
pub fn spawn_checkbox(
    commands: &mut Commands,
    parent: Entity,
    spec: &CheckboxSpec,
) -> SpawnedCheckbox {
    let checkbox = commands
        .spawn((
            Checkbox,
            TabIndex(spec.tab_index),
            Node {
                align_items: AlignItems::Center,
                column_gap: Val::Px(LABEL_GAP),
                ..default()
            },
            ClassList::new_with_classes([CHECKBOX_CLASS]),
            Pickable::default(),
            Name::new(format!("{}:checkbox", spec.element)),
            ChildOf(parent),
        ))
        // **It ticks itself.** `bevy_ui_widgets`' headless `Checkbox` only
        // *announces* a toggle — a click or `Space` raises `ValueChange<bool>`
        // and nothing adds or removes `Checked` unless something observes it.
        // Without this a checkbox with no binding behind it is a box that never
        // ticks, which is what a skin author sees in the gallery and what every
        // caller wiring its own behaviour would have to remember. A bound
        // checkbox is unharmed: `settings_binding` answers the same event by
        // writing the store and reflecting it, so both paths converge on the
        // state the store ends up holding.
        .observe(checkbox_self_update)
        .id();
    let box_node = commands
        .spawn((
            Node {
                width: Val::Px(BOX_SIZE),
                height: Val::Px(BOX_SIZE),
                flex_shrink: 0.0,
                border: UiRect::all(Val::Px(1.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            ClassList::new_with_classes([CHECKBOX_BOX_CLASS]),
            // The box and its tick are not pick targets: the whole row is, so a
            // click on the caption toggles too — the reference's behaviour, and
            // the reason the hover never reads as "only the box is live".
            Pickable::IGNORE,
            Name::new(format!("{}:checkbox-box", spec.element)),
            ChildOf(checkbox),
        ))
        .id();
    commands.spawn((
        // It names no glyph: the mark is `.sk-checkbox:checked
        // .sk-checkbox-tick::before`'s `content`, so the skin chooses it.
        //
        // The one character it does carry is a **zero-width space**, and it is
        // there for the measure rather than for anything anyone can see.
        // `bevy_text` pushes each span's font as a *range* style and skips
        // empty ranges, so a text node holding no characters is laid out at
        // parley's own defaults — a 20 px line, whatever font the node asked
        // for. Inside this 14 px square that is six pixels of overflow, and
        // taffy folds a child's content into every ancestor's `content_size`,
        // so it reappears as the checkbox overflowing, then its row, then the
        // tab panel, then the floater: four viewer-wide layout sweeps failing
        // at once. One default-ignorable character gives the line a range to
        // style; the shaper gives it zero advance, so the tick stays centred.
        // See [[viewer-bevy-empty-text-measures-at-parley-defaults]] — with
        // that fixed upstream this goes back to `Text::default()`.
        //
        // `PseudoElementsSupport` is safe to insert anywhere as of the
        // `dbaaa48` bevy_flair pin: its text branch used to spawn the
        // pseudo-element without a `StyleData` that `PseudoElement`'s insert
        // hook `expect`s, so this component took down any app without the full
        // style plugin — every widget unit test included. The block branch of
        // the same function always spawned one; now both do. Since `946f8a2`
        // the pseudo-element also **inherits** the font below, as CSS says it
        // does — without that it brought bevy's default 20 px along with it.
        Text::new("\u{200b}"),
        PseudoElementsSupport,
        UiFont::Sans.at(BOX_SIZE * TICK_FONT_SCALE),
        // The line box is the box, so the mark cannot be taller than the square
        // it marks whatever font size a skin gives it.
        LineHeight::Px(BOX_SIZE),
        ClassList::new_with_classes([CHECKBOX_TICK_CLASS]),
        Pickable::IGNORE,
        ChildOf(box_node),
    ));
    let mut label = commands.spawn((
        UiFont::Sans.at(spec.font_size),
        ClassList::new_with_classes([TEXT_CLASS]),
        Pickable::IGNORE,
        Name::new(format!("{}:checkbox-label", spec.element)),
        ChildOf(checkbox),
    ));
    if spec.translate_label {
        // Empty until the bundle resolves the key, as every translated label is.
        label.insert((Text::default(), Translated::new(spec.label.clone())));
    } else {
        label.insert(Text::new(spec.label.clone()));
    }
    SpawnedCheckbox {
        checkbox,
        box_node,
        label: label.id(),
    }
}

/// Gallery element: the four looks a skin has to dress, side by side.
///
/// All four at once because they are a *set*: a skin author changing
/// `--check-bg-checked` needs to see it against the resting box, and the two
/// refused states are the pair that is easiest to get wrong — a greyed box with
/// a mark that stayed bright, or a refused empty box that still looks live.
///
/// The refused pair carry `InteractionDisabled`, which is also what makes them
/// inert here: the gallery's specimens are live widgets, so the two enabled
/// boxes really do tick when clicked.
pub fn spawn_checkbox_element(
    commands: &mut Commands,
    parent: Entity,
    cx: sl_viewer_ui_core::ui_element::ElementCx,
) -> Entity {
    let column = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(6.0),
                ..default()
            },
            Name::new("checkbox-states"),
            ChildOf(parent),
        ))
        .id();
    for (element, caption, checked, refused) in [
        ("checkbox-resting", "Show property lines", false, false),
        ("checkbox-checked", "Play media automatically", true, false),
        (
            "checkbox-refused",
            "Allow other residents to fly",
            false,
            true,
        ),
        (
            "checkbox-refused-checked",
            "Everyone can see this",
            true,
            true,
        ),
    ] {
        let spawned = spawn_checkbox(
            commands,
            column,
            &CheckboxSpec {
                element,
                label: cx.text(caption),
                tab_index: 0,
                font_size: cx.font_size,
                translate_label: false,
            },
        );
        if checked {
            commands.entity(spawned.checkbox).insert(Checked);
        }
        if refused {
            commands
                .entity(spawned.checkbox)
                .insert(InteractionDisabled);
        }
    }
    column
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ui_widgets::ValueChange;
    use pretty_assertions::{assert_eq, assert_ne};
    use sl_viewer_testkit::{LayoutTest, overflow_violations, settle, spawn_under_root};

    /// A boxed error so tests can use `?` instead of the disallowed
    /// `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// A world with just enough to spawn into.
    fn app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app
    }

    /// Spawn one checkbox and hand back its parts.
    fn spawn(app: &mut App, translate: bool) -> SpawnedCheckbox {
        let parent = app.world_mut().spawn(Node::default()).id();
        let spec = CheckboxSpec {
            element: "demo",
            label: "Show property lines".to_owned(),
            tab_index: 3,
            font_size: 13.0,
            translate_label: translate,
        };
        let mut queue = bevy::ecs::world::CommandQueue::default();
        let mut commands = Commands::new(&mut queue, app.world());
        let spawned = spawn_checkbox(&mut commands, parent, &spec);
        queue.apply(app.world_mut());
        spawned
    }

    /// **Every state the skin selects on has something to select.**
    ///
    /// The widget paints nothing: its four looks are `.sk-checkbox`,
    /// `:checked`, `:disabled` and the compounds, reaching the box and the tick
    /// from the row. That only works if the row carries `Checkbox` (so
    /// `bevy_flair` has a `:checked` to sync) and each node carries its class —
    /// neither of which any headless assertion about colour would notice,
    /// because there is no colour to assert.
    #[test]
    fn the_parts_carry_what_the_rules_select_on() -> Result<(), TestError> {
        let mut app = app();
        let spawned = spawn(&mut app, false);
        let world = app.world();

        assert!(
            world.get::<Checkbox>(spawned.checkbox).is_some(),
            "without the headless widget there is no `:checked` to style"
        );
        assert_eq!(
            world.get::<TabIndex>(spawned.checkbox).map(|index| index.0),
            Some(3),
            "the checkbox is the focus stop, not the box or the caption"
        );
        for (entity, class) in [
            (spawned.checkbox, CHECKBOX_CLASS),
            (spawned.box_node, CHECKBOX_BOX_CLASS),
            (spawned.label, TEXT_CLASS),
        ] {
            assert!(
                world
                    .get::<ClassList>(entity)
                    .is_some_and(|classes| classes.contains(class)),
                "`{class}` is missing, so its rule can never match"
            );
        }
        let tick = world
            .get::<Children>(spawned.box_node)
            .and_then(|kids| kids.iter().next())
            .ok_or("the box has no tick")?;
        assert!(
            world
                .get::<ClassList>(tick)
                .is_some_and(|classes| classes.contains(CHECKBOX_TICK_CLASS)),
            "the tick carries no class, so it can never be revealed"
        );
        assert_eq!(
            world.get::<Text>(tick).map(|text| text.0.clone()),
            Some("\u{200b}".to_owned()),
            "the widget must name no glyph — the skin's `content` does. The one \
             character here is the zero-width space the measure needs"
        );
        assert!(
            world.get::<PseudoElementsSupport>(tick).is_some(),
            "without this there is no `::before` for `content` to write"
        );
        assert!(
            world
                .get::<ClassList>(tick)
                .is_some_and(|classes| classes.contains(CHECKBOX_TICK_CLASS)),
            "and no class for the rule to select"
        );
        Ok(())
    }

    /// **The caption clicks too.** Only the checkbox is a pick target; the box
    /// and the tick ignore the pointer, so a press anywhere on the row reaches
    /// the widget rather than being swallowed by whichever child is under the
    /// cursor.
    #[test]
    fn only_the_checkbox_takes_the_pointer() -> Result<(), TestError> {
        let mut app = app();
        let spawned = spawn(&mut app, false);
        let world = app.world();
        assert_eq!(
            world.get::<Pickable>(spawned.box_node),
            Some(&Pickable::IGNORE)
        );
        assert_eq!(
            world.get::<Pickable>(spawned.label),
            Some(&Pickable::IGNORE)
        );
        assert_ne!(
            world.get::<Pickable>(spawned.checkbox),
            Some(&Pickable::IGNORE),
            "the row is what a click has to reach"
        );
        Ok(())
    }

    /// A translated caption starts empty and carries its key, the way every
    /// other translated label in the widget set does.
    #[test]
    fn a_translated_caption_waits_for_its_bundle() -> Result<(), TestError> {
        let mut app = app();
        let spawned = spawn(&mut app, true);
        let world = app.world();
        assert_eq!(
            world.get::<Text>(spawned.label).map(|text| text.0.clone()),
            Some(String::new())
        );
        assert!(world.get::<Translated>(spawned.label).is_some());
        Ok(())
    }

    /// The state lives where the engine puts it: toggling `Checked` on the
    /// checkbox is the whole of "it is ticked now", with no glyph to rewrite
    /// and no colour to repaint.
    #[test]
    fn checking_it_touches_nothing_but_the_marker() -> Result<(), TestError> {
        let mut app = app();
        let spawned = spawn(&mut app, false);
        let before = app
            .world()
            .get::<ClassList>(spawned.box_node)
            .map(|classes| format!("{classes:?}"));
        app.world_mut().entity_mut(spawned.checkbox).insert(Checked);
        app.update();
        let after = app
            .world()
            .get::<ClassList>(spawned.box_node)
            .map(|classes| format!("{classes:?}"));
        assert_eq!(
            before, after,
            "a tick must not move a class — `:checked` is the selector"
        );
        Ok(())
    }

    /// **A click ticks it.** The headless widget only announces the toggle, so
    /// the marker the whole skin selects on moves only because `spawn_checkbox`
    /// attaches `checkbox_self_update`. Triggered directly rather than through a
    /// pointer: what is in question is the wiring, not the hit test.
    #[test]
    fn announcing_a_change_moves_the_marker() -> Result<(), TestError> {
        let mut app = app();
        let spawned = spawn(&mut app, false);
        app.world_mut().trigger(ValueChange {
            source: spawned.checkbox,
            value: true,
            is_final: true,
        });
        app.update();
        assert!(
            app.world().get::<Checked>(spawned.checkbox).is_some(),
            "a checkbox nothing else drives has to tick itself"
        );
        app.world_mut().trigger(ValueChange {
            source: spawned.checkbox,
            value: false,
            is_final: true,
        });
        app.update();
        assert!(
            app.world().get::<Checked>(spawned.checkbox).is_none(),
            "and untick itself"
        );
        Ok(())
    }

    /// **The tick fits its box.** The box is a fixed square and the tick is a
    /// text node inside it, so the tick's *line box* — not its ink — is what
    /// has to fit: a text node one line tall reports that line's height as its
    /// content, and a line taller than the square makes the box overflow
    /// itself. That is not cosmetic. Taffy folds a child's content into every
    /// ancestor's `content_size`, so six pixels of overflow inside a 14 px box
    /// propagate out through the checkbox, its row, the tab panel and the
    /// floater — which is exactly how this was found, as four viewer-wide
    /// layout sweeps failing at once.
    #[test]
    fn the_tick_fits_the_box_it_sits_in() -> Result<(), TestError> {
        let mut app = LayoutTest::new().build();
        let row = spawn_under_root(&mut app, (Node::default(), Name::new("row")));
        let mut queue = bevy::ecs::world::CommandQueue::default();
        let mut commands = Commands::new(&mut queue, app.world());
        spawn_checkbox(
            &mut commands,
            row,
            &CheckboxSpec {
                element: "demo",
                label: "Show property lines".to_owned(),
                tab_index: 0,
                font_size: 13.0,
                translate_label: false,
            },
        );
        queue.apply(app.world_mut());
        settle(&mut app);
        assert_eq!(
            overflow_violations(&mut app),
            Vec::<String>::new(),
            "a checkbox spills out of its own boxes"
        );
        Ok(())
    }
}
