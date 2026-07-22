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
//! # The three sources
//!
//! - **Search** — the wire's name lookup: `AvatarPickerRequest` with a
//!   client query id, answered by `AvatarPickerReply` (matching avatars by
//!   partial legacy name).
//! - **Friends** — the held friends roster ([`crate::people::FriendsModel`]).
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
    AgentKey, Command, QueryId, SlCommand, SlEvent, SlIdentity, SlSessionEvent, Uuid,
};

use crate::avatars::AvatarState;
use crate::floater::{FloaterCaps, FloaterSpec, spawn_floater};
use crate::i18n::Translated;
use crate::people::FriendsModel;
use crate::ui::{UiPanelShown, UiRoot, UiScaffoldSystems, column, row};
use crate::ui_font::UiFont;
use crate::ui_tab::{DEFAULT_ELLIPSIS, TabPlacement, TabSpec, TabStrip, spawn_tab_strip};

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
}

/// The confirmed pick.
#[derive(Message, Debug, Clone)]
pub(crate) struct AvatarPicked {
    /// The tag of the feature that opened the picker.
    pub(crate) requester: &'static str,
    /// The chosen avatar.
    pub(crate) agent: AgentKey,
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
    /// The row label.
    label: String,
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
    /// The selected row index.
    selected: Option<usize>,
    /// The in-flight search query id, so a stale reply is ignored.
    pending_query: Option<QueryId>,
    /// Bumped whenever `rows` / `selected` change, driving the list rebuild.
    revision: u64,
}

impl AvatarPickerState {
    /// Replace the rows and clear the selection.
    fn set_rows(&mut self, rows: Vec<PickerRow>) {
        self.rows = rows;
        self.rows.truncate(MAX_ROWS);
        self.selected = None;
        self.revision = self.revision.wrapping_add(1);
    }

    /// Select a row.
    const fn select(&mut self, index: usize) {
        if index < self.rows.len() {
            self.selected = Some(index);
            self.revision = self.revision.wrapping_add(1);
        }
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

/// Fire the wire name search for the field's current text.
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
    let query_id = QueryId::from(Uuid::new_v4());
    state.pending_query = Some(query_id);
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
    let Some(row) = state.selected.and_then(|index| state.rows.get(index)) else {
        return;
    };
    picked.write(AvatarPicked {
        requester,
        agent: row.agent,
    });
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
            let rows = results
                .iter()
                .map(|result| PickerRow {
                    agent: result.avatar_id,
                    label: format!("{} {}", result.first_name, result.last_name),
                })
                .collect();
            state.set_rows(rows);
        }
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
            .map(|(agent, name)| PickerRow { agent, label: name })
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
                    (distance, PickerRow { agent, label: name })
                })
                .collect();
            with_distance.sort_by(|a, b| a.0.total_cmp(&b.0));
            with_distance.into_iter().map(|(_d, row)| row).collect()
        }
    };
    // Write-guarded: replacing the rows every frame would defeat the
    // revision-driven rebuild.
    if rows != state.rows {
        let selected = state
            .selected
            .and_then(|index| state.rows.get(index).cloned());
        state.set_rows(rows);
        // Keep the selection on the same agent if it survived the refresh.
        if let Some(previous) = selected
            && let Some(index) = state
                .rows
                .iter()
                .position(|row| row.agent == previous.agent)
        {
            state.selected = Some(index);
        }
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
    for (index, row_data) in state.rows.iter().enumerate() {
        let selected = state.selected == Some(index);
        commands
            .spawn((
                Button,
                Node {
                    padding: UiRect::axes(Val::Px(6.0), Val::Px(2.0)),
                    ..default()
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
                move |press: On<Pointer<Press>>, mut state: ResMut<AvatarPickerState>| {
                    if press.button == PointerButton::Primary {
                        state.select(index);
                    }
                },
            )
            .with_child((
                Text::new(row_data.label.clone()),
                UiFont::Sans.at(PICKER_FONT_SIZE),
                TextColor(LABEL_COLOR),
                Pickable::IGNORE,
            ));
    }
}
