//! The **reusable radio-button widget** (`viewer-ui-radio-widget`): a grouping
//! container holding a set of mutually-exclusive labelled options, exactly one
//! selected, that emits the chosen one.
//!
//! # What it is, and why it is not the tab widget
//!
//! A radio group and a [tab strip](crate::ui_tab) are both single-select strips
//! of [`RadioButton`]s under one [`RadioGroup`], and they share the ARIA
//! pattern: the **group** is the focus stop, arrow keys move the selection
//! within it, and the app owns the [`Checked`] state upstream leaves to it. But
//! they answer different questions and so are separate widgets. A tab strip
//! *switches which panel is shown* — its whole reason to exist is the panels it
//! fronts, and its buttons look like tabs merging into their content. A radio
//! group *sets a value* — a build-tool mode, a small closed preference — with no
//! panel behind it, drawn as the reference's `LLRadioGroup`: a filled-dot
//! indicator beside each label. Forcing one to be the other would bloat both.
//!
//! # The single source of truth
//!
//! [`RadioSelection`] on the group carries the selected index and is the only
//! thing that decides selection. Everything visible is derived from it — the
//! per-item [`Checked`] markers (which the group's own arrow-key handler reads
//! to find the current option) and the `◉` / `○` indicator glyphs — so nothing
//! can drift. [`on_radio_value_change`] (an observer per group, mirroring
//! [`crate::ui_tab`]'s strip) is the only writer of `active` from a click or
//! arrow key, and [`apply_radio_selection`] is the only writer of the derived
//! visuals. Because the reconcile keys off `Changed<RadioSelection>`, a consumer
//! that sets `active` from **outside** — the Build Tools floater syncing its
//! [`EditTool`](crate::edit_tool) — drives the exact same visual path with no
//! second mechanism.
//!
//! # Constructible without wiring
//!
//! Per the registry rule ([`crate::ui_element`]) the widget never reaches a
//! session: it switches its own selection and emits a [`UiAction`] naming that a
//! choice was made (the *which* is readable from [`RadioSelection::active`]). A
//! consumer that must *do* something reacts to `Changed<RadioSelection>` and
//! reads the index — it is not wired into the widget. Two gallery elements (one
//! per [`RadioLayout`]) register it so [`crate::ui_test`] sweeps both layouts.
//!
//! Reference (Firestorm, read-only): `indra/llui/llradiogroup.{h,cpp}`
//! (`LLRadioGroup`, `LLRadioCtrl`).

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::ui::Checked;
use bevy::ui_widgets::{RadioButton, RadioGroup, ValueChange};

use crate::i18n::Translated;
use crate::ui::{column, row};
use crate::ui_element::UiAction;
use crate::ui_font::UiFont;

/// The gap between adjacent options, in logical pixels.
const GROUP_GAP: f32 = 8.0;

/// The gap between an option's indicator and its label, in logical pixels.
const ITEM_GAP: f32 = 6.0;

/// The indicator glyph of the selected option — a ringed dot (`◉`, U+25C9), the
/// reference's filled radio.
const SELECTED_GLYPH: &str = "\u{25c9}";

/// The indicator glyph of an unselected option — an empty ring (`○`, U+25CB).
const UNSELECTED_GLYPH: &str = "\u{25cb}";

/// The selected indicator's colour — a bright accent, the loudest "this one"
/// signal independent of keyboard focus.
const INDICATOR_ON: Color = Color::srgb(0.52, 0.68, 0.95);

/// An unselected indicator's colour — muted, so the filled option reads at a
/// glance.
const INDICATOR_OFF: Color = Color::srgb(0.50, 0.55, 0.63);

/// An option label's colour.
const LABEL_COLOR: Color = Color::srgb(0.90, 0.92, 0.96);

/// The action a group emits when the user picks a different option. A single
/// verb — "a choice was made" — because the *which* is readable directly from
/// [`RadioSelection::active`]; the [`UiAction`] exists so the harness can assert
/// the change without a consumer behind it.
pub(crate) const RADIO_SELECTED_ACTION: &str = "select-radio";

/// Which axis a radio group's options flow along, named by axis rather than by
/// side so the choice is independent of reading direction — see the [module
/// documentation](self).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RadioLayout {
    /// The options flow along the inline (text) axis — a horizontal row that
    /// wraps when it outgrows its space and mirrors under RTL for free.
    Row,
    /// The options stack down the block axis — a vertical column, the reference
    /// viewer's usual radio-group shape.
    Column,
}

impl RadioLayout {
    /// The container node the options flow in: a wrapping [`crate::ui::row`] for
    /// [`Row`](Self::Row), a [`crate::ui::column`] for [`Column`](Self::Column).
    fn container_node(self) -> Node {
        match self {
            Self::Row => Node {
                // Wraps rather than overflowing once the options outgrow the
                // space — the row-level half of the content-driven convention.
                flex_wrap: FlexWrap::Wrap,
                row_gap: Val::Px(GROUP_GAP),
                ..row(Val::Px(GROUP_GAP))
            },
            Self::Column => column(Val::Px(GROUP_GAP)),
        }
    }
}

/// Everything a radio group is built from — a struct rather than a positional
/// call so the knobs read at the call site.
#[derive(Debug, Clone)]
pub(crate) struct RadioSpec<'labels> {
    /// The element id the group reports in its [`UiAction`], and the prefix of
    /// its nodes' [`Name`]s.
    pub(crate) element: &'static str,
    /// The option labels, in order; their count is the number of options.
    pub(crate) labels: &'labels [String],
    /// The initially-selected option, clamped into range.
    pub(crate) active: usize,
    /// The group's single focus stop (the group, not the options) — pick it to
    /// slot the group into the surrounding tab order.
    pub(crate) tab_index: i32,
    /// The labels' font size, in logical pixels.
    pub(crate) font_size: f32,
    /// Which axis the options flow along.
    pub(crate) layout: RadioLayout,
    /// Whether [`labels`](Self::labels) are Fluent **keys** to translate
    /// (re-resolved on locale change / bundle load) rather than literal display
    /// text. Use it for real UI; `false` for the gallery and tests, whose labels
    /// are fixed sample text.
    pub(crate) translate_labels: bool,
}

impl RadioSpec<'_> {
    /// The clamped active index this spec resolves to — an out-of-range value
    /// would leave no option checked, which the arrow handler reads as "start
    /// from the end". `saturating_sub` keeps an empty group at 0 without
    /// underflow.
    fn resolved_active(&self) -> usize {
        self.active.min(self.labels.len().saturating_sub(1))
    }

    /// The text a label node starts with: empty for a translated group (the key
    /// is not display text, and [`Translated`] fills the real text once the
    /// bundle loads), otherwise the literal label.
    fn initial_label(&self, label: &str) -> String {
        if self.translate_labels {
            String::new()
        } else {
            label.to_owned()
        }
    }
}

/// A radio group's state: which option is selected. The **single source of
/// truth** — the [`Checked`] flags and the indicator glyphs are derived from it,
/// so nothing can drift.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RadioSelection {
    /// The element id this group reports in its [`UiAction`].
    pub(crate) element: &'static str,
    /// The index of the selected option, into the group's options in spawn
    /// order.
    pub(crate) active: usize,
}

/// A radio option: which group it belongs to and its index within it. Carried so
/// the selection observer and the reconcile can find every option of a group and
/// place it against the group's [`active`](RadioSelection::active) index.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RadioItem {
    /// The group ([`RadioGroup`]) this option belongs to.
    pub(crate) group: Entity,
    /// This option's index within the group.
    pub(crate) index: usize,
}

/// An option's indicator glyph node, naming its group and index so
/// [`apply_radio_selection`] can swap the glyph and colour when the selection
/// changes.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
struct RadioIndicator {
    /// The group whose selection this indicator reflects.
    group: Entity,
    /// The option index this indicator belongs to.
    index: usize,
}

/// The plugin the viewer (and the gallery) adds for the radio widget's runtime
/// half: reconciling each option's [`Checked`] marker and indicator glyph from
/// the group's [`RadioSelection`] whenever it changes.
///
/// A no-op where it has nothing to act on, so adding it is always safe.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct RadioWidgetPlugin;

impl Plugin for RadioWidgetPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, apply_radio_selection);
    }
}

/// Spawn a radio group under `parent`: a single-select set of labelled options
/// with the filled-dot indicator, keyboard selection, and a [`UiAction`] on
/// change. Returns the [`RadioGroup`] container entity.
///
/// [`RadioSpec::active`] is clamped into range, so a caller cannot spawn a group
/// with nothing selected. The returned group carries [`RadioSelection`], whose
/// `active` is the source of truth: a consumer that only needs the selection
/// reacts to `Changed<RadioSelection>` and reads it. The option entities are not
/// returned — each carries [`RadioItem`], so a consumer that needs one finds it
/// by that component (the tab widget's [`crate::ui_tab::TabContainerHandle`]
/// convention: no handle field without a runtime reader).
pub(crate) fn spawn_radio_group(
    commands: &mut Commands,
    parent: Entity,
    spec: &RadioSpec,
) -> Entity {
    let active = spec.resolved_active();
    let group = commands
        .spawn((
            RadioGroup,
            RadioSelection {
                element: spec.element,
                active,
            },
            spec.layout.container_node(),
            TabIndex(spec.tab_index),
            Name::new(format!("{}:radio-group", spec.element)),
            ChildOf(parent),
        ))
        .observe(on_radio_value_change)
        .id();

    for (index, label) in spec.labels.iter().enumerate() {
        spawn_radio_item(commands, group, spec, index, label, index == active);
    }

    group
}

/// Spawn one option — a [`RadioButton`] styled as a radio row: a `◉` / `○`
/// indicator followed by its label. Not focusable itself; per the ARIA
/// radiogroup pattern the group is the focus stop and the arrows move the
/// selection within it.
fn spawn_radio_item(
    commands: &mut Commands,
    group: Entity,
    spec: &RadioSpec,
    index: usize,
    label: &str,
    active: bool,
) -> Entity {
    let item = commands
        .spawn((
            RadioButton,
            RadioItem { group, index },
            Node {
                align_items: AlignItems::Center,
                ..row(Val::Px(ITEM_GAP))
            },
            Pickable::default(),
            Name::new(format!("{}:radio:{index}", spec.element)),
            ChildOf(group),
        ))
        .id();
    // The initial `Checked` is set here — the group's arrow-key handler reads it
    // to find the current option, and setting it at spawn avoids a one-frame
    // unselected flash before [`apply_radio_selection`] first runs.
    if active {
        commands.entity(item).insert(Checked);
    }

    commands.spawn((
        Text::new(if active {
            SELECTED_GLYPH
        } else {
            UNSELECTED_GLYPH
        }),
        UiFont::Sans.at(spec.font_size),
        TextColor(if active { INDICATOR_ON } else { INDICATOR_OFF }),
        RadioIndicator { group, index },
        // The indicator is part of the option's hit target, not its own; let the
        // click fall through to the `RadioButton`.
        Pickable::IGNORE,
        Name::new(format!("{}:radio-dot:{index}", spec.element)),
        ChildOf(item),
    ));

    let label_entity = commands
        .spawn((
            Text::new(spec.initial_label(label)),
            UiFont::Sans.at(spec.font_size),
            TextColor(LABEL_COLOR),
            Pickable::IGNORE,
            Name::new(format!("{}:radio-label:{index}", spec.element)),
            ChildOf(item),
        ))
        .id();
    if spec.translate_labels {
        commands
            .entity(label_entity)
            .insert(Translated::new(label.to_owned()));
    }

    item
}

/// The group's selection observer: on a [`RadioGroup`] value change — a click or
/// an arrow key — move [`RadioSelection::active`] to the picked option and emit
/// the [`UiAction`]. The visuals are left to [`apply_radio_selection`], which
/// picks up the `Changed<RadioSelection>` this write triggers.
///
/// A no-op selection (the active option re-picked) returns before emitting, so
/// the action means a real change.
fn on_radio_value_change(
    change: On<ValueChange<Entity>>,
    mut groups: Query<&mut RadioSelection>,
    items: Query<&RadioItem>,
    mut actions: MessageWriter<UiAction>,
) {
    let group_id = change.source;
    // The event's value is the newly-picked option; its `RadioItem` names the
    // index to move to. A value that is not one of this group's options
    // (impossible in practice, but the query is fallible) is ignored.
    let Ok(picked) = items.get(change.value).map(|item| item.index) else {
        return;
    };
    let Ok(mut selection) = groups.get_mut(group_id) else {
        return;
    };
    if selection.active == picked {
        return;
    }
    selection.active = picked;
    let element = selection.element;
    actions.write(UiAction {
        element,
        action: RADIO_SELECTED_ACTION,
    });
}

/// Reconcile every option's [`Checked`] marker and indicator glyph/colour to its
/// group's [`RadioSelection`] whenever the selection changes — from a click, an
/// arrow key, or an external write (the Build Tools floater syncing its tool).
///
/// The single writer of the derived visuals, keyed off the one source of truth,
/// so a click and an outside change drive the exact same path. Runs only for
/// groups whose selection actually changed, and guards each write so a settled
/// option does not re-trigger.
fn apply_radio_selection(
    changed: Query<(Entity, &RadioSelection), Changed<RadioSelection>>,
    items: Query<(Entity, &RadioItem)>,
    mut indicators: Query<(&RadioIndicator, &mut Text, &mut TextColor)>,
    mut commands: Commands,
) {
    for (group_id, selection) in &changed {
        for (item_entity, item) in &items {
            if item.group != group_id {
                continue;
            }
            let is_active = item.index == selection.active;
            if is_active {
                commands.entity(item_entity).insert(Checked);
            } else {
                commands.entity(item_entity).remove::<Checked>();
            }
        }
        for (indicator, mut text, mut color) in &mut indicators {
            if indicator.group != group_id {
                continue;
            }
            let is_active = indicator.index == selection.active;
            let wanted_glyph = if is_active {
                SELECTED_GLYPH
            } else {
                UNSELECTED_GLYPH
            };
            if text.0 != wanted_glyph {
                wanted_glyph.clone_into(&mut text.0);
            }
            let wanted_color = if is_active {
                INDICATOR_ON
            } else {
                INDICATOR_OFF
            };
            if color.0 != wanted_color {
                color.0 = wanted_color;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Gallery elements — one per layout, so `crate::ui_test` sweeps both axes across
// every script, direction, scale and font size.
// ---------------------------------------------------------------------------

/// The option labels the gallery elements use — short, so a script swap keeps
/// them label-sized. The Build Tools floater's own tool modes, so the gallery
/// specimen reads as the live control.
const SAMPLE_LABELS: [&str; 3] = ["Move", "Rotate", "Stretch"];

/// Spawn a gallery radio group at `layout`: three options, the middle one
/// selected so a check that assumes index 0 does not pass by luck. The shared
/// body of the two registered elements.
fn spawn_radio_element(
    commands: &mut Commands,
    parent: Entity,
    cx: crate::ui_element::ElementCx,
    layout: RadioLayout,
    element: &'static str,
) -> Entity {
    let labels: Vec<String> = SAMPLE_LABELS.iter().map(|label| cx.text(label)).collect();
    spawn_radio_group(
        commands,
        parent,
        &RadioSpec {
            element,
            labels: &labels,
            active: 1,
            tab_index: 1,
            font_size: cx.font_size,
            layout,
            translate_labels: false,
        },
    )
}

/// Gallery element: a horizontal (inline-axis) radio group.
pub(crate) fn spawn_radio_row(
    commands: &mut Commands,
    parent: Entity,
    cx: crate::ui_element::ElementCx,
) -> Entity {
    spawn_radio_element(commands, parent, cx, RadioLayout::Row, "radio-group-row")
}

/// Gallery element: a vertical (block-axis) radio group.
pub(crate) fn spawn_radio_column(
    commands: &mut Commands,
    parent: Entity,
    cx: crate::ui_element::ElementCx,
) -> Entity {
    spawn_radio_element(
        commands,
        parent,
        cx,
        RadioLayout::Column,
        "radio-group-column",
    )
}

#[cfg(test)]
mod tests {
    use bevy::prelude::*;
    use bevy::ui::Checked;
    use bevy::ui_widgets::ValueChange;
    use pretty_assertions::assert_eq;

    use super::{
        RadioItem, RadioLayout, RadioSelection, RadioSpec, RadioWidgetPlugin, spawn_radio_group,
    };
    use crate::ui_element::UiAction;

    /// A boxed error so tests can use `?` instead of the disallowed
    /// `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// The group entity built by the setup system, published for the test body.
    #[derive(Resource, Debug, Clone, Copy)]
    struct TestRadio(Entity);

    /// Every [`UiAction`] the group has emitted, copied out each frame before the
    /// message buffer is cleared — the same trick [`crate::ui_test`] uses.
    #[derive(Resource, Debug, Default)]
    struct Recorded(Vec<UiAction>);

    /// A headless app that spawns one radio group (three options, the given
    /// `active`) and runs the reconcile system, so a test can drive it by
    /// triggering [`ValueChange`] the way the widget primitives do.
    fn app(active: usize) -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_message::<UiAction>()
            .add_plugins(RadioWidgetPlugin)
            .init_resource::<Recorded>()
            .insert_resource(ActiveSeed(active))
            .add_systems(Startup, setup)
            .add_systems(Update, record_actions);
        app.update();
        app
    }

    /// Copy this frame's [`UiAction`]s into [`Recorded`] before the buffer clears.
    fn record_actions(mut actions: MessageReader<UiAction>, mut recorded: ResMut<Recorded>) {
        recorded.0.extend(actions.read().copied());
    }

    /// The seed for [`setup`]'s initially-selected option.
    #[derive(Resource, Debug, Clone, Copy)]
    struct ActiveSeed(usize);

    /// Spawn the group under a bare parent and publish its handle.
    fn setup(mut commands: Commands, seed: Res<ActiveSeed>) {
        let parent = commands.spawn_empty().id();
        let labels = ["Move".to_owned(), "Rotate".to_owned(), "Stretch".to_owned()];
        let group = spawn_radio_group(
            &mut commands,
            parent,
            &RadioSpec {
                element: "test-radio",
                labels: &labels,
                active: seed.0,
                tab_index: 0,
                font_size: 14.0,
                layout: RadioLayout::Column,
                translate_labels: false,
            },
        );
        commands.insert_resource(TestRadio(group));
    }

    /// The group entity the setup system built.
    fn group_of(app: &App) -> Entity {
        app.world().resource::<TestRadio>().0
    }

    /// Read the group's current selection index.
    fn selection(app: &App, group: Entity) -> usize {
        app.world()
            .entity(group)
            .get::<RadioSelection>()
            .map_or(usize::MAX, |sel| sel.active)
    }

    /// The group's option entity at `index` — the handle no longer returns the
    /// options, so the tests recover one by its [`RadioItem`] component (a `find`
    /// rather than an index, per the no-indexing clippy convention).
    fn item(app: &mut App, group: Entity, index: usize) -> Entity {
        app.world_mut()
            .query::<(Entity, &RadioItem)>()
            .iter(app.world())
            .find(|(_, item)| item.group == group && item.index == index)
            .map_or(Entity::PLACEHOLDER, |(entity, _)| entity)
    }

    /// Whether the group's option at `index` currently carries [`Checked`].
    fn is_checked(app: &mut App, group: Entity, index: usize) -> bool {
        let entity = item(app, group, index);
        app.world().entity(entity).contains::<Checked>()
    }

    /// The group starts on its declared active option, which carries `Checked`
    /// and no other does.
    #[test]
    fn starts_on_the_declared_option() -> Result<(), TestError> {
        let mut app = app(1);
        let group = group_of(&app);
        assert_eq!(selection(&app, group), 1);
        assert!(!is_checked(&mut app, group, 0));
        assert!(is_checked(&mut app, group, 1));
        assert!(!is_checked(&mut app, group, 2));
        Ok(())
    }

    /// An out-of-range active is clamped, so a group is never spawned with
    /// nothing selected.
    #[test]
    fn out_of_range_active_is_clamped() -> Result<(), TestError> {
        let mut app = app(9);
        let group = group_of(&app);
        assert_eq!(selection(&app, group), 2);
        assert!(is_checked(&mut app, group, 2));
        Ok(())
    }

    /// Picking a different option moves the selection, reconciles `Checked` onto
    /// the new option and off the old, and emits a `UiAction`.
    #[test]
    fn picking_moves_selection_and_emits() -> Result<(), TestError> {
        let mut app = app(0);
        let group = group_of(&app);
        let third = item(&mut app, group, 2);
        app.world_mut().trigger(ValueChange {
            source: group,
            value: third,
            is_final: true,
        });
        app.update();
        assert_eq!(selection(&app, group), 2);
        assert!(!is_checked(&mut app, group, 0));
        assert!(is_checked(&mut app, group, 2));

        assert_eq!(
            app.world().resource::<Recorded>().0,
            vec![UiAction {
                element: "test-radio",
                action: super::RADIO_SELECTED_ACTION,
            }]
        );
        Ok(())
    }

    /// Re-picking the already-selected option is a no-op: no action is emitted.
    #[test]
    fn re_picking_the_active_option_emits_nothing() -> Result<(), TestError> {
        let mut app = app(1);
        let group = group_of(&app);
        let second = item(&mut app, group, 1);
        app.world_mut().trigger(ValueChange {
            source: group,
            value: second,
            is_final: true,
        });
        app.update();
        assert!(app.world().resource::<Recorded>().0.is_empty());
        Ok(())
    }

    /// An external write to `RadioSelection` (a consumer syncing its own state)
    /// drives the derived visuals through the same reconcile path.
    #[test]
    fn external_selection_write_reconciles_visuals() -> Result<(), TestError> {
        let mut app = app(0);
        let group = group_of(&app);
        if let Some(mut sel) = app
            .world_mut()
            .entity_mut(group)
            .get_mut::<RadioSelection>()
        {
            sel.active = 2;
        }
        app.update();
        assert!(!is_checked(&mut app, group, 0));
        assert!(is_checked(&mut app, group, 2));
        Ok(())
    }
}
