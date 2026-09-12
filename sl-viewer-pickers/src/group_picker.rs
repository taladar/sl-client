//! The reusable **group picker** floater
//! (`viewer-region-estate-group-picker`): "Choose Group" — the reference
//! viewer's `LLFloaterGroupPicker` (`floater_choose_group.xml`), the dialog
//! behind the estate's allowed-groups Add, About Land's group **Set…** and the
//! build tool's set-group.
//!
//! # Two sources, and which of them a caller may have
//!
//! - **My Groups** — the agent's own memberships ([`GroupsModel`], the
//!   login-time `AgentGroupDataUpdate`), which is the reference's *entire*
//!   list (`init_group_list` walks `gAgent.mGroups`).
//! - **Search** — the directory's group query (`Command::DirFindQuery` with
//!   [`DirFindFlags::GROUPS`], answered by `DirGroupsReply`), the same call the
//!   Search floater's Groups category makes.
//!
//! The second source is **not** offered to every caller, and that is a
//! protocol fact rather than a UI preference: the simulator refuses to set a
//! parcel's or an object's group to one the agent is not a member of, so
//! offering a stranger's group to About Land's Set… would only ever produce a
//! silently-refused update. Where the answer is merely *recorded* against an id
//! — the estate's allowed-groups list — any group will do, and being able to
//! name one you are not in is a plain improvement on the reference, which can
//! only allow groups its own user happens to have joined.
//!
//! So a caller asks for what it can use: [`OpenGroupPicker::new`] offers the
//! memberships alone, and [`OpenGroupPicker::searching_the_directory`] adds the
//! Search tab. The tab strip is hidden entirely for the first kind, so a
//! member-only picker is the reference's single list and looks like it.
//!
//! The search asks for every maturity band. The Search floater filters by the
//! person's content preferences because it is *browsing*; this picker is
//! resolving a group somebody already named, and a band filter there would hide
//! the one group the search exists to find.
//!
//! # None is an answer, where the caller says so
//!
//! The reference builds the "none" row into the list and lets a caller take it
//! back out (`removeNoneOption`); here the caller says so up front
//! ([`OpenGroupPicker::without_none`]), so the row is never built for a list
//! that cannot hold it. Setting a parcel's or an object's group may clear it;
//! *adding* to the estate's allowed-groups list may not.
//!
//! Nothing is preselected. The active (worn) group is drawn **bold**, as the
//! reference draws it, but bold is a hint about which group you are wearing,
//! not a pick: OK with nothing chosen does nothing, so a stray press cannot
//! commit a group — or the null group — the person never picked.
//!
//! # One window per control, and nothing remembered
//!
//! A **keyed** floater, keyed by the opening window and the field together
//! ([`picker_identity`]), for the same reason the resident and texture pickers
//! are: two About Region windows, or one window's two group controls, each want
//! their own answer, and a singleton picker with one `requester` slot lets the
//! second open quietly discard the first's claim. Every piece of per-window
//! state is a component on the window root, so closing it — or closing the
//! window that opened it ([`FloaterOwner`]) — ends it outright and persists
//! nothing.
//!
//! # Title
//!
//! The reference titles the window "Groups" and captions the list "Choose a
//! group:". This one folds the caption into the title ("Choose Group"), which
//! is what the caption says and what the sibling resident picker's title reads
//! like; a window that is only ever a chooser should not be titled as if it
//! were the group *list* (which this viewer has, as the People pane's Groups
//! tab).
//!
//! Reference (Firestorm, read-only): `llfloatergroups.cpp`
//! (`LLFloaterGroupPicker`, `init_group_list`), `floater_choose_group.xml`.

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::text::EditableText;
use sl_client_bevy::{
    Command, DirFindFlags, DirGroupResult, GroupKey, QueryId, SlCommand, SlEvent, SlSessionEvent,
    Uuid,
};

use crate::floater::{
    Floater, FloaterCaps, FloaterCommand, FloaterHandle, FloaterOp, FloaterOwner, FloaterSpec,
    FloaterSystems, KeyedFloaterOpen, KeyedFloaters, host_floater, picker_identity,
};
use crate::i18n::Translated;
use crate::ui::{UiScaffoldSystems, column, row};
use crate::ui_font::UiFont;
use crate::ui_tab::{DEFAULT_ELLIPSIS, TabPlacement, TabSpec, TabStrip, spawn_tab_strip};
use crate::world_api::{GroupPicked, GroupPickerScope, GroupsModel, OpenGroupPicker};

/// The floater's [`crate::floater::FloaterSpec::id`].
const PICKER_FLOATER_ID: &str = "group-picker";

/// The picker font size, in logical pixels.
const PICKER_FONT_SIZE: f32 = 14.0;

/// The label colour.
const LABEL_COLOR: Color = Color::srgb(0.90, 0.92, 0.96);

/// A button's background.
const BUTTON_BACKGROUND: Color = Color::srgb(0.13, 0.15, 0.20);
/// A button's border colour.
const BUTTON_BORDER: Color = Color::srgb(0.34, 0.40, 0.52);

/// The trailing detail column's colour — dimmer than the name it trails.
const DETAIL_COLOR: Color = Color::srgb(0.62, 0.66, 0.74);

/// A selected row's background.
const SELECTED_ROW_BACKGROUND: Color = Color::srgba(0.24, 0.34, 0.52, 0.55);

/// The Fluent key for the "you are in no groups" row.
const NO_GROUPS_KEY: &str = "group-picker-no-groups";

/// The Fluent key for the "the search matched nothing" row.
const NOT_FOUND_KEY: &str = "group-picker-not-found";

/// The Fluent key the "none" row is labelled with.
const NONE_ROW_KEY: &str = "group-picker-none";

/// The list's viewport height, in logical pixels. The list scrolls: an agent
/// may be in dozens of groups, and the reference's own list is a scroll list.
const LIST_HEIGHT: f32 = 220.0;

/// The most search-result rows shown. A directory reply is bounded upstream;
/// this is the same clamp the resident picker puts on its own sources so the
/// plain column stays cheap.
const MAX_ROWS: usize = 100;

/// Which source a window is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum PickerTab {
    /// The agent's own memberships.
    #[default]
    MyGroups,
    /// The directory name search.
    Search,
}

/// The tab order, matching the spawned strip.
const TAB_ORDER: [PickerTab; 2] = [PickerTab::MyGroups, PickerTab::Search];

/// One selectable row: a group, or the "none" row.
#[derive(Debug, Clone, PartialEq, Eq)]
struct GroupPickerRow {
    /// The group the row names, or `None` for the "none" row.
    group: Option<GroupKey>,
    /// The row label — the group's name. Empty for the "none" row, which is
    /// labelled from [`NONE_ROW_KEY`] instead so it translates.
    name: String,
    /// The dimmer trailing column: a search row's member count, empty for a
    /// membership row (the reference's group search shows the same count).
    detail: String,
    /// Whether this is the agent's **active** (worn) group, which the reference
    /// draws bold.
    active: bool,
}

/// One picker window's live state — a component on the window root, so it dies
/// with the instance and the next open starts from an empty selection.
#[derive(Component, Debug, Default)]
pub(crate) struct GroupPickerState {
    /// The control this window is answering — the button that opened it.
    /// `None` once a pick has been confirmed or cancelled.
    requester: Option<Entity>,
    /// Whether the "none" row is offered.
    allow_none: bool,
    /// Which sources this window may show.
    scope: GroupPickerScope,
    /// The source currently shown.
    tab: PickerTab,
    /// The current rows, top to bottom.
    rows: Vec<GroupPickerRow>,
    /// The selected row, if any. A group picker takes exactly one answer, so
    /// this is an index and not the resident picker's list.
    selected: Option<usize>,
    /// The in-flight directory query id, so a stale reply is ignored.
    pending_query: Option<QueryId>,
    /// Whether a search has answered since this tab was opened — what tells an
    /// empty list "nothing matched" from "nothing has been searched for yet".
    searched: bool,
    /// Bumped whenever `rows` / `selected` change, driving the list rebuild.
    revision: u64,
    /// The revision the visible list was last built from, so a rebuild happens
    /// exactly when something it shows moved. Per window, not a system `Local`,
    /// which could only ever track one of them.
    built_revision: Option<u64>,
}

impl GroupPickerState {
    /// The rows the **My Groups** tab should be showing, from the membership
    /// model: the "none" row where the open allowed it, then the agent's groups
    /// in the model's name order (which is the order the reference sorts its
    /// list in).
    fn membership_rows(&self, groups: &GroupsModel) -> Vec<GroupPickerRow> {
        let none = self.allow_none.then(|| GroupPickerRow {
            group: None,
            name: String::new(),
            detail: String::new(),
            active: false,
        });
        none.into_iter()
            .chain(groups.ordered().into_iter().map(|row| GroupPickerRow {
                group: Some(row.group),
                name: row.name,
                detail: String::new(),
                active: row.active,
            }))
            .collect()
    }

    /// Replace the rows, carrying the selection across **by group** rather than
    /// by index: a membership change (joining or leaving a group while the
    /// picker is open) re-orders the list under the person, and a selection
    /// left on an index would silently become some other group.
    fn set_rows(&mut self, rows: Vec<GroupPickerRow>) {
        if rows == self.rows {
            return;
        }
        let selected = self
            .selected
            .and_then(|index| self.rows.get(index))
            .map(|row| row.group);
        self.rows = rows;
        self.rows.truncate(MAX_ROWS);
        self.selected =
            selected.and_then(|group| self.rows.iter().position(|row| row.group == group));
        self.revision = self.revision.wrapping_add(1);
    }

    /// Select a row, ignoring a click past the end of a list that has just
    /// shrunk.
    fn select(&mut self, index: usize) {
        if index >= self.rows.len() || self.selected == Some(index) {
            return;
        }
        self.selected = Some(index);
        self.revision = self.revision.wrapping_add(1);
    }

    /// Clear the selection — what a fresh open starts from, so a leftover pick
    /// cannot confirm a group the person never chose.
    const fn clear_selection(&mut self) {
        if self.selected.is_some() {
            self.selected = None;
            self.revision = self.revision.wrapping_add(1);
        }
    }

    /// The confirmed answer, if a row is selected: the group (or `None` for the
    /// "none" row) and the label that row carried.
    fn pick(&self) -> Option<(Option<GroupKey>, String)> {
        self.selected
            .and_then(|index| self.rows.get(index))
            .map(|row| (row.group, row.name.clone()))
    }
}

/// One picker window's parts.
#[derive(Component)]
pub(crate) struct GroupPickerUi {
    /// The source tab strip (hidden for a member-only picker).
    tab_strip: Entity,
    /// The search text field.
    search_field: Entity,
    /// The search row (hidden off the Search tab).
    search_row: Entity,
    /// The row list container.
    list: Entity,
}

/// The plugin owning the group picker.
#[derive(Debug)]
pub struct GroupPickerPlugin;

impl Plugin for GroupPickerPlugin {
    /// Register the messages and systems. Nothing is spawned up front: a keyed
    /// window exists only while a group is being picked.
    fn build(&self, app: &mut App) {
        app.add_message::<OpenGroupPicker>()
            .add_message::<GroupPicked>()
            .init_resource::<GroupsModel>()
            .add_systems(
                Update,
                (
                    // After the manager's command pass — see `FloaterSystems`:
                    // the click on a Set… button also raises the window it was
                    // clicked in, and the later raise wins the z-order.
                    handle_open_requests
                        .after(FloaterSystems::Commands)
                        .after(UiScaffoldSystems::SpawnRoot),
                    bridge_picker_tabs,
                    ingest_group_search_replies,
                    refresh_membership_rows,
                    rebuild_picker_list,
                )
                    .chain(),
            );
    }
}

/// The group picker floater's [`FloaterSpec`] — shared with the `FLOATERS`
/// registry, so the swept window is the one the viewer spawns.
#[must_use]
pub fn group_picker_floater_spec() -> FloaterSpec {
    FloaterSpec {
        id: PICKER_FLOATER_ID,
        title: "Choose Group".to_owned(),
        position: Vec2::new(360.0, 150.0),
        default_size: None,
        min_size: None,
        dock_host: None,
        caps: FloaterCaps {
            resizable: false,
            minimizable: false,
            closable: true,
            dockable: false,
        },
    }
}

/// Build one picker window's content: the source tabs, the search row, the row
/// list and the OK / Cancel row.
///
/// Both tabs are built whatever the open's scope, and a member-only picker
/// hides the strip and the search row ([`bridge_picker_tabs`]). Building only
/// what one open asked for would leave the window wrong if the same control
/// ever opened it the other way, and a keyed window outlives the open that
/// spawned it.
fn build_picker_content(commands: &mut Commands, handle: &FloaterHandle) -> GroupPickerUi {
    commands
        .entity(handle.title_text)
        .insert(Translated::new("group-picker-title"));
    let content = handle.content;

    let tab_labels: [String; 2] = [
        "group-picker-tab-mine".to_owned(),
        "group-picker-tab-search".to_owned(),
    ];
    let tab_strip = spawn_tab_strip(
        commands,
        content,
        &TabSpec {
            element: "group-picker-tabs",
            placement: TabPlacement::BlockStart,
            labels: &tab_labels,
            active: 0,
            tab_index: 1,
            font_size: PICKER_FONT_SIZE,
            strip_width: None,
            ellipsis: DEFAULT_ELLIPSIS,
            translate_labels: true,
        },
    );

    // The search row: a name fragment and a Go button.
    let search_row = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                ..row(Val::Px(6.0))
            },
            ChildOf(content),
        ))
        .id();
    let search_field = crate::ui_text_input::spawn_text_input(
        commands,
        search_row,
        &crate::ui_text_input::TextInputSpec {
            font_size: PICKER_FONT_SIZE,
            width_glyphs: 18.0,
            tab_index: 2,
            ..crate::ui_text_input::TextInputSpec::new(
                "group-picker-search",
                crate::ui_text_input::TextInputKind::Line,
            )
        },
    );
    let _go = spawn_picker_button(commands, search_row, "group-picker-go", PickerButton::Go, 3);

    let list = commands
        .spawn((
            Node {
                height: Val::Px(LIST_HEIGHT),
                overflow: Overflow::scroll_y(),
                ..column(Val::Px(2.0))
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.25)),
            ChildOf(content),
        ))
        .id();

    let buttons = commands
        .spawn((
            Node {
                ..row(Val::Px(8.0))
            },
            ChildOf(content),
        ))
        .id();
    let _ok = spawn_picker_button(commands, buttons, "group-picker-ok", PickerButton::Ok, 4);
    let _cancel = spawn_picker_button(
        commands,
        buttons,
        "group-picker-cancel",
        PickerButton::Cancel,
        5,
    );

    GroupPickerUi {
        tab_strip,
        search_field,
        search_row,
        list,
    }
}

/// Which of a window's buttons a node is — a component rather than a closure
/// per button, because every one of them has to find the window it was pressed
/// in ([`host_floater`]) and a captured handle cannot.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum PickerButton {
    /// Run the directory search for the field's current text.
    Go,
    /// Confirm the selected group.
    Ok,
    /// Close without picking.
    Cancel,
}

/// Act on a press of one of a window's buttons, in the window it was pressed in.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy observer's parameters are its injected world access: the press, the \
              button's action, the window it lives in (found through the parent chain), the \
              search field it reads, and the three channels a press can write to"
)]
fn on_picker_button(
    press: On<Pointer<Press>>,
    buttons: Query<&PickerButton>,
    mut windows: Query<(&mut GroupPickerState, &GroupPickerUi)>,
    parents: Query<&ChildOf>,
    floaters: Query<(Entity, &Floater)>,
    fields: Query<&EditableText>,
    mut picked: MessageWriter<GroupPicked>,
    mut chrome: MessageWriter<FloaterCommand>,
    mut sl: MessageWriter<SlCommand>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let Ok(button) = buttons.get(press.entity) else {
        return;
    };
    let Some(window) = host_floater(press.entity, &parents, &floaters) else {
        return;
    };
    let Ok((mut state, ui)) = windows.get_mut(window) else {
        return;
    };
    match *button {
        PickerButton::Go => send_search(ui, &fields, &mut state, &mut sl),
        PickerButton::Ok => {
            let Some(requester) = state.requester else {
                return;
            };
            // Nothing selected is not an answer — least of all the null group,
            // which a "confirm whatever is current" OK would commit.
            let Some((group, name)) = state.pick() else {
                return;
            };
            picked.write(GroupPicked {
                requester,
                group,
                name,
            });
            // Clearing the requester first is what stops the close from being
            // read as an unanswered one; the close then ends the window, since
            // a keyed instance is despawned by it.
            state.requester = None;
            chrome.write(FloaterCommand {
                floater: window,
                op: FloaterOp::Close,
            });
        }
        PickerButton::Cancel => {
            state.requester = None;
            chrome.write(FloaterCommand {
                floater: window,
                op: FloaterOp::Close,
            });
        }
    }
}

/// Spawn one bordered translated button, tagged with what it does.
fn spawn_picker_button(
    commands: &mut Commands,
    parent: Entity,
    label_key: &'static str,
    action: PickerButton,
    tab_index: i32,
) -> Entity {
    commands
        .spawn((
            Button,
            action,
            TabIndex(tab_index),
            Node {
                padding: UiRect::axes(Val::Px(10.0), Val::Px(3.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(BUTTON_BORDER),
            BackgroundColor(BUTTON_BACKGROUND),
            Pickable::default(),
            Name::new(format!("group-picker:{label_key}")),
            ChildOf(parent),
        ))
        .observe(on_picker_button)
        .with_child((
            Text::default(),
            Translated::new(label_key),
            UiFont::Sans.at(PICKER_FONT_SIZE),
            TextColor(LABEL_COLOR),
            Pickable::IGNORE,
        ))
        .id()
}

/// Fire the directory group search for the field's current text — the same
/// query the Search floater's Groups category makes, with every maturity band
/// included (see the module header).
fn send_search(
    ui: &GroupPickerUi,
    fields: &Query<&EditableText>,
    state: &mut GroupPickerState,
    commands: &mut MessageWriter<SlCommand>,
) {
    if state.scope != GroupPickerScope::AnyGroup {
        return;
    }
    let Ok(field) = fields.get(ui.search_field) else {
        return;
    };
    let query_text = field.value().to_string().trim().to_owned();
    if query_text.is_empty() {
        return;
    }
    let query_id = QueryId::from(Uuid::new_v4());
    state.pending_query = Some(query_id);
    state.searched = false;
    commands.write(SlCommand(Command::DirFindQuery {
        query_id,
        query_text,
        flags: DirFindFlags::GROUPS
            .union(DirFindFlags::INC_PG)
            .union(DirFindFlags::INC_MATURE)
            .union(DirFindFlags::INC_ADULT),
        query_start: 0,
    }));
}

/// Open (or re-open) the picker window for whoever asked.
///
/// Keyed by the **opening window and the field together**: the control that
/// asked gets its own window, a second press on it finds that window rather
/// than stacking another, and the same control in a second instance of the
/// opening window gets a picker of its own.
fn handle_open_requests(
    mut opens: MessageReader<OpenGroupPicker>,
    mut floaters: KeyedFloaters,
    mut states: Query<&mut GroupPickerState>,
    parents: Query<&ChildOf>,
    openers: Query<(Entity, &Floater)>,
    mut commands: Commands,
) {
    let requests: Vec<OpenGroupPicker> = opens.read().cloned().collect();
    for open in requests {
        let (owner, key) = picker_identity(open.requester, &open.field, &parents, &openers);
        let opened = floaters.open(group_picker_floater_spec(), key);
        match opened {
            KeyedFloaterOpen::Spawned(handle) => {
                let ui = build_picker_content(&mut commands, &handle);
                let state = GroupPickerState {
                    requester: Some(open.requester),
                    allow_none: open.allow_none,
                    scope: open.scope,
                    ..GroupPickerState::default()
                };
                // Seeded here rather than after the insert: the components only
                // reach the world when this frame's commands flush, so a window
                // spawned now is not queryable yet.
                commands.entity(handle.root).insert((state, ui));
                if let Some(owner) = owner {
                    commands.entity(handle.root).insert(FloaterOwner(owner));
                }
            }
            KeyedFloaterOpen::Existing(window) => {
                if let Ok(mut state) = states.get_mut(window) {
                    state.requester = Some(open.requester);
                    state.scope = open.scope;
                    if state.allow_none != open.allow_none {
                        state.allow_none = open.allow_none;
                        state.rows.clear();
                    }
                    state.clear_selection();
                }
            }
        }
    }
}

/// Track each window's tab strip into that window's state, and show only the
/// chrome its scope allows: a member-only picker has no second source, so its
/// strip and search row are hidden and its tab is forced back to the
/// memberships.
fn bridge_picker_tabs(
    mut windows: Query<(&mut GroupPickerState, &GroupPickerUi)>,
    strips: Query<&TabStrip>,
    mut nodes: Query<&mut Node>,
) {
    for (mut state, ui) in &mut windows {
        let searchable = state.scope == GroupPickerScope::AnyGroup;
        if searchable {
            if let Ok(strip) = strips.get(ui.tab_strip)
                && let Some(tab) = TAB_ORDER.get(strip.active).copied()
                && state.tab != tab
            {
                state.tab = tab;
                state.searched = false;
                state.set_rows(Vec::new());
            }
        } else if state.tab != PickerTab::MyGroups {
            state.tab = PickerTab::MyGroups;
            state.set_rows(Vec::new());
        }
        let show = |entity: Entity, shown: bool, nodes: &mut Query<&mut Node>| {
            if let Ok(mut node) = nodes.get_mut(entity) {
                let wanted = if shown { Display::Flex } else { Display::None };
                if node.display != wanted {
                    node.display = wanted;
                }
            }
        };
        show(ui.tab_strip, searchable, &mut nodes);
        show(
            ui.search_row,
            searchable && state.tab == PickerTab::Search,
            &mut nodes,
        );
    }
}

/// Fold a directory reply into the rows of the window that asked — the one
/// whose `pending_query` the reply's id matches, since every window mints its
/// own.
///
/// Only the first page is taken. The Search floater pages this query because it
/// is a browsing surface; a picker is resolving a group somebody already named,
/// and a name that needs a second page needs a better name.
fn ingest_group_search_replies(
    mut events: MessageReader<SlEvent>,
    mut windows: Query<&mut GroupPickerState>,
    groups: Res<GroupsModel>,
) {
    let frame: Vec<SlEvent> = events.read().cloned().collect();
    for event in &frame {
        let SlSessionEvent::DirGroupsReply { query_id, results } = &event.0 else {
            continue;
        };
        let Some(mut state) = windows.iter_mut().find(|state| {
            state
                .pending_query
                .is_some_and(|pending| pending.get() == *query_id)
        }) else {
            continue;
        };
        state.pending_query = None;
        state.searched = true;
        let rows = results
            .iter()
            .filter(|result| !result.group_id.uuid().is_nil())
            .map(|result| search_row(result, &groups))
            .collect();
        state.set_rows(rows);
    }
}

/// One search result as a row. The member count trails the name in the dimmer
/// column, as the reference's group search shows it, and the **worn** group is
/// marked from the membership model — a search can return a group you are in,
/// and it should not look different there than it does on the other tab.
fn search_row(result: &DirGroupResult, groups: &GroupsModel) -> GroupPickerRow {
    GroupPickerRow {
        group: Some(result.group_id),
        name: result.group_name.clone(),
        detail: result.members.to_string(),
        active: groups.active() == Some(result.group_id),
    }
}

/// Keep every **My Groups** window's rows current from the membership model —
/// which is the whole content of that tab, so this is also what fills a window
/// that has just been spawned.
fn refresh_membership_rows(mut windows: Query<&mut GroupPickerState>, groups: Res<GroupsModel>) {
    for mut state in &mut windows {
        if state.tab != PickerTab::MyGroups {
            continue;
        }
        let rows = state.membership_rows(&groups);
        // Write-guarded inside `set_rows`: replacing the rows every frame would
        // defeat the revision-driven rebuild.
        state.set_rows(rows);
    }
}

/// Rebuild a window's visible list whenever its revision moved: despawn the old
/// rows and spawn one clickable row per group.
fn rebuild_picker_list(
    mut windows: Query<(&mut GroupPickerState, &GroupPickerUi)>,
    children: Query<&Children>,
    mut commands: Commands,
) {
    for (mut state, ui) in &mut windows {
        if state.built_revision == Some(state.revision) {
            continue;
        }
        state.built_revision = Some(state.revision);
        if let Ok(existing) = children.get(ui.list) {
            for child in existing {
                commands.entity(*child).despawn();
            }
        }
        // An empty list says why it is empty: a search that matched nothing is
        // otherwise indistinguishable from one that was never run, and an agent
        // in no groups from a membership list that has not arrived.
        if state.rows.is_empty() {
            let key = match state.tab {
                PickerTab::MyGroups => Some(NO_GROUPS_KEY),
                PickerTab::Search => state.searched.then_some(NOT_FOUND_KEY),
            };
            if let Some(key) = key {
                commands.spawn((
                    Text::default(),
                    Translated::new(key),
                    UiFont::Sans.at(PICKER_FONT_SIZE),
                    TextColor(DETAIL_COLOR),
                    Node {
                        padding: UiRect::axes(Val::Px(6.0), Val::Px(2.0)),
                        ..default()
                    },
                    Pickable::IGNORE,
                    Name::new("group-picker-empty"),
                    ChildOf(ui.list),
                ));
            }
        }
        for (index, row_data) in state.rows.iter().enumerate() {
            let selected = state.selected == Some(index);
            // The active (worn) group is drawn bold, as the reference draws it.
            let font = if row_data.active {
                UiFont::Sans
                    .at(PICKER_FONT_SIZE)
                    .with_font_weight(FontWeight::BOLD)
            } else {
                UiFont::Sans.at(PICKER_FONT_SIZE)
            };
            let label = row_data.name.clone();
            let detail = row_data.detail.clone();
            let is_none_row = row_data.group.is_none();
            commands
                .spawn((
                    Button,
                    Node {
                        padding: UiRect::axes(Val::Px(6.0), Val::Px(2.0)),
                        align_items: AlignItems::Center,
                        ..row(Val::Px(8.0))
                    },
                    BackgroundColor(if selected {
                        SELECTED_ROW_BACKGROUND
                    } else {
                        Color::NONE
                    }),
                    Pickable::default(),
                    // Numbered, so a test (and a person reading the entity
                    // tree) can tell one row from another.
                    Name::new(format!("group-picker-row:{index}")),
                    ChildOf(ui.list),
                ))
                .observe(
                    move |press: On<Pointer<Press>>,
                          mut windows: Query<&mut GroupPickerState>,
                          parents: Query<&ChildOf>,
                          floaters: Query<(Entity, &Floater)>| {
                        if press.button != PointerButton::Primary {
                            return;
                        }
                        // The row's own window, not "the" picker: two are up
                        // when two instanced windows are each picking a group.
                        let Some(window) = host_floater(press.entity, &parents, &floaters) else {
                            return;
                        };
                        let Ok(mut state) = windows.get_mut(window) else {
                            return;
                        };
                        state.select(index);
                    },
                )
                .with_children(|entry| {
                    let mut text = entry.spawn((
                        Text::new(label),
                        font,
                        TextColor(LABEL_COLOR),
                        Pickable::IGNORE,
                    ));
                    // The "none" row carries no group name, so it is labelled
                    // from the catalogue instead and follows the UI language.
                    if is_none_row {
                        text.insert(Translated::new(NONE_ROW_KEY));
                    }
                    if !detail.is_empty() {
                        entry.spawn((
                            Text::new(detail),
                            UiFont::Sans.at(PICKER_FONT_SIZE),
                            TextColor(DETAIL_COLOR),
                            Pickable::IGNORE,
                        ));
                    }
                });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        GroupKey, GroupPickerRow, GroupPickerScope, GroupPickerState, GroupsModel, PickerTab,
    };
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{DirGroupResult, GroupMembership, LandArea, TextureKey, Uuid};

    /// A group id that is only ever compared, never resolved.
    fn group(id: u128) -> GroupKey {
        GroupKey::from(Uuid::from_u128(id))
    }

    /// One membership record, as `AgentGroupDataUpdate` delivers it.
    fn membership(id: u128, name: &str) -> GroupMembership {
        GroupMembership {
            group_id: group(id),
            group_powers: 0,
            accept_notices: true,
            group_insignia_id: TextureKey::from(Uuid::nil()),
            contribution: LandArea(0),
            group_name: name.to_owned(),
        }
    }

    /// A model holding three groups, the second of them worn.
    fn model() -> GroupsModel {
        let mut model = GroupsModel::default();
        model.apply_memberships(&[
            membership(1, "Builders"),
            membership(2, "Anglers"),
            membership(3, "Cartographers"),
        ]);
        model.set_active(Some(group(2)), "Angler");
        model
    }

    /// A member-only picker in the given "none" mode, filled from the model.
    fn picker(allow_none: bool) -> GroupPickerState {
        let mut state = GroupPickerState {
            allow_none,
            ..GroupPickerState::default()
        };
        let rows = state.membership_rows(&model());
        state.set_rows(rows);
        state
    }

    /// The My Groups list is the agent's groups in name order, with the worn one
    /// marked — the reference's sorted list with its bold row.
    #[test]
    fn the_list_is_the_agents_groups_in_name_order() {
        let state = picker(false);
        assert_eq!(
            state
                .rows
                .iter()
                .map(|row| row.name.clone())
                .collect::<Vec<_>>(),
            vec![
                "Anglers".to_owned(),
                "Builders".to_owned(),
                "Cartographers".to_owned(),
            ]
        );
        assert_eq!(
            state.rows.iter().map(|row| row.active).collect::<Vec<_>>(),
            vec![true, false, false]
        );
    }

    /// A picker that allows **none** puts that row first, above the groups —
    /// where the reference adds it (`ADD_TOP`). One that does not never builds
    /// it, so a list the null group cannot go on cannot be given it.
    #[test]
    fn the_none_row_is_first_and_only_where_it_was_allowed() {
        let with_none = picker(true);
        assert_eq!(with_none.rows.first().map(|row| row.group), Some(None));
        assert_eq!(with_none.rows.len(), 4);
        let without = picker(false);
        assert!(without.rows.iter().all(|row| row.group.is_some()));
        assert_eq!(without.rows.len(), 3);
    }

    /// Nothing is selected to begin with, and OK with nothing selected has no
    /// answer to send — the bold worn group is a hint, not a pick.
    #[test]
    fn nothing_is_picked_until_a_row_is_clicked() {
        let mut state = picker(true);
        assert_eq!(state.selected, None);
        assert_eq!(state.pick(), None);
        state.select(2);
        assert_eq!(state.pick(), Some((Some(group(1)), "Builders".to_owned())));
    }

    /// The "none" row answers with no group, and with no name for a consumer to
    /// show.
    #[test]
    fn the_none_row_answers_with_no_group() {
        let mut state = picker(true);
        state.select(0);
        assert_eq!(state.pick(), Some((None, String::new())));
    }

    /// A click past the end of the list is ignored rather than selecting a row
    /// that is not there (a stale click against rows that have just shrunk).
    #[test]
    fn a_click_past_the_last_row_is_ignored() {
        let mut state = picker(false);
        state.select(9);
        assert_eq!(state.selected, None);
    }

    /// Leaving a group while the picker is open re-orders the list under the
    /// person; the selection follows its **group**, not its index, so OK still
    /// confirms what the row said when it was clicked.
    #[test]
    fn a_membership_change_carries_the_selection_by_group() {
        let mut state = picker(false);
        state.select(2);
        assert_eq!(state.pick().map(|pick| pick.0), Some(Some(group(3))));
        // "Anglers" is gone, so "Cartographers" moves up a row.
        let mut model = model();
        model.remove(group(2));
        let rows = state.membership_rows(&model);
        state.set_rows(rows);
        assert_eq!(state.selected, Some(1));
        assert_eq!(state.pick().map(|pick| pick.0), Some(Some(group(3))));
    }

    /// A selection whose group has gone drops rather than sliding onto whatever
    /// row took its place — confirming a group the person never chose is the
    /// one failure a picker must not have.
    #[test]
    fn a_selection_whose_group_left_is_dropped() {
        let mut state = picker(false);
        state.select(0);
        let mut model = model();
        model.remove(group(2));
        let rows = state.membership_rows(&model);
        state.set_rows(rows);
        assert_eq!(state.selected, None);
        assert_eq!(state.pick(), None);
    }

    /// Re-opening the picker for a second control starts from nothing selected.
    #[test]
    fn clearing_the_selection_leaves_no_answer() {
        let mut state = picker(true);
        state.select(1);
        state.clear_selection();
        assert_eq!(state.pick(), None);
    }

    /// An identical refresh does not bump the revision, so the list is not
    /// rebuilt every frame under a model that has not moved.
    #[test]
    fn an_unchanged_refresh_does_not_rebuild() {
        let mut state = picker(false);
        let revision = state.revision;
        let rows = state.membership_rows(&model());
        state.set_rows(rows);
        assert_eq!(state.revision, revision);
    }

    /// An agent in no groups gets an empty list rather than a phantom row —
    /// what the "no groups" line is shown for.
    #[test]
    fn an_agent_in_no_groups_has_an_empty_list() {
        let mut state = GroupPickerState::default();
        let rows = state.membership_rows(&GroupsModel::default());
        state.set_rows(rows);
        assert_eq!(state.rows, Vec::<GroupPickerRow>::new());
    }

    /// One directory result, as `DirGroupsReply` delivers it.
    fn result(id: u128, name: &str, members: i32) -> DirGroupResult {
        DirGroupResult {
            group_id: group(id),
            group_name: name.to_owned(),
            members,
            search_order: 0.0,
        }
    }

    /// A search row trails the member count and marks the worn group, so a
    /// group found by search reads the same as the same group found on the
    /// memberships tab.
    #[test]
    fn a_search_row_carries_the_member_count_and_the_worn_mark() {
        let model = model();
        let stranger = super::search_row(&result(9, "Somebody Else's Club", 12), &model);
        assert_eq!(stranger.name, "Somebody Else's Club");
        assert_eq!(stranger.detail, "12");
        assert!(!stranger.active);
        // Group 2 is the worn one, whichever source found it.
        assert!(super::search_row(&result(2, "Anglers", 3), &model).active);
    }

    /// A search is only ever the second source: a picker whose caller can only
    /// use the agent's own groups shows the memberships and nothing else.
    #[test]
    fn a_member_only_picker_stays_on_its_own_groups() {
        let state = picker(false);
        assert_eq!(state.scope, GroupPickerScope::MemberGroups);
        assert_eq!(state.tab, PickerTab::MyGroups);
    }

    /// The window itself, under the real pointer and floater stack — the half
    /// the state tests above cannot reach, and the half that catches a system
    /// scheduled without the messages or resources its parameters name.
    mod windows {
        use super::super::{GroupPickerPlugin, GroupPickerState, GroupPickerUi};
        use super::{group, model};
        use crate::ui::{UiRoot, UiScaffoldSystems};
        use crate::world_api::{GroupPicked, GroupsModel, OpenGroupPicker};
        use bevy::prelude::*;
        use pretty_assertions::assert_eq;
        use sl_client_bevy::{SlCommand, SlEvent};
        use sl_viewer_testkit::interact::{self, InteractionTest};
        use sl_viewer_testkit::{drain, record, settle};

        /// A boxed error so the tests use `?` rather than the disallowed
        /// `unwrap` / `expect`.
        type TestError = Box<dyn core::error::Error>;

        /// The set-group button's node name.
        const SET_GROUP: &str = "test:set-group";

        /// A second set-group button, for a **different** field — the case the
        /// keying exists for.
        const OTHER_SET_GROUP: &str = "test:other-set-group";

        /// A stand-in for a caller's Set… button: pressing it opens the picker
        /// naming itself, exactly as About Land's and the build tool's do.
        #[derive(Component, Debug, Clone, Copy)]
        struct OpenButton(&'static str);

        /// Spawn one such button.
        fn spawn_open_button(commands: &mut Commands, root: Entity, name: &'static str) {
            commands
                .spawn((
                    Button,
                    Node {
                        width: Val::Px(60.0),
                        height: Val::Px(20.0),
                        ..default()
                    },
                    Pickable::default(),
                    OpenButton(name),
                    Name::new(name),
                    ChildOf(root),
                ))
                .observe(
                    |press: On<Pointer<Press>>,
                     buttons: Query<&OpenButton>,
                     mut opens: MessageWriter<OpenGroupPicker>| {
                        if let Ok(button) = buttons.get(press.entity) {
                            opens.write(OpenGroupPicker::new(press.entity, button.0));
                        }
                    },
                );
        }

        /// Two set-group buttons and the picker, under the real pointer stack.
        fn picker_app() -> App {
            let mut app = InteractionTest::new().build();
            app.add_message::<SlCommand>()
                .add_message::<SlEvent>()
                // The manager itself, not a stand-in: a picker window is
                // spawned, raised and — on OK / Cancel — despawned by it.
                .add_plugins((crate::floater::FloaterPlugin, GroupPickerPlugin))
                .insert_resource(model());
            record::<GroupPicked>(&mut app);
            app.add_systems(
                Startup,
                (|mut commands: Commands, root: Res<UiRoot>| {
                    spawn_open_button(&mut commands, root.0, SET_GROUP);
                    spawn_open_button(&mut commands, root.0, OTHER_SET_GROUP);
                })
                .after(UiScaffoldSystems::SpawnRoot),
            );
            settle(&mut app);
            settle(&mut app);
            app
        }

        /// The open picker windows.
        fn picker_windows(app: &mut App) -> Vec<Entity> {
            app.world_mut()
                .query_filtered::<Entity, With<GroupPickerState>>()
                .iter(app.world())
                .collect()
        }

        /// A press opens a picker listing the agent's groups; clicking a row
        /// and pressing OK answers with that row's group and closes.
        #[test]
        fn a_press_a_row_and_ok() -> Result<(), TestError> {
            let mut app = picker_app();
            assert!(
                picker_windows(&mut app).is_empty(),
                "no group is being picked yet"
            );

            interact::click_node(&mut app, SET_GROUP)?;
            settle(&mut app);
            let window = *picker_windows(&mut app)
                .first()
                .ok_or("the button opened no picker")?;
            let labels: Vec<String> = app
                .world()
                .get::<GroupPickerState>(window)
                .ok_or("the window carries no state")?
                .rows
                .iter()
                .map(|row| row.name.clone())
                .collect();
            assert_eq!(
                labels,
                vec![
                    String::new(),
                    "Anglers".to_owned(),
                    "Builders".to_owned(),
                    "Cartographers".to_owned(),
                ],
                "the none row, then the memberships by name"
            );

            // OK with nothing picked is not an answer.
            interact::click_node(&mut app, "group-picker:group-picker-ok")?;
            settle(&mut app);
            assert!(
                drain::<GroupPicked>(&mut app).is_empty(),
                "OK answered before a row was chosen"
            );
            assert_eq!(picker_windows(&mut app).len(), 1, "and it stayed open");

            interact::click_node(&mut app, "group-picker-row:2")?;
            settle(&mut app);
            interact::click_node(&mut app, "group-picker:group-picker-ok")?;
            settle(&mut app);

            let replies = drain::<GroupPicked>(&mut app);
            let reply = replies.last().ok_or("OK answered nothing")?;
            assert_eq!(reply.group, Some(group(1)));
            assert_eq!(reply.name, "Builders");
            assert!(
                picker_windows(&mut app).is_empty(),
                "OK closes the picker window"
            );
            Ok(())
        }

        /// Cancel answers nothing and closes — the difference from OK being
        /// that a consumer hears no pick at all, not a pick of "none".
        #[test]
        fn cancel_answers_nothing() -> Result<(), TestError> {
            let mut app = picker_app();
            interact::click_node(&mut app, SET_GROUP)?;
            settle(&mut app);
            interact::click_node(&mut app, "group-picker-row:1")?;
            settle(&mut app);
            interact::click_node(&mut app, "group-picker:group-picker-cancel")?;
            settle(&mut app);

            assert!(
                drain::<GroupPicked>(&mut app).is_empty(),
                "Cancel answered a pick"
            );
            assert!(picker_windows(&mut app).is_empty(), "Cancel closes it");
            Ok(())
        }

        /// **Two fields are two windows**
        /// (`viewer-audit-picker-requester-identity`): the parcel's group and
        /// the object's are two questions, and answering one must not close the
        /// other.
        #[test]
        fn two_fields_open_two_windows() -> Result<(), TestError> {
            let mut app = picker_app();
            interact::click_node(&mut app, SET_GROUP)?;
            settle(&mut app);
            interact::click_node(&mut app, OTHER_SET_GROUP)?;
            settle(&mut app);
            assert_eq!(
                picker_windows(&mut app).len(),
                2,
                "the second field reused the first window"
            );
            Ok(())
        }

        /// A member-only picker is the reference's single list: no tab strip,
        /// no search row. Both are built either way, so that a re-open for a
        /// caller that *can* search finds them there.
        #[test]
        fn a_member_only_picker_hides_the_search_chrome() -> Result<(), TestError> {
            let mut app = picker_app();
            interact::click_node(&mut app, SET_GROUP)?;
            settle(&mut app);
            let window = *picker_windows(&mut app)
                .first()
                .ok_or("the button opened no picker")?;
            let ui = app
                .world()
                .get::<GroupPickerUi>(window)
                .ok_or("the window carries no parts")?;
            let (strip, search_row) = (ui.tab_strip, ui.search_row);
            let displays: Vec<Display> = [strip, search_row]
                .into_iter()
                .filter_map(|entity| app.world().get::<Node>(entity))
                .map(|node| node.display)
                .collect();
            assert_eq!(
                displays,
                vec![Display::None, Display::None],
                "a picker that cannot search still showed its tabs / search row"
            );
            Ok(())
        }

        /// A membership that arrives while the picker is open lands in the
        /// list: the window reads the model every frame rather than copying it
        /// once at open.
        #[test]
        fn a_group_joined_while_open_joins_the_list() -> Result<(), TestError> {
            let mut app = picker_app();
            interact::click_node(&mut app, SET_GROUP)?;
            settle(&mut app);
            app.world_mut()
                .resource_mut::<GroupsModel>()
                .remove(group(1));
            settle(&mut app);
            let window = *picker_windows(&mut app)
                .first()
                .ok_or("the button opened no picker")?;
            let labels: Vec<String> = app
                .world()
                .get::<GroupPickerState>(window)
                .ok_or("the window carries no state")?
                .rows
                .iter()
                .map(|row| row.name.clone())
                .collect();
            assert!(
                !labels.iter().any(|name| name == "Builders"),
                "a group the agent left is still offered: {labels:?}"
            );
            Ok(())
        }
    }
}
