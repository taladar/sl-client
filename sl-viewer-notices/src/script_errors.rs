//! The **Script Errors** window (`viewer-script-warning-window`): what scripts
//! say on `DEBUG_CHANNEL` — their run-time errors, refused library calls and
//! warnings — gathered in one place instead of drowning in nearby chat.
//!
//! A line reaches the window when [`script_chat_route`] says so, which is the
//! same rule the nearby-chat transcript and the overlay ask, so a line is
//! shown in exactly one of them. By default only the agent's own objects'
//! lines are shown, in the window (the reference's rule on Second Life); the
//! Chat preferences tab moves them to nearby chat or nowhere, lets other
//! residents' objects through, routes `llOwnerSay` here too and empties the
//! window when it is closed.
//!
//! The window has an **All** tab and one tab per object that spoke, in the
//! order they first spoke. Each line is stamped with the local time it
//! arrived; a message of several lines (XEngine's .NET stack, YEngine's
//! report) keeps its line breaks, one row each. An object's tab heads its
//! rows with the object's name as a link — the object inspector, with Show on
//! Map — and where it was, and marks the object in the world with a beacon
//! while the tab is shown. A new line opens the window, as the reference
//! does.
//!
//! What each grid's lines say, and to whom, is measured in
//! `book/src/gridspec/lsl.md` § Run-time errors: two lines per fault on
//! Second Life (`<object> [script:<name>] Script run-time error`, then the
//! error), one on OpenSim naming script, event, prim and place; Second Life
//! stops passing an object's lines after the eighth until it has been quiet a
//! while.
//!
//! Reference (Firestorm, read-only): `llfloaterscriptdebug.cpp`,
//! `floater_script_debug.xml`, `floater_script_debug_panel.xml`.

use std::collections::VecDeque;

use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use bevy_flair::style::components::ClassList;
use sl_client_bevy::{
    ChatAudible, ChatMessage, ChatSource, ChatType, ObjectKey, RegionHandle, Rotation,
    SlCurrentRegion, SlEvent, SlIdentity, SlRegionIdentity, SlSessionEvent, Uuid, Vector,
};
use sl_viewer_platform::local_time::LocalTimeZone;
use sl_viewer_ui_core::i18n::Translated;
use sl_viewer_ui_core::skin::{
    CONSOLE_ERROR_CLASS, LIST_SURFACE_CLASS, TEXT_CLASS, set_state_class_on,
};
use sl_viewer_ui_core::skin_palette::SkinPalette;
use sl_viewer_ui_core::ui::{UiPanelShown, UiRoot, UiScaffoldSystems, column, row};
use sl_viewer_ui_core::ui_element::{ContentMayOverflow, ElementCx, TextMayClip};
use sl_viewer_ui_core::ui_ellipsis::{RevealEllipsis, spawn_ellipsis_marker};
use sl_viewer_ui_core::ui_font::UiFont;
use sl_viewer_ui_core::ui_spawn::{self, ButtonKind, ButtonSpec, UiLabel};
use sl_viewer_ui_core::virtual_list::{
    VirtualList, VirtualRow, VirtualViewport, amend_row_node, layout_virtual_lists,
    spawn_specimen_row, spawn_virtual_scrollbar,
};

use crate::floater::{
    DeferredFloaterContent, Floater, FloaterCaps, FloaterHandle, FloaterSpec, floater_panel,
    floater_shown, show_floater, spawn_floater,
};
use crate::linkified_text::{LinkTextStyle, spawn_linkified_text};
use crate::settings::ViewerSettings;
use crate::social::{DebugBeacon, DebugBeacons};
use crate::ui_tab::{
    DEFAULT_ELLIPSIS, DynamicTabStrip, TabCaption, TabPlacement, TabSpec, TabStrip,
    spawn_dynamic_tab_strip,
};
use crate::world_api::rlv::swallows_owner_say;
use crate::world_api::{
    ObjectState, SETTING_SCRIPT_ERRORS_CLEAR_ON_CLOSE, SETTING_SCRIPT_ERRORS_SHOW_IN,
    ScriptChatRoute, script_chat_route,
};

/// The floater's stable id.
pub const SCRIPT_ERRORS_FLOATER_ID: &str = "script-errors";

/// The owner name this window's beacon is published under.
const BEACON_OWNER: &str = "script-errors";

/// The beacon's colour: the reference's script-error red.
const BEACON_COLOR: Color = Color::srgb(1.0, 0.25, 0.25);

/// The most messages the window keeps; the oldest goes first. The reference
/// keeps a text editor's worth; a bound keeps a script that errors every
/// second from growing the viewer without end.
pub const MESSAGE_CAP: usize = 1000;

/// Font size of the rows and the header, logical px.
const FONT_SIZE: f32 = 13.0;

/// A row's height, logical px.
const ROW_HEIGHT: f32 = 18.0;

/// The header's and buttons' label colour, before the skin loads.
const LABEL_COLOR: Color = SkinPalette::FALLBACK.text_primary;

/// An action button's background, before the skin loads.
const ACTION_BACKGROUND: Color = Color::srgb(0.24, 0.29, 0.38);

/// What a row is indented by when it continues the line above it — the
/// second and later lines of one message.
const CONTINUATION_INDENT: &str = "    ";

/// Why a row's clip may slice the line it holds: the list's rows are one
/// uniform height, so a long line is cut behind a revealed `…` rather than
/// wrapped into the rows below it.
const LINE_CLIP_REASON: &str = "a script-error row is one uniform-height row, cut at the row's end behind its revealed `…` marker";

// --- Pure model -------------------------------------------------------------

/// What kind of line a message is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptLineKind {
    /// Said on `DEBUG_CHANNEL`: an error or a warning.
    Debug,
    /// An `llOwnerSay`, routed here by the preference.
    OwnerSay,
}

/// An object (or, rarely, an avatar typing on the debug channel) that has
/// said something here.
#[derive(Debug, Clone, PartialEq)]
pub struct ScriptSpeaker {
    /// The speaker's id: the object's key, or the avatar's.
    pub id: Uuid,
    /// Whether the speaker is an object, so its name links to the inspector.
    pub is_object: bool,
    /// Its name, as it last spoke.
    pub name: String,
    /// Its owner, when the grid named one.
    pub owner: Option<Uuid>,
    /// The region it last spoke in, when known.
    pub region: Option<String>,
    /// The region the beacon is placed in, when known.
    pub region_handle: Option<RegionHandle>,
    /// Where it was when it last spoke, region metres.
    pub position: Vec3,
}

/// One message, as it arrived.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptMessage {
    /// The speaker, by its index in [`ScriptErrorsLog::speakers`].
    pub speaker: usize,
    /// The local time it arrived, as `HH:MM`.
    pub time: String,
    /// The text, verbatim.
    pub text: String,
    /// What kind of line it is.
    pub kind: ScriptLineKind,
}

/// One row of the list: a line of a message, as the active tab shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptRow {
    /// The row's text.
    pub text: String,
    /// The kind of the message it belongs to.
    pub kind: ScriptLineKind,
}

/// Everything the window has been told, oldest first.
#[derive(Resource, Debug, Default)]
pub struct ScriptErrorsLog {
    /// Every speaker, in the order it first spoke — the order of the tabs
    /// after All.
    speakers: Vec<ScriptSpeaker>,
    /// The messages, oldest first, at most [`MESSAGE_CAP`].
    messages: VecDeque<ScriptMessage>,
    /// Bumped on every change, so the view knows when to rebuild.
    revision: u64,
    /// Bumped on every [`clear`](Self::clear), so a tab strip built before
    /// it knows its speakers' tabs are stale, however many speakers have
    /// spoken since.
    generation: u64,
}

impl ScriptErrorsLog {
    /// Every speaker, in the order it first spoke.
    #[must_use]
    pub fn speakers(&self) -> &[ScriptSpeaker] {
        &self.speakers
    }

    /// The messages, oldest first.
    #[must_use]
    pub const fn messages(&self) -> &VecDeque<ScriptMessage> {
        &self.messages
    }

    /// The revision the log is at.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// How many times the log has been cleared.
    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    /// Take a message from `speaker` (whose name, owner and place replace
    /// what was known of it), dropping the oldest past [`MESSAGE_CAP`].
    pub fn push(
        &mut self,
        speaker: ScriptSpeaker,
        time: String,
        text: String,
        kind: ScriptLineKind,
    ) {
        let index = if let Some((index, known)) = self
            .speakers
            .iter_mut()
            .enumerate()
            .find(|(_index, known)| known.id == speaker.id)
        {
            *known = speaker;
            index
        } else {
            self.speakers.push(speaker);
            self.speakers.len().saturating_sub(1)
        };
        self.messages.push_back(ScriptMessage {
            speaker: index,
            time,
            text,
            kind,
        });
        while self.messages.len() > MESSAGE_CAP {
            let _dropped = self.messages.pop_front();
        }
        self.revision = self.revision.wrapping_add(1);
    }

    /// Forget everything, the tabs' speakers too.
    pub fn clear(&mut self) {
        self.speakers.clear();
        self.messages.clear();
        self.revision = self.revision.wrapping_add(1);
        self.generation = self.generation.wrapping_add(1);
    }

    /// The rows a tab shows: every message for the All tab (`None`), each
    /// headed by its speaker's name, or one speaker's messages without it.
    /// A message of several lines is a row per line, the later ones
    /// indented; a trailing line break (YEngine ends with one) adds no row.
    #[must_use]
    pub fn rows(&self, speaker: Option<usize>) -> Vec<ScriptRow> {
        let mut rows = Vec::new();
        for message in &self.messages {
            if speaker.is_some_and(|wanted| wanted != message.speaker) {
                continue;
            }
            let name = self
                .speakers
                .get(message.speaker)
                .map_or("", |known| known.name.as_str());
            let mut lines = message.text.trim_end_matches(['\n', '\r']).split('\n');
            let first = lines.next().unwrap_or_default().trim_end_matches('\r');
            let head = match speaker {
                None => format!("[{}] {name}: {first}", message.time),
                Some(_one) => format!("[{}] {first}", message.time),
            };
            rows.push(ScriptRow {
                text: head,
                kind: message.kind,
            });
            for line in lines {
                rows.push(ScriptRow {
                    text: format!("{CONTINUATION_INDENT}{}", line.trim_end_matches('\r')),
                    kind: message.kind,
                });
            }
        }
        rows
    }
}

/// The speaker a chat line came from, as the window keeps it.
#[must_use]
pub fn speaker_of(message: &ChatMessage, region: Option<&SlRegionIdentity>) -> ScriptSpeaker {
    let position = Vec3::new(
        message.position.x(),
        message.position.y(),
        message.position.z(),
    );
    ScriptSpeaker {
        id: message.source.source_id(),
        is_object: matches!(message.source, ChatSource::Object(_)),
        name: message.from_name.clone(),
        owner: message.owner_id,
        region: region
            .and_then(|identity| identity.0.sim_name.as_ref())
            .map(ToString::to_string),
        region_handle: region.map(|identity| identity.0.region_handle),
        position,
    }
}

/// The header an object's tab is headed by: its name as a link to the object
/// inspector (carrying its owner and place, for Show on Map), then where it
/// was. An avatar's name is not a link.
#[must_use]
pub fn speaker_header(speaker: &ScriptSpeaker) -> String {
    let place = format!(
        "({:.0}, {:.0}, {:.0})",
        speaker.position.x, speaker.position.y, speaker.position.z
    );
    let label = speaker.name.replace(['[', ']'], "");
    if !speaker.is_object {
        return format!("{label} {place}");
    }
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    let _named = query.append_pair("name", &speaker.name);
    if let Some(owner) = speaker.owner {
        let _owned = query.append_pair("owner", &owner.to_string());
    }
    if let Some(region) = &speaker.region {
        let _placed = query.append_pair(
            "slurl",
            &format!(
                "{region}/{:.0}/{:.0}/{:.0}",
                speaker.position.x, speaker.position.y, speaker.position.z
            ),
        );
    }
    format!(
        "[secondlife:///app/objectim/{}?{} {label}] {place}",
        speaker.id,
        query.finish()
    )
}

/// The local time `HH:MM` a message arriving now is stamped with.
fn now_stamp(zone: Option<&LocalTimeZone>) -> String {
    let zone = zone.map_or_else(jiff::tz::TimeZone::system, |zone| zone.zone().clone());
    jiff::Timestamp::now()
        .to_zoned(zone)
        .strftime("%H:%M")
        .to_string()
}

// --- Resources ----------------------------------------------------------------

/// The floater's retained entities, once its content is built.
#[derive(Resource, Debug)]
struct ScriptErrorsUi {
    /// The tab strip: All, then a tab per speaker.
    strip: DynamicTabStrip,
    /// The speakers' tab buttons, in speaker order.
    speaker_tabs: Vec<Entity>,
    /// The log generation the speakers' tabs were made in.
    tabs_generation: u64,
    /// The header row an object's tab shows above its rows.
    header: Entity,
    /// The rows' virtualized viewport.
    viewport: Entity,
}

/// What the rows were last built against.
#[derive(Resource, Debug, Default)]
struct ScriptErrorsView {
    /// The rows the list binds, for the active tab.
    rows: Vec<ScriptRow>,
    /// The log revision the rows were built at.
    built_revision: u64,
    /// The tab they were built for.
    built_tab: usize,
    /// Whether they have been built at all.
    built: bool,
    /// What the header was last built for: the speaker and the log revision.
    header_for: Option<(Option<usize>, u64)>,
    /// Whether the window was shown on the previous frame, so a close is
    /// noticed.
    was_open: bool,
}

/// Marks a pooled row's text node, so the binder can find it.
#[derive(Component, Debug, Clone, Copy)]
struct ScriptRowText(Entity);

// --- Plugin -------------------------------------------------------------------

/// Registers the Script Errors window, its log and its systems.
#[derive(Debug, Clone, Copy, Default)]
pub struct ScriptErrorsPlugin;

impl Plugin for ScriptErrorsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ScriptErrorsLog>()
            .init_resource::<ScriptErrorsView>()
            .init_resource::<DebugBeacons>()
            .add_systems(
                Startup,
                spawn_script_errors_floater.after(UiScaffoldSystems::SpawnRoot),
            )
            // Not gated: lines arrive, and open the window, while it is shut.
            .add_systems(Update, ingest_script_errors.before(layout_virtual_lists))
            .add_systems(
                Update,
                (sync_speaker_tabs, size_rows, bind_rows, refresh_header)
                    .chain()
                    .after(layout_virtual_lists)
                    .run_if(floater_shown(SCRIPT_ERRORS_FLOATER_ID)),
            )
            // Not gated either: both notice the frame the window stops being
            // shown.
            .add_systems(Update, (publish_beacon, clear_on_close));
    }
}

// --- Floater ------------------------------------------------------------------

/// The Script Errors window's [`FloaterSpec`].
#[must_use]
pub fn script_errors_floater_spec() -> FloaterSpec {
    FloaterSpec {
        id: SCRIPT_ERRORS_FLOATER_ID,
        title: "Script Errors".to_owned(),
        position: Vec2::new(320.0, 160.0),
        default_size: Some(Vec2::new(600.0, 300.0)),
        min_size: Some(Vec2::new(360.0, 180.0)),
        dock_host: None,
        caps: FloaterCaps {
            resizable: true,
            minimizable: true,
            closable: true,
            dockable: false,
        },
    }
}

/// Startup: chrome only.
fn spawn_script_errors_floater(mut commands: Commands, root: Res<UiRoot>) {
    let handle = spawn_floater(&mut commands, root.0, script_errors_floater_spec());
    commands
        .entity(handle.title_text)
        .insert(Translated::new("script-errors-title"));
    let builder = commands.register_system(build_script_errors_content);
    commands
        .entity(handle.root)
        .insert(DeferredFloaterContent { builder, handle });
}

/// First-open content build.
fn build_script_errors_content(In(handle): In<FloaterHandle>, mut commands: Commands) {
    let ui = spawn_script_errors_content(&mut commands, handle.content, FONT_SIZE);
    commands.insert_resource(ui);
}

/// Build the window's content into `parent`: the tab strip, the header, the
/// rows and the Clear button. Shared by the live floater and its specimen.
fn spawn_script_errors_content(
    commands: &mut Commands,
    parent: Entity,
    font_size: f32,
) -> ScriptErrorsUi {
    let content = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                min_height: Val::Px(0.0),
                ..column(Val::Px(4.0))
            },
            Name::new("script-errors-content"),
            ChildOf(parent),
        ))
        .id();
    let strip = spawn_dynamic_tab_strip(
        commands,
        content,
        &TabSpec {
            element: "script-errors-tabs",
            placement: TabPlacement::BlockStart,
            labels: &[],
            active: 0,
            tab_index: 0,
            font_size,
            strip_width: None,
            ellipsis: DEFAULT_ELLIPSIS,
            translate_labels: false,
            names: &[],
        },
    );
    let _all = strip.add_tab(commands, TabCaption::Key("script-errors-all"), None);
    let header = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_shrink: 0.0,
                display: Display::None,
                ..row(Val::Px(4.0))
            },
            Name::new("script-errors-header"),
            ChildOf(content),
        ))
        .id();
    let viewport = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                overflow: Overflow::clip(),
                position_type: PositionType::Relative,
                ..default()
            },
            ClassList::new_with_classes([LIST_SURFACE_CLASS]),
            VirtualList::new(row_height(font_size)),
            VirtualViewport,
            Pickable::default(),
            Name::new("script-errors-rows"),
            ChildOf(content),
        ))
        .id();
    spawn_virtual_scrollbar(commands, viewport);
    let buttons = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_shrink: 0.0,
                justify_content: JustifyContent::FlexEnd,
                ..row(Val::Px(6.0))
            },
            Name::new("script-errors-buttons"),
            ChildOf(content),
        ))
        .id();
    let clear = ui_spawn::spawn_button(
        commands,
        buttons,
        ButtonSpec::flat(UiLabel::key("script-errors-clear"), "script-errors-clear")
            .kind(ButtonKind::Headless)
            .tab_index(2)
            .colors(ACTION_BACKGROUND, ACTION_BACKGROUND)
            .label_color(LABEL_COLOR)
            .font_size(font_size),
    )
    .button;
    commands.entity(clear).observe(
        |_activate: On<Activate>, log: Option<ResMut<ScriptErrorsLog>>| {
            if let Some(mut log) = log {
                log.clear();
            }
        },
    );
    ScriptErrorsUi {
        strip,
        speaker_tabs: Vec::new(),
        tabs_generation: 0,
        header,
        viewport,
    }
}

/// A row's height at `font_size`: [`ROW_HEIGHT`] at [`FONT_SIZE`], in
/// proportion above it.
fn row_height(font_size: f32) -> f32 {
    ROW_HEIGHT.max((font_size * ROW_HEIGHT / FONT_SIZE).ceil())
}

/// Dress a pooled row: one no-wrap text node in a clip, cut behind the shared
/// `…` marker. Returns the text node.
fn spawn_row(commands: &mut Commands, row_entity: Entity, font_size: f32) -> Entity {
    amend_row_node(commands, row_entity, |node| {
        node.align_items = AlignItems::Center;
        node.padding = UiRect::horizontal(Val::Px(4.0));
    });
    let clip = commands
        .spawn((
            Node {
                min_width: Val::Px(0.0),
                overflow: Overflow::clip(),
                align_items: AlignItems::Center,
                ..default()
            },
            ContentMayOverflow {
                reason: LINE_CLIP_REASON,
            },
            TextMayClip {
                reason: LINE_CLIP_REASON,
            },
            Pickable::IGNORE,
            ChildOf(row_entity),
        ))
        .id();
    let text = commands
        .spawn((
            Text::new(String::new()),
            TextLayout::no_wrap(),
            UiFont::Mono.at(font_size),
            ClassList::new_with_classes([TEXT_CLASS]),
            Node {
                flex_shrink: 0.0,
                ..default()
            },
            Pickable::IGNORE,
            Name::new("script-errors-line"),
            ChildOf(clip),
        ))
        .id();
    let marker = spawn_ellipsis_marker(
        commands,
        row_entity,
        font_size,
        LABEL_COLOR,
        DEFAULT_ELLIPSIS,
    );
    commands.entity(clip).insert(RevealEllipsis { marker });
    commands.entity(row_entity).insert(ScriptRowText(text));
    text
}

// --- Gallery specimen -----------------------------------------------------------

/// The Script Errors window's gallery / `ui_test` specimen: the live content,
/// with the All tab's rows built by the live [`ScriptErrorsLog::rows`] from
/// one fault each grid would report — Second Life's two lines, YEngine's one
/// — and drawn by the live row builder.
pub fn spawn_script_errors_specimen(
    commands: &mut Commands,
    parent: Entity,
    cx: ElementCx,
) -> Entity {
    let ui = spawn_script_errors_content(commands, parent, cx.font_size);
    let mut log = ScriptErrorsLog::default();
    let speaker = ScriptSpeaker {
        id: Uuid::nil(),
        is_object: true,
        name: "Faulty prim".to_owned(),
        owner: None,
        region: Some("Mauve".to_owned()),
        region_handle: None,
        position: Vec3::new(120.0, 128.0, 27.5),
    };
    for text in [
        "Faulty prim [script:Math 1] Script run-time error",
        "Math Error",
        "YEngine: Division by Zero (script: Math 1 event: link_message primID: \
         00000000-0000-0000-0000-000000000000 at: <120,128,27>) Script must be Reset to \
         re-enable.\n",
    ] {
        log.push(
            speaker.clone(),
            "12:34".to_owned(),
            text.to_owned(),
            ScriptLineKind::Debug,
        );
    }
    let _tab = ui
        .strip
        .add_tab(commands, TabCaption::Literal(&speaker.name), None);
    let rows = log.rows(None);
    for (index, shown) in rows.iter().enumerate() {
        let row = spawn_specimen_row(commands, ui.viewport, index, row_height(cx.font_size));
        let holder = spawn_row(commands, row, cx.font_size);
        commands
            .entity(holder)
            .insert(Text::new(cx.text(&shown.text)));
        let error = shown.kind == ScriptLineKind::Debug;
        commands
            .entity(holder)
            .entry::<ClassList>()
            .and_modify(move |mut classes| {
                sl_viewer_ui_core::skin::set_state_class(&mut classes, CONSOLE_ERROR_CLASS, error);
            });
    }
    let count = rows.len();
    commands
        .entity(ui.viewport)
        .entry::<VirtualList>()
        .and_modify(move |mut list| list.item_count = count);
    parent
}

// --- Systems --------------------------------------------------------------------

/// What the ingest reads besides the events, bundled as one
/// [`SystemParam`](bevy::ecs::system::SystemParam).
#[derive(bevy::ecs::system::SystemParam)]
struct IngestFacts<'w, 's> {
    /// The settings the route reads.
    settings: Option<Res<'w, ViewerSettings>>,
    /// Our own agent, whose objects' lines are shown.
    identity: Option<Res<'w, SlIdentity>>,
    /// The object mirror the RLV owner-say gate reads.
    objects: Option<Res<'w, ObjectState>>,
    /// The local time zone a line is stamped in.
    zone: Option<Res<'w, LocalTimeZone>>,
    /// The region the agent is in, which a line's position is local to.
    regions: Query<'w, 's, &'static SlRegionIdentity, With<SlCurrentRegion>>,
}

/// Take every line [`script_chat_route`] sends to the window, and open the
/// window on one, as the reference does.
///
/// Only fully audible lines with text are taken: Second Life sends an avatar
/// past say range a fault's lines barely audible, their text left out
/// (`book/src/gridspec/lsl.md` § Run-time errors), which says nothing. An RLV
/// owner-say `@`-command is the RLV intake's, not a message.
fn ingest_script_errors(
    mut events: MessageReader<SlEvent>,
    mut log: ResMut<ScriptErrorsLog>,
    facts: IngestFacts,
    floaters: Query<(Entity, &Floater)>,
    mut panels: Query<&mut UiPanelShown>,
) {
    let own_agent = facts
        .identity
        .as_deref()
        .and_then(|identity| identity.agent_id);
    let mut arrived = false;
    for event in events.read() {
        let SlSessionEvent::ChatReceived(message) = &event.0 else {
            continue;
        };
        if message.message.is_empty()
            || !matches!(message.audible, ChatAudible::Fully)
            || swallows_owner_say(
                facts.settings.as_deref(),
                facts.objects.as_deref(),
                message.source,
                message.chat_type,
                &message.message,
            )
            || script_chat_route(facts.settings.as_deref(), own_agent, message)
                != ScriptChatRoute::Window
        {
            continue;
        }
        let kind = if message.chat_type == ChatType::DebugChannel {
            ScriptLineKind::Debug
        } else {
            ScriptLineKind::OwnerSay
        };
        log.push(
            speaker_of(message, facts.regions.iter().next()),
            now_stamp(facts.zone.as_deref()),
            message.message.clone(),
            kind,
        );
        arrived = true;
    }
    if arrived
        && facts
            .settings
            .as_deref()
            .and_then(|settings| settings.store().get_str(SETTING_SCRIPT_ERRORS_SHOW_IN).ok())
            .is_none_or(|place| place == "window")
    {
        show_floater(&floaters, &mut panels, SCRIPT_ERRORS_FLOATER_ID);
    }
}

/// Keep a tab per speaker: drop them all when the log was cleared since they
/// were made, and add one for each speaker without one.
///
/// Compared with the log rather than run on its change: the first line opens
/// the window in the frame it arrives, before the window's content is built,
/// and a change seen then is gone by the time the strip exists.
fn sync_speaker_tabs(
    mut commands: Commands,
    log: Res<ScriptErrorsLog>,
    ui: Option<ResMut<ScriptErrorsUi>>,
) {
    let Some(mut ui) = ui else {
        return;
    };
    let strip = ui.strip;
    if ui.tabs_generation != log.generation() {
        for button in ui.speaker_tabs.drain(..) {
            strip.remove_tab(&mut commands, button);
        }
        ui.tabs_generation = log.generation();
    }
    let known = ui.speaker_tabs.len();
    for speaker in log.speakers().iter().skip(known) {
        let tab = strip.add_tab(&mut commands, TabCaption::Literal(&speaker.name), None);
        ui.speaker_tabs.push(tab.button);
    }
}

/// The active tab of the window's strip: `0` for All, else a speaker's tab.
fn active_tab(ui: &ScriptErrorsUi, strips: &Query<&TabStrip>) -> usize {
    strips.get(ui.strip.strip).map_or(0, |strip| strip.active)
}

/// The speaker an active tab shows, `None` for All.
const fn tab_speaker(tab: usize) -> Option<usize> {
    tab.checked_sub(1)
}

/// Rebuild the rows when the log or the active tab changed, and pin the list
/// to the newest.
fn size_rows(
    log: Res<ScriptErrorsLog>,
    ui: Option<Res<ScriptErrorsUi>>,
    strips: Query<&TabStrip>,
    mut view: ResMut<ScriptErrorsView>,
    mut lists: Query<&mut VirtualList>,
) {
    let Some(ui) = ui else {
        return;
    };
    let tab = active_tab(&ui, &strips);
    if view.built && view.built_revision == log.revision() && view.built_tab == tab {
        return;
    }
    view.built = true;
    view.built_revision = log.revision();
    view.built_tab = tab;
    view.rows = log.rows(tab_speaker(tab));
    if let Ok(mut list) = lists.get_mut(ui.viewport) {
        let count = view.rows.len();
        list.item_count = count;
        list.scroll_to_index(count);
    }
}

/// Dress a freshly-pooled row, and bind every row to the line it now shows:
/// its text, and the error class on a `DEBUG_CHANNEL` line.
fn bind_rows(
    mut commands: Commands,
    view: Res<ScriptErrorsView>,
    ui: Option<Res<ScriptErrorsUi>>,
    new_rows: Query<(Entity, &ChildOf), Added<VirtualRow>>,
    rows: Query<(Ref<VirtualRow>, &ChildOf, Ref<ScriptRowText>)>,
    mut texts: Query<&mut Text>,
    mut classes: Query<&mut ClassList>,
) {
    let Some(ui) = ui else {
        return;
    };
    for (row_entity, child_of) in &new_rows {
        if child_of.parent() == ui.viewport {
            let _text = spawn_row(&mut commands, row_entity, FONT_SIZE);
        }
    }
    let refresh_all = view.is_changed();
    for (row, child_of, holder) in &rows {
        if child_of.parent() != ui.viewport
            || (!refresh_all && !row.is_changed() && !holder.is_added())
        {
            continue;
        }
        let shown = row.index.and_then(|index| view.rows.get(index));
        let value = shown.map_or_else(String::new, |shown| shown.text.clone());
        if let Ok(mut text) = texts.get_mut(holder.0)
            && text.0 != value
        {
            text.0 = value;
        }
        let error = shown.is_some_and(|shown| shown.kind == ScriptLineKind::Debug);
        set_state_class_on(&mut classes, holder.0, CONSOLE_ERROR_CLASS, error);
    }
}

/// Head a speaker's tab with its name as a link and where it was; hide the
/// header on the All tab.
fn refresh_header(
    mut commands: Commands,
    log: Res<ScriptErrorsLog>,
    ui: Option<Res<ScriptErrorsUi>>,
    strips: Query<&TabStrip>,
    mut view: ResMut<ScriptErrorsView>,
    mut nodes: Query<&mut Node>,
) {
    let Some(ui) = ui else {
        return;
    };
    let speaker = tab_speaker(active_tab(&ui, &strips));
    let wanted = (speaker, log.revision());
    if view.header_for == Some(wanted) {
        return;
    }
    view.header_for = Some(wanted);
    commands.entity(ui.header).despawn_related::<Children>();
    let shown = speaker.and_then(|index| log.speakers().get(index));
    if let Ok(mut node) = nodes.get_mut(ui.header) {
        node.display = if shown.is_some() {
            Display::Flex
        } else {
            Display::None
        };
    }
    if let Some(shown) = shown {
        let _header = spawn_linkified_text(
            &mut commands,
            ui.header,
            &speaker_header(shown),
            LinkTextStyle::at(FONT_SIZE),
        );
    }
}

/// Mark the object a shown speaker's tab is for with a beacon; drop it when
/// the All tab is active or the window is not shown.
fn publish_beacon(
    log: Res<ScriptErrorsLog>,
    ui: Option<Res<ScriptErrorsUi>>,
    strips: Query<&TabStrip>,
    floaters: Query<(Entity, &Floater)>,
    panels: Query<&UiPanelShown>,
    mut beacons: ResMut<DebugBeacons>,
) {
    let shown = floater_panel(&floaters, SCRIPT_ERRORS_FLOATER_ID)
        .and_then(|panel| panels.get(panel).ok())
        .is_some_and(|shown| shown.0);
    let wanted = ui
        .as_deref()
        .filter(|_ui| shown)
        .and_then(|ui| tab_speaker(active_tab(ui, &strips)))
        .and_then(|index| log.speakers().get(index))
        .filter(|speaker| speaker.is_object)
        .and_then(|speaker| {
            speaker.region_handle.map(|region| DebugBeacon {
                region,
                anchor: Some(ObjectKey::from(speaker.id)),
                position: Vector {
                    x: speaker.position.x,
                    y: speaker.position.y,
                    z: speaker.position.z,
                },
                rotation: Rotation {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    s: 1.0,
                },
                offset: Vector {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                color: BEACON_COLOR,
            })
        });
    match wanted {
        Some(beacon) => beacons.set(BEACON_OWNER, vec![beacon]),
        None if beacons.has(BEACON_OWNER) => beacons.clear(BEACON_OWNER),
        None => {}
    }
}

/// Empty the window when it is closed, if the preference asks.
fn clear_on_close(
    mut view: ResMut<ScriptErrorsView>,
    settings: Option<Res<ViewerSettings>>,
    floaters: Query<(Entity, &Floater)>,
    panels: Query<&UiPanelShown>,
    mut log: ResMut<ScriptErrorsLog>,
) {
    let open = floater_panel(&floaters, SCRIPT_ERRORS_FLOATER_ID)
        .and_then(|panel| panels.get(panel).ok())
        .is_some_and(|shown| shown.0);
    if open == view.was_open {
        return;
    }
    view.was_open = open;
    let clear = settings
        .as_deref()
        .and_then(|settings| {
            settings
                .store()
                .get_bool(SETTING_SCRIPT_ERRORS_CLEAR_ON_CLOSE)
                .ok()
        })
        .unwrap_or(false);
    if !open && clear && !log.messages().is_empty() {
        log.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MESSAGE_CAP, ScriptErrorsLog, ScriptLineKind, ScriptRow, ScriptSpeaker, speaker_header,
    };
    use bevy::prelude::Vec3;
    use pretty_assertions::assert_eq;
    use sl_client_bevy::Uuid;

    /// An object speaker named `name`, with the id `id`.
    fn object(id: u128, name: &str) -> ScriptSpeaker {
        ScriptSpeaker {
            id: Uuid::from_u128(id),
            is_object: true,
            name: name.to_owned(),
            owner: Some(Uuid::from_u128(7)),
            region: Some("Default Region".to_owned()),
            region_handle: None,
            position: Vec3::new(120.0, 128.4, 27.5),
        }
    }

    /// A debug row of `text`.
    fn row(text: &str) -> ScriptRow {
        ScriptRow {
            text: text.to_owned(),
            kind: ScriptLineKind::Debug,
        }
    }

    #[test]
    fn the_all_tab_names_each_speaker_and_a_speakers_tab_does_not() {
        let mut log = ScriptErrorsLog::default();
        log.push(
            object(1, "Faulty prim"),
            "10:00".to_owned(),
            "Faulty prim [script:Math 1] Script run-time error".to_owned(),
            ScriptLineKind::Debug,
        );
        log.push(
            object(2, "Other"),
            "10:01".to_owned(),
            "Could not find script 'Nope'.".to_owned(),
            ScriptLineKind::Debug,
        );
        log.push(
            object(1, "Faulty prim"),
            "10:02".to_owned(),
            "Math Error".to_owned(),
            ScriptLineKind::Debug,
        );
        assert_eq!(
            log.rows(None),
            vec![
                row("[10:00] Faulty prim: Faulty prim [script:Math 1] Script run-time error"),
                row("[10:01] Other: Could not find script 'Nope'."),
                row("[10:02] Faulty prim: Math Error"),
            ]
        );
        assert_eq!(
            log.rows(Some(0)),
            vec![
                row("[10:00] Faulty prim [script:Math 1] Script run-time error"),
                row("[10:02] Math Error"),
            ]
        );
        assert_eq!(log.speakers().len(), 2);
    }

    #[test]
    fn a_message_of_several_lines_is_a_row_each_and_a_trailing_break_adds_none() {
        let mut log = ScriptErrorsLog::default();
        log.push(
            object(1, "Object"),
            "10:00".to_owned(),
            "System.OverflowException: overflow.\n  at Op_Division\r\n  at run\n".to_owned(),
            ScriptLineKind::Debug,
        );
        assert_eq!(
            log.rows(Some(0)),
            vec![
                row("[10:00] System.OverflowException: overflow."),
                row("      at Op_Division"),
                row("      at run"),
            ]
        );
    }

    #[test]
    fn a_speaker_that_speaks_again_keeps_its_tab_under_its_new_name() {
        let mut log = ScriptErrorsLog::default();
        log.push(
            object(1, "Object"),
            "10:00".to_owned(),
            "a".to_owned(),
            ScriptLineKind::Debug,
        );
        log.push(
            object(1, "Renamed"),
            "10:01".to_owned(),
            "b".to_owned(),
            ScriptLineKind::OwnerSay,
        );
        assert_eq!(
            log.speakers()
                .iter()
                .map(|speaker| speaker.name.clone())
                .collect::<Vec<_>>(),
            vec!["Renamed".to_owned()]
        );
        assert_eq!(
            log.rows(None),
            vec![
                row("[10:00] Renamed: a"),
                ScriptRow {
                    text: "[10:01] Renamed: b".to_owned(),
                    kind: ScriptLineKind::OwnerSay,
                },
            ]
        );
    }

    #[test]
    fn the_log_keeps_the_newest_messages_up_to_its_cap() {
        let mut log = ScriptErrorsLog::default();
        for index in 0..MESSAGE_CAP.saturating_add(5) {
            log.push(
                object(1, "Object"),
                "10:00".to_owned(),
                index.to_string(),
                ScriptLineKind::Debug,
            );
        }
        assert_eq!(log.messages().len(), MESSAGE_CAP);
        assert_eq!(
            log.messages().front().map(|message| message.text.clone()),
            Some("5".to_owned())
        );
        log.clear();
        assert!(log.messages().is_empty() && log.speakers().is_empty());
    }

    #[test]
    fn an_objects_header_links_to_the_inspector_with_its_owner_and_place() {
        assert_eq!(
            speaker_header(&object(1, "Faulty [prim]")),
            "[secondlife:///app/objectim/00000000-0000-0000-0000-000000000001?\
             name=Faulty+%5Bprim%5D&owner=00000000-0000-0000-0000-000000000007&\
             slurl=Default+Region%2F120%2F128%2F28 Faulty prim] (120, 128, 28)"
        );
        let avatar = ScriptSpeaker {
            is_object: false,
            ..object(2, "Some Body")
        };
        assert_eq!(speaker_header(&avatar), "Some Body (120, 128, 28)");
    }
}
