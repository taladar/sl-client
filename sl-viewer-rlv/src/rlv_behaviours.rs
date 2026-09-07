//! The **Restrictions** floater (`rlv_behaviours`): everything RLV currently
//! holds over this viewer, and who put it there.
//!
//! Three lists behind three tabs, which is how the reference arranges them
//! because they answer three different questions:
//!
//! - **Restrictions** — what is blocked, and which object blocked it. One row
//!   per held command, the `keyword[:option]` text as the object spelled it, so
//!   a `_sec` suffix and an unrecognised option are visible rather than
//!   normalised away.
//! - **Exceptions** — the holes poked in those blocks. `@sendim:<uuid>=add` is
//!   not a restriction and does not belong in the same list as one; the
//!   reference splits it out for exactly the eleven behaviours whose options
//!   mean "except", and so does this.
//! - **Modifiers** — the typed slots (`@fartouch` reach, the camera limits, the
//!   IM distances) with the value in force and which object is primary for it.
//!   A slot with no value shows its default, marked as such.
//!
//! # What the issuer column shows
//!
//! The object's key, and — where the state machine has been told — the
//! attachment point it hangs from. Resolving that key to the *item name* the
//! reference shows needs a lookup this viewer does not have yet (an object's
//! `ObjectProperties` are only kept for the selection), and the reference falls
//! back to printing the key for the same reason whenever the object is out of
//! range. Printing the key is therefore not a placeholder; it is the answer the
//! reference gives when it cannot do better, and the attachment point beside it
//! is more than the reference offers in that case.
//!
//! Reference (Firestorm, read-only): `rlvfloaters.cpp`
//! (`RlvFloaterBehaviours`), `floater_rlv_behaviours.xml`.

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use sl_client_bevy::Uuid;
use sl_rlv::{
    RlvBehaviour, RlvException, RlvExceptionOption, RlvHeldCommand, RlvParamKind, RlvState,
};
use sl_viewer_platform::clipboard::{ViewerClipboard, copy_to_clipboard};
use sl_viewer_ui_core::i18n::{TransArgs, Translated, Translator};
use sl_viewer_ui_core::ui::{UiRoot, UiScaffoldSystems, column, row};
use sl_viewer_ui_core::ui_font::UiFont;
use sl_viewer_ui_core::virtual_list::{
    VirtualList, VirtualRow, layout_virtual_lists, spawn_virtual_scrollbar,
};
use sl_viewer_ui_widgets::floater::{
    DeferredFloaterContent, FloaterCaps, FloaterHandle, FloaterSpec, floater_shown, spawn_floater,
};
use sl_viewer_ui_widgets::ui_tab::{
    TabPlacement, TabSpec, fill_tab_container, spawn_tab_container,
};
use sl_viewer_ui_widgets::ui_table::{
    TableAlign, TableColumn, TableColumnKind, TableColumnWidth, TableRowCells, TableSelectionMode,
    TableSpec, set_table_cell, spawn_table, spawn_table_row,
};
use sl_viewer_world_api::rlv::RlvSession;

use crate::style::{
    ACTION_BACKGROUND, DIM_LABEL_COLOR, FONT_SIZE, LABEL_COLOR, LIST_BACKGROUND, ROW_HEIGHT,
};

/// The floater's stable id (persistence, the menu's check mark,
/// `SL_VIEWER_OPEN_FLOATER`).
pub const BEHAVIOURS_FLOATER_ID: &str = "rlv-behaviours";

/// The Restrictions tab's table: the command text beside the object holding it.
static RESTRICTION_TABLE: TableSpec = TableSpec {
    element: "rlv-restrictions",
    selection: TableSelectionMode::None,
    columns: &[
        TableColumn {
            header_key: "rlv-col-behaviour",
            token: "behaviour",
            kind: TableColumnKind::Text,
            width: TableColumnWidth::Flex(1.0),
            align: TableAlign::Start,
            sortable: false,
        },
        TableColumn {
            header_key: "rlv-col-issuer",
            token: "issuer",
            kind: TableColumnKind::Text,
            width: TableColumnWidth::Flex(1.0),
            align: TableAlign::Start,
            sortable: false,
        },
    ],
    // The natural order — object, then the order each object issued its
    // commands in — is the one that reads: a collar's restrictions stay
    // together and in the order it set them.
    default_sort: &[],
    builtin_sort: false,
    row_height: ROW_HEIGHT,
    font_size: FONT_SIZE,
    header_color: DIM_LABEL_COLOR,
    cell_color: LABEL_COLOR,
    column_gap: 4.0,
    row_padding: 4.0,
    sort_setting: None,
    widths_setting: None,
};

/// The Exceptions tab's table.
static EXCEPTION_TABLE: TableSpec = TableSpec {
    element: "rlv-exceptions",
    selection: TableSelectionMode::None,
    columns: &[
        TableColumn {
            header_key: "rlv-col-behaviour",
            token: "behaviour",
            kind: TableColumnKind::Text,
            width: TableColumnWidth::Fixed { default: 140.0 },
            align: TableAlign::Start,
            sortable: false,
        },
        TableColumn {
            header_key: "rlv-col-option",
            token: "option",
            kind: TableColumnKind::Text,
            width: TableColumnWidth::Flex(1.0),
            align: TableAlign::Start,
            sortable: false,
        },
        TableColumn {
            header_key: "rlv-col-issuer",
            token: "issuer",
            kind: TableColumnKind::Text,
            width: TableColumnWidth::Flex(1.0),
            align: TableAlign::Start,
            sortable: false,
        },
    ],
    default_sort: &[],
    builtin_sort: false,
    row_height: ROW_HEIGHT,
    font_size: FONT_SIZE,
    header_color: DIM_LABEL_COLOR,
    cell_color: LABEL_COLOR,
    column_gap: 4.0,
    row_padding: 4.0,
    sort_setting: None,
    widths_setting: None,
};

/// The Modifiers tab's table.
static MODIFIER_TABLE: TableSpec = TableSpec {
    element: "rlv-modifiers",
    selection: TableSelectionMode::None,
    columns: &[
        TableColumn {
            header_key: "rlv-col-modifier",
            token: "modifier",
            kind: TableColumnKind::Text,
            width: TableColumnWidth::Flex(1.0),
            align: TableAlign::Start,
            sortable: false,
        },
        TableColumn {
            header_key: "rlv-col-value",
            token: "value",
            kind: TableColumnKind::Text,
            width: TableColumnWidth::Fixed { default: 140.0 },
            align: TableAlign::Start,
            sortable: false,
        },
        TableColumn {
            header_key: "rlv-col-primary",
            token: "primary",
            kind: TableColumnKind::Text,
            width: TableColumnWidth::Flex(1.0),
            align: TableAlign::Start,
            sortable: false,
        },
    ],
    default_sort: &[],
    builtin_sort: false,
    row_height: ROW_HEIGHT,
    font_size: FONT_SIZE,
    header_color: DIM_LABEL_COLOR,
    cell_color: LABEL_COLOR,
    column_gap: 4.0,
    row_padding: 4.0,
    sort_setting: None,
    widths_setting: None,
};

// --- Pure view model ------------------------------------------------------

/// The behaviours whose *option* means "except this one" rather than "restrict
/// this one" — the eleven the reference lists under its Exceptions tab
/// (`rlvGetShowException`, `rlvfloaters.cpp:102`).
///
/// It is a property of the behaviour, not of the command, which is why the
/// reference asks the behaviour and so does this: `@sendim:<uuid>=n` grants an
/// exception, while `@sendim=n` restricts, and both are held commands.
#[must_use]
pub const fn is_exception_behaviour(behaviour: RlvBehaviour) -> bool {
    matches!(
        behaviour,
        RlvBehaviour::Recvchat
            | RlvBehaviour::Recvemote
            | RlvBehaviour::Sendim
            | RlvBehaviour::Recvim
            | RlvBehaviour::Startim
            | RlvBehaviour::Tplure
            | RlvBehaviour::Tprequest
            | RlvBehaviour::Accepttp
            | RlvBehaviour::Accepttprequest
            | RlvBehaviour::Shownames
            | RlvBehaviour::Shownametags
    )
}

/// A behaviour's canonical restriction keyword — what the Exceptions tab and
/// the derived-exception rows name it by.
///
/// The dictionary keys a keyword on `(behaviour, param kind)`, so the name is
/// asked for as a restriction: `@sendim`, never the `=force` or query spelling
/// of a behaviour that also has one. A behaviour with no restriction spelling
/// (only the strictness marker `Permissive` is one) falls back to its own
/// debug name, which is still a name and never blank.
#[must_use]
pub fn behaviour_name(behaviour: RlvBehaviour) -> String {
    behaviour
        .canonical_keyword(RlvParamKind::AddRem)
        .map_or_else(|| format!("{behaviour:?}").to_lowercase(), str::to_owned)
}

/// How an object is named in the issuer column: its key, with the attachment
/// point beside it where the state machine has been told one.
#[must_use]
pub fn issuer_label(state: &RlvState, object: Uuid) -> String {
    match state.object_attachment(object) {
        Some(attachment) => format!("{object} ({})", attachment.point.name()),
        None => object.to_string(),
    }
}

/// What an exception lets through, as one line.
#[must_use]
pub fn exception_option_label(option: RlvExceptionOption) -> String {
    match option {
        RlvExceptionOption::Avatar(id) => id.to_string(),
        RlvExceptionOption::Channel(channel) => channel.to_string(),
        RlvExceptionOption::Behaviour(behaviour) => behaviour_name(behaviour),
        // `#[non_exhaustive]`: a kind of exception added to the engine later
        // shows as something rather than failing to compile a window that has
        // nothing to say about it.
        _ => "(unknown)".to_owned(),
    }
}

/// One row of the Restrictions tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestrictionRow {
    /// The `keyword[:option]` text as the object spelled it.
    pub text: String,
    /// The object holding it.
    pub issuer: String,
}

/// One row of the Exceptions tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExceptionRow {
    /// The behaviour the exception applies to.
    pub behaviour: String,
    /// What it lets through.
    pub option: String,
    /// The object that granted it.
    pub issuer: String,
}

/// One row of the Modifiers tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModifierRow {
    /// The slot's name.
    pub name: String,
    /// Its value in force, or the default marked as such.
    pub value: String,
    /// The object that is primary for it, or empty when none is.
    pub primary: String,
}

/// Project the state machine onto the three tabs' rows.
///
/// One pass over the held commands fills the first two lists, because a command
/// is in exactly one of them: the reference decides by asking whether the
/// behaviour is one of the eleven *and* whether the command carried an option,
/// and a command that carried none is a restriction whatever its behaviour is.
#[must_use]
pub fn project(state: &RlvState) -> (Vec<RestrictionRow>, Vec<ExceptionRow>, Vec<ModifierRow>) {
    let mut restrictions = Vec::new();
    let mut exceptions = Vec::new();
    for object in state.restricting_objects() {
        let issuer = issuer_label(state, object);
        for held in state.restrictions_of(object) {
            if held_is_exception(held) {
                exceptions.push(ExceptionRow {
                    behaviour: behaviour_name(held.behaviour),
                    option: held.option.clone().unwrap_or_default(),
                    issuer: issuer.clone(),
                });
            } else {
                restrictions.push(RestrictionRow {
                    text: held.as_string(),
                    issuer: issuer.clone(),
                });
            }
        }
    }
    // The state machine also records exceptions it derived rather than held —
    // the `_sec` marks against `Permissive`, and the channels `@sendchannel`
    // left open. They are listed too, so the tab is the whole exception set and
    // not just the part that happens to be a held command.
    for exception in state.exceptions() {
        if let Some(row) = derived_exception_row(state, exception) {
            exceptions.push(row);
        }
    }

    let modifiers = state
        .modifiers()
        .active()
        .map(|(modifier, value)| ModifierRow {
            name: modifier.name().to_owned(),
            value: format!("{value:?}"),
            primary: state
                .modifiers()
                .primary_object(modifier)
                .map(|object| issuer_label(state, object))
                .unwrap_or_default(),
        })
        .collect();

    (restrictions, exceptions, modifiers)
}

/// Whether this held command belongs under the Exceptions tab: one of the
/// eleven behaviours, *with* an option. Without one it restricts.
const fn held_is_exception(held: &RlvHeldCommand) -> bool {
    held.option.is_some() && is_exception_behaviour(held.behaviour)
}

/// The Exceptions row for an exception the state machine derived rather than
/// held as a command, or `None` when a held command already covers it (so the
/// same `@sendim:<uuid>` is not listed twice).
fn derived_exception_row(state: &RlvState, exception: &RlvException) -> Option<ExceptionRow> {
    if is_exception_behaviour(exception.behaviour) {
        // A held command produced this one, and the loop above already listed
        // it with the spelling the object used.
        return None;
    }
    Some(ExceptionRow {
        behaviour: behaviour_name(exception.behaviour),
        option: exception_option_label(exception.option),
        issuer: issuer_label(state, exception.object),
    })
}

/// The whole restriction set as the block of text the Copy button offers —
/// the reference's `getFormattedBehaviourString`: one paragraph per object,
/// each of its commands on its own indented line.
#[must_use]
pub fn formatted_restrictions(state: &RlvState) -> String {
    let mut text = String::new();
    for object in state.restricting_objects() {
        text.push('\n');
        text.push_str(&issuer_label(state, object));
        text.push_str(":\n");
        for held in state.restrictions_of(object) {
            text.push_str("  -> ");
            text.push_str(&held.as_string());
            text.push('\n');
        }
    }
    text
}

// --- Resources ------------------------------------------------------------

/// The floater's live view state: the three projections and the state revision
/// they were built from.
#[derive(Resource, Debug, Default)]
struct BehavioursView {
    /// The Restrictions tab's rows.
    restrictions: Vec<RestrictionRow>,
    /// The Exceptions tab's rows.
    exceptions: Vec<ExceptionRow>,
    /// The Modifiers tab's rows.
    modifiers: Vec<ModifierRow>,
    /// The [`RlvSession`] revision the three were projected at.
    built_revision: u64,
    /// Whether anything has been projected yet — a fresh session is at revision
    /// zero, which is also the initial value of `built_revision`.
    built: bool,
}

/// The floater's retained entities, published by the first-open build.
#[derive(Resource, Debug)]
struct BehavioursUi {
    /// The Restrictions table's virtualized viewport.
    restriction_viewport: Entity,
    /// The Exceptions table's viewport.
    exception_viewport: Entity,
    /// The Modifiers table's viewport.
    modifier_viewport: Entity,
    /// The summary line under the tabs.
    count_text: Entity,
}

/// Which list a pooled row belongs to, so one bind system can serve all three.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum BehavioursList {
    /// A Restrictions row.
    Restrictions,
    /// An Exceptions row.
    Exceptions,
    /// A Modifiers row.
    Modifiers,
}

impl BehavioursList {
    /// The table spec rows of this list are built from.
    const fn spec(self) -> &'static TableSpec {
        match self {
            Self::Restrictions => &RESTRICTION_TABLE,
            Self::Exceptions => &EXCEPTION_TABLE,
            Self::Modifiers => &MODIFIER_TABLE,
        }
    }
}

// --- Plugin ---------------------------------------------------------------

/// Registers the Restrictions floater and the systems that keep it current.
#[derive(Debug, Clone, Copy, Default)]
pub struct RlvBehavioursPlugin;

impl Plugin for RlvBehavioursPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BehavioursView>()
            .add_systems(
                Startup,
                spawn_behaviours_floater.after(UiScaffoldSystems::SpawnRoot),
            )
            .add_systems(
                Update,
                rebuild_behaviours_view
                    .before(layout_virtual_lists)
                    .run_if(floater_shown(BEHAVIOURS_FLOATER_ID)),
            )
            .add_systems(
                Update,
                (populate_behaviours_rows, bind_behaviours_rows)
                    .chain()
                    .after(layout_virtual_lists)
                    .run_if(floater_shown(BEHAVIOURS_FLOATER_ID)),
            );
    }
}

// --- Floater --------------------------------------------------------------

/// The Restrictions floater's [`FloaterSpec`].
#[must_use]
pub fn rlv_behaviours_floater_spec() -> FloaterSpec {
    FloaterSpec {
        id: BEHAVIOURS_FLOATER_ID,
        title: "RLVa Restrictions".to_owned(),
        position: Vec2::new(320.0, 150.0),
        default_size: Some(Vec2::new(640.0, 360.0)),
        min_size: Some(Vec2::new(420.0, 220.0)),
        dock_host: None,
        caps: FloaterCaps {
            resizable: true,
            minimizable: false,
            closable: true,
            dockable: false,
        },
    }
}

/// Startup: spawn the chrome; the content builds on first open.
fn spawn_behaviours_floater(mut commands: Commands, root: Res<UiRoot>) {
    let handle = spawn_floater(&mut commands, root.0, rlv_behaviours_floater_spec());
    commands
        .entity(handle.title_text)
        .insert(Translated::new("rlv-behaviours-title"));
    let builder = commands.register_system(build_behaviours_content);
    commands
        .entity(handle.root)
        .insert(DeferredFloaterContent { builder, handle });
}

/// First-open content build: the three tabs, their tables, the summary line and
/// the Copy button.
fn build_behaviours_content(In(handle): In<FloaterHandle>, mut commands: Commands) {
    let content = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                min_height: Val::Px(0.0),
                ..column(Val::Px(4.0))
            },
            Name::new("rlv-behaviours-content"),
            ChildOf(handle.content),
        ))
        .id();

    let labels = [
        "rlv-behaviours-tab-restrictions".to_owned(),
        "rlv-behaviours-tab-exceptions".to_owned(),
        "rlv-behaviours-tab-modifiers".to_owned(),
    ];
    let tabs = spawn_tab_container(
        &mut commands,
        content,
        &TabSpec {
            element: "rlv-behaviours",
            placement: TabPlacement::BlockStart,
            labels: &labels,
            active: 0,
            tab_index: 0,
            font_size: FONT_SIZE,
            strip_width: None,
            ellipsis: sl_viewer_ui_widgets::ui_tab::DEFAULT_ELLIPSIS,
            translate_labels: true,
        },
    );
    fill_tab_container(&mut commands, TabPlacement::BlockStart, &tabs);

    let mut viewports = Vec::with_capacity(3);
    for (index, list) in [
        BehavioursList::Restrictions,
        BehavioursList::Exceptions,
        BehavioursList::Modifiers,
    ]
    .into_iter()
    .enumerate()
    {
        let Some(panel) = tabs.panels.get(index).copied() else {
            continue;
        };
        let table = spawn_table(&mut commands, panel, list.spec());
        commands.entity(table.viewport).insert((
            BackgroundColor(LIST_BACKGROUND),
            TabIndex(1),
            list,
        ));
        spawn_virtual_scrollbar(&mut commands, table.viewport);
        viewports.push(table.viewport);
    }

    let footer = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                ..row(Val::Px(6.0))
            },
            Name::new("rlv-behaviours-footer"),
            ChildOf(content),
        ))
        .id();
    let count_text = commands
        .spawn((
            Text::default(),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(DIM_LABEL_COLOR),
            Node {
                flex_shrink: 1.0,
                padding: UiRect::axes(Val::Px(4.0), Val::Px(2.0)),
                ..default()
            },
            Pickable::IGNORE,
            Name::new("rlv-behaviours-count"),
            ChildOf(footer),
        ))
        .id();
    spawn_copy_button(&mut commands, footer);

    // Three viewports were just spawned in that order; anything else is a
    // spawn that failed, and the floater is better empty than half-bound.
    if let [restriction_viewport, exception_viewport, modifier_viewport] = viewports[..] {
        commands.insert_resource(BehavioursUi {
            restriction_viewport,
            exception_viewport,
            modifier_viewport,
            count_text,
        });
    }
}

/// The Copy button and its press observer: the whole restriction set onto the
/// OS clipboard, as the reference's `copy_btn` offers it.
fn spawn_copy_button(commands: &mut Commands, parent: Entity) {
    commands
        .spawn((
            Node {
                flex_shrink: 0.0,
                padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(ACTION_BACKGROUND),
            Pickable {
                should_block_lower: true,
                is_hoverable: true,
            },
            Name::new("rlv-behaviours-copy"),
            ChildOf(parent),
        ))
        .with_child((
            Text::new(String::new()),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(LABEL_COLOR),
            Translated::new("rlv-behaviours-copy"),
            Pickable::IGNORE,
        ))
        .observe(
            move |mut press: On<Pointer<Press>>,
                  session: Res<RlvSession>,
                  clipboard: Res<ViewerClipboard>| {
                press.propagate(false);
                if press.button != PointerButton::Primary {
                    return;
                }
                copy_to_clipboard(&clipboard, &formatted_restrictions(session.state()));
            },
        );
}

// --- View systems ---------------------------------------------------------

/// Reproject the three lists when the held set changed, and keep each virtual
/// list's item count and the summary line in step.
fn rebuild_behaviours_view(
    session: Res<RlvSession>,
    mut view: ResMut<BehavioursView>,
    ui: Option<Res<BehavioursUi>>,
    translator: Translator,
    mut lists: Query<&mut VirtualList>,
    mut texts: Query<&mut Text>,
) {
    let Some(ui) = ui else {
        return;
    };
    if view.built && view.built_revision == session.revision() {
        return;
    }
    view.built = true;
    view.built_revision = session.revision();
    let (restrictions, exceptions, modifiers) = project(session.state());
    view.restrictions = restrictions;
    view.exceptions = exceptions;
    view.modifiers = modifiers;

    for (viewport, count) in [
        (ui.restriction_viewport, view.restrictions.len()),
        (ui.exception_viewport, view.exceptions.len()),
        (ui.modifier_viewport, view.modifiers.len()),
    ] {
        if let Ok(mut list) = lists.get_mut(viewport) {
            list.item_count = count;
        }
    }

    let label = translator.format(
        "rlv-behaviours-count",
        &TransArgs::new()
            .int(
                "restrictions",
                i64::try_from(view.restrictions.len()).unwrap_or(i64::MAX),
            )
            .int(
                "exceptions",
                i64::try_from(view.exceptions.len()).unwrap_or(i64::MAX),
            )
            .int(
                "objects",
                i64::try_from(session.state().restricting_objects().count()).unwrap_or(i64::MAX),
            ),
    );
    if let Ok(mut text) = texts.get_mut(ui.count_text)
        && text.0 != label
    {
        text.0 = label;
    }
}

/// Build the cells of each freshly-pooled row, in whichever of the three lists
/// it was pooled under.
fn populate_behaviours_rows(
    mut commands: Commands,
    lists: Query<&BehavioursList>,
    new_rows: Query<(Entity, &ChildOf), Added<VirtualRow>>,
) {
    for (row_entity, child_of) in &new_rows {
        let Ok(list) = lists.get(child_of.parent()) else {
            continue;
        };
        spawn_table_row(&mut commands, row_entity, child_of.parent(), list.spec());
    }
}

/// Bind each pooled row to the entry it now presents.
fn bind_behaviours_rows(
    view: Res<BehavioursView>,
    lists: Query<&BehavioursList>,
    mut rows: Query<(Ref<VirtualRow>, &ChildOf, &TableRowCells)>,
    mut texts: Query<(&mut Text, &mut TextColor)>,
) {
    let refresh_all = view.is_changed();
    for (row, child_of, cells) in &mut rows {
        let Ok(list) = lists.get(child_of.parent()) else {
            continue;
        };
        if !refresh_all && !row.is_changed() {
            continue;
        }
        let values = row
            .index
            .and_then(|index| row_cells(&view, *list, index))
            .unwrap_or_default();
        for column in 0..list.spec().columns.len() {
            let Some(cell) = cells.cell(column) else {
                continue;
            };
            let (value, color) = values
                .get(column)
                .map_or((String::new(), LABEL_COLOR), |(value, color)| {
                    (value.clone(), *color)
                });
            set_table_cell(&mut texts, cell, &value, color);
        }
    }
}

/// The cell texts and colours of one row of `list`, in column order, or `None`
/// past the end.
///
/// Built in column order rather than by index so a cell can only land in the
/// wrong column by being written in the wrong place in the source — visible
/// beside the table spec it mirrors, where an index constant naming the wrong
/// number would not be. `a_row_fills_every_column_of_its_table` holds the
/// counts to the specs.
fn row_cells(
    view: &BehavioursView,
    list: BehavioursList,
    index: usize,
) -> Option<Vec<(String, Color)>> {
    match list {
        BehavioursList::Restrictions => view.restrictions.get(index).map(|entry| {
            vec![
                (entry.text.clone(), LABEL_COLOR),
                (entry.issuer.clone(), DIM_LABEL_COLOR),
            ]
        }),
        BehavioursList::Exceptions => view.exceptions.get(index).map(|entry| {
            vec![
                (entry.behaviour.clone(), LABEL_COLOR),
                (entry.option.clone(), LABEL_COLOR),
                (entry.issuer.clone(), DIM_LABEL_COLOR),
            ]
        }),
        BehavioursList::Modifiers => view.modifiers.get(index).map(|entry| {
            vec![
                (entry.name.clone(), LABEL_COLOR),
                (entry.value.clone(), LABEL_COLOR),
                (entry.primary.clone(), DIM_LABEL_COLOR),
            ]
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BehavioursList, BehavioursView, behaviour_name, exception_option_label,
        formatted_restrictions, is_exception_behaviour, issuer_label, project, row_cells,
    };
    use pretty_assertions::assert_eq;
    use sl_rlv::{RlvBehaviour, RlvExceptionOption, RlvState, parse_chat_line};
    use uuid::Uuid;

    /// A `Box<dyn Error>` alias, so a test can use `?`.
    type TestError = Box<dyn core::error::Error>;

    /// Feed one chat line to `state` as if `object` had said it.
    fn say(state: &mut RlvState, object: Uuid, line: &str) -> Result<(), TestError> {
        let commands = parse_chat_line(line).ok_or("not an RLV line")?;
        for command in commands {
            let command = command.map_err(|error| error.to_string())?;
            state.apply(object, &command);
        }
        Ok(())
    }

    /// A restriction with no option lists under Restrictions; the same
    /// behaviour *with* one lists under Exceptions. That is the whole of the
    /// reference's split, and getting it backwards would put a block in the
    /// exception list.
    #[test]
    fn an_option_moves_a_command_to_the_exception_tab() -> Result<(), TestError> {
        let collar = Uuid::from_u128(1);
        let friend = Uuid::from_u128(2);
        let mut state = RlvState::new();
        say(&mut state, collar, "@sendim=n")?;
        say(&mut state, collar, &format!("@sendim:{friend}=add"))?;

        let (restrictions, exceptions, _modifiers) = project(&state);
        assert_eq!(restrictions.len(), 1);
        let restriction = restrictions.first().ok_or("no restriction row")?;
        assert_eq!(restriction.text, "sendim");
        assert_eq!(exceptions.len(), 1);
        let exception = exceptions.first().ok_or("no exception row")?;
        assert_eq!(exception.behaviour, "sendim");
        assert_eq!(exception.option, friend.to_string());
        Ok(())
    }

    /// A behaviour outside the eleven keeps its option in the Restrictions
    /// list, because there the option restricts rather than excepts.
    #[test]
    fn an_option_that_restricts_stays_a_restriction() -> Result<(), TestError> {
        let collar = Uuid::from_u128(1);
        let mut state = RlvState::new();
        say(&mut state, collar, "@remattach:chest=n")?;

        let (restrictions, exceptions, _modifiers) = project(&state);
        assert_eq!(restrictions.len(), 1);
        assert_eq!(
            restrictions.first().ok_or("no restriction row")?.text,
            "remattach:chest"
        );
        assert!(
            exceptions.is_empty(),
            "an attachment-point option is not an exception"
        );
        Ok(())
    }

    /// The eleven the reference splits out, spelled once here so a change to
    /// the list is a change to this test.
    #[test]
    fn the_exception_behaviours_are_the_references_eleven() {
        for behaviour in [
            RlvBehaviour::Recvchat,
            RlvBehaviour::Recvemote,
            RlvBehaviour::Sendim,
            RlvBehaviour::Recvim,
            RlvBehaviour::Startim,
            RlvBehaviour::Tplure,
            RlvBehaviour::Tprequest,
            RlvBehaviour::Accepttp,
            RlvBehaviour::Accepttprequest,
            RlvBehaviour::Shownames,
            RlvBehaviour::Shownametags,
        ] {
            assert!(
                is_exception_behaviour(behaviour),
                "{behaviour:?} should list under Exceptions"
            );
        }
        assert!(!is_exception_behaviour(RlvBehaviour::Detach));
        assert!(!is_exception_behaviour(RlvBehaviour::Fly));
    }

    /// Two objects holding restrictions are listed as two paragraphs, each
    /// naming its issuer, in the copyable block.
    #[test]
    fn the_copy_block_groups_by_object() -> Result<(), TestError> {
        let collar = Uuid::from_u128(1);
        let cuffs = Uuid::from_u128(2);
        let mut state = RlvState::new();
        say(&mut state, collar, "@detach=n,fly=n")?;
        say(&mut state, cuffs, "@sendchat=n")?;

        let text = formatted_restrictions(&state);
        assert!(text.contains(&format!("{collar}:")), "{text}");
        assert!(text.contains(&format!("{cuffs}:")), "{text}");
        assert!(text.contains("  -> detach\n"), "{text}");
        assert!(text.contains("  -> fly\n"), "{text}");
        assert!(text.contains("  -> sendchat\n"), "{text}");
        Ok(())
    }

    /// An object the state machine knows the seat of is named with its
    /// attachment point; one it does not is named by key alone.
    #[test]
    fn an_issuer_carries_its_attachment_point_when_known() -> Result<(), TestError> {
        let collar = Uuid::from_u128(1);
        let mut state = RlvState::new();
        say(&mut state, collar, "@detach=n")?;
        assert_eq!(issuer_label(&state, collar), collar.to_string());

        let point = sl_rlv::RlvAttachmentPoint::from_name("chest").ok_or("no chest point")?;
        state.set_object_attachment(
            collar,
            Some(sl_rlv::RlvObjectAttachment::new(collar, point)),
        );
        assert_eq!(issuer_label(&state, collar), format!("{collar} (chest)"));
        Ok(())
    }

    /// Each kind of exception option renders as the thing it names.
    #[test]
    fn an_exception_option_renders_as_what_it_names() {
        let avatar = Uuid::from_u128(7);
        assert_eq!(
            exception_option_label(RlvExceptionOption::Avatar(avatar)),
            avatar.to_string()
        );
        assert_eq!(
            exception_option_label(RlvExceptionOption::Channel(-1812)),
            "-1812"
        );
        assert_eq!(
            exception_option_label(RlvExceptionOption::Behaviour(RlvBehaviour::Sendim)),
            "sendim"
        );
        // A behaviour with no restriction spelling still names itself.
        assert!(!behaviour_name(RlvBehaviour::Unknown).is_empty());
    }

    /// Every list's row carries exactly one cell per column of its table — the
    /// guard that replaces the column-index constants: a spec that grows a
    /// column without the projection growing a cell fails here rather than
    /// drawing a blank column.
    #[test]
    fn a_row_fills_every_column_of_its_table() -> Result<(), TestError> {
        let collar = Uuid::from_u128(1);
        let friend = Uuid::from_u128(2);
        let mut state = RlvState::new();
        say(&mut state, collar, "@sendim=n")?;
        say(&mut state, collar, &format!("@sendim:{friend}=add"))?;
        say(&mut state, collar, "@fartouch:5=n")?;

        let (restrictions, exceptions, modifiers) = project(&state);
        assert!(!restrictions.is_empty() && !exceptions.is_empty() && !modifiers.is_empty());
        for (list, rows) in [
            (BehavioursList::Restrictions, restrictions.len()),
            (BehavioursList::Exceptions, exceptions.len()),
            (BehavioursList::Modifiers, modifiers.len()),
        ] {
            let view = BehavioursView {
                restrictions: restrictions.clone(),
                exceptions: exceptions.clone(),
                modifiers: modifiers.clone(),
                built_revision: 0,
                built: true,
            };
            for index in 0..rows {
                let cells = row_cells(&view, list, index).ok_or("row past the end")?;
                assert_eq!(
                    cells.len(),
                    list.spec().columns.len(),
                    "{list:?} row {index} does not fill its table"
                );
            }
        }
        Ok(())
    }

    /// An empty state projects to three empty lists rather than to anything
    /// that has to be special-cased downstream.
    #[test]
    fn an_unrestricted_viewer_projects_to_nothing() {
        let (restrictions, exceptions, modifiers) = project(&RlvState::new());
        assert!(restrictions.is_empty());
        assert!(exceptions.is_empty());
        assert!(modifiers.is_empty());
        assert!(formatted_restrictions(&RlvState::new()).is_empty());
    }
}
