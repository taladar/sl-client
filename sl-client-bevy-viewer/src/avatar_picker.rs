//! The reusable **avatar picker** floater (`viewer-inventory-share-picker`):
//! "Choose Resident" — search by name, pick from friends, or pick from the
//! avatars near you — the reference viewer's `LLFloaterAvatarPicker`
//! (`floater_avatar_picker.xml`), the dialog behind Share, Pay, group
//! invites and teleport offers.
//!
//! # Reusable by requester tag
//!
//! A feature opens the picker with [`OpenAvatarPicker`] carrying its own
//! `requester` tag; when the user confirms, the picker emits
//! [`AvatarPicked`] with the same tag, and only the requesting feature acts
//! on it — the same out-of-band shape as the context-menu targets. The first
//! consumer is the inventory context menu's **Share** entry
//! ([`crate::inventory_actions`]).
//!
//! # One resident or several
//!
//! A request opens the picker in one of two modes — [`OpenAvatarPicker::one`]
//! or [`OpenAvatarPicker::many`], the reference's `allow_multiple` flag. In the
//! many mode the results list takes `Ctrl` (toggle) and `Shift` (range) clicks,
//! through the same [`crate::ui_table::apply_selection_click`] algebra the table
//! widget uses. Either way the answer is the *same* message: [`AvatarPicked`]
//! carries a list of [`PickedAvatar`], and a one-resident request simply answers
//! with one element — so no consumer needs a second channel, and a consumer that
//! only ever wants one reads [`AvatarPicked::first`].
//!
//! # The three sources
//!
//! - **Search** — the grid's name lookup. Typed text goes out as
//!   [`Command::AvatarPickerRequest`], which the runtimes drive over the
//!   **`AvatarPickerSearch` capability** where the region has it (matching the
//!   username and display name as well as the legacy name) and over the legacy
//!   `AvatarPickerRequest` message otherwise; either way the answer arrives as
//!   `AvatarPickerReply`. Text that parses as a **uuid** is looked up by id
//!   through `GetDisplayNames` instead, as the reference does — pasting a key
//!   is a normal way to name someone. A search that matches nobody says so
//!   rather than leaving a blank list.
//! - **Friends** — the held friends roster ([`crate::world_api::FriendsModel`]).
//! - **Near Me** — the avatars this viewer currently knows in-world
//!   ([`crate::avatars::AvatarState`]), sorted by distance from the own
//!   avatar (the reference's radius slider is folded into the sort — the
//!   nearest are on top).
//!
//! Reference (Firestorm, read-only): `llfloateravatarpicker.cpp`,
//! `floater_avatar_picker.xml`.

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::text::EditableText;
use sl_client_bevy::{
    AgentKey, AvatarPickerResult, Command, QueryId, SlCommand, SlEvent, SlIdentity, SlSessionEvent,
    Uuid,
};

use crate::avatars::AvatarState;
use crate::floater::{FloaterCaps, FloaterSpec, spawn_floater};
use crate::i18n::Translated;
use crate::ui::{UiPanelShown, UiRoot, UiScaffoldSystems, column, row};
use crate::ui_font::UiFont;
use crate::ui_tab::{DEFAULT_ELLIPSIS, TabPlacement, TabSpec, TabStrip, spawn_tab_strip};
use crate::world_api::FriendsModel;

/// The floater's [`crate::floater::FloaterSpec::id`].
const PICKER_FLOATER_ID: &str = "avatar-picker";

/// The picker font size, in logical pixels.
const PICKER_FONT_SIZE: f32 = 14.0;

/// The label colour.
const LABEL_COLOR: Color = Color::srgb(0.90, 0.92, 0.96);

/// A button's background / border.
const BUTTON_BACKGROUND: Color = Color::srgb(0.13, 0.15, 0.20);
/// A button's border colour.
const BUTTON_BORDER: Color = Color::srgb(0.34, 0.40, 0.52);

/// The Fluent key for the "nobody matched" row.
const NOT_FOUND_KEY: &str = "avatar-picker-not-found";

/// The username column's colour — dimmer than the name it trails.
const USERNAME_COLOR: Color = Color::srgb(0.62, 0.66, 0.74);

/// A selected result row's background.
const SELECTED_ROW_BACKGROUND: Color = Color::srgba(0.24, 0.34, 0.52, 0.55);

/// The result list's viewport height, in logical pixels.
const LIST_HEIGHT: f32 = 220.0;

/// The most rows shown per source (a search reply is bounded upstream; the
/// friends / nearby lists are clamped the same so the plain column stays
/// cheap).
const MAX_ROWS: usize = 100;

/// Ask the picker to open for a feature. `requester` tags the eventual
/// [`AvatarPicked`] so only the asking feature consumes it.
#[derive(Message, Debug, Clone, Copy)]
pub(crate) struct OpenAvatarPicker {
    /// The feature tag echoed back in [`AvatarPicked`].
    pub(crate) requester: &'static str,
    /// Whether the user may choose several residents at once — the reference's
    /// `allow_multiple`. Build one with [`OpenAvatarPicker::one`] or
    /// [`OpenAvatarPicker::many`] rather than by hand, so the choice reads at
    /// the call site.
    pub(crate) allow_multiple: bool,
}

impl OpenAvatarPicker {
    /// Ask for exactly one resident.
    pub(crate) const fn one(requester: &'static str) -> Self {
        Self {
            requester,
            allow_multiple: false,
        }
    }

    /// Ask for any number of residents at once.
    pub(crate) const fn many(requester: &'static str) -> Self {
        Self {
            requester,
            allow_multiple: true,
        }
    }
}

/// One resident the picker returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PickedAvatar {
    /// The chosen avatar.
    pub(crate) agent: AgentKey,
    /// The label the picked row carried — the avatar's name as the source that
    /// produced the row knew it (a search reply's legacy name, the friend's
    /// name, or the nearby avatar's name). Consumers that must *record* a name
    /// against the id (the block list writes it into the mute entry) take it
    /// from here rather than re-resolving.
    pub(crate) name: String,
}

/// The confirmed pick: every chosen resident, in list order. A picker opened
/// with [`OpenAvatarPicker::one`] answers with exactly one element.
#[derive(Message, Debug, Clone)]
pub(crate) struct AvatarPicked {
    /// The tag of the feature that opened the picker.
    pub(crate) requester: &'static str,
    /// The chosen residents — never empty (the picker does not confirm an empty
    /// selection).
    pub(crate) picks: Vec<PickedAvatar>,
}

impl AvatarPicked {
    /// The first chosen resident — for a single-resident requester, *the* pick.
    pub(crate) fn first(&self) -> Option<&PickedAvatar> {
        self.picks.first()
    }
}

/// Which source tab is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum PickerTab {
    /// The wire name search.
    #[default]
    Search,
    /// The friends roster.
    Friends,
    /// The known in-world avatars, nearest first.
    NearMe,
}

/// The tab order, matching the spawned strip.
const TAB_ORDER: [PickerTab; 3] = [PickerTab::Search, PickerTab::Friends, PickerTab::NearMe];

/// One selectable result row.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PickerRow {
    /// The avatar the row names.
    agent: AgentKey,
    /// The row label — the avatar's **display name** where the source knows one
    /// (the modern search), else its legacy `First Last`.
    label: String,
    /// The dimmer trailing column: the avatar's **username** / SLID, empty when
    /// the source has none (the friends / nearby lists, and the legacy search
    /// message, which carries only the legacy pair). The reference's picker
    /// shows the same two columns.
    username: String,
}

/// The picker's live state.
#[derive(Resource, Debug, Default)]
pub(crate) struct AvatarPickerState {
    /// Who asked for the picker (None while closed).
    requester: Option<&'static str>,
    /// The active source tab.
    tab: PickerTab,
    /// The current rows, top to bottom.
    rows: Vec<PickerRow>,
    /// The selected row indices, ascending. At most one unless `allow_multiple`.
    selected: Vec<usize>,
    /// The range anchor a `Shift`+click ranges from — the last row plainly
    /// clicked or `Ctrl`-toggled on.
    anchor: Option<usize>,
    /// Whether this request lets the user choose several residents.
    allow_multiple: bool,
    /// The in-flight search query id, so a stale reply is ignored.
    pending_query: Option<QueryId>,
    /// Bumped whenever `rows` / `selected` change, driving the list rebuild.
    revision: u64,
    /// The agent a **by-uuid** search is waiting on, so its `GetDisplayNames`
    /// reply — which carries no query id — is recognised as this search's
    /// answer and not as some other feature's name lookup.
    pending_agent: Option<AgentKey>,
    /// Whether a search has answered since the picker opened — what tells an
    /// empty list "nobody matched" from "nothing has been searched for yet",
    /// which is the difference between a *result* and a blank panel.
    searched: bool,
}

impl AvatarPickerState {
    /// Replace the rows and clear the selection.
    fn set_rows(&mut self, rows: Vec<PickerRow>) {
        self.rows = rows;
        self.rows.truncate(MAX_ROWS);
        self.selected.clear();
        self.anchor = None;
        self.revision = self.revision.wrapping_add(1);
    }

    /// Apply a click on a row under the held modifier keys — the table widget's
    /// selection algebra, so a modified click means the same thing here as it
    /// does in every list that *is* a table.
    fn select(&mut self, index: usize, ctrl: bool, shift: bool) {
        if index >= self.rows.len() {
            return;
        }
        let before = self.selected.clone();
        crate::ui_table::apply_selection_click(
            &mut self.selected,
            &mut self.anchor,
            index,
            self.allow_multiple,
            ctrl,
            shift,
        );
        if self.selected != before {
            self.revision = self.revision.wrapping_add(1);
        }
    }

    /// Replace the rows, carrying the selection across by **agent** rather than
    /// by row index: the Near Me list re-sorts as people move, so whoever is
    /// still listed stays picked and whoever left drops out (a selected row that
    /// is no longer shown would confirm invisibly).
    fn set_rows_keeping_selection(&mut self, rows: Vec<PickerRow>) {
        let selected: Vec<AgentKey> = self
            .selected
            .iter()
            .filter_map(|index| self.rows.get(*index))
            .map(|row| row.agent)
            .collect();
        let anchor = self
            .anchor
            .and_then(|index| self.rows.get(index))
            .map(|row| row.agent);
        self.set_rows(rows);
        self.selected = selected
            .iter()
            .filter_map(|agent| self.rows.iter().position(|row| row.agent == *agent))
            .collect();
        self.selected.sort_unstable();
        self.anchor = anchor.and_then(|agent| self.rows.iter().position(|row| row.agent == agent));
    }

    /// The chosen residents, top to bottom — what [`AvatarPicked`] carries.
    fn picks(&self) -> Vec<PickedAvatar> {
        self.selected
            .iter()
            .filter_map(|index| self.rows.get(*index))
            .map(|row| PickedAvatar {
                agent: row.agent,
                name: row.label.clone(),
            })
            .collect()
    }
}

/// Entity handles for the picker's parts.
#[derive(Resource)]
pub(crate) struct AvatarPickerUi {
    /// The floater root (carries [`UiPanelShown`]).
    panel: Entity,
    /// The source tab strip.
    tab_strip: Entity,
    /// The search text field.
    search_field: Entity,
    /// The search row (hidden on the non-search tabs).
    search_row: Entity,
    /// The result list container.
    list: Entity,
}

/// The plugin owning the avatar picker.
pub(crate) struct AvatarPickerPlugin;

impl Plugin for AvatarPickerPlugin {
    /// Register the messages, state and systems, and spawn the floater.
    fn build(&self, app: &mut App) {
        app.init_resource::<AvatarPickerState>()
            .add_message::<OpenAvatarPicker>()
            .add_message::<AvatarPicked>()
            .add_systems(
                Startup,
                spawn_picker_floater.after(UiScaffoldSystems::SpawnRoot),
            )
            .add_systems(
                Update,
                (
                    handle_open_requests,
                    bridge_picker_tabs,
                    ingest_picker_replies,
                    ingest_picker_id_lookups,
                    refresh_local_sources,
                    rebuild_picker_list,
                )
                    .chain(),
            );
    }
}

/// Spawn the picker floater (hidden): the source tabs, the search row, the
/// result list, and the OK / Cancel row.
fn spawn_picker_floater(mut commands: Commands, root: Res<UiRoot>) {
    let handle = spawn_floater(
        &mut commands,
        root.0,
        FloaterSpec {
            id: PICKER_FLOATER_ID,
            title: "Choose Resident".to_owned(),
            position: Vec2::new(320.0, 120.0),
            default_size: None,
            min_size: None,
            dock_host: None,
            caps: FloaterCaps {
                resizable: false,
                minimizable: false,
                closable: true,
                dockable: false,
            },
        },
    );
    commands
        .entity(handle.title_text)
        .insert(Translated::new("avatar-picker-title"));
    let content = handle.content;

    let tab_labels: [String; 3] = [
        "avatar-picker-tab-search".to_owned(),
        "avatar-picker-tab-friends".to_owned(),
        "avatar-picker-tab-near-me".to_owned(),
    ];
    let tab_strip = spawn_tab_strip(
        &mut commands,
        content,
        &TabSpec {
            element: "avatar-picker-tabs",
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
        &mut commands,
        search_row,
        &crate::ui_text_input::TextInputSpec {
            font_size: PICKER_FONT_SIZE,
            width_glyphs: 18.0,
            tab_index: 2,
            ..crate::ui_text_input::TextInputSpec::new(
                "avatar-picker-search",
                crate::ui_text_input::TextInputKind::Line,
            )
        },
    );
    let go = spawn_picker_button(&mut commands, search_row, "avatar-picker-go", 3);
    commands.entity(go).observe(
        |press: On<Pointer<Press>>,
         ui: Option<Res<AvatarPickerUi>>,
         fields: Query<&EditableText>,
         mut state: ResMut<AvatarPickerState>,
         mut commands: MessageWriter<SlCommand>| {
            if press.button != PointerButton::Primary {
                return;
            }
            let Some(ui) = ui else {
                return;
            };
            send_search(&ui, &fields, &mut state, &mut commands);
        },
    );

    // The result list: a fixed-height clipped column the rebuild fills.
    let list = commands
        .spawn((
            Node {
                height: Val::Px(LIST_HEIGHT),
                overflow: Overflow::clip(),
                ..column(Val::Px(2.0))
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.25)),
            ChildOf(content),
        ))
        .id();

    // OK / Cancel.
    let buttons = commands
        .spawn((
            Node {
                ..row(Val::Px(8.0))
            },
            ChildOf(content),
        ))
        .id();
    let ok = spawn_picker_button(&mut commands, buttons, "avatar-picker-ok", 4);
    commands.entity(ok).observe(
        |press: On<Pointer<Press>>,
         ui: Option<Res<AvatarPickerUi>>,
         mut state: ResMut<AvatarPickerState>,
         mut panels: Query<&mut UiPanelShown>,
         mut picked: MessageWriter<AvatarPicked>| {
            if press.button != PointerButton::Primary {
                return;
            }
            let Some(ui) = ui else {
                return;
            };
            confirm_pick(&ui, &mut state, &mut panels, &mut picked);
        },
    );
    let cancel = spawn_picker_button(&mut commands, buttons, "avatar-picker-cancel", 5);
    commands.entity(cancel).observe(
        |press: On<Pointer<Press>>,
         ui: Option<Res<AvatarPickerUi>>,
         mut state: ResMut<AvatarPickerState>,
         mut panels: Query<&mut UiPanelShown>| {
            if press.button != PointerButton::Primary {
                return;
            }
            let Some(ui) = ui else {
                return;
            };
            state.requester = None;
            if let Ok(mut shown) = panels.get_mut(ui.panel) {
                shown.0 = false;
            }
        },
    );

    commands.insert_resource(AvatarPickerUi {
        panel: handle.root,
        tab_strip,
        search_field,
        search_row,
        list,
    });
}

/// Spawn one bordered translated button.
fn spawn_picker_button(
    commands: &mut Commands,
    parent: Entity,
    label_key: &'static str,
    tab_index: i32,
) -> Entity {
    commands
        .spawn((
            Button,
            TabIndex(tab_index),
            Node {
                padding: UiRect::axes(Val::Px(10.0), Val::Px(3.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(BUTTON_BORDER),
            BackgroundColor(BUTTON_BACKGROUND),
            Pickable::default(),
            Name::new(format!("avatar-picker:{label_key}")),
            ChildOf(parent),
        ))
        .with_child((
            Text::default(),
            Translated::new(label_key),
            UiFont::Sans.at(PICKER_FONT_SIZE),
            TextColor(LABEL_COLOR),
            Pickable::IGNORE,
        ))
        .id()
}

/// Fire the name search for the field's current text.
///
/// Text that parses as a **uuid** is looked up by id instead
/// (`Command::RequestDisplayNames`, the `GetDisplayNames` capability), the
/// reference's own split (`llfloateravatarpicker.cpp`, `findByIdCoro`) — pasting
/// a key is a normal way to name someone you cannot spell, and the name search
/// would never match one. That reply arrives as
/// [`SlSessionEvent::DisplayNames`] rather than a picker reply, so it carries no
/// query id: the pending query is cleared and the id itself remembered instead.
fn send_search(
    ui: &AvatarPickerUi,
    fields: &Query<&EditableText>,
    state: &mut AvatarPickerState,
    commands: &mut MessageWriter<SlCommand>,
) {
    let Ok(field) = fields.get(ui.search_field) else {
        return;
    };
    let name = field.value().to_string().trim().to_owned();
    if name.is_empty() {
        return;
    }
    state.searched = false;
    if let Ok(id) = Uuid::parse_str(&name) {
        let agent = AgentKey::from(id);
        state.pending_query = None;
        state.pending_agent = Some(agent);
        state.set_rows(Vec::new());
        commands.write(SlCommand(Command::RequestDisplayNames(vec![agent])));
        return;
    }
    let query_id = QueryId::from(Uuid::new_v4());
    state.pending_query = Some(query_id);
    state.pending_agent = None;
    commands.write(SlCommand(Command::AvatarPickerRequest { query_id, name }));
}

/// Confirm the selection: emit [`AvatarPicked`] to the requester and close.
fn confirm_pick(
    ui: &AvatarPickerUi,
    state: &mut AvatarPickerState,
    panels: &mut Query<&mut UiPanelShown>,
    picked: &mut MessageWriter<AvatarPicked>,
) {
    let Some(requester) = state.requester else {
        return;
    };
    let picks = state.picks();
    if picks.is_empty() {
        return;
    }
    picked.write(AvatarPicked { requester, picks });
    state.requester = None;
    if let Ok(mut shown) = panels.get_mut(ui.panel) {
        shown.0 = false;
    }
}

/// Open the picker when a feature asks for it.
fn handle_open_requests(
    mut opens: MessageReader<OpenAvatarPicker>,
    ui: Option<Res<AvatarPickerUi>>,
    mut state: ResMut<AvatarPickerState>,
    mut panels: Query<&mut UiPanelShown>,
) {
    let Some(ui) = ui else {
        return;
    };
    for open in opens.read() {
        state.requester = Some(open.requester);
        state.allow_multiple = open.allow_multiple;
        state.searched = false;
        state.set_rows(Vec::new());
        if let Ok(mut shown) = panels.get_mut(ui.panel) {
            shown.0 = true;
        }
    }
}

/// Track the tab strip's active tab into the state.
fn bridge_picker_tabs(
    ui: Option<Res<AvatarPickerUi>>,
    strips: Query<&TabStrip, Changed<TabStrip>>,
    mut state: ResMut<AvatarPickerState>,
    mut nodes: Query<&mut Node>,
) {
    let Some(ui) = ui else {
        return;
    };
    let Ok(strip) = strips.get(ui.tab_strip) else {
        return;
    };
    let Some(tab) = TAB_ORDER.get(strip.active).copied() else {
        return;
    };
    if state.tab != tab {
        state.tab = tab;
        state.searched = false;
        state.set_rows(Vec::new());
    }
    // The search row only applies to the Search tab.
    if let Ok(mut node) = nodes.get_mut(ui.search_row) {
        node.display = if tab == PickerTab::Search {
            Display::Flex
        } else {
            Display::None
        };
    }
}

/// The label a search match shows: its **display name** when the grid supplied
/// one (the modern capability), else the legacy `First Last` pair — the same
/// fallback the reference's `LLAvatarName` makes, so a resident who never set a
/// display name still reads normally.
fn picker_label(result: &AvatarPickerResult) -> String {
    if result.display_name.is_empty() {
        format!("{} {}", result.first_name, result.last_name)
            .trim()
            .to_owned()
    } else {
        result.display_name.clone()
    }
}

/// Fold a search reply into the rows (ignoring stale query ids).
fn ingest_picker_replies(mut events: MessageReader<SlEvent>, mut state: ResMut<AvatarPickerState>) {
    for event in events.read() {
        if let SlSessionEvent::AvatarPickerReply { query_id, results } = &event.0 {
            let expected = state
                .pending_query
                .is_some_and(|pending| pending.get() == *query_id);
            if !expected {
                continue;
            }
            state.pending_query = None;
            // A nil id with no name is the legacy message's "no matches"
            // sentinel; it must not become a row that looks pickable.
            let rows = results
                .iter()
                .filter(|result| !result.avatar_id.uuid().is_nil())
                .map(|result| PickerRow {
                    label: picker_label(result),
                    agent: result.avatar_id,
                    username: result.username.clone(),
                })
                .collect();
            state.set_rows(rows);
            state.searched = true;
        }
    }
}

/// Fold a **by-uuid** search's `GetDisplayNames` reply into the rows: the one
/// record it asked about, if the grid resolved it. An id the grid could not
/// resolve comes back flagged `missing`, which is a "not found", not a row.
fn ingest_picker_id_lookups(
    mut events: MessageReader<SlEvent>,
    mut state: ResMut<AvatarPickerState>,
) {
    for event in events.read() {
        let SlSessionEvent::DisplayNames(names) = &event.0 else {
            continue;
        };
        let Some(agent) = state.pending_agent else {
            continue;
        };
        let Some(record) = names.iter().find(|name| name.id == agent) else {
            continue;
        };
        state.pending_agent = None;
        state.searched = true;
        if record.missing {
            state.set_rows(Vec::new());
            continue;
        }
        let label = if record.display_name.is_empty() {
            format!("{} {}", record.legacy_first_name, record.legacy_last_name)
                .trim()
                .to_owned()
        } else {
            record.display_name.clone()
        };
        state.set_rows(vec![PickerRow {
            agent,
            label,
            username: record.username.clone(),
        }]);
    }
}

/// Keep the Friends / Near Me tabs' rows current from their local sources.
fn refresh_local_sources(
    ui: Option<Res<AvatarPickerUi>>,
    panels: Query<&UiPanelShown>,
    friends: Res<FriendsModel>,
    avatars: Res<AvatarState>,
    identity: Option<Res<SlIdentity>>,
    transforms: Query<&GlobalTransform>,
    mut state: ResMut<AvatarPickerState>,
) {
    let Some(ui) = ui else {
        return;
    };
    // Only while open, and only for the locally-sourced tabs.
    let open = panels.get(ui.panel).is_ok_and(|shown| shown.0);
    if !open {
        return;
    }
    let own = identity.and_then(|identity| identity.agent_id);
    let rows: Vec<PickerRow> = match state.tab {
        PickerTab::Search => return,
        PickerTab::Friends => friends
            .roster()
            .into_iter()
            .map(|(agent, name)| PickerRow {
                agent,
                label: name,
                username: String::new(),
            })
            .collect(),
        PickerTab::NearMe => {
            let own_position = own
                .and_then(|agent| avatars.root_entity_of(agent))
                .and_then(|entity| transforms.get(entity).ok())
                .map(|transform| transform.translation());
            let mut with_distance: Vec<(f32, PickerRow)> = avatars
                .known_agents()
                .into_iter()
                .filter(|(agent, _entity)| Some(*agent) != own)
                .map(|(agent, entity)| {
                    let distance = match (
                        own_position,
                        transforms.get(entity).ok().map(|t| t.translation()),
                    ) {
                        (Some(own_at), Some(at)) => own_at.distance(at),
                        _unknown => f32::MAX,
                    };
                    let name = avatars
                        .name_of(agent)
                        .map_or_else(|| "(resolving)".to_owned(), str::to_owned);
                    (
                        distance,
                        PickerRow {
                            agent,
                            label: name,
                            username: String::new(),
                        },
                    )
                })
                .collect();
            with_distance.sort_by(|a, b| a.0.total_cmp(&b.0));
            with_distance.into_iter().map(|(_d, row)| row).collect()
        }
    };
    // Write-guarded: replacing the rows every frame would defeat the
    // revision-driven rebuild.
    if rows != state.rows {
        state.set_rows_keeping_selection(rows);
    }
}

/// Rebuild the visible list whenever the state's revision moved: despawn the
/// old rows and spawn one clickable row per result.
fn rebuild_picker_list(
    ui: Option<Res<AvatarPickerUi>>,
    state: Res<AvatarPickerState>,
    mut last_revision: Local<Option<u64>>,
    children: Query<&Children>,
    mut commands: Commands,
) {
    let Some(ui) = ui else {
        return;
    };
    if *last_revision == Some(state.revision) {
        return;
    }
    *last_revision = Some(state.revision);
    if let Ok(existing) = children.get(ui.list) {
        for child in existing {
            commands.entity(*child).despawn();
        }
    }
    // A search that answered nobody says so. An empty list is otherwise
    // indistinguishable from a search that never ran — the reference shows a
    // "not found" row for exactly this reason.
    if state.rows.is_empty() && state.searched && state.tab == PickerTab::Search {
        commands.spawn((
            Text::default(),
            Translated::new(NOT_FOUND_KEY),
            UiFont::Sans.at(PICKER_FONT_SIZE),
            TextColor(USERNAME_COLOR),
            Node {
                padding: UiRect::axes(Val::Px(6.0), Val::Px(2.0)),
                ..default()
            },
            Pickable::IGNORE,
            Name::new("avatar-picker-not-found"),
            ChildOf(ui.list),
        ));
    }
    for (index, row_data) in state.rows.iter().enumerate() {
        let selected = state.selected.contains(&index);
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
                Name::new("avatar-picker-row"),
                ChildOf(ui.list),
            ))
            .observe(
                move |press: On<Pointer<Press>>,
                      keyboard: Res<ButtonInput<KeyCode>>,
                      mut state: ResMut<AvatarPickerState>| {
                    if press.button != PointerButton::Primary {
                        return;
                    }
                    let ctrl = keyboard.pressed(KeyCode::ControlLeft)
                        || keyboard.pressed(KeyCode::ControlRight);
                    let shift = keyboard.pressed(KeyCode::ShiftLeft)
                        || keyboard.pressed(KeyCode::ShiftRight);
                    state.select(index, ctrl, shift);
                },
            )
            .with_children(|row| {
                row.spawn((
                    Text::new(row_data.label.clone()),
                    UiFont::Sans.at(PICKER_FONT_SIZE),
                    TextColor(LABEL_COLOR),
                    Pickable::IGNORE,
                ));
                // The username column, only where the source knows one.
                if !row_data.username.is_empty() {
                    row.spawn((
                        Text::new(row_data.username.clone()),
                        UiFont::Sans.at(PICKER_FONT_SIZE),
                        TextColor(USERNAME_COLOR),
                        Pickable::IGNORE,
                    ));
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use super::{AgentKey, AvatarPickerResult, AvatarPickerState, PickerRow, Uuid};
    use pretty_assertions::assert_eq;

    /// An agent id that is only ever compared, never resolved.
    fn agent(id: u128) -> AgentKey {
        AgentKey::from(Uuid::from_u128(id))
    }

    /// A search match, with the modern identity the capability supplies.
    fn search_result(id: u128, display: &str, username: &str) -> AvatarPickerResult {
        AvatarPickerResult {
            avatar_id: agent(id),
            first_name: "Legacy".to_owned(),
            last_name: "Name".to_owned(),
            username: username.to_owned(),
            display_name: display.to_owned(),
        }
    }

    /// A row is labelled with the **display name** the modern search supplies,
    /// and falls back to the legacy pair for a match that has none — which is
    /// every match on the legacy message path.
    #[test]
    fn a_row_prefers_the_display_name() {
        assert_eq!(
            super::picker_label(&search_result(1, "Marina", "marina.vector")),
            "Marina"
        );
        assert_eq!(
            super::picker_label(&search_result(2, "", "marina.vector")),
            "Legacy Name"
        );
    }

    /// A results list of `count` rows, agent `n` labelled `resident n`.
    fn rows(count: u128) -> Vec<PickerRow> {
        (0..count)
            .map(|id| PickerRow {
                agent: agent(id),
                label: format!("resident {id}"),
                username: String::new(),
            })
            .collect()
    }

    /// A picker holding `count` rows in the given mode.
    fn picker(count: u128, allow_multiple: bool) -> AvatarPickerState {
        let mut state = AvatarPickerState {
            allow_multiple,
            ..AvatarPickerState::default()
        };
        state.set_rows(rows(count));
        state
    }

    /// A single-resident request answers with one pick however the user clicks:
    /// the modifiers that mean "and this one too" in the many mode do nothing.
    #[test]
    fn single_mode_never_selects_more_than_one() {
        let mut state = picker(5, false);
        state.select(1, false, false);
        state.select(3, true, false);
        state.select(4, false, true);
        assert_eq!(state.selected, vec![4]);
        assert_eq!(state.picks().len(), 1);
    }

    /// The many mode takes the table widget's algebra: Ctrl toggles a row in and
    /// out, Shift ranges from the anchor.
    #[test]
    fn multi_mode_ctrl_toggles_and_shift_ranges() {
        let mut state = picker(6, true);
        state.select(1, false, false);
        state.select(3, true, false);
        assert_eq!(state.selected, vec![1, 3]);
        state.select(1, true, false);
        assert_eq!(state.selected, vec![3]);
        // The Ctrl+click left the anchor on row 1, so the range runs from there.
        state.select(4, false, true);
        assert_eq!(state.selected, vec![1, 2, 3, 4]);
    }

    /// A click past the end of the list is ignored rather than selecting a row
    /// that is not there (a stale click against rows that have just shrunk).
    #[test]
    fn a_click_past_the_last_row_is_ignored() {
        let mut state = picker(2, true);
        state.select(7, false, false);
        assert!(state.selected.is_empty());
    }

    /// The reply carries every selected row, top to bottom, with the label the
    /// row was showing — the name a consumer records against the id.
    #[test]
    fn picks_are_in_row_order_with_their_labels() {
        let mut state = picker(4, true);
        state.select(2, false, false);
        state.select(0, true, false);
        let picks = state.picks();
        assert_eq!(
            picks.iter().map(|pick| pick.agent).collect::<Vec<_>>(),
            vec![agent(0), agent(2)]
        );
        assert_eq!(
            picks
                .iter()
                .map(|pick| pick.name.clone())
                .collect::<Vec<_>>(),
            vec!["resident 0".to_owned(), "resident 2".to_owned()]
        );
    }

    /// The Near Me list re-sorts under the user, so the selection is carried
    /// across a refresh by agent: whoever is still listed stays picked at their
    /// new index, whoever left drops out, and the anchor follows the same way.
    #[test]
    fn a_refresh_carries_the_selection_by_agent() {
        let mut state = picker(4, true);
        state.select(1, false, false);
        state.select(3, true, false);
        assert_eq!(state.anchor, Some(3));
        // Row 1's resident walked away; the rest re-sorted.
        state.set_rows_keeping_selection(vec![
            PickerRow {
                agent: agent(3),
                label: "resident 3".to_owned(),
                username: String::new(),
            },
            PickerRow {
                agent: agent(0),
                label: "resident 0".to_owned(),
                username: String::new(),
            },
        ]);
        assert_eq!(state.selected, vec![0]);
        assert_eq!(state.anchor, Some(0));
        assert_eq!(state.picks().first().map(|pick| pick.agent), Some(agent(3)));
    }

    /// Opening the picker afresh starts from nothing selected — a leftover pick
    /// from the last request would confirm someone the user never chose.
    #[test]
    fn replacing_the_rows_clears_the_selection() {
        let mut state = picker(3, true);
        state.select(0, false, false);
        state.select(2, true, false);
        state.set_rows(Vec::new());
        assert!(state.selected.is_empty());
        assert_eq!(state.anchor, None);
        assert!(state.picks().is_empty());
    }
}
