//! The avatar radar floater — the Firestorm-style nearby-avatar presence tool
//! (reference `fsradar` / `fspanelradar` / `fsfloaterradar`, read-only).
//!
//! A standalone floater with a live, sortable, filterable table of nearby
//! avatars: name, in-region / status glyphs, group title, payment info,
//! account age, a first-seen clock, and the distance — the range cell
//! coloured by chat / shout band. Enter / leave / young-account alerts are
//! produced by the pure model ([`crate::radar_model`]) on a 1 s sweep that
//! runs **whether or not the floater is open** (reference behaviour), each
//! gated by its own opt-in setting and reported to Nearby Chat or as a toast
//! per the output preference, with a UI sound
//! ([`crate::ui_sounds::UiSound::RadarAlert`]).
//!
//! Data sources, all shared rather than re-derived: the merged coarse + full
//! avatar list ([`AvatarState::map_avatars`]), the batched name resolution
//! ([`AvatarState::request_name`]), the typing set
//! ([`NameTagStatuses`]), the away animation
//! ([`crate::name_tag_content::AWAY_ANIM`] via [`AnimationPlayback`]), the
//! grid-overridable chat ranges ([`ChatRanges`]), and the minimap's tracking
//! target ([`MapTracking`]) for the Track row action.
//!
//! Deliberate divergences from the reference radar: no voice / notes columns
//! (no voice or notes support yet), no per-column show / hide bitmask, no
//! LSL-bridge altitude correction, no script-channel (Phoenix) alerts, and no
//! camera-zoom action (no focus-other-avatar camera primitive yet); friend /
//! muted name styling is colour-only (table cells have no bold / italic).
//! The range cell's chat / shout colours follow the name tag's distance
//! palette (green / yellow); beyond-shout keeps the plain cell colour, like
//! the reference radar's silver.

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui::Checked;
use sl_client_bevy::{
    AgentKey, Command, GlobalCoordinates, MuteType, SlCommand, SlEvent, SlIdentity, SlSessionEvent,
    Vector,
};
use sl_settings::SettingValue;

use crate::animations::AnimationPlayback;
use crate::avatar_profile::{FLAG_IDENTIFIED, FLAG_TRANSACTED, OpenAvatarProfile};
use crate::avatars::AvatarState;
use crate::chat::LocalChatNotice;
use crate::conversations::{ConversationKey, NearbyChatNotice, OpenConversation};
use crate::derender::{DerenderKind, RequestDerender};
use crate::floater::{
    DeferredFloaterContent, FloaterCaps, FloaterHandle, FloaterSpec, floater_shown, spawn_floater,
};
use crate::i18n::{TransArgs, Translated, Translator};
use crate::menu::{MenuCommand, MenuDef, MenuItemDef, OpenContextMenu};
use crate::minimap::{ChatRanges, MapTracking, TrackTarget, global_from_bevy, origin_global};
use crate::mutes::{MuteModel, RequestBlock};
use crate::name_tag_content::{AWAY_ANIM, NameTagStatuses};
use crate::notifications::ShowNotification;
use crate::people::FriendsModel;
use crate::radar_model::{
    PaymentInfo, RadarAlert, RadarAlertKind, RadarModel, RadarRow, RadarSample, RangeBand,
    SortColumn, SweepConfig, counts, elapsed_seconds, format_range, format_seen, matches_filter,
    range_band, sort_rows, within_limit,
};
use crate::session::SETTING_DRAW_DISTANCE;
use crate::settings::ViewerSettings;
use crate::settings_binding::{SettingBinding, bound_checkbox};
use crate::terrain::TerrainState;
use crate::ui::{UiRoot, UiScaffoldSystems, column, row};
use crate::ui_element::UiAction;
use crate::ui_font::UiFont;
use crate::ui_search::{SearchFieldSpec, spawn_search_field};
use crate::ui_sounds::{PlayUiSound, UiSound};
use crate::ui_table::{
    TableAlign, TableColumn, TableColumnKind, TableColumnWidth, TableRowCells, TableSelectionMode,
    TableSortDefault, TableSpec, TableState, register_table_settings, set_table_cell, spawn_table,
    spawn_table_row,
};
use crate::ui_text_input::{TextInputKind, TextInputSpec, spawn_text_input};
use crate::virtual_list::{VirtualList, VirtualRow, layout_virtual_lists, spawn_virtual_scrollbar};

/// The radar floater's stable id (persistence, `SL_VIEWER_OPEN_FLOATER`).
pub(crate) const RADAR_FLOATER_ID: &str = "radar";

/// The `element` the radar's menu / UI actions are attributed to.
const RADAR_ELEMENT: &str = "radar";

/// Seconds between model sweeps (the reference's 1 s radar timer).
const SWEEP_SECONDS: f32 = 1.0;

/// At most this many `RequestAvatarProperties` go out per sweep (age /
/// payment backfill), so a crowded region does not burst the grid.
const PROPERTIES_PER_SWEEP: usize = 5;

/// Seconds within which a second press on the same row is a double-click.
const DOUBLE_CLICK_SECS: f32 = 0.4;

// --- Settings ([`RADAR_SECTION`], account scope on write) -----------------

/// The persisted-settings section the radar's knobs live under (`[radar]`).
const RADAR_SECTION: &[&str] = &["radar"];

/// Report entering chat (say) range.
pub(crate) const SETTING_CHAT_ENTER: &str = "RadarReportChatRangeEnter";
/// Report leaving chat (say) range.
pub(crate) const SETTING_CHAT_LEAVE: &str = "RadarReportChatRangeLeave";
/// Report entering draw distance.
pub(crate) const SETTING_DRAW_ENTER: &str = "RadarReportDrawRangeEnter";
/// Report leaving draw distance.
pub(crate) const SETTING_DRAW_LEAVE: &str = "RadarReportDrawRangeLeave";
/// Report entering the own region.
pub(crate) const SETTING_SIM_ENTER: &str = "RadarReportSimRangeEnter";
/// Report leaving the own region.
pub(crate) const SETTING_SIM_LEAVE: &str = "RadarReportSimRangeLeave";
/// Where alerts go: `"chat"` (Nearby Chat line) or `"toast"`.
pub(crate) const SETTING_ALERT_OUTPUT: &str = "RadarAlertOutput";
/// Arm the young-account alert.
pub(crate) const SETTING_AGE_ALERT: &str = "RadarAgeAlert";
/// The young-account threshold, days.
pub(crate) const SETTING_AGE_DAYS: &str = "RadarAgeAlertDays";
/// Limit the radar list to [`SETTING_RANGE`] metres.
const SETTING_LIMIT: &str = "RadarLimitByRange";
/// The near-me range limit, metres (the reference's `NearMeRange`).
const SETTING_RANGE: &str = "RadarNearMeRange";

// --- Palette / geometry (the sibling panels' values) ----------------------

/// Header / cell font size, logical px.
const FONT_SIZE: f32 = 13.0;

/// Table row height, logical px.
const ROW_HEIGHT: f32 = 22.0;

/// The default cell / label colour.
const LABEL_COLOR: Color = Color::srgb(0.90, 0.92, 0.96);

/// The dimmed header / secondary colour.
const DIM_LABEL_COLOR: Color = Color::srgb(0.62, 0.66, 0.74);

/// A friend's name colour (the name tag's `NameTagFriend`).
const FRIEND_COLOR: Color = Color::srgb(0.75, 0.92, 0.49);

/// A muted avatar's name colour (the name tag's `NameTagMuted`).
const MUTED_COLOR: Color = Color::srgb(0.4, 0.4, 0.4);

/// The range cell inside chat range (name-tag chat distance colour).
const CHAT_RANGE_COLOR: Color = Color::srgb(0.0, 1.0, 0.0);

/// The range cell inside shout range (name-tag shout distance colour).
const SHOUT_RANGE_COLOR: Color = Color::srgb(1.0, 1.0, 0.0);

/// The list viewport backdrop.
const LIST_BACKGROUND: Color = Color::srgba(0.0, 0.0, 0.0, 0.25);

/// A selected row's background highlight.
const SELECTED_BACKGROUND: Color = Color::srgba(0.24, 0.34, 0.52, 0.55);

/// An action button's background.
const ACTION_BACKGROUND: Color = Color::srgb(0.24, 0.29, 0.38);

/// The limit checkbox's box border.
const CHECK_BORDER: Color = Color::srgb(0.55, 0.60, 0.70);

/// The limit checkbox's unchecked fill.
const CHECK_OFF: Color = Color::srgba(0.10, 0.12, 0.16, 0.9);

/// The limit checkbox's checked fill.
const CHECK_ON: Color = Color::srgb(0.45, 0.62, 0.90);

/// The limit checkbox's box side, logical px.
const CHECK_SIZE: f32 = 14.0;

// --- Table ----------------------------------------------------------------

/// Column index of the name cell.
const COL_NAME: usize = 0;
/// Column index of the in-region dot cell.
const COL_REGION: usize = 1;
/// Column index of the typing / sitting / away glyph cell.
const COL_STATUS: usize = 2;
/// Column index of the group-title cell.
const COL_TITLE: usize = 3;
/// Column index of the payment cell.
const COL_PAYMENT: usize = 4;
/// Column index of the account-age cell.
const COL_AGE: usize = 5;
/// Column index of the first-seen clock cell.
const COL_SEEN: usize = 6;
/// Column index of the range cell.
const COL_RANGE: usize = 7;

/// The radar table: a flexible name over the status / info columns, default
/// sort range-ascending (the reference default). Selection is radar-owned
/// (keyed by agent, not row index) because the list re-sorts every sweep.
static RADAR_TABLE: TableSpec = TableSpec {
    element: "radar",
    selection: TableSelectionMode::None,
    columns: &[
        TableColumn {
            header_key: "radar-col-name",
            token: "name",
            kind: TableColumnKind::Text,
            width: TableColumnWidth::Flex(1.0),
            align: TableAlign::Start,
            sortable: true,
        },
        TableColumn {
            header_key: "radar-col-region",
            token: "region",
            kind: TableColumnKind::Text,
            width: TableColumnWidth::Fixed { default: 22.0 },
            align: TableAlign::Center,
            sortable: false,
        },
        TableColumn {
            header_key: "radar-col-status",
            token: "status",
            kind: TableColumnKind::Text,
            width: TableColumnWidth::Fixed { default: 44.0 },
            align: TableAlign::Center,
            sortable: false,
        },
        TableColumn {
            header_key: "radar-col-title",
            token: "title",
            kind: TableColumnKind::Text,
            width: TableColumnWidth::Fixed { default: 90.0 },
            align: TableAlign::Start,
            sortable: true,
        },
        TableColumn {
            header_key: "radar-col-payment",
            token: "payment",
            kind: TableColumnKind::Text,
            width: TableColumnWidth::Fixed { default: 34.0 },
            align: TableAlign::Center,
            sortable: true,
        },
        TableColumn {
            header_key: "radar-col-age",
            token: "age",
            kind: TableColumnKind::Text,
            width: TableColumnWidth::Fixed { default: 48.0 },
            align: TableAlign::End,
            sortable: true,
        },
        TableColumn {
            header_key: "radar-col-seen",
            token: "seen",
            kind: TableColumnKind::Text,
            width: TableColumnWidth::Fixed { default: 64.0 },
            align: TableAlign::End,
            sortable: true,
        },
        TableColumn {
            header_key: "radar-col-range",
            token: "range",
            kind: TableColumnKind::Text,
            width: TableColumnWidth::Fixed { default: 64.0 },
            align: TableAlign::End,
            sortable: true,
        },
    ],
    default_sort: &[TableSortDefault {
        column: COL_RANGE,
        ascending: true,
    }],
    builtin_sort: true,
    row_height: ROW_HEIGHT,
    font_size: FONT_SIZE,
    header_color: DIM_LABEL_COLOR,
    cell_color: LABEL_COLOR,
    column_gap: 4.0,
    row_padding: 4.0,
    sort_setting: Some("RadarSortOrder"),
    widths_setting: Some("RadarColumnWidths"),
};

// --- Context menu ---------------------------------------------------------

/// Condition: the pressed avatar is the current tracking target.
const COND_TRACKING: &str = "radar-tracking";
/// Condition: the pressed avatar's full position is known (teleport-to).
const COND_POSITION: &str = "radar-position-known";
/// Condition: the pressed avatar is muted.
const COND_MUTED: &str = "radar-muted";
/// Condition: the pressed avatar is not muted.
const COND_NOT_MUTED: &str = "radar-not-muted";
/// Condition: the pressed avatar is not already a friend.
const COND_NOT_FRIEND: &str = "radar-not-friend";

/// The radar row context menu — every action our client supports of the
/// reference's radar menu (see the module docs for what is deliberately
/// absent).
static RADAR_MENU: MenuDef = MenuDef {
    label: "Radar",
    items: &[
        MenuItemDef::Command(MenuCommand::new("View Profile", "profile")),
        MenuItemDef::Command(MenuCommand::new("IM", "im")),
        MenuItemDef::Separator,
        MenuItemDef::Command(
            MenuCommand::new("Start Tracking", "start-tracking").visible_when(COND_NOT_MUTED),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Stop Tracking", "stop-tracking").visible_when(COND_TRACKING),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Teleport To", "teleport-to").enabled_when(COND_POSITION),
        ),
        MenuItemDef::Command(MenuCommand::new("Offer Teleport", "offer-teleport")),
        MenuItemDef::Separator,
        MenuItemDef::Command(
            MenuCommand::new("Add Friend", "add-friend").visible_when(COND_NOT_FRIEND),
        ),
        MenuItemDef::Command(MenuCommand::new("Block", "block").visible_when(COND_NOT_MUTED)),
        MenuItemDef::Command(MenuCommand::new("Unblock", "unblock").visible_when(COND_MUTED)),
        MenuItemDef::Separator,
        // Client-side derender (`viewer-derender-blacklist`), the reference
        // radar's own Derender / Derender + blacklist pair: the radar is where
        // a griefer is spotted, so it is where they are unrendered from.
        MenuItemDef::Command(MenuCommand::new("Derender", "derender")),
        MenuItemDef::Command(MenuCommand::new(
            "Derender + Blacklist",
            "derender-blacklist",
        )),
    ],
};

// --- Resources ------------------------------------------------------------

/// The always-live radar state: the pure model, the sweep timer, and the
/// sweep-time context snapshot the view projection reuses.
#[derive(Resource)]
pub(crate) struct RadarState {
    /// The pure sweep / alert model.
    model: RadarModel,
    /// The 1 s sweep timer.
    timer: Timer,
    /// The floater's live name filter (lowercased by the match helper).
    filter: String,
    /// The chat (say) range used by the last sweep, metres.
    chat_range: f32,
    /// The shout range at the last sweep, metres (range-cell colouring only).
    shout_range: f32,
    /// The draw distance at the last sweep, metres.
    draw_distance: f32,
}

impl Default for RadarState {
    fn default() -> Self {
        Self {
            model: RadarModel::default(),
            timer: Timer::from_seconds(SWEEP_SECONDS, TimerMode::Repeating),
            filter: String::new(),
            chat_range: 20.0,
            shout_range: 100.0,
            draw_distance: 128.0,
        }
    }
}

/// The floater's view projection: the sorted, filtered rows the virtual list
/// binds, plus the stamps it was built against.
#[derive(Resource, Default)]
struct RadarView {
    /// The display rows, in table order.
    rows: Vec<RadarRow>,
    /// The `(total, in region, in chat range)` counts (unfiltered).
    counts: (usize, usize, usize),
    /// The model revision the rows were built at.
    built_revision: u64,
    /// The table sort revision the rows were ordered at.
    built_sort_revision: u64,
    /// The name filter the rows were filtered by.
    built_filter: String,
    /// The range limit the rows were filtered by (`None` = unlimited).
    built_limit: Option<f32>,
}

/// The radar's own row selection, keyed by agent (not row index) so it
/// survives the every-sweep re-sort and the virtualized row recycling.
#[derive(Resource, Debug, Default)]
struct SelectedRadarAvatar(Option<AgentKey>);

/// Double-click bookkeeping, by agent (rows are recycled).
#[derive(Resource, Debug, Default)]
struct RadarClickTracker {
    /// The agent the last press selected.
    agent: Option<AgentKey>,
    /// When that press landed, seconds since startup.
    time: f32,
}

/// The avatar the open context menu targets.
#[derive(Resource, Debug, Default)]
struct RadarMenuTarget(Option<AgentKey>);

/// The radar floater's retained entities (inserted by the deferred content
/// build; consumers take `Option<Res<RadarUi>>` until then).
#[derive(Resource)]
struct RadarUi {
    /// The table root (carries [`TableState`]).
    table: Entity,
    /// The virtualized viewport (carries [`VirtualList`]).
    viewport: Entity,
    /// The counts line text.
    counts_text: Entity,
    /// The filter box's [`EditableText`] entity.
    filter_field: Entity,
    /// The range-limit numeric field's [`EditableText`] entity.
    range_field: Entity,
}

/// Marker on the limit checkbox's box, for the checked-state fill sync.
#[derive(Component, Debug, Clone, Copy)]
struct RadarLimitCheckbox;

/// The agent a pooled radar row currently presents.
#[derive(Component, Debug, Clone, Copy)]
struct BoundRadar(Option<AgentKey>);

/// One alert produced by a sweep, for the reporter.
#[derive(Message, Debug, Clone, Copy)]
struct RadarAlertMessage(RadarAlert);

// --- Plugin ---------------------------------------------------------------

/// Registers the radar model, sweep systems, floater and settings.
pub(crate) struct RadarPlugin;

impl Plugin for RadarPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RadarState>()
            .init_resource::<RadarView>()
            .init_resource::<SelectedRadarAvatar>()
            .init_resource::<RadarClickTracker>()
            .init_resource::<RadarMenuTarget>()
            .add_message::<RadarAlertMessage>()
            .add_systems(
                Startup,
                (
                    register_radar_settings,
                    spawn_radar_floater.after(UiScaffoldSystems::SpawnRoot),
                ),
            )
            .add_systems(
                Update,
                (
                    sweep_radar,
                    ingest_radar_properties,
                    report_radar_alerts,
                    handle_radar_actions,
                    (
                        mirror_radar_filter,
                        apply_radar_range_field,
                        sync_radar_limit_checkbox,
                        rebuild_radar_view,
                    )
                        .run_if(floater_shown(RADAR_FLOATER_ID)),
                )
                    .chain()
                    .before(layout_virtual_lists),
            )
            .add_systems(
                Update,
                (populate_radar_rows, bind_radar_rows)
                    .chain()
                    .after(layout_virtual_lists)
                    .run_if(floater_shown(RADAR_FLOATER_ID)),
            );
    }
}

/// Register the radar's persisted settings (alert toggles, output channel,
/// age alert, range limit) and the table's sort / width persistence.
fn register_radar_settings(settings: Option<ResMut<ViewerSettings>>) {
    let Some(mut settings) = settings else {
        return;
    };
    for (name, comment) in [
        (SETTING_CHAT_ENTER, "Report avatars entering chat range"),
        (SETTING_CHAT_LEAVE, "Report avatars leaving chat range"),
        (SETTING_DRAW_ENTER, "Report avatars entering draw distance"),
        (SETTING_DRAW_LEAVE, "Report avatars leaving draw distance"),
        (SETTING_SIM_ENTER, "Report avatars entering the region"),
        (SETTING_SIM_LEAVE, "Report avatars leaving the region"),
        (
            SETTING_AGE_ALERT,
            "Alert on avatars below the age threshold",
        ),
    ] {
        settings.register_in(RADAR_SECTION, name, SettingValue::Bool(false), comment);
    }
    settings.register_in(
        RADAR_SECTION,
        SETTING_ALERT_OUTPUT,
        SettingValue::String("chat".to_owned()),
        "Where radar alerts go: 'chat' (Nearby Chat line) or 'toast'",
    );
    settings.register_in(
        RADAR_SECTION,
        SETTING_AGE_DAYS,
        SettingValue::I32(7),
        "The radar age alert's threshold, in days",
    );
    settings.register_in(
        RADAR_SECTION,
        SETTING_LIMIT,
        SettingValue::Bool(false),
        "Limit the radar list to the near-me range",
    );
    settings.register_in(
        RADAR_SECTION,
        SETTING_RANGE,
        SettingValue::F32(162.0),
        "The radar near-me range limit, in metres",
    );
    register_table_settings(&mut settings, RADAR_SECTION, &RADAR_TABLE);
}

// --- Floater --------------------------------------------------------------

/// Startup: spawn the radar floater chrome; the content builds on first open.
fn spawn_radar_floater(mut commands: Commands, root: Res<UiRoot>) {
    let handle = spawn_floater(
        &mut commands,
        root.0,
        FloaterSpec {
            id: RADAR_FLOATER_ID,
            title: "Radar".to_owned(),
            position: Vec2::new(320.0, 120.0),
            default_size: Some(Vec2::new(680.0, 380.0)),
            min_size: Some(Vec2::new(480.0, 220.0)),
            dock_host: None,
            caps: FloaterCaps {
                resizable: true,
                minimizable: false,
                closable: true,
                dockable: false,
            },
        },
    );
    commands
        .entity(handle.title_text)
        .insert(Translated::new("radar-title"));
    let builder = commands.register_system(build_radar_content);
    commands
        .entity(handle.root)
        .insert(DeferredFloaterContent { builder, handle });
}

/// First-open content build: the filter / limit row, the counts line, the
/// table, and the trailing action buttons. Ends with the [`RadarUi`] insert
/// that wakes the `Option<Res<RadarUi>>` consumers.
fn build_radar_content(
    In(handle): In<FloaterHandle>,
    mut commands: Commands,
    settings: Option<Res<ViewerSettings>>,
) {
    let content = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                min_height: Val::Px(0.0),
                ..column(Val::Px(4.0))
            },
            Name::new("radar-content"),
            ChildOf(handle.content),
        ))
        .id();

    // Filter box + range limit controls.
    let controls = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                ..row(Val::Px(6.0))
            },
            Name::new("radar-controls"),
            ChildOf(content),
        ))
        .id();
    let search = spawn_search_field(
        &mut commands,
        controls,
        &SearchFieldSpec {
            tab_index: 0,
            font_size: FONT_SIZE,
            min_width: 140.0,
            placeholder: "Filter by name".to_owned(),
            search_glyph: true,
            ..SearchFieldSpec::new("radar-filter")
        },
    );
    if let Some(placeholder) = search.placeholder {
        commands
            .entity(placeholder)
            .insert(Translated::new("radar-filter-placeholder"));
    }
    commands.spawn((
        bound_checkbox(SettingBinding::account(SETTING_LIMIT)),
        Node {
            width: Val::Px(CHECK_SIZE),
            height: Val::Px(CHECK_SIZE),
            flex_shrink: 0.0,
            border: UiRect::all(Val::Px(2.0)),
            ..default()
        },
        BorderColor::all(CHECK_BORDER),
        BackgroundColor(CHECK_OFF),
        TabIndex(0),
        RadarLimitCheckbox,
        Pickable::default(),
        Name::new("radar-limit-checkbox"),
        ChildOf(controls),
    ));
    commands.spawn((
        Text::default(),
        Translated::new("radar-limit-range"),
        UiFont::Sans.at(FONT_SIZE),
        TextColor(DIM_LABEL_COLOR),
        Pickable::IGNORE,
        ChildOf(controls),
    ));
    let initial_range = settings
        .as_deref()
        .and_then(|settings| settings.store().get_f32(SETTING_RANGE).ok())
        .unwrap_or(162.0);
    let range_field = spawn_text_input(
        &mut commands,
        controls,
        &TextInputSpec {
            initial: format!("{initial_range}"),
            font_size: FONT_SIZE,
            width_glyphs: 6.0,
            ..TextInputSpec::new("radar-range", TextInputKind::Float)
        },
    );

    // The counts line.
    let counts_text = commands
        .spawn((
            Text::default(),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(DIM_LABEL_COLOR),
            Node {
                flex_shrink: 0.0,
                ..default()
            },
            Pickable::IGNORE,
            Name::new("radar-counts"),
            ChildOf(content),
        ))
        .id();

    // The table over its trailing action column.
    let body = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                ..row(Val::Px(6.0))
            },
            Name::new("radar-body"),
            ChildOf(content),
        ))
        .id();
    let table_column = commands
        .spawn((
            Node {
                flex_grow: 1.0,
                min_width: Val::Px(0.0),
                min_height: Val::Px(0.0),
                ..column(Val::Px(2.0))
            },
            ChildOf(body),
        ))
        .id();
    let table = spawn_table(&mut commands, table_column, &RADAR_TABLE);
    commands
        .entity(table.viewport)
        .insert((BackgroundColor(LIST_BACKGROUND), TabIndex(1)));
    spawn_virtual_scrollbar(&mut commands, table.viewport);

    // The trailing action buttons, acting on the selection.
    let actions = commands
        .spawn((
            Node {
                width: Val::Px(92.0),
                flex_shrink: 0.0,
                align_items: AlignItems::Stretch,
                ..column(Val::Px(4.0))
            },
            Name::new("radar-actions"),
            ChildOf(body),
        ))
        .id();
    spawn_radar_action_button(&mut commands, actions, "radar-action-profile", |agent| {
        RadarButtonAction::Profile(agent)
    });
    spawn_radar_action_button(&mut commands, actions, "radar-action-im", |agent| {
        RadarButtonAction::Im(agent)
    });

    commands.insert_resource(RadarUi {
        table: table.root,
        viewport: table.viewport,
        counts_text,
        filter_field: search.field,
        range_field,
    });
}

/// What a trailing action button does with the selected agent.
#[derive(Debug, Clone, Copy)]
enum RadarButtonAction {
    /// Open the avatar's profile floater.
    Profile(AgentKey),
    /// Open a one-to-one IM conversation.
    Im(AgentKey),
}

/// Spawn one trailing action button that maps the current selection through
/// `action` on press.
fn spawn_radar_action_button(
    commands: &mut Commands,
    parent: Entity,
    label_key: &'static str,
    action: fn(AgentKey) -> RadarButtonAction,
) {
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
            Name::new("radar-action"),
            ChildOf(parent),
        ))
        .with_child((
            Text::new(String::new()),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(LABEL_COLOR),
            Translated::new(label_key),
            Pickable::IGNORE,
        ))
        .observe(
            move |mut press: On<Pointer<Press>>,
                  selected: Res<SelectedRadarAvatar>,
                  mut profiles: MessageWriter<OpenAvatarProfile>,
                  mut conversations: MessageWriter<OpenConversation>| {
                press.propagate(false);
                if press.button != PointerButton::Primary {
                    return;
                }
                let Some(agent) = selected.0 else {
                    return;
                };
                match action(agent) {
                    RadarButtonAction::Profile(agent) => {
                        profiles.write(OpenAvatarProfile { agent });
                    }
                    RadarButtonAction::Im(agent) => {
                        conversations.write(OpenConversation {
                            key: ConversationKey::Direct(agent),
                        });
                    }
                }
            },
        );
}

// --- Sweep / model systems ------------------------------------------------

/// Tick the sweep timer; on each fire, sample the merged nearby-avatar list,
/// run the pure model sweep, emit its alerts, and backfill names / profile
/// properties for tracked avatars. Runs whether or not the floater is open.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources: the clock, the model \
              state, the avatar / terrain / identity sources, the thresholds, and the \
              alert / command outputs"
)]
fn sweep_radar(
    time: Res<Time>,
    mut state: ResMut<RadarState>,
    mut avatars: ResMut<AvatarState>,
    identity: Res<SlIdentity>,
    terrain: Res<TerrainState>,
    ranges: Res<ChatRanges>,
    settings: Option<Res<ViewerSettings>>,
    transforms: Query<&GlobalTransform>,
    mut alerts: MessageWriter<RadarAlertMessage>,
    mut sl_commands: MessageWriter<SlCommand>,
) {
    if !state.timer.tick(time.delta()).just_finished() {
        return;
    }
    let Some(own_agent) = identity.agent_id else {
        return;
    };
    let origin = origin_global(terrain.origin().or(identity.region_handle));
    let Some((own_east, own_north, own_up)) = avatars
        .root_entity_of(own_agent)
        .and_then(|entity| transforms.get(entity).ok())
        .map(|transform| global_from_bevy(origin, transform.translation()))
    else {
        return;
    };
    let own_region = crate::minimap::region_handle_at(own_east, own_north);
    let draw_distance = settings
        .as_deref()
        .and_then(|settings| settings.store().get_f32(SETTING_DRAW_DISTANCE).ok())
        .unwrap_or(128.0);
    let age_alert_days = settings.as_deref().and_then(|settings| {
        let store = settings.store();
        if store.get_bool(SETTING_AGE_ALERT).ok()? {
            u32::try_from(store.get_i32(SETTING_AGE_DAYS).ok()?).ok()
        } else {
            None
        }
    });
    state.chat_range = ranges.say;
    state.shout_range = ranges.shout;
    state.draw_distance = draw_distance;

    let mut samples: Vec<RadarSample> = Vec::new();
    for avatar in avatars.map_avatars() {
        if avatar.agent == own_agent {
            continue;
        }
        let Ok(transform) = transforms.get(avatar.anchor) else {
            continue;
        };
        let (east, north, up) = global_from_bevy(origin, transform.translation());
        let altitude_unknown = avatar
            .coarse_z
            .is_some_and(crate::minimap_math::coarse_altitude_unknown);
        let distance = if altitude_unknown {
            None
        } else {
            let de = crate::minimap::narrow(east - own_east);
            let dn = crate::minimap::narrow(north - own_north);
            let du = up - own_up;
            Some((de * de + dn * dn + du * du).sqrt())
        };
        samples.push(RadarSample {
            agent: avatar.agent,
            distance,
            region: crate::minimap::region_handle_at(east, north),
            coarse_only: avatar.coarse_z.is_some(),
            position: (!altitude_unknown).then_some((east, north, up)),
        });
    }
    let cfg = SweepConfig {
        chat_range: state.chat_range,
        draw_distance,
        own_region,
        now_seconds: time.elapsed_secs_f64(),
        age_alert_days,
    };
    for alert in state.model.sweep(&samples, &cfg) {
        alerts.write(RadarAlertMessage(alert));
    }
    // Backfill: names for unresolved rows (batched by the avatar module) and
    // a throttled trickle of profile-properties requests (age / payment).
    let pending_names: Vec<AgentKey> = state
        .model
        .entries()
        .filter(|(agent, _entry)| avatars.name_record(**agent).is_none())
        .map(|(agent, _entry)| *agent)
        .collect();
    for agent in pending_names {
        avatars.request_name(agent);
    }
    for agent in state.model.take_property_requests(PROPERTIES_PER_SWEEP) {
        sl_commands.write(SlCommand(Command::RequestAvatarProperties(agent)));
    }
}

/// Fold `AvatarProperties` replies into the model: account age (parsed from
/// `born_on`) and payment-info status. Runs unconditionally so the age alert
/// works with the floater closed.
fn ingest_radar_properties(mut events: MessageReader<SlEvent>, mut state: ResMut<RadarState>) {
    for event in events.read() {
        let SlSessionEvent::AvatarProperties(props) = &event.0 else {
            continue;
        };
        let today = jiff::Zoned::now().date();
        let age_days = crate::radar_model::parse_born_on(&props.born_on, today);
        let payment = if props.flags & FLAG_TRANSACTED != 0 {
            PaymentInfo::Transacted
        } else if props.flags & FLAG_IDENTIFIED != 0 {
            PaymentInfo::Identified
        } else {
            PaymentInfo::None
        };
        state
            .model
            .set_properties(props.avatar_id, age_days, payment);
    }
}

/// Report sweep alerts per the notification settings: each kind has its own
/// opt-in toggle; the output preference picks a Nearby Chat line (overlay +
/// transcript, clickable name) or a `RadarAlert` toast. Any reported alert
/// also raises the radar UI sound (its own enable lives with the UI sounds).
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources: the alert stream, the \
              setting gates, the name source, and the three output channels plus the sound"
)]
fn report_radar_alerts(
    mut alerts: MessageReader<RadarAlertMessage>,
    state: Res<RadarState>,
    settings: Option<Res<ViewerSettings>>,
    avatars: Res<AvatarState>,
    translator: Translator,
    mut notices: MessageWriter<LocalChatNotice>,
    mut transcript: MessageWriter<NearbyChatNotice>,
    mut toasts: MessageWriter<ShowNotification>,
    mut sounds: MessageWriter<PlayUiSound>,
) {
    let Some(settings) = settings.as_deref() else {
        // No settings store (the gallery): drop the alerts unreported.
        alerts.clear();
        return;
    };
    let store = settings.store();
    let enabled = |name: &str| store.get_bool(name).unwrap_or(false);
    let toast_output = store.get_str(SETTING_ALERT_OUTPUT).unwrap_or("chat") == "toast";
    let mut reported = false;
    for RadarAlertMessage(alert) in alerts.read() {
        let setting = match alert.kind {
            RadarAlertKind::ChatEnter => SETTING_CHAT_ENTER,
            RadarAlertKind::ChatLeave => SETTING_CHAT_LEAVE,
            RadarAlertKind::DrawEnter => SETTING_DRAW_ENTER,
            RadarAlertKind::DrawLeave => SETTING_DRAW_LEAVE,
            RadarAlertKind::SimEnter => SETTING_SIM_ENTER,
            RadarAlertKind::SimLeave => SETTING_SIM_LEAVE,
            RadarAlertKind::Age => SETTING_AGE_ALERT,
        };
        if !enabled(setting) {
            continue;
        }
        let message = alert_message(&translator, &state, alert);
        let name = avatars.label_text(alert.agent);
        if toast_output {
            toasts.write(
                ShowNotification::new("RadarAlert")
                    .arg("NAME", name)
                    .arg("MESSAGE", message)
                    .with_context(alert.agent.uuid().to_string()),
            );
        } else {
            notices.write(LocalChatNotice::new(format!("{name} {message}")));
            transcript.write(NearbyChatNotice {
                speaker: name,
                speaker_agent: Some(alert.agent),
                body: message,
            });
        }
        reported = true;
    }
    if reported {
        sounds.write(PlayUiSound(UiSound::RadarAlert));
    }
}

/// Compose one alert's localized message body (without the leading name).
fn alert_message(translator: &Translator, state: &RadarState, alert: &RadarAlert) -> String {
    let with_distance = |key: &str, unknown_key: &str| match alert.distance {
        Some(distance) => translator.format(
            key,
            &TransArgs::new().text("distance", &format!("{distance:.2}")),
        ),
        None => translator.get(unknown_key),
    };
    match alert.kind {
        RadarAlertKind::ChatEnter => {
            with_distance("radar-alert-chat-enter", "radar-alert-chat-enter-unknown")
        }
        RadarAlertKind::ChatLeave => translator.get("radar-alert-chat-leave"),
        RadarAlertKind::DrawEnter => {
            with_distance("radar-alert-draw-enter", "radar-alert-draw-enter-unknown")
        }
        RadarAlertKind::DrawLeave => translator.get("radar-alert-draw-leave"),
        RadarAlertKind::SimEnter => {
            with_distance("radar-alert-sim-enter", "radar-alert-sim-enter-unknown")
        }
        RadarAlertKind::SimLeave => translator.get("radar-alert-sim-leave"),
        RadarAlertKind::Age => {
            let days = state
                .model
                .entry(alert.agent)
                .and_then(|entry| entry.age_days)
                .unwrap_or(0);
            translator.format(
                "radar-alert-age",
                &TransArgs::new().int("days", i64::from(days)),
            )
        }
    }
}

// --- View systems (floater open) ------------------------------------------

/// Mirror the filter field's live text into [`RadarState::filter`].
fn mirror_radar_filter(
    ui: Option<Res<RadarUi>>,
    fields: Query<&EditableText>,
    mut state: ResMut<RadarState>,
) {
    let Some(ui) = ui else {
        return;
    };
    let Ok(field) = fields.get(ui.filter_field) else {
        return;
    };
    let term = field.value().to_string();
    if state.filter != term {
        state.filter = term;
    }
}

/// Write a committed range-field value through to the [`SETTING_RANGE`]
/// setting (per avatar), so the limit persists like the reference's
/// `NearMeRange`.
fn apply_radar_range_field(
    ui: Option<Res<RadarUi>>,
    fields: Query<&EditableText>,
    settings: Option<ResMut<ViewerSettings>>,
) {
    let (Some(ui), Some(mut settings)) = (ui, settings) else {
        return;
    };
    let Ok(field) = fields.get(ui.range_field) else {
        return;
    };
    let Some(crate::ui_text_input::TextInputValue::Float(value)) =
        TextInputKind::Float.parse(field.value().to_string().trim())
    else {
        return;
    };
    let value = crate::minimap::narrow(value).max(0.0);
    let current = settings.store().get_f32(SETTING_RANGE).unwrap_or(162.0);
    if (current - value).abs() > f32::EPSILON {
        settings.set_account(SETTING_RANGE, SettingValue::F32(value));
        settings.save_async();
    }
}

/// Fill the limit checkbox's box from its bound `Checked` state (the binding
/// layer keeps `Checked` in step with the setting; this is only the paint).
fn sync_radar_limit_checkbox(
    mut boxes: Query<(&mut BackgroundColor, Has<Checked>), With<RadarLimitCheckbox>>,
) {
    for (mut background, checked) in &mut boxes {
        let wanted = if checked { CHECK_ON } else { CHECK_OFF };
        if background.0 != wanted {
            background.0 = wanted;
        }
    }
}

/// Rebuild the view projection when the model revision, the table sort, the
/// filter, or the range limit moved: project every tracked avatar plus its
/// live statuses into display rows, filter, sort by the widget's key stack,
/// and keep the counts line and the virtual list's item count in step.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources: the model, the view, \
              the UI handles, and the five per-avatar status sources the rows project"
)]
fn rebuild_radar_view(
    state: Res<RadarState>,
    mut view: ResMut<RadarView>,
    ui: Option<Res<RadarUi>>,
    settings: Option<Res<ViewerSettings>>,
    avatars: Res<AvatarState>,
    statuses: Res<NameTagStatuses>,
    playback: Res<AnimationPlayback>,
    friends: Option<Res<FriendsModel>>,
    mutes: Res<MuteModel>,
    identity: Res<SlIdentity>,
    time: Res<Time>,
    translator: Translator,
    tables: Query<&TableState>,
    mut lists: Query<&mut VirtualList>,
    mut texts: Query<&mut Text>,
) {
    let Some(ui) = ui else {
        return;
    };
    let sort = tables
        .get(ui.table)
        .ok()
        .map(|table| (table.sort_revision(), table.sort().keys().to_vec()));
    let sort_revision = sort.as_ref().map_or(0, |(revision, _keys)| *revision);
    let limit = settings.as_deref().and_then(|settings| {
        let store = settings.store();
        if store.get_bool(SETTING_LIMIT).ok()? {
            store.get_f32(SETTING_RANGE).ok()
        } else {
            None
        }
    });
    if view.built_revision == state.model.revision()
        && view.built_sort_revision == sort_revision
        && view.built_filter == state.filter
        && view.built_limit == limit
    {
        return;
    }
    view.built_revision = state.model.revision();
    view.built_sort_revision = sort_revision;
    view.built_filter.clone_from(&state.filter);
    view.built_limit = limit;

    let own_region = identity.region_handle;
    let now = time.elapsed_secs_f64();
    let mut rows: Vec<RadarRow> = state
        .model
        .entries()
        .map(|(agent, entry)| {
            let record = avatars.name_record(*agent);
            let username = record
                .and_then(|record| record.username.clone())
                .unwrap_or_default();
            RadarRow {
                agent: *agent,
                name: avatars.label_text(*agent),
                username,
                title: avatars.title_of(*agent).unwrap_or_default().to_owned(),
                payment: entry.payment,
                age_days: entry.age_days,
                seen_seconds: elapsed_seconds(now, entry.first_seen),
                distance: entry.last_distance,
                coarse_only: entry.coarse_only,
                in_own_region: own_region.is_some() && entry.last_region == own_region,
                typing: statuses.is_typing(*agent),
                sitting: avatars.is_seated(*agent),
                away: AWAY_ANIM.is_some_and(|away| playback.is_playing(*agent, away)),
                friend: friends
                    .as_deref()
                    .is_some_and(|friends| friends.is_friend(*agent)),
                muted: mutes.is_muted(agent.uuid()),
            }
        })
        .collect();
    view.counts = counts(&rows, state.chat_range);
    rows.retain(|row| matches_filter(row, &state.filter) && within_limit(row, limit));
    let keys: Vec<(SortColumn, bool)> = sort
        .map(|(_revision, keys)| keys)
        .unwrap_or_default()
        .iter()
        .filter_map(|key| {
            RADAR_TABLE
                .columns
                .get(key.column)
                .and_then(|column| SortColumn::from_token(column.token))
                .map(|column| (column, key.ascending))
        })
        .collect();
    sort_rows(&mut rows, &keys);
    view.rows = rows;

    if let Ok(mut list) = lists.get_mut(ui.viewport) {
        list.item_count = view.rows.len();
    }
    let (total, in_region, in_chat) = view.counts;
    let label = translator.format(
        "radar-counts",
        &TransArgs::new()
            .int("total", i64::try_from(total).unwrap_or(i64::MAX))
            .int("region", i64::try_from(in_region).unwrap_or(i64::MAX))
            .int("chat", i64::try_from(in_chat).unwrap_or(i64::MAX)),
    );
    if let Ok(mut text) = texts.get_mut(ui.counts_text)
        && text.0 != label
    {
        text.0 = label;
    }
}

/// Build the widget cells of each freshly-pooled radar row and attach the
/// press observer.
fn populate_radar_rows(
    mut commands: Commands,
    ui: Option<Res<RadarUi>>,
    new_rows: Query<(Entity, &ChildOf), Added<VirtualRow>>,
) {
    let Some(ui) = ui else {
        return;
    };
    for (row_entity, child_of) in &new_rows {
        if child_of.parent() != ui.viewport {
            continue;
        }
        spawn_table_row(&mut commands, row_entity, ui.table, &RADAR_TABLE);
        commands
            .entity(row_entity)
            .insert(BoundRadar(None))
            .observe(on_radar_row_press);
    }
}

/// The name cell's composition: `Display Name (username)` when the username
/// is known and adds anything.
fn name_cell_text(row: &RadarRow) -> String {
    if row.username.is_empty() || row.username == row.name {
        row.name.clone()
    } else {
        format!("{} ({})", row.name, row.username)
    }
}

/// The status cell's glyphs: `T` typing, `S` sitting, `A` away (the
/// reference's letter-box icons).
fn status_cell_text(row: &RadarRow) -> String {
    let mut glyphs: Vec<&str> = Vec::new();
    if row.typing {
        glyphs.push("T");
    }
    if row.sitting {
        glyphs.push("S");
    }
    if row.away {
        glyphs.push("A");
    }
    glyphs.join(" ")
}

/// Bind each pooled radar row to the [`RadarRow`] it now presents: the cell
/// texts, the per-cell colours (range band, friend / muted name), and the
/// selection highlight.
fn bind_radar_rows(
    view: Res<RadarView>,
    state: Res<RadarState>,
    selected: Res<SelectedRadarAvatar>,
    ui: Option<Res<RadarUi>>,
    mut rows: Query<(
        Entity,
        Ref<VirtualRow>,
        &ChildOf,
        &TableRowCells,
        &mut BoundRadar,
    )>,
    mut backgrounds: Query<&mut BackgroundColor>,
    mut texts: Query<(&mut Text, &mut TextColor)>,
) {
    let Some(ui) = ui else {
        return;
    };
    let refresh_all = view.is_changed() || selected.is_changed();
    for (row_entity, row, child_of, cells, mut bound) in &mut rows {
        if child_of.parent() != ui.viewport {
            continue;
        }
        if !refresh_all && !row.is_changed() {
            continue;
        }
        let data = row.index.and_then(|index| view.rows.get(index));
        bound.0 = data.map(|data| data.agent);
        let Some(data) = data else {
            for column in 0..RADAR_TABLE.columns.len() {
                if let Some(cell) = cells.cell(column) {
                    set_table_cell(&mut texts, cell, "", LABEL_COLOR);
                }
            }
            continue;
        };
        let name_color = if data.muted {
            MUTED_COLOR
        } else if data.friend {
            FRIEND_COLOR
        } else {
            LABEL_COLOR
        };
        let range_color = match range_band(data.distance, state.chat_range, state.shout_range) {
            RangeBand::Chat => CHAT_RANGE_COLOR,
            RangeBand::Shout => SHOUT_RANGE_COLOR,
            RangeBand::Beyond => LABEL_COLOR,
            RangeBand::Unknown => DIM_LABEL_COLOR,
        };
        let region_color = if data.in_own_region {
            LABEL_COLOR
        } else {
            DIM_LABEL_COLOR
        };
        let cell_values: [(usize, String, Color); 8] = [
            (COL_NAME, name_cell_text(data), name_color),
            (
                COL_REGION,
                if data.coarse_only { "○" } else { "●" }.to_owned(),
                region_color,
            ),
            (COL_STATUS, status_cell_text(data), DIM_LABEL_COLOR),
            (COL_TITLE, data.title.clone(), LABEL_COLOR),
            (
                COL_PAYMENT,
                data.payment.cell_text().to_owned(),
                LABEL_COLOR,
            ),
            (
                COL_AGE,
                data.age_days.map(|age| age.to_string()).unwrap_or_default(),
                LABEL_COLOR,
            ),
            (COL_SEEN, format_seen(data.seen_seconds), DIM_LABEL_COLOR),
            (
                COL_RANGE,
                format_range(data.distance, state.draw_distance),
                range_color,
            ),
        ];
        for (column, value, color) in cell_values {
            if let Some(cell) = cells.cell(column) {
                set_table_cell(&mut texts, cell, &value, color);
            }
        }
        if let Ok(mut background) = backgrounds.get_mut(row_entity) {
            let wanted = if selected.0 == Some(data.agent) {
                SELECTED_BACKGROUND
            } else {
                Color::NONE
            };
            if background.0 != wanted {
                background.0 = wanted;
            }
        }
    }
}

// --- Gallery specimen -----------------------------------------------------

/// One static specimen row: name (+ its tint), status glyphs, and the range
/// cell (+ its band tint). The values are data (avatar names, clocks,
/// metres), so only the translatable strings go through the sample-text
/// transform.
type SpecimenRow = (&'static str, Color, &'static str, &'static str, Color);

/// The gallery / `ui_test` specimen: a static sketch of the radar floater's
/// content — the counts line, three auto-sized rows spanning the range bands
/// (friend / plain / muted-coarse), and the trailing action buttons. The
/// live floater binds the shared virtualized table widget (swept by its own
/// consumers); here the radar-specific composition is static so its layout
/// is swept.
pub(crate) fn spawn_radar_specimen(
    commands: &mut Commands,
    parent: Entity,
    cx: crate::ui_element::ElementCx,
) -> Entity {
    let root = commands
        .spawn((
            column(Val::Px(4.0)),
            Name::new("radar-specimen"),
            ChildOf(parent),
        ))
        .id();
    commands.spawn((
        Text::new(cx.text("3 nearby — 2 in region, 1 in chat range")),
        TextLayout {
            linebreak: LineBreak::NoWrap,
            ..default()
        },
        UiFont::Sans.at(cx.font_size),
        TextColor(DIM_LABEL_COLOR),
        ChildOf(root),
    ));
    let rows: [SpecimenRow; 3] = [
        (
            "Nearby Resident (nearby.resident)",
            FRIEND_COLOR,
            "● T S",
            "8.51",
            CHAT_RANGE_COLOR,
        ),
        (
            "Passer-by Resident",
            LABEL_COLOR,
            "● A",
            "54.20",
            SHOUT_RANGE_COLOR,
        ),
        (
            "Faraway Resident",
            MUTED_COLOR,
            "○",
            ">128.00",
            DIM_LABEL_COLOR,
        ),
    ];
    for (index, (name, name_color, glyphs, range, range_color)) in rows.iter().enumerate() {
        let row_node = commands
            .spawn((
                Node {
                    height: Val::Px(ROW_HEIGHT),
                    align_items: AlignItems::Center,
                    padding: UiRect::horizontal(Val::Px(4.0)),
                    ..row(Val::Px(10.0))
                },
                BackgroundColor(if index == 0 {
                    SELECTED_BACKGROUND
                } else {
                    Color::NONE
                }),
                ChildOf(root),
            ))
            .id();
        for (value, color) in [
            (*name, *name_color),
            (*glyphs, DIM_LABEL_COLOR),
            (*range, *range_color),
        ] {
            commands.spawn((
                Text::new(value),
                TextLayout {
                    linebreak: LineBreak::NoWrap,
                    ..default()
                },
                UiFont::Sans.at(cx.font_size),
                TextColor(color),
                Pickable::IGNORE,
                ChildOf(row_node),
            ));
        }
    }
    let actions = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                ..row(Val::Px(6.0))
            },
            ChildOf(root),
        ))
        .id();
    for label in ["Profile", "IM"] {
        commands
            .spawn((
                Node {
                    padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
                    ..default()
                },
                BackgroundColor(ACTION_BACKGROUND),
                ChildOf(actions),
            ))
            .with_child((
                Text::new(cx.text(label)),
                TextLayout {
                    linebreak: LineBreak::NoWrap,
                    ..default()
                },
                UiFont::Sans.at(cx.font_size),
                TextColor(LABEL_COLOR),
            ));
    }
    root
}

/// A press on a pooled radar row: primary selects (double-press opens the
/// profile); secondary selects and opens the context menu with the open-time
/// condition snapshot.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy observer's parameters are its injected resources: the row pool, the \
              selection / double-click / menu-target stashes, and the fact sources the \
              condition snapshot reads"
)]
fn on_radar_row_press(
    mut press: On<Pointer<Press>>,
    rows: Query<&BoundRadar>,
    ui: Res<RadarUi>,
    time: Res<Time>,
    tracking: Res<MapTracking>,
    friends: Option<Res<FriendsModel>>,
    mutes: Res<MuteModel>,
    state: Res<RadarState>,
    mut focus: ResMut<InputFocus>,
    mut selected: ResMut<SelectedRadarAvatar>,
    mut clicks: ResMut<RadarClickTracker>,
    mut target: ResMut<RadarMenuTarget>,
    mut menus: MessageWriter<OpenContextMenu>,
    mut profiles: MessageWriter<OpenAvatarProfile>,
) {
    let Ok(BoundRadar(Some(agent))) = rows.get(press.entity).copied() else {
        return;
    };
    press.propagate(false);
    focus.set(ui.viewport, FocusCause::Navigated);
    selected.0 = Some(agent);
    match press.button {
        PointerButton::Primary => {
            let now = time.elapsed_secs();
            if clicks.agent == Some(agent) && now - clicks.time <= DOUBLE_CLICK_SECS {
                profiles.write(OpenAvatarProfile { agent });
                clicks.agent = None;
            } else {
                clicks.agent = Some(agent);
                clicks.time = now;
            }
        }
        PointerButton::Secondary => {
            target.0 = Some(agent);
            let mut conditions: Vec<&'static str> = Vec::new();
            if tracking.target == Some(TrackTarget::Avatar(agent)) {
                conditions.push(COND_TRACKING);
            }
            if state
                .model
                .entry(agent)
                .is_some_and(|entry| entry.position.is_some())
            {
                conditions.push(COND_POSITION);
            }
            if mutes.is_muted(agent.uuid()) {
                conditions.push(COND_MUTED);
            } else {
                conditions.push(COND_NOT_MUTED);
            }
            if !friends
                .as_deref()
                .is_some_and(|friends| friends.is_friend(agent))
            {
                conditions.push(COND_NOT_FRIEND);
            }
            menus.write(OpenContextMenu {
                menu: &RADAR_MENU,
                at: press.pointer_location.position,
                element: RADAR_ELEMENT,
                conditions,
            });
        }
        _other => {}
    }
}

/// Dispatch the radar context menu's picks onto the shared avatar-action
/// channels (profile, IM, tracking, teleports, friendship, mutes, derender).
#[expect(
    clippy::too_many_arguments,
    reason = "the action dispatch fans out to the tracking resource and the shared \
              avatar-action message / command channels"
)]
fn handle_radar_actions(
    mut actions: MessageReader<UiAction>,
    target: Res<RadarMenuTarget>,
    state: Res<RadarState>,
    avatars: Res<AvatarState>,
    mut tracking: ResMut<MapTracking>,
    mut sl_commands: MessageWriter<SlCommand>,
    mut blocks: MessageWriter<RequestBlock>,
    mut derenders: MessageWriter<RequestDerender>,
    mut conversations: MessageWriter<OpenConversation>,
    mut profiles: MessageWriter<OpenAvatarProfile>,
) {
    for action in actions.read() {
        if action.element != RADAR_ELEMENT {
            continue;
        }
        let Some(agent) = target.0 else {
            continue;
        };
        match action.action {
            "profile" => {
                profiles.write(OpenAvatarProfile { agent });
            }
            "im" => {
                conversations.write(OpenConversation {
                    key: ConversationKey::Direct(agent),
                });
            }
            "start-tracking" => {
                tracking.target = Some(TrackTarget::Avatar(agent));
            }
            "stop-tracking" => {
                tracking.target = None;
            }
            "teleport-to" => {
                let Some((east, north, up)) =
                    state.model.entry(agent).and_then(|entry| entry.position)
                else {
                    continue;
                };
                let global = GlobalCoordinates::new(east, north, f64::from(up));
                let Some((grid, local)) = global.split() else {
                    continue;
                };
                sl_commands.write(SlCommand(Command::Teleport {
                    region_handle: sl_client_bevy::RegionHandle::from_grid(grid.x(), grid.y()),
                    position: local,
                    look_at: Vector {
                        x: 1.0,
                        y: 0.0,
                        z: 0.0,
                    },
                }));
            }
            "offer-teleport" => {
                sl_commands.write(SlCommand(Command::OfferTeleport {
                    targets: vec![agent],
                    message: String::new(),
                }));
            }
            "add-friend" => {
                sl_commands.write(SlCommand(Command::OfferFriendship {
                    to_agent_id: agent,
                    message: String::new(),
                }));
            }
            "block" => {
                let name = avatars
                    .name_of(agent)
                    .map(ToOwned::to_owned)
                    .unwrap_or_default();
                blocks.write(RequestBlock::new(agent.uuid(), name, MuteType::Agent));
            }
            "unblock" => {
                let name = avatars
                    .name_of(agent)
                    .map(ToOwned::to_owned)
                    .unwrap_or_default();
                sl_commands.write(SlCommand(Command::Unmute {
                    id: agent.uuid(),
                    name,
                }));
            }
            action @ ("derender" | "derender-blacklist") => {
                let name = avatars
                    .name_of(agent)
                    .map(ToOwned::to_owned)
                    .unwrap_or_default();
                derenders.write(RequestDerender::new(
                    agent.uuid(),
                    name,
                    DerenderKind::Resident,
                    action == "derender-blacklist",
                ));
            }
            _other => {}
        }
    }
}
