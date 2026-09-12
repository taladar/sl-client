//! The reusable **experience picker** floater: "Choose Experience" — the
//! reference viewer's `LLFloaterExperiencePicker`
//! (`floater_experience_search.xml`), the dialog behind every estate and parcel
//! experience list's **Add** button.
//!
//! # Reusable by requester tag
//!
//! A feature opens the picker with [`sl_viewer_world_api::OpenExperiencePicker`]
//! carrying its own `requester` tag; when the user confirms, the picker emits
//! [`sl_viewer_world_api::ExperiencePicked`] with the same tag, and only the
//! requesting feature acts on it. That is the same out-of-band contract
//! [`sl_viewer_world_api::OpenAvatarPicker`] already uses, so a list wanting an
//! experience is written exactly like a list wanting a resident.
//!
//! Only ever **one** experience per pick: the reference's estate lists open it
//! with `allow_multiple` false and `close_on_select` true
//! (`LLPanelExperienceListEditor::onAdd`), so a confirmed pick names one
//! experience and ends the window.
//!
//! # One window per requester, and nothing remembered
//!
//! This is a **keyed** floater ([`FloaterKey::subject`]), keyed by the requester
//! tag: the list that asked gets its own window, a second Add on the same list
//! raises the one it already has, and closing — or picking — ends that instance
//! outright. The reference does the same and then some: every Add mints a fresh
//! key (`mKey.generateNewID()`) and marks the previous picker dead.
//!
//! Being subject-keyed is also what keeps it out of the persisted floater
//! geometry ([`crate::floater`]'s `persist_id`): a transient dialog that
//! remembered it was open would **reopen itself at the next login**, over a
//! world the person had not asked it about, holding a page of results for a
//! list they last touched days ago. Every piece of per-window state is
//! therefore a component on the window root, and the next open starts from an
//! empty search rather than from whatever a hidden shell had accumulated.
//!
//! # One search, not two
//!
//! The window is the Experiences floater's Search tab with a reply row, and it
//! is that literally: the paging state, the row rendering, the column set and
//! the rating ceiling all come from the crate's `experience_search` module,
//! which both surfaces share. The reference achieves the same by embedding one
//! `LLPanelExperiencePicker` in both windows.
//!
//! # Filters
//!
//! An open states which experiences it may offer
//! ([`sl_viewer_world_api::ExperiencePickerFilter`]) — the reference's filter
//! predicates, named for the three combinations its callers build. A record the
//! metadata cache has not resolved is *shown*: the reference's filters read a
//! cached record and a miss leaves the row in, so hiding unresolved rows would
//! make the visible results depend on reply order.
//!
//! # Divergence
//!
//! The reference's rating filter is an icon combo whose rows are the three
//! `sim_access` codes; ours is the plain rating combo the Experiences floater
//! uses, reading and writing the **same** persisted
//! [`SETTING_SEARCH_MATURITY`] — one control, as in the reference, where both
//! panels are one class reading one setting. The filter is the one thing a picker *does* remember, because
//! it is a preference rather than a window's state.
//!
//! Reference (Firestorm, read-only): `llfloaterexperiencepicker.cpp`,
//! `llpanelexperiencepicker.cpp`, `panel_experience_search.xml`,
//! `floater_experience_search.xml`.

use bevy::input_focus::InputFocus;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui_widgets::{Activate, Button};
use bevy_flair::style::components::ClassList;

use sl_client_bevy::{Command, ExperienceKey, SlCommand, SlEvent, SlSessionEvent};
use sl_settings::{Scope, SettingValue};

use crate::experience_profile::{
    MATURITY_KEYS, OpenExperienceProfile, maturity_from_index, maturity_index,
};
use crate::experience_search::{
    COL_SEARCH_NAME, COL_SEARCH_OWNER, COL_SEARCH_RATING, ExperienceInfos, ExperienceRow,
    SEARCH_COLUMNS, SearchProgress, filter_admits, render_rows, sort_experience_rows,
};
use crate::experiences_floater::{SETTING_SEARCH_MATURITY, search_ceiling};
use crate::floater::{
    Floater, FloaterCaps, FloaterCommand, FloaterHandle, FloaterKey, FloaterOp, FloaterSpec,
    FloaterSystems, KeyedFloaterOpen, KeyedFloaters, host_floater,
};
use crate::i18n::{TransArgs, Translated, Translator};
use crate::settings::ViewerSettings;
use crate::ui::{UiScaffoldSystems, column, row};
use crate::ui_combo::{ComboChanged, ComboSpec, spawn_combo};
use crate::ui_font::UiFont;
use crate::ui_search::{SearchFieldSpec, spawn_search_field};
use crate::ui_table::{
    TableRowCells, TableSelectionMode, TableSortDefault, TableSpec, TableState, set_table_cell,
    spawn_table, spawn_table_row,
};
use crate::virtual_list::{VirtualList, VirtualRow, layout_virtual_lists};
use crate::world_api::{
    AvatarState, ExperiencePicked, ExperiencePickerFilter, GroupsModel, OpenExperiencePicker,
};

/// The picker floater's stable [`Floater::id`] — the kind every instance is an
/// instance *of*.
const PICKER_FLOATER_ID: &str = "experience-picker";

/// The skin class every button in this window carries.
const BUTTON_CLASS: &str = "sk-button";

/// The window's body font size, in logical pixels.
const FONT_SIZE: f32 = 13.0;

/// One result row's height, in logical pixels.
const ROW_HEIGHT: f32 = 18.0;

/// The results viewport's height floor, in logical pixels.
const LIST_MIN_HEIGHT: f32 = 180.0;

/// The window's default content size.
const CONTENT_SIZE: Vec2 = Vec2::new(420.0, 340.0);

/// The window's minimum content size.
const MIN_CONTENT_SIZE: Vec2 = Vec2::new(320.0, 240.0);

/// A label / cell's text colour.
const TEXT_COLOR: Color = Color::srgb(0.90, 0.93, 0.97);

/// A dimmer secondary-text colour.
const DIM_TEXT_COLOR: Color = Color::srgb(0.64, 0.68, 0.76);

/// A disabled action's text colour.
const DISABLED_TEXT_COLOR: Color = Color::srgb(0.45, 0.47, 0.52);

/// An action button's background.
const BUTTON_BACKGROUND: Color = Color::srgb(0.16, 0.19, 0.25);

/// An action button's border.
const BUTTON_BORDER: Color = Color::srgb(0.40, 0.50, 0.62);

/// The results list's background.
const LIST_BACKGROUND: Color = Color::srgba(0.0, 0.0, 0.0, 0.25);

/// The results sort by name, like the reference's `sortByColumnIndex(1, true)`.
static PICKER_SORT: [TableSortDefault; 1] = [TableSortDefault {
    column: COL_SEARCH_NAME,
    ascending: true,
}];

/// The picker's results table — the shared search columns, sortable in place
/// but persisting **nothing**: this window is transient (see the module docs),
/// and a dialog that outlived itself in the settings file only to come back
/// holding a stale sort is the shape of bug being avoided.
static PICKER_TABLE: TableSpec = TableSpec {
    element: "experience-picker-results",
    columns: &SEARCH_COLUMNS,
    selection: TableSelectionMode::Single,
    default_sort: &PICKER_SORT,
    builtin_sort: true,
    row_height: ROW_HEIGHT,
    font_size: FONT_SIZE,
    header_color: DIM_TEXT_COLOR,
    cell_color: TEXT_COLOR,
    column_gap: 4.0,
    row_padding: 4.0,
    sort_setting: None,
    widths_setting: None,
};

// ---------------------------------------------------------------------------
// State.
// ---------------------------------------------------------------------------

/// One picker window's data: who asked, what they will accept, and the page the
/// search is on. **One per window** — a keyed instance's state lives on the
/// window, and dies with it.
#[derive(Component, Debug, Default)]
struct ExperiencePickerState {
    /// The tag of the feature this window is answering.
    requester: Option<&'static str>,
    /// What this window's open will accept.
    filter: ExperiencePickerFilter,
    /// The current page's results, in reply order.
    results: Vec<ExperienceKey>,
    /// The search's progress.
    progress: SearchProgress,
    /// The page the last search asked for (one-based, as the cap numbers them).
    page: i32,
    /// The query the last search asked for, so a paging step re-asks the same
    /// words.
    query: String,
    /// Resolved experience metadata, folded in as replies arrive.
    infos: ExperienceInfos,
    /// Bumped on any change the view rebuild must react to.
    revision: u64,
}

impl ExperiencePickerState {
    /// Bump the revision so the view rebuild reacts.
    const fn touch(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    /// Reset to a fresh open for `requester` under `filter` — what an Add on an
    /// already-open window does, so the second open cannot confirm a row the
    /// first one's filter admitted.
    fn restart(&mut self, requester: &'static str, filter: ExperiencePickerFilter) {
        self.requester = Some(requester);
        self.filter = filter;
        self.results.clear();
        self.query.clear();
        self.page = 0;
        self.progress = SearchProgress::Idle;
        self.touch();
    }
}

/// One picker window's rendered rows, rebuilt when anything they derive from
/// moves.
#[derive(Component, Debug, Default)]
struct ExperiencePickerView {
    /// The rows the table currently shows, filtered and sorted.
    rows: Vec<ExperienceRow>,
    /// The state revision these rows were built from.
    built_revision: Option<u64>,
    /// The table sort revision these rows were built from.
    built_sort: u64,
}

/// One picker window's entities.
#[derive(Component, Debug)]
struct ExperiencePickerUi {
    /// The results table root (carries [`TableState`]).
    table: Entity,
    /// The results viewport (carries [`VirtualList`]).
    viewport: Entity,
    /// The query field.
    search_field: Entity,
    /// The status / page line.
    status: Entity,
    /// The rating-filter combo.
    maturity_combo: Entity,
}

/// Which of a window's buttons a node is.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum PickerButton {
    /// Run the search from page one.
    Find,
    /// Step the search page (`true` forward).
    Page(bool),
    /// Confirm the selected row.
    Select,
    /// Close without picking.
    Cancel,
    /// Open the selected row's profile window.
    Profile,
}

// ---------------------------------------------------------------------------
// Plugin.
// ---------------------------------------------------------------------------

/// The plugin owning the experience picker.
#[derive(Debug)]
pub struct ExperiencePickerPlugin;

impl Plugin for ExperiencePickerPlugin {
    /// Register the messages and systems. Nothing is spawned up front: a keyed
    /// window exists only while something is being picked.
    fn build(&self, app: &mut App) {
        app.add_message::<OpenExperiencePicker>()
            .add_message::<ExperiencePicked>()
            .add_systems(
                Update,
                // After the manager's command pass — see `FloaterSystems`: the
                // click on an Add button also raises the window it was clicked
                // in, and the later raise wins the z-order.
                open_experience_picker
                    .after(FloaterSystems::Commands)
                    .after(UiScaffoldSystems::SpawnRoot)
                    .before(layout_virtual_lists),
            )
            .add_systems(
                Update,
                (
                    ingest_picker_events,
                    picker_search_on_enter,
                    track_picker_maturity,
                    rebuild_picker_view,
                    paint_picker_actions,
                )
                    .chain()
                    .after(open_experience_picker)
                    .before(layout_virtual_lists)
                    .run_if(any_with_component::<ExperiencePickerState>),
            )
            // The row pool is filled by `layout_virtual_lists`, so the passes
            // that build and bind those rows follow it.
            .add_systems(
                Update,
                (populate_picker_rows, bind_picker_rows)
                    .chain()
                    .after(layout_virtual_lists)
                    .run_if(any_with_component::<ExperiencePickerState>),
            );
    }
}

/// The experience picker's [`FloaterSpec`] — shared with the `FLOATERS`
/// registry, so the swept window is the one the viewer spawns.
#[must_use]
pub fn experience_picker_floater_spec() -> FloaterSpec {
    FloaterSpec {
        id: PICKER_FLOATER_ID,
        title: "Choose Experience".to_owned(),
        position: Vec2::new(340.0, 130.0),
        default_size: Some(CONTENT_SIZE),
        min_size: Some(MIN_CONTENT_SIZE),
        dock_host: None,
        caps: FloaterCaps {
            resizable: true,
            minimizable: false,
            closable: true,
            dockable: false,
        },
    }
}

// ---------------------------------------------------------------------------
// Open.
// ---------------------------------------------------------------------------

/// Open (or re-open) the picker window for whoever asked.
///
/// Keyed by the requester tag: the list that asked gets its own window, and a
/// second Add on that same list finds it rather than stacking another.
fn open_experience_picker(
    mut opens: MessageReader<OpenExperiencePicker>,
    mut windows: KeyedFloaters,
    mut states: Query<&mut ExperiencePickerState>,
    settings: Option<Res<ViewerSettings>>,
    mut spawner: Commands,
) {
    let requests: Vec<OpenExperiencePicker> = opens.read().copied().collect();
    let rating = maturity_index(search_ceiling(settings.as_deref()));
    for open in requests {
        let opened = windows.open(
            experience_picker_floater_spec(),
            FloaterKey::subject(&open.requester),
        );
        match opened {
            KeyedFloaterOpen::Spawned(handle) => {
                let ui = build_picker_content(&mut spawner, &handle, rating);
                spawner
                    .entity(handle.title_text)
                    .insert(Translated::new("experience-picker-title"));
                let mut state = ExperiencePickerState::default();
                state.restart(open.requester, open.filter);
                // Seeded here rather than after the insert: the components only
                // reach the world when this frame's commands flush, so a window
                // spawned now is not queryable yet.
                spawner
                    .entity(handle.root)
                    .insert((state, ExperiencePickerView::default(), ui));
            }
            KeyedFloaterOpen::Existing(window) => {
                if let Ok(mut state) = states.get_mut(window) {
                    state.restart(open.requester, open.filter);
                }
            }
        }
    }
}

/// Build one window's content: the query row, the rating filter, the results
/// table and the reply row.
fn build_picker_content(
    commands: &mut Commands,
    handle: &FloaterHandle,
    rating: usize,
) -> ExperiencePickerUi {
    let content = handle.content;
    let query_row = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                flex_shrink: 0.0,
                ..row(Val::Px(6.0))
            },
            ChildOf(content),
        ))
        .id();
    let search = spawn_search_field(
        commands,
        query_row,
        &SearchFieldSpec {
            tab_index: 1,
            font_size: FONT_SIZE,
            min_width: 160.0,
            placeholder: String::new(),
            search_glyph: true,
            ..SearchFieldSpec::new("experience-picker-query")
        },
    );
    if let Some(placeholder) = search.placeholder {
        commands
            .entity(placeholder)
            .insert(Translated::new("experiences-search-placeholder"));
    }
    let _find = spawn_action(
        commands,
        query_row,
        "experiences-find",
        PickerButton::Find,
        2,
    );

    let filter_row = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                flex_shrink: 0.0,
                ..row(Val::Px(6.0))
            },
            ChildOf(content),
        ))
        .id();
    commands.spawn((
        Text::default(),
        Translated::new("experiences-search-rating"),
        UiFont::Sans.at(FONT_SIZE),
        TextColor(DIM_TEXT_COLOR),
        Pickable::IGNORE,
        ChildOf(filter_row),
    ));
    let labels: Vec<String> = MATURITY_KEYS.iter().map(|key| (*key).to_owned()).collect();
    let maturity_combo = spawn_combo(
        commands,
        filter_row,
        &ComboSpec {
            element: "experience-picker-rating",
            labels: &labels,
            // Seeded from the store as the window is built: the filter is a
            // preference, so it survives the window that does not.
            active: rating,
            tab_index: 3,
            font_size: FONT_SIZE,
            translate_labels: true,
        },
    );

    // A floor, not a size: inside a scrolling column a purely `flex_grow` table
    // resolves to zero height, and a layout sweep passes on a zero-height
    // widget. The floor puts rows on screen; the grow takes a resized window.
    let wrapper = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                min_height: Val::Px(LIST_MIN_HEIGHT),
                ..column(Val::Px(2.0))
            },
            ChildOf(content),
        ))
        .id();
    let table = spawn_table(commands, wrapper, &PICKER_TABLE);
    commands
        .entity(table.viewport)
        .insert(BackgroundColor(LIST_BACKGROUND));

    let actions = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                flex_shrink: 0.0,
                // Five buttons and a status line: at this window's minimum
                // width, in a longer language, or at a larger UI scale they do
                // not fit one line. Wrapping moves whole buttons onto a second
                // row; without it the row would squeeze them and each label
                // would wrap inside its own button instead.
                flex_wrap: FlexWrap::Wrap,
                row_gap: Val::Px(4.0),
                ..row(Val::Px(6.0))
            },
            ChildOf(content),
        ))
        .id();
    let _select = spawn_action(
        commands,
        actions,
        "experience-picker-select",
        PickerButton::Select,
        4,
    );
    let _cancel = spawn_action(
        commands,
        actions,
        "experience-picker-cancel",
        PickerButton::Cancel,
        5,
    );
    let _profile = spawn_action(
        commands,
        actions,
        "experiences-profile",
        PickerButton::Profile,
        6,
    );
    let _previous = spawn_action(
        commands,
        actions,
        "experiences-page-previous",
        PickerButton::Page(false),
        7,
    );
    let _next = spawn_action(
        commands,
        actions,
        "experiences-page-next",
        PickerButton::Page(true),
        8,
    );
    let status = commands
        .spawn((
            Text::default(),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(DIM_TEXT_COLOR),
            Pickable::IGNORE,
            Name::new("experience-picker-status"),
            ChildOf(actions),
        ))
        .id();

    ExperiencePickerUi {
        table: table.root,
        viewport: table.viewport,
        search_field: search.field,
        status,
        maturity_combo,
    }
}

/// Spawn one of a window's action buttons, wired to the shared observer.
fn spawn_action(
    commands: &mut Commands,
    parent: Entity,
    label_key: &'static str,
    button: PickerButton,
    tab: i32,
) -> Entity {
    let entity = commands
        .spawn((
            Button,
            TabIndex(tab),
            Node {
                padding: UiRect::axes(Val::Px(8.0), Val::Px(3.0)),
                border: UiRect::all(Val::Px(2.0)),
                // A button yields by moving to the action row's next line, not
                // by squeezing its label: left to shrink, "◀ Previous" comes
                // out as two lines with the arrow alone on the first.
                flex_shrink: 0.0,
                ..default()
            },
            BackgroundColor(BUTTON_BACKGROUND),
            BorderColor::all(BUTTON_BORDER),
            ClassList::new_with_classes([BUTTON_CLASS]),
            Name::new("experience-picker-button"),
            ChildOf(parent),
        ))
        .id();
    commands.spawn((
        Text::default(),
        Translated::new(label_key),
        UiFont::Sans.at(FONT_SIZE),
        TextColor(TEXT_COLOR),
        Pickable::IGNORE,
        ChildOf(entity),
    ));
    commands
        .entity(entity)
        .insert(button)
        .observe(on_picker_button);
    entity
}

// ---------------------------------------------------------------------------
// Ingest.
// ---------------------------------------------------------------------------

/// Fold the search replies into every open picker's state.
fn ingest_picker_events(
    mut events: MessageReader<SlEvent>,
    mut windows: Query<&mut ExperiencePickerState>,
) {
    let frame: Vec<&SlEvent> = events.read().collect();
    if frame.is_empty() {
        return;
    }
    for mut state in &mut windows {
        for event in &frame {
            match &event.0 {
                SlSessionEvent::ExperienceSearchResults(page) => {
                    // A page can only belong to a window that asked for one.
                    // Without the guard an idle picker would silently repoint
                    // its list at the Experiences floater's search — the cap
                    // carries no correlation id, so "did I ask?" is all there
                    // is to go on.
                    if !matches!(state.progress, SearchProgress::Searching) {
                        continue;
                    }
                    state.results = page_results(page);
                    state.progress = SearchProgress::from_page(page);
                    for info in &page.infos {
                        if !info.missing {
                            let _previous = state.infos.insert(info.public_id, info.clone());
                        }
                    }
                    state.touch();
                }
                SlSessionEvent::ExperienceInfo(list) => {
                    for info in list.iter().filter(|info| !info.missing) {
                        let _previous = state.infos.insert(info.public_id, info.clone());
                    }
                    state.touch();
                }
                _other => {}
            }
        }
    }
}

/// `Enter` in a query field runs that window's search — the reference's default
/// button.
fn picker_search_on_enter(
    mut keyboard: ResMut<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    mut windows: Query<(&ExperiencePickerUi, &mut ExperiencePickerState)>,
    fields: Query<&EditableText>,
    mut sl: MessageWriter<SlCommand>,
) {
    if !keyboard.just_pressed(KeyCode::Enter) {
        return;
    }
    let Some(focused) = focus.get() else {
        return;
    };
    for (ui, mut state) in &mut windows {
        if ui.search_field != focused {
            continue;
        }
        let query = fields
            .get(ui.search_field)
            .map(|field| field.value().to_string())
            .unwrap_or_default();
        run_picker_search(&mut state, query, 1, &mut sl);
        keyboard.clear_just_pressed(KeyCode::Enter);
        return;
    }
}

/// Persist the rating filter when a picker's combo changes, and re-filter that
/// window's current page. The setting is shared with the Experiences floater's
/// search tab, as the reference shares one control between its two panels.
fn track_picker_maturity(
    mut changes: MessageReader<ComboChanged>,
    mut windows: Query<(&ExperiencePickerUi, &mut ExperiencePickerState)>,
    mut settings: Option<ResMut<ViewerSettings>>,
) {
    let frame: Vec<ComboChanged> = changes.read().copied().collect();
    if frame.is_empty() {
        return;
    }
    for (ui, mut state) in &mut windows {
        for change in &frame {
            if change.combo != ui.maturity_combo {
                continue;
            }
            if let Some(settings) = settings.as_mut() {
                settings.set(
                    Scope::Account,
                    SETTING_SEARCH_MATURITY,
                    SettingValue::I32(maturity_from_index(change.active)),
                );
            }
            state.touch();
        }
    }
}

/// The ids a search page offers: every record it actually named.
///
/// A **`missing`** record — the `DoesNotExist` placeholder a cap builds for an
/// id it could not resolve — is dropped rather than listed. It has to be: the
/// placeholder carries no properties, an unresolved record is admitted by every
/// filter on purpose (see [`filter_admits`]), and the two list filters are
/// otherwise disjoint. Listing one would be the single way an experience could
/// be offered to *both* the Allowed and the Blocked picker — and it would be
/// offered under a short-id label, since there is no name to show either.
fn page_results(page: &sl_client_bevy::ExperienceSearchPage) -> Vec<ExperienceKey> {
    page.infos
        .iter()
        .filter(|info| !info.missing)
        .map(|info| info.public_id)
        .collect()
}

/// The page a Next / Previous step lands on. The cap numbers pages from one,
/// so Previous stops there rather than asking for page zero.
const fn stepped_page(page: i32, forward: bool) -> i32 {
    if forward {
        page.saturating_add(1)
    } else {
        let back = page.saturating_sub(1);
        if back < 1 { 1 } else { back }
    }
}

/// Run one window's search from `page`, remembering the query so a paging step
/// re-asks the same words.
fn run_picker_search(
    state: &mut ExperiencePickerState,
    query: String,
    page: i32,
    sl: &mut MessageWriter<SlCommand>,
) {
    state.query = query;
    state.page = page.max(1);
    state.progress = SearchProgress::Searching;
    state.results.clear();
    state.touch();
    sl.write(SlCommand(Command::FindExperiences {
        query: state.query.clone(),
        page: state.page,
    }));
}

// ---------------------------------------------------------------------------
// View.
// ---------------------------------------------------------------------------

/// Rebuild each window's visible rows: its page, minus what its open's filter
/// and the rating ceiling exclude, in its table's sort order.
fn rebuild_picker_view(
    mut windows: Query<(
        Ref<ExperiencePickerState>,
        &mut ExperiencePickerView,
        &ExperiencePickerUi,
    )>,
    avatars: Res<AvatarState>,
    groups: Res<GroupsModel>,
    settings: Option<Res<ViewerSettings>>,
    translator: Translator,
    tables: Query<&TableState>,
    mut lists: Query<&mut VirtualList>,
) {
    let caches_moved = avatars.is_changed() || groups.is_changed() || translator.changed();
    let ceiling = search_ceiling(settings.as_deref());
    for (state, view, ui) in &mut windows {
        let view = view.into_inner();
        let sort_revision = tables.get(ui.table).map_or(0, TableState::sort_revision);
        if !caches_moved
            && view.built_revision == Some(state.revision)
            && view.built_sort == sort_revision
        {
            continue;
        }
        view.built_revision = Some(state.revision);
        view.built_sort = sort_revision;

        let visible: Vec<ExperienceKey> = state
            .results
            .iter()
            .copied()
            .filter(|id| filter_admits(state.filter, &state.infos, *id))
            .filter(|id| {
                state
                    .infos
                    .get(id)
                    .is_none_or(|info| info.maturity <= ceiling)
            })
            .collect();
        let mut rows = render_rows(&state.infos, &visible, &avatars, &groups, &translator);
        sort_experience_rows(&mut rows, &picker_sort_keys(&tables, ui.table));
        view.rows = rows;

        if let Ok(mut list) = lists.get_mut(ui.viewport)
            && list.item_count != view.rows.len()
        {
            list.item_count = view.rows.len();
        }
    }
}

/// A results table's sort, as (column token, ascending) pairs.
fn picker_sort_keys(tables: &Query<&TableState>, table: Entity) -> Vec<(&'static str, bool)> {
    let Ok(state) = tables.get(table) else {
        return Vec::new();
    };
    state
        .sort()
        .keys()
        .iter()
        .filter_map(|key| {
            PICKER_TABLE
                .columns
                .get(key.column)
                .map(|column| (column.token, key.ascending))
        })
        .collect()
}

/// Build the table cells of each freshly-pooled row, in whichever window's list
/// it was pooled into.
fn populate_picker_rows(
    mut commands: Commands,
    windows: Query<&ExperiencePickerUi>,
    new_rows: Query<(Entity, &ChildOf), Added<VirtualRow>>,
) {
    for (row_entity, child_of) in &new_rows {
        let parent = child_of.parent();
        for ui in &windows {
            if ui.viewport != parent {
                continue;
            }
            spawn_table_row(&mut commands, row_entity, ui.table, &PICKER_TABLE);
            break;
        }
    }
}

/// Bind each pooled row to the result its window now shows there.
fn bind_picker_rows(
    windows: Query<(Ref<ExperiencePickerView>, &ExperiencePickerUi)>,
    rows: Query<(Ref<VirtualRow>, &ChildOf, &TableRowCells)>,
    mut texts: Query<(&mut Text, &mut TextColor)>,
) {
    for (view, ui) in &windows {
        let refresh_all = view.is_changed();
        for (row, child_of, cells) in &rows {
            if child_of.parent() != ui.viewport {
                continue;
            }
            if !refresh_all && !row.is_changed() {
                continue;
            }
            let data = row.index.and_then(|index| view.rows.get(index));
            for (column, value) in [
                (COL_SEARCH_RATING, data.map(|row| row.rating.as_str())),
                (COL_SEARCH_NAME, data.map(|row| row.name.as_str())),
                (COL_SEARCH_OWNER, data.map(|row| row.owner.as_str())),
            ] {
                if let Some(cell) = cells.cell(column) {
                    set_table_cell(&mut texts, cell, value.unwrap_or(""), TEXT_COLOR);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Actions.
// ---------------------------------------------------------------------------

/// Grey the actions that would do nothing, and write each window's status line.
fn paint_picker_actions(
    windows: Query<(Entity, &ExperiencePickerState, &ExperiencePickerUi)>,
    tables: Query<&TableState>,
    buttons: Query<(Entity, &PickerButton, &Children)>,
    parents: Query<&ChildOf>,
    floaters: Query<(Entity, &Floater)>,
    translator: Translator,
    mut texts: Query<(&mut Text, &mut TextColor)>,
) {
    for (window, state, ui) in &windows {
        let selected = picker_selection(&tables, ui.table).is_some();
        for (entity, button, children) in &buttons {
            if host_floater(entity, &parents, &floaters) != Some(window) {
                continue;
            }
            let enabled = match *button {
                PickerButton::Select | PickerButton::Profile => selected,
                PickerButton::Page(true) => state.progress.offers_next(),
                PickerButton::Page(false) => state.progress.offers_previous(),
                PickerButton::Find | PickerButton::Cancel => true,
            };
            let wanted = if enabled {
                TEXT_COLOR
            } else {
                DISABLED_TEXT_COLOR
            };
            for child in children {
                if let Ok((_text, mut color)) = texts.get_mut(*child)
                    && color.0 != wanted
                {
                    color.0 = wanted;
                }
            }
        }
        let line = match state.progress {
            SearchProgress::Idle => String::new(),
            SearchProgress::Searching => translator.get("experiences-searching"),
            SearchProgress::Done { .. } => translator.format(
                "experiences-search-page",
                &TransArgs::new().int("page", i64::from(state.page)),
            ),
        };
        if let Ok((mut text, _color)) = texts.get_mut(ui.status)
            && text.0 != line
        {
            text.0 = line;
        }
    }
}

/// The index of a table's selected result row, if any.
fn picker_selection(tables: &Query<&TableState>, table: Entity) -> Option<usize> {
    tables
        .get(table)
        .ok()
        .and_then(TableState::primary_selected)
}

/// Every button, resolved by its [`PickerButton`] kind, in the window it was
/// pressed in.
#[expect(
    clippy::too_many_arguments,
    reason = "an observer's parameters are its injected world access: the button kind, \
              the window it belongs to and that window's state and rows, the table \
              selection, the query field and the three message sinks"
)]
fn on_picker_button(
    activate: On<Activate>,
    buttons: Query<&PickerButton>,
    mut windows: Query<(
        &ExperiencePickerUi,
        &ExperiencePickerView,
        &mut ExperiencePickerState,
    )>,
    parents: Query<&ChildOf>,
    floaters: Query<(Entity, &Floater)>,
    tables: Query<&TableState>,
    fields: Query<&EditableText>,
    mut profiles: MessageWriter<OpenExperienceProfile>,
    mut picked: MessageWriter<ExperiencePicked>,
    mut chrome: MessageWriter<FloaterCommand>,
    mut sl: MessageWriter<SlCommand>,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    let Some(window) = host_floater(activate.entity, &parents, &floaters) else {
        return;
    };
    let Ok((ui, view, mut state)) = windows.get_mut(window) else {
        return;
    };
    let row = picker_selection(&tables, ui.table).and_then(|index| view.rows.get(index));
    match *button {
        PickerButton::Find => {
            let query = fields
                .get(ui.search_field)
                .map(|field| field.value().to_string())
                .unwrap_or_default();
            run_picker_search(&mut state, query, 1, &mut sl);
        }
        PickerButton::Page(forward) => {
            let page = stepped_page(state.page, forward);
            let query = state.query.clone();
            run_picker_search(&mut state, query, page, &mut sl);
        }
        PickerButton::Profile => {
            if let Some(row) = row {
                profiles.write(OpenExperienceProfile { experience: row.id });
            }
        }
        PickerButton::Select => {
            let (Some(requester), Some(row)) = (state.requester, row) else {
                return;
            };
            picked.write(ExperiencePicked {
                requester,
                experience: row.id,
                name: row.name.clone(),
            });
            // The reference's `close_on_select`. A keyed instance's Close
            // despawns it, so the window and its search go together.
            chrome.write(FloaterCommand {
                floater: window,
                op: FloaterOp::Close,
            });
        }
        PickerButton::Cancel => {
            chrome.write(FloaterCommand {
                floater: window,
                op: FloaterOp::Close,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{ExperienceInfo, ExperienceProperties, Uuid};
    use sl_types::experience::{PROPERTY_GRID, PROPERTY_PRIVILEGED};

    use super::*;

    /// An experience id from a small number, so a test reads its own ids.
    fn key(n: u128) -> ExperienceKey {
        ExperienceKey::from(Uuid::from_u128(n))
    }

    /// A record with the given property bits.
    fn info(n: u128, properties: i32) -> ExperienceInfo {
        ExperienceInfo {
            public_id: key(n),
            name: format!("Experience {n}"),
            properties: ExperienceProperties(properties),
            ..ExperienceInfo::default()
        }
    }

    /// The three filters admit exactly what the reference's predicate lists do:
    /// Key takes anything, Allowed refuses a grid-scoped experience, Blocked
    /// takes only grid-scoped ones and refuses a privileged one.
    #[test]
    fn filters_match_the_reference_predicates() {
        let land = ExperienceProperties(0);
        let grid = ExperienceProperties(PROPERTY_GRID);
        let privileged = ExperienceProperties(PROPERTY_GRID | PROPERTY_PRIVILEGED);

        assert!(ExperiencePickerFilter::Any.admits(land));
        assert!(ExperiencePickerFilter::Any.admits(grid));
        assert!(ExperiencePickerFilter::Any.admits(privileged));

        assert!(ExperiencePickerFilter::LandScoped.admits(land));
        assert!(!ExperiencePickerFilter::LandScoped.admits(grid));
        assert!(!ExperiencePickerFilter::LandScoped.admits(privileged));

        assert!(!ExperiencePickerFilter::GridScopedUnprivileged.admits(land));
        assert!(ExperiencePickerFilter::GridScopedUnprivileged.admits(grid));
        assert!(!ExperiencePickerFilter::GridScopedUnprivileged.admits(privileged));
    }

    /// **Nothing can be offered to both the Allowed and the Blocked list.**
    ///
    /// The two lists are kept apart by *scope*, not by a duplicate check: an
    /// experience is either grid-scoped or it is not, Allowed takes only the
    /// land-scoped ones and Blocked only the grid-scoped ones. That is the
    /// reference's arrangement too (`refreshFromRegion` gives one
    /// `FilterWithProperty(PROPERTY_GRID)` and the other
    /// `FilterWithoutProperty(PROPERTY_GRID)`), and it is worth pinning as a
    /// property rather than trusting two `match` arms to stay opposite —
    /// checked over every combination of the property bits.
    ///
    /// The Key list deliberately overlaps both: any experience may be Key, and
    /// the reference puts no filter on it at all.
    #[test]
    fn allowed_and_blocked_can_never_be_offered_the_same_experience() {
        for bits in 0..256_i32 {
            let properties = ExperienceProperties(bits);
            assert!(
                !(ExperiencePickerFilter::LandScoped.admits(properties)
                    && ExperiencePickerFilter::GridScopedUnprivileged.admits(properties)),
                "properties {bits:#010b} passed both the Allowed and the Blocked filter"
            );
            // And between them they cover everything a Key list could hold,
            // minus the privileged experiences nothing may block.
            assert!(
                ExperiencePickerFilter::Any.admits(properties),
                "the Key list refuses nothing"
            );
        }
    }

    /// A `missing` record is not offered at all — the one way an experience
    /// could otherwise reach both pickers, since an unresolved record is
    /// admitted by every filter by design.
    #[test]
    fn a_missing_record_is_not_offered() {
        let page = sl_client_bevy::ExperienceSearchPage {
            infos: vec![
                info(1, PROPERTY_GRID),
                ExperienceInfo {
                    public_id: key(2),
                    missing: true,
                    ..ExperienceInfo::default()
                },
                info(3, 0),
            ],
            has_next_page: false,
            has_previous_page: false,
        };
        assert_eq!(page_results(&page), vec![key(1), key(3)]);

        // Had it been listed, it would have passed both list filters.
        let empty = ExperienceInfos::new();
        assert!(filter_admits(
            ExperiencePickerFilter::LandScoped,
            &empty,
            key(2)
        ));
        assert!(filter_admits(
            ExperiencePickerFilter::GridScopedUnprivileged,
            &empty,
            key(2)
        ));
    }

    /// An id the metadata cache has not resolved yet passes **every** filter:
    /// the reference reads properties off a cached record and a miss leaves the
    /// row in, so which rows are visible must not depend on reply order.
    #[test]
    fn an_unresolved_record_is_admitted_by_every_filter() {
        let mut infos = ExperienceInfos::new();
        let _previous = infos.insert(key(1), info(1, PROPERTY_GRID));
        for filter in [
            ExperiencePickerFilter::Any,
            ExperiencePickerFilter::LandScoped,
            ExperiencePickerFilter::GridScopedUnprivileged,
        ] {
            assert!(
                filter_admits(filter, &infos, key(99)),
                "an unresolved id must survive {filter:?}"
            );
        }
        assert!(!filter_admits(
            ExperiencePickerFilter::LandScoped,
            &infos,
            key(1)
        ));
    }

    /// The paging arrows never walk off either end: Previous stops at page one
    /// (the cap numbers pages from one), and neither step can overflow.
    #[test]
    fn a_page_step_stays_in_range() {
        assert_eq!(stepped_page(1, true), 2);
        assert_eq!(stepped_page(2, false), 1);
        assert_eq!(stepped_page(1, false), 1);
        assert_eq!(stepped_page(0, false), 1);
        assert_eq!(stepped_page(i32::MAX, true), i32::MAX);
    }

    /// Re-opening a window that is already up **restarts** it: a page of
    /// results a previous Add left behind must not be confirmable against the
    /// new request, whose filter may admit less.
    #[test]
    fn re_opening_restarts_the_search() {
        let mut state = ExperiencePickerState::default();
        state.restart("trusted", ExperiencePickerFilter::Any);
        state.query = "tour".to_owned();
        state.page = 3;
        state.results = vec![key(1), key(2)];
        state.progress = SearchProgress::Done {
            has_next_page: true,
            has_previous_page: true,
        };

        state.restart("blocked", ExperiencePickerFilter::GridScopedUnprivileged);
        assert_eq!(state.requester, Some("blocked"));
        assert_eq!(state.filter, ExperiencePickerFilter::GridScopedUnprivileged);
        assert!(state.results.is_empty(), "a stale page must not survive");
        assert!(state.query.is_empty());
        assert_eq!(state.page, 0);
        assert_eq!(state.progress, SearchProgress::Idle);
        // The resolved-metadata cache is *not* cleared: it is a cache of what
        // the grid said about ids, not state of this open, and re-fetching it
        // would only make the next page's rows arrive unnamed.
    }

    /// A rating ceiling hides a result above it, and an unresolved record is
    /// shown — the same reply-order independence the filters have.
    #[test]
    fn the_rating_ceiling_hides_only_what_it_knows_is_too_strong() {
        let mut infos = ExperienceInfos::new();
        let _general = infos.insert(
            key(1),
            ExperienceInfo {
                maturity: 13,
                ..info(1, 0)
            },
        );
        let _adult = infos.insert(
            key(2),
            ExperienceInfo {
                maturity: 42,
                ..info(2, 0)
            },
        );
        let visible: Vec<ExperienceKey> = [key(1), key(2), key(3)]
            .into_iter()
            .filter(|id| infos.get(id).is_none_or(|info| info.maturity <= 13))
            .collect();
        assert_eq!(visible, vec![key(1), key(3)]);
    }
}
