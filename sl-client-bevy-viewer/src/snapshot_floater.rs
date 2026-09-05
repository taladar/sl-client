//! The **snapshot floater** (`viewer-snapshot-floater`): the viewer's photo tool,
//! promoting the debug `screenshot.rs` harness into a real floater — a framed
//! preview, **include-UI / include-HUD** toggles, a **format** picker and a
//! **save-to-disk** destination, laid out as **destination tabs**.
//!
//! # Refresh, not a live feed — and why the window is captured
//!
//! Like the reference viewer, the preview is **not** a continuously re-rendered
//! feed: a **Refresh** button (or a save) takes one shot on demand. That shot is
//! the **actual primary window**, read straight back with
//! [`Screenshot::primary_window`], so it is guaranteed to match what is on screen
//! — the same tone-mapped, environment-lit frame the eye sees, rather than a
//! second camera that would have to re-derive the whole lighting / tonemap /
//! image-based-lighting pipeline (and get it subtly wrong).
//!
//! Capturing the window is also what makes the **include-UI** and
//! **include-HUD** toggles fall out naturally, exactly as the reference offers
//! them:
//!
//! - **Include UI** off (the default) hides the whole UI ([`UiRoot`], via
//!   `Display::None`) for the shot frame, so the photo is a clean world view;
//!   on, the interface (this floater included) is left in the frame.
//! - **Include HUD** off (the default) hides the worn-HUD attachment subtree
//!   ([`crate::hud::HudScreen`], via `Visibility::Hidden`) for the shot; on, it
//!   stays in the frame. (The HUD *camera* is deliberately left alone — the UI
//!   renders through it, and it shares the world camera's HDR view-target chain,
//!   so toggling it would drop the interface and break the HUD lighting.)
//! - **Hide L$ balance** ([[viewer-snapshot-hide-balance]]) blanks the status
//!   bar's balance read-out ([`crate::status_bar::BalanceReadout`], via
//!   `Visibility::Hidden`) for the shot, so a photo shared publicly does not leak
//!   the shooter's balance. It only matters while the UI is in the frame (with
//!   **Include UI** off the whole status bar is gone already), so it is inert
//!   there. The read-out's slot keeps its width, so blanking it does not shift
//!   the row — the reference viewer's `RenderHideBalanceInSnapshot`.
//!
//! Because those changes must be *rendered* before the shutter, a capture is a
//! tiny state machine: hide what the toggles exclude, wait a frame, take the
//! shot, then restore. The brief blink while the UI is hidden is the same one the
//! reference viewer shows.
//!
//! # Save to disk
//!
//! **Save to Disk** writes the captured frame at the **window's own resolution**
//! (free-form disk output, unlike the power-of-two texture-to-inventory path) in
//! the picked format, into the platform Pictures folder
//! ([`crate::paths::snapshots_dir`]), and echoes the saved path to nearby chat —
//! the running local-chat index photographers rely on, matching the quick key
//! ([[viewer-snapshot-quick-key]]).
//!
//! The include-UI / include-HUD / hide-balance / format choices persist per
//! avatar ([`crate::settings`]).
//!
//! # The other destinations are their own tabs (and tasks)
//!
//! The **Postcard / e-mail** ([[viewer-snapshot-postcard]]), **Profile feed**
//! ([[viewer-snapshot-profile-feed]]) and **Inventory texture**
//! ([[viewer-snapshot-to-inventory]]) destinations are placeholder tabs here;
//! each lands in its own roadmap item because their resolution rules (the
//! inventory path wants power-of-two, biased-scaled dimensions), costs and auth
//! differ from a free-form disk save. They downscale the captured frame to their
//! own constraints when they land; disk does not.
//!
//! Reference (Firestorm, read-only): `llsnapshotlivepreview`,
//! `llfloatersnapshot`, `panel_snapshot_*`.

use std::path::PathBuf;

use bevy::asset::RenderAssetUsages;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use bevy::tasks::{IoTaskPool, Task, block_on, poll_once};
use bevy::ui_widgets::{Activate, Button};
use bevy_flair::style::components::ClassList;

use crate::hud::HudScreen;
use crate::i18n::{TransArgs, Translated, Translator};
use crate::settings::ViewerSettings;
use crate::status_bar::BalanceReadout;
use crate::ui::{UiRoot, UiScaffoldSystems, column, row};
use crate::ui_combo::{ComboChanged, ComboSelection, ComboSpec, spawn_combo};
use crate::ui_font::UiFont;
use crate::ui_tab::{
    DEFAULT_ELLIPSIS, TabContainerHandle, TabPlacement, TabSpec, fill_tab_container,
    spawn_tab_container,
};
use crate::world_api::LocalChatNotice;

use sl_settings::SettingValue;

/// The floater's stable id (see [`crate::floater::Floater::id`]).
pub(crate) const SNAPSHOT_FLOATER_ID: &str = "snapshot";

/// The body font size, in logical pixels.
const FONT_SIZE: f32 = 13.0;

/// A control label's colour.
const LABEL_COLOR: Color = Color::srgb(0.90, 0.92, 0.96);

/// A dim / secondary text colour (the hint and status lines).
const HINT_COLOR: Color = Color::srgb(0.62, 0.66, 0.74);

/// A checked toggle's tick colour.
const CHECK_COLOR: Color = Color::srgb(0.55, 0.85, 0.60);

/// A button's background.
const BUTTON_BACKGROUND: Color = Color::srgb(0.13, 0.15, 0.20);

/// A button's border.
const BUTTON_BORDER: Color = Color::srgb(0.34, 0.40, 0.52);

/// The bevy_flair class the skinnable buttons carry.
const BUTTON_CLASS: &str = "sk-button";

/// The preview frame's border.
const PREVIEW_BORDER: Color = Color::srgb(0.30, 0.34, 0.42);

/// The preview frame's fill behind the (letterboxed) image.
const PREVIEW_BACKGROUND: Color = Color::srgb(0.02, 0.02, 0.03);

/// The widest the preview image is drawn, in logical pixels; its height follows
/// the captured frame's aspect (letterboxed inside a fixed frame so the floater
/// does not resize between shots). Sized for a comfortable compose-the-shot view.
const PREVIEW_MAX_WIDTH: f32 = 640.0;

/// The tallest the preview image is drawn, in logical pixels (see
/// [`PREVIEW_MAX_WIDTH`]).
const PREVIEW_MAX_HEIGHT: f32 = 400.0;

/// The glyph for a checked toggle.
const CHECKED_GLYPH: &str = "\u{2611}";

/// The glyph for an unchecked toggle.
const UNCHECKED_GLYPH: &str = "\u{2610}";

/// How many frames to wait after hiding the excluded layers before the shutter,
/// so the hidden UI / disabled HUD camera is actually rendered out first.
const HIDE_FRAMES: u8 = 1;

/// One selectable output format: the file extension that drives the encoder and its
/// display label.
#[derive(Debug, Clone, Copy)]
struct FormatPreset {
    /// The file extension (also what `image` infers the encoder from).
    extension: &'static str,
    /// The combo label shown for it.
    label: &'static str,
}

/// The selectable output formats, in menu order. PNG (lossless, the default), JPEG
/// (small, lossy), BMP and TGA — the set the workspace `image` build encodes.
const FORMATS: &[FormatPreset] = &[
    FormatPreset {
        extension: "png",
        label: "PNG",
    },
    FormatPreset {
        extension: "jpg",
        label: "JPEG",
    },
    FormatPreset {
        extension: "bmp",
        label: "BMP",
    },
    FormatPreset {
        extension: "tga",
        label: "TGA",
    },
];

/// The default format index (PNG).
const DEFAULT_FORMAT: usize = 0;

/// The settings section the snapshot preferences are grouped under in the
/// persisted file (`[snapshot]`).
const SETTINGS_SECTION: &[&str] = &["snapshot"];

/// The (flat, global) setting name for the last-used format index. The store's
/// lookup namespace is flat — the section above is only file grouping — so names
/// are prefixed to stay distinct, as [`crate::floater_persist`]'s keys are.
const SETTING_FORMAT: &str = "snapshot_format";

/// The setting name for whether the UI is kept in the shot.
const SETTING_INCLUDE_UI: &str = "snapshot_include_ui";

/// The setting name for whether worn HUD attachments are kept in the shot.
const SETTING_INCLUDE_HUD: &str = "snapshot_include_hud";

/// The setting name for whether the status-bar L$ balance is blanked in a shot
/// that includes the UI (the reference viewer's `RenderHideBalanceInSnapshot`).
const SETTING_HIDE_BALANCE: &str = "snapshot_hide_balance";

// ---------------------------------------------------------------------------
// Plugin.
// ---------------------------------------------------------------------------

/// The plugin wiring the snapshot floater into the viewer.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct SnapshotFloaterPlugin;

impl Plugin for SnapshotFloaterPlugin {
    /// Spawn the floater once the UI root exists, then drive the preferences, the
    /// capture state machine and the shot processing. The persisted defaults are
    /// registered separately via [`register_settings`], called from
    /// [`ViewerSettings`]'s `FromWorld` like every other feature's settings.
    fn build(&self, app: &mut App) {
        app.init_resource::<SnapshotState>()
            .init_resource::<CapturedShot>()
            .add_message::<RequestSnapshotCapture>()
            .add_systems(
                Startup,
                spawn_snapshot_floater.after(UiScaffoldSystems::SpawnRoot),
            )
            .add_systems(
                Update,
                (
                    load_persisted_preferences,
                    apply_format_combo,
                    update_toggle_glyphs,
                    update_status_text,
                    snapshot_hotkey,
                    start_capture,
                    drive_capture,
                    process_shot,
                    poll_snapshot_saves,
                )
                    .chain(),
            );
    }
}

/// Register the snapshot settings defaults so the store round-trips them (called
/// from [`ViewerSettings`]'s `FromWorld`).
pub(crate) fn register_settings(settings: &mut ViewerSettings) {
    let format = i32::try_from(DEFAULT_FORMAT).unwrap_or(0);
    settings.register_in(
        SETTINGS_SECTION,
        SETTING_FORMAT,
        SettingValue::I32(format),
        "last-used snapshot output format (index into the floater's list)",
    );
    settings.register_in(
        SETTINGS_SECTION,
        SETTING_INCLUDE_UI,
        SettingValue::Bool(false),
        "keep the viewer UI in saved snapshots",
    );
    settings.register_in(
        SETTINGS_SECTION,
        SETTING_INCLUDE_HUD,
        SettingValue::Bool(false),
        "keep worn HUD attachments in saved snapshots",
    );
    settings.register_in(
        SETTINGS_SECTION,
        SETTING_HIDE_BALANCE,
        SettingValue::Bool(false),
        "blank the status-bar L$ balance in snapshots that include the UI",
    );
}

// ---------------------------------------------------------------------------
// State + handles.
// ---------------------------------------------------------------------------

/// Where a capture is in the hide → shoot → restore cycle. This drives only the
/// **visual** hide/restore of the excluded layers; whether a new capture may start
/// is the separate [`SnapshotState::busy`] latch, so a delayed or lost screenshot
/// callback can never strand the UI hidden — the restore runs on this timer
/// regardless.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum CapturePhase {
    /// No capture in flight.
    #[default]
    Idle,
    /// Excluded layers hidden; counting down before the shutter.
    Waiting(u8),
    /// The screenshot has been requested this frame; the hidden layers are restored
    /// next frame (after the shot's frame has rendered).
    Restoring,
}

/// The transient status-line state (rendered to text by [`update_status_text`], so
/// it re-localises when the locale loads / changes).
#[derive(Debug, Clone, Default)]
enum StatusKind {
    /// The idle "Ready" line.
    #[default]
    Ready,
    /// A capture is in flight.
    Working,
    /// A finished, already-formatted message (a saved path, or an error).
    Message(String),
}

/// The floater's preferences and in-flight capture state.
#[derive(Resource, Debug)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each bool is an independent flag: the three persisted include-UI / include-HUD / \
              hide-balance preferences, the save-vs-refresh mode of the current capture, the three \
              what-this-capture-hid flags, and the one-shot settings-loaded latch"
)]
struct SnapshotState {
    /// The selected format index, into [`FORMATS`].
    format: usize,
    /// Whether the viewer UI is kept in the shot.
    include_ui: bool,
    /// Whether worn HUD attachments are kept in the shot.
    include_hud: bool,
    /// Whether the status-bar L$ balance is blanked in a UI-included shot.
    hide_balance: bool,
    /// Where the current capture is in its hide/restore cycle (visual only).
    phase: CapturePhase,
    /// Whether a capture is in flight — set when one starts, cleared when its shot
    /// is processed. Guards against a second capture overwriting the first's frame;
    /// kept separate from [`Self::phase`] so the UI-restore never waits on it.
    busy: bool,
    /// Whether the in-flight capture also saves to disk (vs. only refreshing the
    /// preview).
    save_after: bool,
    /// Whether this capture hid the UI (so [`process_shot`] restores exactly what
    /// it hid).
    hid_ui: bool,
    /// Whether this capture disabled the HUD camera.
    hid_hud: bool,
    /// Whether this capture blanked the status-bar balance.
    hid_balance: bool,
    /// Whether the persisted preferences have been loaded into the controls yet.
    loaded: bool,
    /// A per-session counter appended to the filename so two saves in the same
    /// wall-clock second do not collide.
    counter: u32,
    /// The status line's current state.
    status: StatusKind,
}

impl Default for SnapshotState {
    /// Start on the built-in defaults, nothing in flight.
    fn default() -> Self {
        Self {
            format: DEFAULT_FORMAT,
            include_ui: false,
            include_hud: false,
            hide_balance: false,
            phase: CapturePhase::Idle,
            busy: false,
            save_after: false,
            hid_ui: false,
            hid_hud: false,
            hid_balance: false,
            loaded: false,
            counter: 0,
            status: StatusKind::Ready,
        }
    }
}

impl SnapshotState {
    /// The selected format's file extension.
    fn extension(&self) -> &'static str {
        FORMATS
            .get(self.format)
            .or_else(|| FORMATS.get(DEFAULT_FORMAT))
            .map_or("png", |preset| preset.extension)
    }
}

/// The captured window frame, handed from the [`ScreenshotCaptured`] observer to
/// [`process_shot`] (which restores the UI, updates the preview and saves).
#[derive(Resource, Debug, Default)]
struct CapturedShot(Option<Image>);

/// The system's local time zone, resolved **once at startup**
/// ([`capture_local_timezone`]) and reused to stamp snapshot filenames.
///
/// Resolving the zone reads the `TZ` environment variable, and reading the
/// environment is only sound while the process is still single-threaded — before
/// Bevy's task pools spawn. So it is captured in `main`, not on each save; a save
/// then only reads the (thread-safe) monotonic clock and applies this cached zone.
#[derive(Resource, Clone)]
pub(crate) struct LocalTimeZone(jiff::tz::TimeZone);

impl LocalTimeZone {
    /// Resolve the system time zone now. Call this **early**, while the process is
    /// still single-threaded (see the type docs).
    #[must_use]
    pub(crate) fn capture() -> Self {
        Self(jiff::tz::TimeZone::system())
    }

    /// The captured zone, for another surface rendering a stored timestamp in
    /// local time (the derender blacklist's Date column,
    /// [`crate::asset_blacklist`]).
    #[must_use]
    pub(crate) const fn zone(&self) -> &jiff::tz::TimeZone {
        &self.0
    }
}

/// The floater's live entity handles.
#[derive(Resource, Debug)]
pub(crate) struct SnapshotUi {
    /// The preview [`ImageNode`], resized to the captured frame's aspect.
    preview: Entity,
    /// The "click Refresh" hint shown until the first capture.
    preview_hint: Entity,
    /// The include-UI checkbox glyph node.
    ui_glyph: Entity,
    /// The include-HUD checkbox glyph node.
    hud_glyph: Entity,
    /// The hide-L$-balance checkbox glyph node.
    balance_glyph: Entity,
    /// The format combo anchor.
    format_combo: Entity,
    /// The transient status text node.
    status: Entity,
}

/// A press on a control that requests a capture (Refresh or a destination's save).
#[derive(Message, Debug, Clone, Copy)]
struct RequestSnapshotCapture {
    /// Whether to also save the shot to disk (vs. only refresh the preview).
    save: bool,
}

/// Which include-toggle a checkbox drives.
#[derive(Component, Debug, Clone, Copy)]
enum SnapshotToggle {
    /// Keep the viewer UI in the shot.
    Ui,
    /// Keep worn HUD attachments in the shot.
    Hud,
    /// Blank the status-bar L$ balance in a UI-included shot.
    Balance,
}

// ---------------------------------------------------------------------------
// Spawn.
// ---------------------------------------------------------------------------

/// The snapshot floater's [`FloaterSpec`](crate::floater::FloaterSpec) — shared
/// with the `FLOATERS` registry, so the swept window is the one the viewer
/// spawns.
pub(crate) fn snapshot_floater_spec() -> crate::floater::FloaterSpec {
    crate::floater::FloaterSpec {
        id: SNAPSHOT_FLOATER_ID,
        title: "Snapshot".to_owned(),
        position: Vec2::new(320.0, 90.0),
        default_size: None,
        min_size: None,
        dock_host: None,
        caps: crate::floater::FloaterCaps {
            resizable: false,
            minimizable: false,
            closable: true,
            dockable: false,
        },
    }
}

/// Spawn the (hidden) snapshot floater: the preview frame, the include toggles, the
/// Refresh button, the format picker and the destination tabs.
fn spawn_snapshot_floater(mut commands: Commands, root: Res<UiRoot>) {
    let handle = crate::floater::spawn_floater(&mut commands, root.0, snapshot_floater_spec());
    commands
        .entity(handle.title_text)
        .insert(Translated::new("snapshot-title"));
    let builder = commands.register_system(build_snapshot_content);
    commands
        .entity(handle.root)
        .insert(crate::floater::DeferredFloaterContent { builder, handle });
}

/// First-open content build (see the chrome spawn above): preview, toggles,
/// format row and buttons, ending with the [`SnapshotUi`] insert.
fn build_snapshot_content(
    In(handle): In<crate::floater::FloaterHandle>,
    mut commands: Commands,
    state: Res<SnapshotState>,
) {
    let content = commands
        .spawn((
            Node {
                width: Val::Px(PREVIEW_MAX_WIDTH),
                ..column(Val::Px(8.0))
            },
            ChildOf(handle.content),
        ))
        .id();

    let (preview, preview_hint) = spawn_preview(&mut commands, content);

    // The Refresh row.
    let refresh_row = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                ..row(Val::Px(8.0))
            },
            ChildOf(content),
        ))
        .id();
    let refresh = spawn_text_button(&mut commands, refresh_row, "snapshot-refresh", 1);
    commands.entity(refresh).observe(
        |_activate: On<Activate>, mut requests: MessageWriter<RequestSnapshotCapture>| {
            requests.write(RequestSnapshotCapture { save: false });
        },
    );

    let (ui_button, ui_glyph) = spawn_checkbox(&mut commands, content, "snapshot-include-ui", 2);
    commands
        .entity(ui_button)
        .insert(SnapshotToggle::Ui)
        .observe(toggle_pressed);
    let (hud_button, hud_glyph) = spawn_checkbox(&mut commands, content, "snapshot-include-hud", 3);
    commands
        .entity(hud_button)
        .insert(SnapshotToggle::Hud)
        .observe(toggle_pressed);
    let (balance_button, balance_glyph) =
        spawn_checkbox(&mut commands, content, "snapshot-hide-balance", 4);
    commands
        .entity(balance_button)
        .insert(SnapshotToggle::Balance)
        .observe(toggle_pressed);

    let format_row = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                ..row(Val::Px(6.0))
            },
            ChildOf(content),
        ))
        .id();
    commands.spawn((
        Text::default(),
        Translated::new("snapshot-format-label"),
        UiFont::Sans.at(FONT_SIZE),
        TextColor(LABEL_COLOR),
        ChildOf(format_row),
    ));
    let format_combo = spawn_combo(
        &mut commands,
        format_row,
        &ComboSpec {
            element: "snapshot-format",
            labels: &format_labels(),
            active: state.format,
            tab_index: 5,
            font_size: FONT_SIZE,
            translate_labels: false,
        },
    );

    let status = commands
        .spawn((
            Text::default(),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(HINT_COLOR),
            Name::new("snapshot-status"),
            ChildOf(content),
        ))
        .id();

    spawn_destination_tabs(&mut commands, content);

    commands.insert_resource(SnapshotUi {
        preview,
        preview_hint,
        ui_glyph,
        hud_glyph,
        balance_glyph,
        format_combo,
        status,
    });
}

/// Spawn the preview frame: a fixed, centred frame holding the (initially empty)
/// preview [`ImageNode`] and a "click Refresh" hint shown until the first shot.
/// Returns the image node and the hint node.
fn spawn_preview(commands: &mut Commands, parent: Entity) -> (Entity, Entity) {
    let frame = commands
        .spawn((
            Node {
                width: Val::Px(PREVIEW_MAX_WIDTH),
                height: Val::Px(PREVIEW_MAX_HEIGHT),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border: UiRect::all(Val::Px(1.0)),
                overflow: Overflow::clip(),
                ..default()
            },
            BorderColor::all(PREVIEW_BORDER),
            BackgroundColor(PREVIEW_BACKGROUND),
            Name::new("snapshot-preview-frame"),
            ChildOf(parent),
        ))
        .id();
    // The preview image, hidden until the first capture points it at a frame.
    let preview = commands
        .spawn((
            ImageNode::default(),
            Node {
                display: Display::None,
                ..default()
            },
            Name::new("snapshot-preview"),
            ChildOf(frame),
        ))
        .id();
    let preview_hint = commands
        .spawn((
            Text::default(),
            Translated::new("snapshot-preview-empty"),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(HINT_COLOR),
            ChildOf(frame),
        ))
        .id();
    (preview, preview_hint)
}

/// Spawn the destination tabs: **Save to Disk** (live) and the placeholder
/// Postcard / Profile / Inventory tabs (each a "coming in its own task" note).
fn spawn_destination_tabs(commands: &mut Commands, parent: Entity) {
    let labels: Vec<String> = [
        "snapshot-tab-disk",
        "snapshot-tab-postcard",
        "snapshot-tab-profile",
        "snapshot-tab-inventory",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    let tabs: TabContainerHandle = spawn_tab_container(
        commands,
        parent,
        &TabSpec {
            element: "snapshot-tabs",
            placement: TabPlacement::BlockStart,
            labels: &labels,
            active: 0,
            tab_index: 6,
            font_size: FONT_SIZE,
            strip_width: None,
            ellipsis: DEFAULT_ELLIPSIS,
            translate_labels: true,
        },
    );
    fill_tab_container(commands, TabPlacement::BlockStart, &tabs);
    let panel = |index: usize| tabs.panels.get(index).copied().unwrap_or(parent);

    // Save to Disk: the Save button and a one-line hint.
    let disk = panel(0);
    let save = spawn_text_button(commands, disk, "snapshot-save-disk", 7);
    commands.entity(save).observe(
        |_activate: On<Activate>, mut requests: MessageWriter<RequestSnapshotCapture>| {
            requests.write(RequestSnapshotCapture { save: true });
        },
    );
    spawn_note(commands, disk, "snapshot-hint");

    // The placeholder destinations, each pointing at its own follow-up task.
    spawn_note(commands, panel(1), "snapshot-postcard-todo");
    spawn_note(commands, panel(2), "snapshot-profile-todo");
    spawn_note(commands, panel(3), "snapshot-inventory-todo");
}

/// Spawn a dim, wrapping note line under `parent`.
fn spawn_note(commands: &mut Commands, parent: Entity, key: &'static str) {
    commands.spawn((
        Text::default(),
        Translated::new(key),
        UiFont::Sans.at(FONT_SIZE),
        TextColor(HINT_COLOR),
        Node {
            max_width: Val::Px(PREVIEW_MAX_WIDTH),
            ..default()
        },
        ChildOf(parent),
    ));
}

/// Spawn a translated-label push button, returning its clickable box.
fn spawn_text_button(
    commands: &mut Commands,
    parent: Entity,
    label_key: &'static str,
    tab: i32,
) -> Entity {
    let button = commands
        .spawn((
            Button,
            TabIndex(tab),
            Node {
                padding: UiRect::axes(Val::Px(10.0), Val::Px(4.0)),
                border: UiRect::all(Val::Px(2.0)),
                align_self: AlignSelf::Start,
                ..default()
            },
            BackgroundColor(BUTTON_BACKGROUND),
            BorderColor::all(BUTTON_BORDER),
            ClassList::new_with_classes([BUTTON_CLASS]),
            Name::new("snapshot-button"),
            ChildOf(parent),
        ))
        .id();
    commands.spawn((
        Text::default(),
        Translated::new(label_key),
        UiFont::Sans.at(FONT_SIZE),
        TextColor(LABEL_COLOR),
        Pickable::IGNORE,
        ChildOf(button),
    ));
    button
}

/// Spawn a glyph checkbox (a clickable box with a ☐/☑ glyph then a label),
/// returning the clickable box and its glyph node. The caller inserts the
/// [`SnapshotToggle`] tag and the press observer.
fn spawn_checkbox(
    commands: &mut Commands,
    parent: Entity,
    label_key: &'static str,
    tab: i32,
) -> (Entity, Entity) {
    let button = commands
        .spawn((
            Button,
            TabIndex(tab),
            Node {
                align_items: AlignItems::Center,
                column_gap: Val::Px(6.0),
                ..row(Val::Px(0.0))
            },
            Name::new("snapshot-checkbox"),
            ChildOf(parent),
        ))
        .id();
    let glyph = commands
        .spawn((
            Text::new(UNCHECKED_GLYPH.to_owned()),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(HINT_COLOR),
            Pickable::IGNORE,
            ChildOf(button),
        ))
        .id();
    commands.spawn((
        Text::default(),
        Translated::new(label_key),
        UiFont::Sans.at(FONT_SIZE),
        TextColor(LABEL_COLOR),
        Pickable::IGNORE,
        ChildOf(button),
    ));
    (button, glyph)
}

/// A checkbox press: flip its toggle in the state and persist the new value.
fn toggle_pressed(
    activate: On<Activate>,
    toggles: Query<&SnapshotToggle>,
    mut state: ResMut<SnapshotState>,
    mut settings: ResMut<ViewerSettings>,
) {
    let Ok(toggle) = toggles.get(activate.entity) else {
        return;
    };
    let (value, name) = match toggle {
        SnapshotToggle::Ui => {
            state.include_ui = !state.include_ui;
            (state.include_ui, SETTING_INCLUDE_UI)
        }
        SnapshotToggle::Hud => {
            state.include_hud = !state.include_hud;
            (state.include_hud, SETTING_INCLUDE_HUD)
        }
        SnapshotToggle::Balance => {
            state.hide_balance = !state.hide_balance;
            (state.hide_balance, SETTING_HIDE_BALANCE)
        }
    };
    settings.set_account(name, SettingValue::Bool(value));
}

/// The format combo's literal labels.
fn format_labels() -> Vec<String> {
    FORMATS
        .iter()
        .map(|preset| preset.label.to_owned())
        .collect()
}

// ---------------------------------------------------------------------------
// Preferences.
// ---------------------------------------------------------------------------

/// Load the persisted format / include-UI / include-HUD / hide-balance
/// preferences once, after login resolves the account settings.
fn load_persisted_preferences(
    settings: Res<ViewerSettings>,
    ui: Option<Res<SnapshotUi>>,
    mut state: ResMut<SnapshotState>,
    mut selections: Query<&mut ComboSelection>,
) {
    if state.loaded {
        return;
    }
    let Some(ui) = ui else {
        return;
    };
    // Wait for the account scope so a per-avatar choice is honoured.
    if !settings.account_loaded() {
        return;
    }
    let store = settings.store();
    if let Ok(value) = store.get_i32(SETTING_FORMAT) {
        state.format = clamp_index(value, FORMATS.len());
    }
    if let Ok(value) = store.get_bool(SETTING_INCLUDE_UI) {
        state.include_ui = value;
    }
    if let Ok(value) = store.get_bool(SETTING_INCLUDE_HUD) {
        state.include_hud = value;
    }
    if let Ok(value) = store.get_bool(SETTING_HIDE_BALANCE) {
        state.hide_balance = value;
    }
    if let Ok(mut selection) = selections.get_mut(ui.format_combo) {
        selection.active = state.format;
    }
    state.loaded = true;
}

/// Clamp a stored index into `[0, len)`, defaulting to 0 on a negative or empty
/// value.
fn clamp_index(value: i32, len: usize) -> usize {
    let last = len.saturating_sub(1);
    usize::try_from(value).unwrap_or(0).min(last)
}

/// Apply the user's format combo picks and persist the choice.
fn apply_format_combo(
    mut changes: MessageReader<ComboChanged>,
    ui: Option<Res<SnapshotUi>>,
    mut state: ResMut<SnapshotState>,
    mut settings: ResMut<ViewerSettings>,
) {
    let Some(ui) = ui else {
        changes.clear();
        return;
    };
    for change in changes.read() {
        if change.combo == ui.format_combo {
            state.format = clamp_index(i32::try_from(change.active).unwrap_or(0), FORMATS.len());
            let value = i32::try_from(state.format).unwrap_or(0);
            settings.set_account(SETTING_FORMAT, SettingValue::I32(value));
        }
    }
}

/// Keep the three checkbox glyphs in sync with the toggles.
fn update_toggle_glyphs(
    state: Res<SnapshotState>,
    ui: Option<Res<SnapshotUi>>,
    mut texts: Query<(&mut Text, &mut TextColor)>,
) {
    if !state.is_changed() {
        return;
    }
    let Some(ui) = ui else {
        return;
    };
    set_check_glyph(&mut texts, ui.ui_glyph, state.include_ui);
    set_check_glyph(&mut texts, ui.hud_glyph, state.include_hud);
    set_check_glyph(&mut texts, ui.balance_glyph, state.hide_balance);
}

/// Set one checkbox glyph's text and colour to reflect its checked state.
fn set_check_glyph(texts: &mut Query<(&mut Text, &mut TextColor)>, node: Entity, checked: bool) {
    let (glyph, color) = if checked {
        (CHECKED_GLYPH, CHECK_COLOR)
    } else {
        (UNCHECKED_GLYPH, HINT_COLOR)
    };
    if let Ok((mut text, mut text_color)) = texts.get_mut(node) {
        if text.0 != glyph {
            glyph.clone_into(&mut text.0);
        }
        if text_color.0 != color {
            text_color.0 = color;
        }
    }
}

/// Render the status line from the state each frame, so it re-localises once the
/// locale bundle loads (a manually-set path message stays as it was formatted).
fn update_status_text(
    state: Res<SnapshotState>,
    ui: Option<Res<SnapshotUi>>,
    translator: Translator,
    mut texts: Query<&mut Text>,
) {
    let Some(ui) = ui else {
        return;
    };
    let wanted = match &state.status {
        StatusKind::Ready => translator.get("snapshot-status-ready"),
        StatusKind::Working => translator.get("snapshot-status-saving"),
        StatusKind::Message(message) => message.clone(),
    };
    if let Ok(mut text) = texts.get_mut(ui.status)
        && text.0 != wanted
    {
        text.0 = wanted;
    }
}

// ---------------------------------------------------------------------------
// Capture state machine.
// ---------------------------------------------------------------------------

/// `Ctrl+`` (Ctrl+Backquote) takes a **quick snapshot straight to disk** with the
/// last-used settings and no floater — the reference's quick-snapshot key
/// (`viewer-snapshot-quick-key`). It just requests the same save the Save button
/// does, so the capture path saves the file and echoes its path to nearby chat;
/// the floater need not be open (its `SnapshotState` / `SnapshotUi` exist from
/// startup). Gated like the other build-tool chords so it never fires while a text
/// field owns the keyboard.
fn snapshot_hotkey(
    keyboard: Res<ButtonInput<KeyCode>>,
    context: Res<crate::world_api::InputContext>,
    mut requests: MessageWriter<RequestSnapshotCapture>,
) {
    if *context == crate::world_api::InputContext::TextEntry {
        return;
    }
    let ctrl = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    if ctrl && keyboard.just_pressed(KeyCode::Backquote) {
        requests.write(RequestSnapshotCapture { save: true });
    }
}

/// Begin a capture on a request: hide the excluded layers (UI / HUD) and arm the
/// shutter countdown. Ignores a request while one is already in flight.
fn start_capture(
    mut requests: MessageReader<RequestSnapshotCapture>,
    mut state: ResMut<SnapshotState>,
    root: Res<UiRoot>,
    mut nodes: Query<&mut Node>,
    mut hud: Query<&mut Visibility, (With<HudScreen>, Without<BalanceReadout>)>,
    mut balance: Query<&mut Visibility, (With<BalanceReadout>, Without<HudScreen>)>,
) {
    // Collapse a burst of clicks; a request while busy is dropped.
    let Some(request) = requests.read().last().copied() else {
        return;
    };
    if state.busy {
        return;
    }
    state.busy = true;
    state.save_after = request.save;
    state.hid_ui = !state.include_ui;
    state.hid_hud = !state.include_hud;
    state.hid_balance = should_hide_balance(state.include_ui, state.hide_balance);
    if state.hid_ui
        && let Ok(mut node) = nodes.get_mut(root.0)
    {
        node.display = Display::None;
    }
    if state.hid_hud {
        for mut visibility in &mut hud {
            *visibility = Visibility::Hidden;
        }
    }
    if state.hid_balance {
        for mut visibility in &mut balance {
            *visibility = Visibility::Hidden;
        }
    }
    state.phase = CapturePhase::Waiting(HIDE_FRAMES);
    state.status = StatusKind::Working;
}

/// Whether this capture blanks the status-bar balance: only when the balance
/// toggle is set **and** the UI is in the frame. With the UI excluded the whole
/// status bar is hidden already ([`UiRoot`] `Display::None`), so blanking the
/// balance would be a redundant no-op — the reference viewer only applies
/// `RenderHideBalanceInSnapshot` to interface-included shots.
const fn should_hide_balance(include_ui: bool, hide_balance: bool) -> bool {
    include_ui && hide_balance
}

/// Advance the shutter countdown; when it elapses, request the window screenshot;
/// the frame after, restore the hidden layers. This restore runs on the timer, not
/// on the shot's arrival, so the UI can never be stranded hidden.
fn drive_capture(
    mut state: ResMut<SnapshotState>,
    mut commands: Commands,
    root: Res<UiRoot>,
    mut nodes: Query<&mut Node>,
    mut hud: Query<&mut Visibility, (With<HudScreen>, Without<BalanceReadout>)>,
    mut balance: Query<&mut Visibility, (With<BalanceReadout>, Without<HudScreen>)>,
    mut ui_sound: MessageWriter<crate::ui_sounds::PlayUiSound>,
) {
    match state.phase {
        CapturePhase::Idle => {}
        CapturePhase::Waiting(frames) if frames > 0 => {
            state.phase = CapturePhase::Waiting(frames.saturating_sub(1));
        }
        CapturePhase::Waiting(_zero) => {
            // The shutter fires now — play the reference viewer's shutter click.
            ui_sound.write(crate::ui_sounds::PlayUiSound(
                crate::ui_sounds::UiSound::Snapshot,
            ));
            // Read the window frame back into `CapturedShot`; `process_shot` finishes.
            commands.spawn(Screenshot::primary_window()).observe(
                |captured: On<ScreenshotCaptured>,
                 mut shot: ResMut<CapturedShot>,
                 mut commands: Commands| {
                    shot.0 = Some(captured.image.clone());
                    // One-shot; drop the capture entity so a save does not leak one.
                    commands.entity(captured.entity).despawn();
                },
            );
            state.phase = CapturePhase::Restoring;
        }
        CapturePhase::Restoring => {
            if state.hid_ui
                && let Ok(mut node) = nodes.get_mut(root.0)
            {
                node.display = Display::Flex;
            }
            if state.hid_hud {
                for mut visibility in &mut hud {
                    *visibility = Visibility::Inherited;
                }
            }
            if state.hid_balance {
                for mut visibility in &mut balance {
                    *visibility = Visibility::Inherited;
                }
            }
            state.phase = CapturePhase::Idle;
        }
    }
}

/// Finish a capture once its frame lands: update the preview, and (for a save)
/// hand the frame to an off-thread [`spawn_save_task`] that encodes and writes it.
/// The visual restore of the hidden layers is handled on a timer by
/// [`drive_capture`], not here, so it never waits on this callback; the saved path
/// is echoed to chat later by [`poll_snapshot_saves`], once the write completes.
#[expect(
    clippy::too_many_arguments,
    reason = "finishing one capture reads the shot + state, updates the preview image asset + its \
              node + hint, and spawns the off-thread write while setting the localised status line"
)]
fn process_shot(
    mut shot: ResMut<CapturedShot>,
    mut state: ResMut<SnapshotState>,
    ui: Option<Res<SnapshotUi>>,
    mut images: ResMut<Assets<Image>>,
    mut nodes: Query<&mut Node>,
    mut image_nodes: Query<&mut ImageNode>,
    mut commands: Commands,
    local_tz: Option<Res<LocalTimeZone>>,
    translator: Translator,
) {
    let Some(image) = shot.0.take() else {
        return;
    };
    // The capture is finished being consumed; allow the next one.
    state.busy = false;

    let Some(ui) = ui else {
        return;
    };
    // The frame as an opaque RGB image (dropping the HDR alpha, which carries
    // brightness) — the one form used for both the preview and the saved file.
    let dynamic = match image.try_into_dynamic() {
        Ok(dynamic) => image::DynamicImage::ImageRgb8(dynamic.to_rgb8()),
        Err(error) => {
            state.status = StatusKind::Message(translator.format(
                "snapshot-save-failed",
                &TransArgs::new().text("error", &error.to_string()),
            ));
            return;
        }
    };

    update_preview(&ui, &dynamic, &mut images, &mut image_nodes, &mut nodes);

    if !state.save_after {
        state.status = StatusKind::Ready;
        return;
    }
    // Resolve the destination on the frame thread (cheap, no IO), then offload the
    // encode + write so the heavy PNG/JPEG deflate and disk write never stall the
    // frame. The status stays `Working` until `poll_snapshot_saves` reports the
    // finished write; only the (synchronous) "no directory" case resolves here.
    match resolve_save_dest(&mut state, local_tz.as_deref()) {
        Ok(dest) => {
            commands.spawn(SnapshotSaveTask(spawn_save_task(dynamic, dest)));
        }
        Err(SaveError::NoDir) => {
            state.status = StatusKind::Message(translator.get("snapshot-no-dir"));
        }
        Err(SaveError::Io(error)) => {
            state.status = StatusKind::Message(translator.format(
                "snapshot-save-failed",
                &TransArgs::new().text("error", &error),
            ));
        }
    }
}

/// A pending off-thread snapshot write, spawned by [`process_shot`] and drained by
/// [`poll_snapshot_saves`]. The task yields the written path on success, or a
/// formatted error string (a `create_dir_all` or encode/write failure) — so a
/// failed save still surfaces on the status line rather than being swallowed.
#[derive(Component)]
struct SnapshotSaveTask(Task<Result<PathBuf, String>>);

/// Poll the off-thread snapshot writes; when one finishes, echo the saved path to
/// nearby chat and the status line (or surface the write error), then drop the task
/// entity. Runs every frame after [`process_shot`]; a write in flight costs one
/// cheap non-blocking poll.
fn poll_snapshot_saves(
    mut commands: Commands,
    mut tasks: Query<(Entity, &mut SnapshotSaveTask)>,
    mut state: ResMut<SnapshotState>,
    mut notices: MessageWriter<LocalChatNotice>,
    translator: Translator,
) {
    for (entity, mut task) in &mut tasks {
        let Some(result) = block_on(poll_once(&mut task.0)) else {
            continue;
        };
        match result {
            Ok(path) => {
                let saved = path.display().to_string();
                let message =
                    translator.format("snapshot-saved", &TransArgs::new().text("path", &saved));
                notices.write(LocalChatNotice::new(message.clone()));
                state.status = StatusKind::Message(message);
            }
            Err(error) => {
                state.status = StatusKind::Message(translator.format(
                    "snapshot-save-failed",
                    &TransArgs::new().text("error", &error),
                ));
            }
        }
        commands.entity(entity).despawn();
    }
}

/// Point the preview [`ImageNode`] at the captured frame, size it to fit the frame
/// by the shot's aspect, and hide the "click Refresh" hint.
fn update_preview(
    ui: &SnapshotUi,
    dynamic: &image::DynamicImage,
    images: &mut Assets<Image>,
    image_nodes: &mut Query<&mut ImageNode>,
    nodes: &mut Query<&mut Node>,
) {
    let width = dynamic.width();
    let height = dynamic.height();
    let handle = images.add(Image::from_dynamic(
        dynamic.clone(),
        true,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    ));
    if let Ok(mut image_node) = image_nodes.get_mut(ui.preview) {
        image_node.image = handle;
    }
    let display = fit_within(width, height);
    if let Ok(mut node) = nodes.get_mut(ui.preview) {
        node.display = Display::Flex;
        node.width = Val::Px(display.x);
        node.height = Val::Px(display.y);
    }
    if let Ok(mut hint) = nodes.get_mut(ui.preview_hint) {
        hint.display = Display::None;
    }
}

/// A disk-save failure resolved **synchronously**, before the off-thread write.
enum SaveError {
    /// No snapshots directory exists on this platform.
    NoDir,
    /// The selected extension has no `image` encoder (should not happen — every
    /// [`FORMATS`] entry is validated in tests — but handled rather than panicked).
    Io(String),
}

/// The resolved destination for a snapshot save: the directory to ensure exists, the
/// full output path, and the encoder inferred from its extension. Only these (pure,
/// no-IO) decisions are made on the frame thread; [`spawn_save_task`] does the
/// `create_dir_all`, encode and write off it.
struct SaveDest {
    /// The snapshots directory (created by the write task if it is missing).
    dir: PathBuf,
    /// The full output path the frame is written to.
    path: PathBuf,
    /// The encoder inferred from the path's extension.
    format: image::ImageFormat,
}

/// Resolve the next output destination under the snapshots directory on the frame
/// thread — a filename carrying a human-readable **local** ISO-8601 stamp (from the
/// startup-captured [`LocalTimeZone`]) plus the per-session counter, and the encoder
/// for the selected format. No IO happens here; the directory create + encode +
/// write is deferred to [`spawn_save_task`].
fn resolve_save_dest(
    state: &mut SnapshotState,
    zone: Option<&LocalTimeZone>,
) -> Result<SaveDest, SaveError> {
    let dir = crate::paths::snapshots_dir().ok_or(SaveError::NoDir)?;
    state.counter = state.counter.wrapping_add(1);
    let name = format!(
        "snapshot-{}-{}.{}",
        local_iso8601_stamp(zone),
        state.counter,
        state.extension()
    );
    let path = dir.join(name);
    let format =
        image::ImageFormat::from_path(&path).map_err(|error| SaveError::Io(error.to_string()))?;
    Ok(SaveDest { dir, path, format })
}

/// Spawn the off-thread encode + write of `dynamic` to `dest` on Bevy's
/// [`IoTaskPool`], returning the [`Task`] [`poll_snapshot_saves`] drains.
///
/// The full-resolution PNG/JPEG encode (a deflate pass) plus the disk write is the
/// several-hundred-millisecond stall that must never run on the frame thread: a
/// hitch there spikes the next frame's `Time::delta`, which — before flexi
/// sub-stepping — made nearby flexi prims visibly re-settle
/// (`viewer-flexi-resettle-after-snapshot`) and still costs a dropped frame for
/// every other per-frame simulation. Off-thread, the capture costs the frame
/// nothing past the (already off-thread) GPU read-back.
fn spawn_save_task(dynamic: image::DynamicImage, dest: SaveDest) -> Task<Result<PathBuf, String>> {
    IoTaskPool::get().spawn(async move {
        let SaveDest { dir, path, format } = dest;
        fs_err::create_dir_all(&dir).map_err(|error| error.to_string())?;
        dynamic
            .save_with_format(&path, format)
            .map_err(|error| error.to_string())?;
        Ok(path)
    })
}

/// The current **local** time as a filename-safe ISO-8601 stamp
/// `YYYY-MM-DDThh-mm-ss` — human-readable (unlike a raw epoch count), with the
/// time's colons rendered as dashes so the name is valid on every filesystem.
///
/// The clock read is thread-safe; only the *zone* (which reads the environment) is
/// the startup-captured [`LocalTimeZone`]. With no captured zone (a harness without
/// it) it falls back to resolving the system zone now.
fn local_iso8601_stamp(zone: Option<&LocalTimeZone>) -> String {
    let zone = zone.map_or_else(jiff::tz::TimeZone::system, |zone| zone.0.clone());
    jiff::Timestamp::now()
        .to_zoned(zone)
        .strftime("%Y-%m-%dT%H-%M-%S")
        .to_string()
}

/// A captured frame's on-screen size, in logical pixels: its aspect fitted inside
/// the [`PREVIEW_MAX_WIDTH`] × [`PREVIEW_MAX_HEIGHT`] frame (letterboxed, never
/// upscaled past the frame).
fn fit_within(width: u32, height: u32) -> Vec2 {
    let width = dimension_to_f32(width);
    let height = dimension_to_f32(height);
    if width <= 0.0 || height <= 0.0 {
        return Vec2::new(PREVIEW_MAX_WIDTH, PREVIEW_MAX_HEIGHT);
    }
    let scale = (PREVIEW_MAX_WIDTH / width).min(PREVIEW_MAX_HEIGHT / height);
    Vec2::new(width * scale, height * scale)
}

/// Convert a pixel dimension to `f32` without an `as` cast (window sizes are all
/// within `u16`).
fn dimension_to_f32(value: u32) -> f32 {
    f32::from(u16::try_from(value).unwrap_or(u16::MAX))
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_FORMAT, FORMATS, PREVIEW_MAX_HEIGHT, PREVIEW_MAX_WIDTH, SnapshotState, clamp_index,
        fit_within, should_hide_balance,
    };
    use pretty_assertions::assert_eq;

    /// The built-in default format index is in range, so the floater opens on a
    /// real format before any saved choice loads.
    #[test]
    fn default_format_in_range() {
        assert!(DEFAULT_FORMAT < FORMATS.len());
    }

    /// A stored index is clamped into range: a negative value floors to the first
    /// option, an over-large one to the last, and an in-range one is preserved.
    #[test]
    fn clamp_index_bounds_the_selection() {
        assert_eq!(clamp_index(-1, 4), 0);
        assert_eq!(clamp_index(0, 4), 0);
        assert_eq!(clamp_index(2, 4), 2);
        assert_eq!(clamp_index(9, 4), 3);
        assert_eq!(clamp_index(2, 0), 0);
    }

    /// The preview always fits inside the frame and never upscales past it, keeping
    /// the captured aspect (a landscape window pins the width, letterboxes height).
    #[test]
    fn preview_fits_and_keeps_aspect() {
        // A 16:9 window fits to the frame width.
        assert_eq!(fit_within(1920, 1080), bevy::math::Vec2::new(640.0, 360.0));
        // A tall window fits to the frame height.
        let portrait = fit_within(600, 800);
        assert!(portrait.y <= PREVIEW_MAX_HEIGHT + 0.01);
        assert!(portrait.x <= PREVIEW_MAX_WIDTH + 0.01);
        // Aspect preserved.
        let ratio = portrait.x / portrait.y;
        assert!((ratio - 0.75).abs() < 0.001, "aspect drifted: {ratio}");
    }

    /// A degenerate (zero) dimension falls back to the full frame rather than
    /// dividing by zero.
    #[test]
    fn preview_survives_a_zero_dimension() {
        assert_eq!(
            fit_within(0, 0),
            bevy::math::Vec2::new(PREVIEW_MAX_WIDTH, PREVIEW_MAX_HEIGHT)
        );
    }

    /// The state resolves its format back to the matching extension, and an
    /// out-of-range index falls back rather than panicking on an empty-list access.
    #[test]
    fn state_resolves_format() {
        let mut state = SnapshotState::default();
        assert_eq!(
            Some(state.extension()),
            FORMATS.get(DEFAULT_FORMAT).map(|preset| preset.extension)
        );
        state.format = FORMATS.len().saturating_add(10);
        assert!(!state.extension().is_empty());
    }

    /// The balance is blanked only when the toggle is set **and** the UI is in
    /// the frame: with the UI excluded the status bar is hidden already, so the
    /// blank is inert there (matching `RenderHideBalanceInSnapshot`, which only
    /// applies to interface-included shots).
    #[test]
    fn balance_hidden_only_with_ui_in_frame() {
        assert!(
            should_hide_balance(true, true),
            "UI in + toggle on hides it"
        );
        assert!(
            !should_hide_balance(false, true),
            "UI excluded: status bar already gone, so a no-op"
        );
        assert!(
            !should_hide_balance(true, false),
            "toggle off keeps the balance even with the UI in frame"
        );
        assert!(!should_hide_balance(false, false), "neither: nothing to do");
    }

    /// Every format's extension is one `image` infers an encoder from.
    #[test]
    fn every_format_extension_is_recognised() {
        for format in FORMATS {
            let path = std::path::PathBuf::from(format!("snap.{}", format.extension));
            assert!(
                image::ImageFormat::from_path(&path).is_ok(),
                "unrecognised extension {}",
                format.extension
            );
        }
    }
}
