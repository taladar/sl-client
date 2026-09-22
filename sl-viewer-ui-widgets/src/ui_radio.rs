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
//! to find the current option) and the disc's `:checked` look — so nothing
//! can drift. `on_radio_value_change` (an observer per group, mirroring
//! [`crate::ui_tab`]'s strip) is the only writer of `active` from a click or
//! arrow key, and `apply_radio_selection` is the only writer of the derived
//! visuals. Because the reconcile keys off `Changed<RadioSelection>`, a consumer
//! that sets `active` from **outside** — the Build Tools floater syncing its
//! `EditTool` — drives the exact same visual path with no
//! second mechanism.
//!
//! # Constructible without wiring
//!
//! Per the registry rule (`ui_element`) the widget never reaches a
//! session: it switches its own selection and emits a `UiAction` naming that a
//! choice was made (the *which* is readable from [`RadioSelection::active`]). A
//! consumer that must *do* something reacts to `Changed<RadioSelection>` and
//! reads the index — it is not wired into the widget. Two gallery elements (one
//! per [`RadioLayout`]) register it so `ui_test` sweeps both layouts.
//!
//! Reference (Firestorm, read-only): `indra/llui/llradiogroup.{h,cpp}`
//! (`LLRadioGroup`, `LLRadioCtrl`).

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::text::LineHeight;
use bevy::ui::Checked;
use bevy::ui_widgets::{RadioButton, RadioGroup, ValueChange};
use bevy_flair::style::components::{ClassList, PseudoElementsSupport};

use sl_viewer_ui_core::i18n::Translated;
use sl_viewer_ui_core::skin::TEXT_CLASS;
use sl_viewer_ui_core::skin_palette::SkinPalette;
use sl_viewer_ui_core::ui::{column, row};
use sl_viewer_ui_core::ui_element::UiAction;
use sl_viewer_ui_core::ui_font::UiFont;

/// The gap between adjacent options, in logical pixels.
const GROUP_GAP: f32 = 8.0;

/// The gap between an option's indicator and its label, in logical pixels.
const ITEM_GAP: f32 = 6.0;

/// The indicator's edge length, in logical pixels — the same square the
/// checkbox's box occupies, because the two controls stand beside each other in
/// the same panels and a ring that measured itself would not line up with a box
/// that does not.
///
/// Reserving it is what makes the captions of a column of options share an edge
/// whatever ring the skin draws — and what keeps the layout the same whether a
/// stylesheet is loaded at all, which a specimen measured headlessly otherwise
/// is not.
const INDICATOR_SIZE: f32 = 14.0;

/// The pip's font size, as a fraction of [`INDICATOR_SIZE`] — the checkbox
/// tick's rule, for the same reason: the caption's size says nothing about the
/// disc the mark has to fit in. Smaller than the tick's share, because a pip
/// sits *inside* a ring rather than filling the box.
const INDICATOR_FONT_SCALE: f32 = 0.6;

/// The skin class on a radio group, so a disabled group greys every indicator
/// in it through `.sk-radio-group:disabled .sk-radio-indicator`.
const GROUP_CLASS: &str = "sk-radio-group";

/// The skin class on one option's box. Its lit state is the skin's `:checked`
/// rule over the `Checked` the widget already maintains for the ARIA
/// radiogroup pattern.
const ITEM_CLASS: &str = "sk-radio";

/// The skin class on an option's **disc** — the round box the mark sits in.
/// Its three looks (resting, lit, and greyed for a group the consumer cannot
/// change) are the skin's, selected through the item's `:checked` and the
/// group's `:disabled`: a fill and a ring colour apiece, which is what lets a
/// skin draw the reference's pale-disc-in-a-dark-ring rather than only recolour
/// a character.
const INDICATOR_CLASS: &str = "sk-radio-indicator";

/// The skin class on the **pip** inside the disc: an empty text node whose
/// `::before` carries the lit mark's `content`, the checkbox tick's trick. An
/// unlit option matches no rule, so it shows nothing.
const PIP_CLASS: &str = "sk-radio-pip";

/// The action a group emits when the user picks a different option. A single
/// verb — "a choice was made" — because the *which* is readable directly from
/// [`RadioSelection::active`]; the `UiAction` exists so the harness can assert
/// the change without a consumer behind it.
pub const RADIO_SELECTED_ACTION: &str = "select-radio";

/// Which axis a radio group's options flow along, named by axis rather than by
/// side so the choice is independent of reading direction — see the [module
/// documentation](self).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RadioLayout {
    /// The options flow along the inline (text) axis — a horizontal row that
    /// wraps when it outgrows its space and mirrors under RTL for free.
    Row,
    /// The options stack down the block axis — a vertical column, the reference
    /// viewer's usual radio-group shape.
    Column,
}

impl RadioLayout {
    /// The container node the options flow in: a wrapping `ui::row` for
    /// [`Row`](Self::Row), a `ui::column` for [`Column`](Self::Column).
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
pub struct RadioSpec<'labels> {
    /// The element id the group reports in its `UiAction`, and the prefix of
    /// its nodes' [`Name`]s.
    pub element: &'static str,
    /// The option labels, in order; their count is the number of options.
    pub labels: &'labels [String],
    /// The initially-selected option, clamped into range.
    pub active: usize,
    /// The group's single focus stop (the group, not the options) — pick it to
    /// slot the group into the surrounding tab order.
    pub tab_index: i32,
    /// The labels' font size, in logical pixels.
    pub font_size: f32,
    /// Which axis the options flow along.
    pub layout: RadioLayout,
    /// Whether [`labels`](Self::labels) are Fluent **keys** to translate
    /// (re-resolved on locale change / bundle load) rather than literal display
    /// text. Use it for real UI; `false` for the gallery and tests, whose labels
    /// are fixed sample text.
    pub translate_labels: bool,
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
pub struct RadioSelection {
    /// The element id this group reports in its `UiAction`.
    pub element: &'static str,
    /// The index of the selected option, into the group's options in spawn
    /// order.
    pub active: usize,
}

/// A radio option: which group it belongs to and its index within it. Carried so
/// the selection observer and the reconcile can find every option of a group and
/// place it against the group's [`active`](RadioSelection::active) index.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct RadioItem {
    /// The group ([`RadioGroup`]) this option belongs to.
    pub group: Entity,
    /// This option's index within the group.
    pub index: usize,
}

/// An option's disc node, naming its group and index.
///
/// It no longer carries anything to *write* — the disc, its lit form and its
/// greyed one are all the skin's, reached by `:checked` / `:disabled` from the
/// option and the group. Kept because the gallery and the tests find a disc by
/// it, and because a marker is the cheapest way to say which of a row's
/// children is the indicator.
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
pub struct RadioWidgetPlugin;

impl Plugin for RadioWidgetPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, apply_radio_selection);
    }
}

/// Spawn a radio group under `parent`: a single-select set of labelled options
/// with the filled-dot indicator, keyboard selection, and a `UiAction` on
/// change. Returns the [`RadioGroup`] container entity.
///
/// [`RadioSpec::active`] is clamped into range, so a caller cannot spawn a group
/// with nothing selected. The returned group carries [`RadioSelection`], whose
/// `active` is the source of truth: a consumer that only needs the selection
/// reacts to `Changed<RadioSelection>` and reads it. The option entities are not
/// returned — each carries [`RadioItem`], so a consumer that needs one finds it
/// by that component (the tab widget's [`crate::ui_tab::TabContainerHandle`]
/// convention: no handle field without a runtime reader).
pub fn spawn_radio_group(commands: &mut Commands, parent: Entity, spec: &RadioSpec) -> Entity {
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
            ClassList::new_with_classes([GROUP_CLASS]),
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

/// Spawn one option — a [`RadioButton`] styled as a radio row: a disc (filled
/// with the skin's pip when lit) followed by its label. Not focusable itself;
/// per the ARIA radiogroup pattern the group is the focus stop and the arrows
/// move the selection within it.
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
            ClassList::new_with_classes([ITEM_CLASS]),
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
    // unselected flash before `apply_radio_selection` first runs.
    if active {
        commands.entity(item).insert(Checked);
    }

    // **The disc is a box, not a character.** The reference draws a radio as a
    // pale disc inside a dark ring — two colours — and a glyph can only ever
    // carry one, so the ring is a round `Node` the skin fills and outlines
    // (`--radio-bg` / `--radio-border`, and the `:checked` pair) exactly as the
    // checkbox's square is. The mark inside it stays a `content` glyph, so a
    // skin that wants a filled dot, a square or a smaller pip still says so in
    // one rule.
    let disc = commands
        .spawn((
            Node {
                width: Val::Px(INDICATOR_SIZE),
                height: Val::Px(INDICATOR_SIZE),
                flex_shrink: 0.0,
                border: UiRect::all(Val::Px(1.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            ClassList::new_with_classes([INDICATOR_CLASS]),
            RadioIndicator { group, index },
            // The disc is part of the option's hit target, not its own; let the
            // click fall through to the `RadioButton`.
            Pickable::IGNORE,
            Name::new(format!("{}:radio-dot:{index}", spec.element)),
            ChildOf(item),
        ))
        .id();
    commands.spawn((
        // It names no mark: the lit pip is `.sk-radio:checked .sk-radio-pip`'s
        // `content`, so the skin owns it. An unlit option has no rule, so its
        // span stays empty.
        //
        // The zero-width space is the same measurement fix as the checkbox
        // tick's, and `ui_checkbox::spawn_checkbox` explains it: `bevy_text`
        // styles each span by *range* and skips empty ones, so a text node with
        // no characters is laid out at parley's defaults — a 20 px line
        // whatever font it asked for, which inside this disc is overflow.
        Text::new("\u{200b}"),
        PseudoElementsSupport,
        UiFont::Sans.at(INDICATOR_SIZE * INDICATOR_FONT_SCALE),
        // The line box is the disc, so the pip cannot be taller than the ring
        // it sits in whatever font size a skin gives it.
        LineHeight::Px(INDICATOR_SIZE),
        TextLayout::justify(Justify::Center),
        ClassList::new_with_classes([PIP_CLASS]),
        Pickable::IGNORE,
        ChildOf(disc),
    ));

    let label_entity = commands
        .spawn((
            Text::new(spec.initial_label(label)),
            UiFont::Sans.at(spec.font_size),
            TextColor(SkinPalette::default().text_primary),
            ClassList::new_with_classes([TEXT_CLASS]),
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
/// the `UiAction`. The visuals are left to `apply_radio_selection`, which
/// picks up the `Changed<RadioSelection>` this write triggers.
///
/// A no-op selection (the active option re-picked) returns before emitting, so
/// the action means a real change.
fn on_radio_value_change(
    change: On<ValueChange<Entity>>,
    mut groups: Query<&mut RadioSelection>,
    items: Query<&RadioItem>,
    disabled: Query<(), With<bevy::ui::InteractionDisabled>>,
    mut actions: MessageWriter<UiAction>,
) {
    let group_id = change.source;
    // A disabled group ignores selection changes (scroll and read-out stay live;
    // only the pick is inert), the interaction half of the disabled state.
    if disabled.contains(group_id) {
        return;
    }
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
    }
}

// ---------------------------------------------------------------------------
// Gallery elements — one per layout, so `ui_test` sweeps both axes across
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
    cx: sl_viewer_ui_core::ui_element::ElementCx,
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
pub fn spawn_radio_row(
    commands: &mut Commands,
    parent: Entity,
    cx: sl_viewer_ui_core::ui_element::ElementCx,
) -> Entity {
    spawn_radio_element(commands, parent, cx, RadioLayout::Row, "radio-group-row")
}

/// Gallery element: a vertical (block-axis) radio group.
pub fn spawn_radio_column(
    commands: &mut Commands,
    parent: Entity,
    cx: sl_viewer_ui_core::ui_element::ElementCx,
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
    use sl_viewer_testkit::{LayoutTest, overflow_violations, settle, spawn_under_root};
    use sl_viewer_ui_core::ui_element::UiAction;

    /// A boxed error so tests can use `?` instead of the disallowed
    /// `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// **The pip fits the disc it sits in.**
    ///
    /// A text node is as tall as its line, mark or no mark, so a pip laid out
    /// at parley's defaults would be taller than the ring around it — and taffy
    /// folds a child's content into every ancestor, so those pixels would
    /// surface as the option's row, the group, and whatever panel holds it
    /// overflowing. Pinned here as well as in `ui_checkbox`, because the two
    /// controls reached the same shape by different routes.
    #[test]
    fn the_pip_fits_the_disc_it_sits_in() -> Result<(), TestError> {
        let mut app = LayoutTest::new().build();
        let row = spawn_under_root(&mut app, (Node::default(), Name::new("row")));
        let labels: Vec<String> = ["Move", "Rotate", "Stretch"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        let mut queue = bevy::ecs::world::CommandQueue::default();
        let mut commands = Commands::new(&mut queue, app.world());
        spawn_radio_group(
            &mut commands,
            row,
            &RadioSpec {
                element: "demo",
                labels: &labels,
                active: 1,
                tab_index: 0,
                font_size: 13.0,
                layout: RadioLayout::Column,
                translate_labels: false,
            },
        );
        queue.apply(app.world_mut());
        settle(&mut app);
        assert_eq!(
            overflow_violations(&mut app),
            Vec::<String>::new(),
            "a radio group spills out of its own boxes"
        );
        Ok(())
    }

    /// The group entity built by the setup system, published for the test body.
    #[derive(Resource, Debug, Clone, Copy)]
    struct TestRadio(Entity);

    /// Every `UiAction` the group has emitted, copied out each frame before the
    /// message buffer is cleared — the same trick `ui_test` uses.
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

    /// Copy this frame's `UiAction`s into [`Recorded`] before the buffer clears.
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
