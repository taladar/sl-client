//! The **reusable combo / dropdown widget** (`viewer-ui-combo-widget`): a button
//! showing the current selection that, when activated, opens a popover list of
//! named options and emits the chosen one — the reference's third `control_name`
//! control (`LLComboBox`), after the checkbox and the radio group.
//!
//! # The single source of truth
//!
//! [`ComboSelection`] on the anchor button carries the selected index and is the
//! only thing that decides the selection; the closed value text is derived from
//! it by [`apply_combo_selection`] (keyed off `Changed<ComboSelection>`), so an
//! external write — a panel syncing the combo to its own state — drives the same
//! visual path as a user pick, exactly as the [radio widget](crate::ui_radio)
//! does. A **user** pick additionally emits a [`ComboChanged`] message
//! (programmatic writes do not), so a consumer distinguishes "the user chose
//! this" from "we set the display".
//!
//! # Self-managed popover
//!
//! Built like the [menu](crate::menu) popups: a `bevy_ui_widgets` [`Popover`]
//! list spawned as a child of the anchor, lifted above the floaters with
//! [`GlobalZIndex`] and escaping any clipping ancestor with `OverrideClip`, its
//! rows consuming their press so an outside press falls through to the root
//! dismiss observer ([`dismiss_combos_on_press`]).
//!
//! Reference (Firestorm, read-only): `llcombobox`, `llfloater` popup handling.

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::ui_widgets::Button;
use bevy::ui_widgets::popover::{Popover, PopoverAlign, PopoverPlacement, PopoverSide};
use bevy_flair::style::components::ClassList;

use crate::i18n::Translated;
use crate::ui::{UiRoot, UiScaffoldSystems, row};
use crate::ui_font::UiFont;

/// The combo popover's z-index — above the floaters, just below the menu popups
/// ([`crate::menu`]'s `10_000`) so a menu opened over a combo still wins.
const COMBO_Z_INDEX: i32 = 9_500;

/// The anchor button's border colour (the bordered-button idiom shared with the
/// build tab's cycle stand-ins).
const COMBO_BORDER: Color = Color::srgba(0.4, 0.4, 0.45, 1.0);

/// The anchor button's background.
const COMBO_BACKGROUND: Color = Color::srgba(0.18, 0.18, 0.2, 1.0);

/// The open popover's background.
const POPOVER_BACKGROUND: Color = Color::srgba(0.14, 0.14, 0.16, 1.0);

/// The popover's border colour.
const POPOVER_BORDER: Color = Color::srgba(0.42, 0.42, 0.48, 1.0);

/// A popover row's colour when the pointer is over it.
const ROW_HOVER: Color = Color::srgba(0.28, 0.34, 0.46, 1.0);

/// The value / option text colour.
const TEXT_COLOR: Color = Color::srgb(0.90, 0.92, 0.96);

/// A [disabled](bevy::ui::InteractionDisabled) combo's value / arrow text colour
/// — a muted grey matching the disabled text field, so a combo the user cannot
/// change reads as disabled while its selection stays legible.
const DISABLED_TEXT_COLOR: Color = Color::srgb(0.45, 0.47, 0.52);

/// A disabled combo's background.
const DISABLED_BACKGROUND: Color = Color::srgba(0.12, 0.12, 0.14, 1.0);

/// A disabled combo's border.
const DISABLED_BORDER: Color = Color::srgba(0.28, 0.28, 0.32, 1.0);

/// The dropdown arrow glyph.
const ARROW_GLYPH: &str = "\u{25be}";

/// The skin class for the value / option text (`--text-primary`).
const VALUE_CLASS: &str = "sk-build-value";

/// Everything a combo is built from — a struct so the knobs read at the call
/// site, mirroring [`crate::ui_radio::RadioSpec`].
#[derive(Debug, Clone)]
pub(crate) struct ComboSpec<'labels> {
    /// The element id the combo reports in [`ComboChanged`] and the prefix of its
    /// nodes' [`Name`]s.
    pub(crate) element: &'static str,
    /// The option labels, in order; their count is the number of options.
    pub(crate) labels: &'labels [String],
    /// The initially-selected option, clamped into range.
    pub(crate) active: usize,
    /// The combo's focus stop.
    pub(crate) tab_index: i32,
    /// The label font size, in logical pixels.
    pub(crate) font_size: f32,
    /// Whether [`labels`](Self::labels) are Fluent **keys** to translate (real
    /// UI) rather than literal display text (the gallery / tests).
    pub(crate) translate_labels: bool,
}

impl ComboSpec<'_> {
    /// The clamped active index, so a combo is never spawned with nothing shown.
    fn resolved_active(&self) -> usize {
        self.active.min(self.labels.len().saturating_sub(1))
    }
}

/// A combo's state: which option is selected. The **single source of truth**;
/// the closed value text is derived from it.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub(crate) struct ComboSelection {
    /// The element id this combo reports in [`ComboChanged`].
    pub(crate) element: &'static str,
    /// The index of the selected option.
    pub(crate) active: usize,
}

/// The combo's option labels, held on the anchor so the popover can be built (and
/// the value text resolved) from the one place.
#[derive(Component, Debug, Clone)]
struct ComboOptions {
    /// The option labels (Fluent keys when `translate`).
    labels: Vec<String>,
    /// Whether the labels are Fluent keys.
    translate: bool,
    /// The option font size.
    font_size: f32,
}

/// The anchor button's value-text node, rewritten by [`apply_combo_selection`].
#[derive(Component, Debug, Clone, Copy)]
struct ComboValueText;

/// An open popover, naming the combo it belongs to so it can be found and closed.
#[derive(Component, Debug, Clone, Copy)]
struct ComboPopover {
    /// The anchor combo.
    combo: Entity,
}

/// One popover row, naming its combo and option index.
#[derive(Component, Debug, Clone, Copy)]
struct ComboOption {
    /// The anchor combo.
    combo: Entity,
    /// This row's option index.
    index: usize,
}

/// Ask a combo to replace its option labels in place — for a list that
/// re-enumerates while visible (the preferences audio tab's output-device
/// list). Applied by [`apply_set_combo_options`]: an equal list is a no-op,
/// the closed value text re-resolves, an out-of-range selection clamps, and
/// the update is **skipped while that combo's popover is open** so the rows
/// are never yanked out from under the pointer — the sender's next refresh
/// lands after it closes. The anchor itself is never respawned (the
/// build-once rule); the popover always rebuilds from [`ComboOptions`] on
/// open, so the next open shows the new list.
#[derive(Message, Debug, Clone)]
pub(crate) struct SetComboOptions {
    /// The anchor combo entity.
    pub(crate) combo: Entity,
    /// The new option labels, in order (Fluent keys where the combo
    /// translates; a key no bundle defines renders as itself).
    pub(crate) labels: Vec<String>,
}

/// Emitted when the **user** picks a different option (not on a programmatic
/// [`ComboSelection`] write) — the consumer's signal that a choice was made.
#[derive(Message, Debug, Clone, Copy)]
pub(crate) struct ComboChanged {
    /// The anchor combo entity, so a consumer that hosts several combos tells
    /// them apart (and can read its [`ComboSelection`] for the element id).
    pub(crate) combo: Entity,
    /// The newly-selected option index.
    pub(crate) active: usize,
}

/// The plugin the viewer (and the gallery) adds for the combo widget: the
/// selection reconcile, the [`ComboChanged`] message, and the root dismiss
/// observer. A no-op where nothing matches, so adding it is always safe.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ComboWidgetPlugin;

impl Plugin for ComboWidgetPlugin {
    /// Register the reconcile system, the change message, and the outside-press
    /// dismiss observer.
    fn build(&self, app: &mut App) {
        app.add_message::<ComboChanged>()
            .add_message::<SetComboOptions>()
            .init_resource::<crate::hud_pick::UiPointerClaim>()
            .add_systems(First, crate::hud_pick::reset_ui_pointer_claim)
            .add_systems(
                Update,
                (
                    apply_set_combo_options,
                    apply_combo_selection,
                    reflect_combo_disabled,
                )
                    .chain(),
            )
            .add_systems(
                Startup,
                attach_combo_dismiss.after(UiScaffoldSystems::SpawnRoot),
            );
    }
}

/// Spawn a combo under `parent`: a bordered value button that opens a popover
/// list of options. Returns the anchor button entity, which carries
/// [`ComboSelection`] (the source of truth a consumer reads / writes) and a
/// [`ComboChanged`] on each user pick.
pub(crate) fn spawn_combo(commands: &mut Commands, parent: Entity, spec: &ComboSpec) -> Entity {
    let active = spec.resolved_active();
    let anchor = commands
        .spawn((
            Button,
            TabIndex(spec.tab_index),
            Node {
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                padding: UiRect::axes(Val::Px(8.0), Val::Px(2.0)),
                border: UiRect::all(Val::Px(1.0)),
                min_width: Val::Px(96.0),
                column_gap: Val::Px(8.0),
                ..row(Val::ZERO)
            },
            BorderColor::all(COMBO_BORDER),
            BackgroundColor(COMBO_BACKGROUND),
            ComboSelection {
                element: spec.element,
                active,
            },
            ComboOptions {
                labels: spec.labels.to_vec(),
                translate: spec.translate_labels,
                font_size: spec.font_size,
            },
            Pickable::default(),
            Name::new(format!("{}:combo", spec.element)),
            ChildOf(parent),
        ))
        .observe(toggle_combo_popover)
        .id();

    // The value text; its content is reconciled from the selection.
    let value = commands
        .spawn((
            Text::default(),
            UiFont::Sans.at(spec.font_size),
            TextColor(TEXT_COLOR),
            ClassList::new_with_classes([VALUE_CLASS]),
            ComboValueText,
            Pickable::IGNORE,
            Name::new(format!("{}:combo-value", spec.element)),
            ChildOf(anchor),
        ))
        .id();
    seed_value_text(commands, value, spec, active);

    commands.spawn((
        Text::new(ARROW_GLYPH),
        UiFont::Sans.at(spec.font_size),
        TextColor(TEXT_COLOR),
        Pickable::IGNORE,
        Name::new(format!("{}:combo-arrow", spec.element)),
        ChildOf(anchor),
    ));

    anchor
}

/// Seed a value-text node to the active option — a `Translated` for a translated
/// combo, the literal label otherwise.
fn seed_value_text(commands: &mut Commands, value: Entity, spec: &ComboSpec, active: usize) {
    if let Some(label) = spec.labels.get(active) {
        if spec.translate_labels {
            commands
                .entity(value)
                .insert(Translated::new(label.clone()));
        } else {
            commands.entity(value).insert(Text::new(label.clone()));
        }
    }
}

/// Toggle the combo's popover: close it if open, else open a fresh one. Consumes
/// the press so opening does not immediately trip the root dismiss observer.
fn toggle_combo_popover(
    mut press: On<Pointer<Press>>,
    anchors: Query<&ComboOptions>,
    disabled: Query<(), With<bevy::ui::InteractionDisabled>>,
    popovers: Query<(Entity, &ComboPopover)>,
    mut claim: ResMut<crate::hud_pick::UiPointerClaim>,
    mut commands: Commands,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    // The open/close decision, logged for `viewer-combo-stops-opening` — a combo
    // that stops dropping down has exactly four possible causes, and this line
    // says which: it is disabled, one of its popovers is still open (so the
    // press closes rather than opens), it has no options to list, or none of
    // those and the popover is being built but not seen. Absent the line
    // entirely, the press never reached the combo at all. `debug!`, so it costs
    // nothing until asked for with
    // `RUST_LOG=sl_client_bevy_viewer::ui_combo=debug`.
    tracing::debug!(
        anchor = ?press.entity,
        disabled = disabled.contains(press.entity),
        open_popovers = popovers.iter().count(),
        mine_open = popovers.iter().any(|(_entity, marker)| marker.combo == press.entity),
        options = anchors.get(press.entity).map(|options| options.labels.len()).ok(),
        "combo press"
    );
    // A disabled combo does not open — it consumes the press so the click lands
    // nowhere, but changes nothing (the reference's disabled-control behaviour).
    if disabled.contains(press.entity) {
        press.propagate(false);
        return;
    }
    press.propagate(false);
    claim.claim();
    let anchor = press.entity;
    // Close any popover already open (for this or any other combo).
    let mut had_open = false;
    for (popover, marker) in &popovers {
        if marker.combo == anchor {
            had_open = true;
        }
        commands.entity(popover).despawn();
    }
    if had_open {
        return;
    }
    let Ok(options) = anchors.get(anchor) else {
        return;
    };
    tracing::debug!(rows = options.labels.len(), "building a combo popover");
    build_combo_popover(&mut commands, anchor, options);
}

/// Build the popover list of option rows anchored to `anchor`.
fn build_combo_popover(commands: &mut Commands, anchor: Entity, options: &ComboOptions) {
    let popup = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                padding: UiRect::all(Val::Px(2.0)),
                border: UiRect::all(Val::Px(1.0)),
                min_width: Val::Px(96.0),
                align_items: AlignItems::Start,
                flex_direction: FlexDirection::Column,
                ..Default::default()
            },
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
            },
            BackgroundColor(POPOVER_BACKGROUND),
            BorderColor::all(POPOVER_BORDER),
            GlobalZIndex(COMBO_Z_INDEX),
            // Escape a clipping ancestor (the build floater's content slot) so the
            // popover draws and clicks in full past the floater edge — the menu
            // popups' trick.
            OverrideClip,
            ComboPopover { combo: anchor },
            Pickable::default(),
            Name::new("combo-popover"),
            ChildOf(anchor),
        ))
        .id();
    for (index, label) in options.labels.iter().enumerate() {
        let row_entity = commands
            .spawn((
                Node {
                    width: Val::Percent(100.0),
                    padding: UiRect::axes(Val::Px(8.0), Val::Px(3.0)),
                    ..Default::default()
                },
                BackgroundColor(Color::NONE),
                ComboOption {
                    combo: anchor,
                    index,
                },
                Pickable::default(),
                Name::new(format!("combo-option:{index}")),
                ChildOf(popup),
            ))
            .observe(select_combo_option)
            .observe(hover_combo_option)
            .observe(unhover_combo_option)
            .id();
        let text = commands
            .spawn((
                Text::default(),
                UiFont::Sans.at(options.font_size),
                TextColor(TEXT_COLOR),
                ClassList::new_with_classes([VALUE_CLASS]),
                Pickable::IGNORE,
                ChildOf(row_entity),
            ))
            .id();
        if options.translate {
            commands.entity(text).insert(Translated::new(label.clone()));
        } else {
            commands.entity(text).insert(Text::new(label.clone()));
        }
    }
}

/// Highlight a popover row under the pointer.
fn hover_combo_option(
    over: On<Pointer<Over>>,
    mut rows: Query<&mut BackgroundColor, With<ComboOption>>,
) {
    if let Ok(mut bg) = rows.get_mut(over.entity) {
        bg.0 = ROW_HOVER;
    }
}

/// Clear a popover row's highlight when the pointer leaves.
fn unhover_combo_option(
    out: On<Pointer<Out>>,
    mut rows: Query<&mut BackgroundColor, With<ComboOption>>,
) {
    if let Ok(mut bg) = rows.get_mut(out.entity) {
        bg.0 = Color::NONE;
    }
}

/// Pick a popover option: move the combo's selection, emit [`ComboChanged`], and
/// close the popover. Consumes the press so it does not reach the root dismiss.
fn select_combo_option(
    mut press: On<Pointer<Press>>,
    rows: Query<&ComboOption>,
    mut combos: Query<&mut ComboSelection>,
    popovers: Query<(Entity, &ComboPopover)>,
    mut changed: MessageWriter<ComboChanged>,
    mut claim: ResMut<crate::hud_pick::UiPointerClaim>,
    mut commands: Commands,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    press.propagate(false);
    // Claim this press so the world pick ignores it. The popover despawns below,
    // which would otherwise leave a stale hover-map entry the block test misses,
    // leaking the press to the world as an empty-space click that deselects.
    claim.claim();
    let Ok(option) = rows.get(press.entity) else {
        return;
    };
    if let Ok(mut selection) = combos.get_mut(option.combo)
        && selection.active != option.index
    {
        selection.active = option.index;
        changed.write(ComboChanged {
            combo: option.combo,
            active: option.index,
        });
    }
    for (popover, marker) in &popovers {
        if marker.combo == option.combo {
            commands.entity(popover).despawn();
        }
    }
}

/// Apply [`SetComboOptions`]: replace the anchor's [`ComboOptions`] labels in
/// place (see the message doc for the skip rules), clamping the selection and
/// touching it so [`apply_combo_selection`] re-resolves the closed value text
/// against the new labels the same frame.
fn apply_set_combo_options(
    mut events: MessageReader<SetComboOptions>,
    mut anchors: Query<(&mut ComboOptions, &mut ComboSelection)>,
    popovers: Query<&ComboPopover>,
) {
    for event in events.read() {
        if event.labels.is_empty() || popovers.iter().any(|popover| popover.combo == event.combo) {
            continue;
        }
        let Ok((mut options, mut selection)) = anchors.get_mut(event.combo) else {
            continue;
        };
        if options.labels == event.labels {
            continue;
        }
        options.labels.clone_from(&event.labels);
        // An unconditional write: the deref marks the selection changed even
        // when the clamped index is equal, which is exactly what re-resolves
        // the closed text after the label under the index changed.
        selection.active = selection.active.min(options.labels.len().saturating_sub(1));
    }
}

/// Reconcile each combo's closed value text to its [`ComboSelection`] whenever it
/// changes — from a user pick or an external write. The sole writer of the
/// derived value text.
fn apply_combo_selection(
    changed: Query<(&ComboSelection, &ComboOptions, &Children), Changed<ComboSelection>>,
    mut values: Query<Entity, With<ComboValueText>>,
    mut commands: Commands,
) {
    for (selection, options, children) in &changed {
        let Some(label) = options.labels.get(selection.active) else {
            continue;
        };
        for child in children.iter() {
            if values.get_mut(child).is_ok() {
                if options.translate {
                    commands
                        .entity(child)
                        .insert(Translated::new(label.clone()));
                } else {
                    commands
                        .entity(child)
                        .remove::<Translated>()
                        .insert(Text::new(label.clone()));
                }
            }
        }
    }
}

/// Grey a combo's anchor and value / arrow text while it is
/// [disabled](bevy::ui::InteractionDisabled), and restore them when enabled — so
/// a consumer disables a combo the same way it disables a text field (adding the
/// marker), and the widget reflects it.
fn reflect_combo_disabled(
    mut anchors: Query<
        (
            &Children,
            Has<bevy::ui::InteractionDisabled>,
            &mut BackgroundColor,
            &mut BorderColor,
        ),
        With<ComboSelection>,
    >,
    mut texts: Query<&mut TextColor>,
) {
    for (children, disabled, mut background, mut border) in &mut anchors {
        let (want_bg, want_border, want_text) = if disabled {
            (DISABLED_BACKGROUND, DISABLED_BORDER, DISABLED_TEXT_COLOR)
        } else {
            (COMBO_BACKGROUND, COMBO_BORDER, TEXT_COLOR)
        };
        if background.0 != want_bg {
            background.0 = want_bg;
        }
        let wanted_border = BorderColor::all(want_border);
        if *border != wanted_border {
            *border = wanted_border;
        }
        for child in children.iter() {
            if let Ok(mut color) = texts.get_mut(child) {
                let wanted = TextColor(want_text);
                if *color != wanted {
                    *color = wanted;
                }
            }
        }
    }
}

/// Attach the root dismiss observer: any press that reaches the UI root (i.e.
/// outside every popover, whose rows consume their own press) closes all open
/// combo popovers.
fn attach_combo_dismiss(root: Option<Res<UiRoot>>, mut commands: Commands) {
    if let Some(root) = root {
        commands.entity(root.0).observe(dismiss_combos_on_press);
    }
}

/// Close every open combo popover on an outside press.
fn dismiss_combos_on_press(
    _press: On<Pointer<Press>>,
    popovers: Query<Entity, With<ComboPopover>>,
    mut commands: Commands,
) {
    for popover in &popovers {
        commands.entity(popover).despawn();
    }
}

/// Gallery element: a combo with three literal options, the middle selected.
pub(crate) fn spawn_combo_element(
    commands: &mut Commands,
    parent: Entity,
    cx: crate::ui_element::ElementCx,
) -> Entity {
    let labels: Vec<String> = ["Low", "Medium", "High"]
        .iter()
        .map(|label| cx.text(label))
        .collect();
    spawn_combo(
        commands,
        parent,
        &ComboSpec {
            element: "combo-demo",
            labels: &labels,
            active: 1,
            tab_index: 1,
            font_size: cx.font_size,
            translate_labels: false,
        },
    )
}

#[cfg(test)]
mod tests {
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;

    use super::{ComboSelection, ComboSpec, spawn_combo};

    /// A boxed error so tests avoid `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// The combo entity built by setup.
    #[derive(Resource, Debug, Clone, Copy)]
    struct TestCombo(Entity);

    /// A minimal app that spawns one combo with the given active option.
    fn app(active: usize) -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(Seed(active))
            .add_systems(Startup, setup);
        app.update();
        app
    }

    /// The seed active option.
    #[derive(Resource, Debug, Clone, Copy)]
    struct Seed(usize);

    /// Spawn the combo and publish its handle.
    fn setup(mut commands: Commands, seed: Res<Seed>) {
        let parent = commands.spawn_empty().id();
        let labels = ["Low".to_owned(), "Medium".to_owned(), "High".to_owned()];
        let combo = spawn_combo(
            &mut commands,
            parent,
            &ComboSpec {
                element: "test-combo",
                labels: &labels,
                active: seed.0,
                tab_index: 0,
                font_size: 14.0,
                translate_labels: false,
            },
        );
        commands.insert_resource(TestCombo(combo));
    }

    /// The combo's current selection index.
    fn selection(app: &App) -> usize {
        let combo = app.world().resource::<TestCombo>().0;
        app.world()
            .entity(combo)
            .get::<ComboSelection>()
            .map_or(usize::MAX, |sel| sel.active)
    }

    /// The combo starts on its declared option.
    #[test]
    fn starts_on_the_declared_option() -> Result<(), TestError> {
        let app = app(1);
        assert_eq!(selection(&app), 1);
        Ok(())
    }

    /// An out-of-range active is clamped, so a combo always shows something.
    #[test]
    fn out_of_range_active_is_clamped() -> Result<(), TestError> {
        let app = app(9);
        assert_eq!(selection(&app), 2);
        Ok(())
    }

    /// An app with the in-place options-update system wired.
    fn options_app(active: usize) -> App {
        let mut app = app(active);
        app.add_message::<super::SetComboOptions>()
            .add_systems(Update, super::apply_set_combo_options);
        app
    }

    /// The combo's current option labels.
    fn labels(app: &App) -> Vec<String> {
        let combo = app.world().resource::<TestCombo>().0;
        app.world()
            .entity(combo)
            .get::<super::ComboOptions>()
            .map(|options| options.labels.clone())
            .unwrap_or_default()
    }

    /// Send a [`super::SetComboOptions`] for the test combo.
    fn set_options(app: &mut App, new_labels: &[&str]) {
        let combo = app.world().resource::<TestCombo>().0;
        app.world_mut()
            .resource_mut::<Messages<super::SetComboOptions>>()
            .write(super::SetComboOptions {
                combo,
                labels: new_labels.iter().map(|label| (*label).to_owned()).collect(),
            });
    }

    /// [`super::SetComboOptions`] replaces the labels in place and clamps a
    /// selection the shorter list left dangling.
    #[test]
    fn set_options_replaces_labels_and_clamps_selection() -> Result<(), TestError> {
        let mut app = options_app(2);
        set_options(&mut app, &["Only", "Two"]);
        app.update();
        assert_eq!(labels(&app), vec!["Only".to_owned(), "Two".to_owned()]);
        assert_eq!(selection(&app), 1, "selection clamped to the new tail");
        // An empty list is ignored — a combo never loses all its options.
        set_options(&mut app, &[]);
        app.update();
        assert_eq!(labels(&app).len(), 2);
        Ok(())
    }

    /// The update is skipped while the combo's popover is open, so the rows
    /// are never replaced under the pointer.
    #[test]
    fn set_options_skipped_while_popover_open() -> Result<(), TestError> {
        let mut app = options_app(0);
        let combo = app.world().resource::<TestCombo>().0;
        app.world_mut().spawn(super::ComboPopover { combo });
        set_options(&mut app, &["Other"]);
        app.update();
        assert_eq!(
            labels(&app),
            vec!["Low".to_owned(), "Medium".to_owned(), "High".to_owned()],
            "an open popover defers the update"
        );
        Ok(())
    }
}
