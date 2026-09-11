//! The **day-cycle editor** (`viewer-environment-day-cycle-editor`): the window
//! that authors a whole day as an EEP settings **asset** in inventory.
//!
//! Where the fixed editors ([`crate::settings_editor`]) hold one sky or one
//! water frame, this one holds a [`DayCycle`]: five tracks — the region-wide
//! water track and four sky tracks stacked by altitude — each a list of
//! keyframes naming a frame, and the frames themselves. The window is the
//! reference's `LLFloaterEditExtDayCycle`: a track picker down the left, a
//! timeline across the top with a scrubber over it, and the *same* knob tabs the
//! fixed editors show, over whichever keyframe is selected.
//!
//! # The scrubber is what you are standing under
//!
//! The reference does not install the day cycle in `ENV_EDIT` and let the clock
//! run it; it blends the cycle at the scrubber's position into a scratch sky and
//! a scratch water and installs *those*
//! (`LLFloaterEditExtDayCycle::updateEditEnvironment` and its two
//! `LLTrackBlenderLoopingManual`s). So does this window — a frozen frame, not an
//! animated day — which is what makes the scrubber a preview of a *time of day*
//! rather than a request to wait for one. Play then simply walks the scrubber:
//! a whole day in sixty seconds, the reference's own
//! `DAY_CYCLE_PLAY_TIME_SECONDS`.
//!
//! While the water track is selected the sky still has to come from somewhere,
//! and the reference takes it from the ground track (`skytrack = mCurrentTrack ?
//! mCurrentTrack : 1`). So does this.
//!
//! # A keyframe's frame is its own
//!
//! Frames are referenced **by name**, and one asset may legally name the same
//! frame from two keyframes — so a knob written straight into "the selected
//! keyframe's frame" would silently change a keyframe nobody selected. Every
//! edit therefore goes through [`DayCycle::split_shared_frame`] first, which
//! gives the keyframe a copy of its own when (and only when) the frame is
//! shared. The reference has the same rule for a different reason: it holds a
//! `shared_ptr` per keyframe and clones on every insert
//! (`buildDerivedClone`), so two keyframes there are never the same object to
//! begin with.
//!
//! # What a save writes
//!
//! That depends on where the cycle came from (the reference's
//! `KEY_EDIT_CONTEXT`). A cycle opened from **inventory** is re-encoded
//! (`environment_asset_to_bytes`) onto the item it came from, or into a fresh
//! item a Save As mints — the same two paths, and the same shared creation
//! queue, the fixed editors use. A cycle opened from **land** — a region's or a
//! parcel's own environment, sent here by the land-environment panel
//! ([`crate::land_environment`]) — has no asset to write onto, so Save hands it
//! back to that panel and the panel publishes it. Save As still files a copy in
//! inventory either way.
//!
//! Handing back rather than publishing here is deliberate: the panel owns the
//! permission tests, the `?parcelid=` scope and the Apply / Revert pair, and a
//! second path to the capability would be a second copy of all three.
//!
//! # Not done here
//!
//! - **No Import.** Reading a legacy WindLight day preset off disk is the
//!   legacy-preset importer's job (`viewer-environment-import-legacy-presets`).
//!
//! Reference (Firestorm, read-only): `llfloatereditextdaycycle.cpp`,
//! `floater_edit_ext_day_cycle.xml`, `llsettingsdaycycle.cpp`.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui_widgets::{SliderRange, SliderValue, ValueChange};
use sl_client_bevy::{
    AssetKey, AssetUpdateLocation, Command, DayCycle, DayTrack, EnvironmentAsset,
    InventoryFolderKey, InventoryKey, KEYFRAME_SLOP, SKY_TRACK_COUNT, SettingsKind, SkySettings,
    SlCommand, SlEvent, SlSessionEvent, UpdatableAssetType, WaterSettings,
    environment_asset_to_bytes,
};
use sl_viewer_inventory::inventory::InventoryModel;
use sl_viewer_inventory::inventory_actions::{SettingsInventorySupport, new_settings_item};
use sl_viewer_notifications::{NotificationResponse, ShowNotification};
use sl_viewer_pickers::ui_texture_picker::TextureSwatchValue;
use sl_viewer_platform::environment_assets::EnvironmentAssetManager;
use sl_viewer_ui_core::i18n::{TransArgs, Translated, Translator};
use sl_viewer_ui_core::ui::{
    LogicalInset, LogicalRect, UiPanelShown, UiRoot, UiScaffoldSystems, column, row,
};
use sl_viewer_ui_core::ui_font::UiFont;
use sl_viewer_ui_widgets::floater::{
    DeferredFloaterContent, FloaterCaps, FloaterCommand, FloaterHandle, FloaterOp, FloaterSpec,
    FloaterSystems, spawn_floater,
};
use sl_viewer_ui_widgets::floater_persist::FloaterOpenExempt;
use sl_viewer_ui_widgets::ui_color_picker::{ColorPicked, ColorSwatchValue};
use sl_viewer_ui_widgets::ui_combo::{
    ComboRow, ComboSelection, ComboSpec, ComboWidgetPlugin, SetComboOptions, spawn_combo,
};
use sl_viewer_ui_widgets::ui_tab::{
    DEFAULT_ELLIPSIS, TabPlacement, TabSpec, fill_tab_container, spawn_tab_container,
};
use sl_viewer_ui_widgets::ui_text_input::{TextInputKind, TextInputSpec, spawn_text_input};
use sl_viewer_ui_widgets::ui_trackball::TrackballAim;
use sl_viewer_world_api::{
    OpenSettingsEditor, OpenSettingsPicker, PendingSettingsCreations, SettingsItemCreated,
    SettingsPicked, TexturePicked,
};
use sl_viewer_world_scene::environment::EnvironmentState;

use crate::knobs::{ColorKnob, SkyKnob, TextureKnob, WaterKnob};
use crate::land_environment::{LandDayCycleEdited, OpenLandDayCycle};
use crate::rows::{
    AimTrackball, paint_action_button, spawn_action_button, spawn_color_row, spawn_slider,
    spawn_texture_row, spawn_trackball_row, tag_aim_slider,
};
use crate::settings_editor::EditedItem;
use crate::style::{
    CONTROL_BORDER, DIM_LABEL_COLOR, FONT_SIZE, LABEL_COLOR, THUMB_FILL, TRACK_FILL,
};
use crate::tabs::{SKY_TABS, TabPage, WATER_TABS};

/// The window's floater id, and the prefix of every node's name in it.
pub const DAY_CYCLE_EDITOR_FLOATER_ID: &str = "day-cycle-editor";

/// The element id the sky knobs are named under, so they do not collide with the
/// water ones in the same window.
const SKY_ELEMENT: &str = "day-cycle-editor-sky";

/// The element id the water knobs are named under.
const WATER_ELEMENT: &str = "day-cycle-editor-water";

/// How many keyframes one track may hold — the reference's `max_sliders` on the
/// timeline widget, which is what actually bounds it there.
const MAX_KEYFRAMES: usize = 20;

/// How long **Play** takes to walk a whole day, in seconds — the reference's
/// `DAY_CYCLE_PLAY_TIME_SECONDS`.
const PLAY_SECONDS: f32 = 60.0;

/// The timeline's width, logical px. Both strips and the tick labels share it.
const TIMELINE_WIDTH: f32 = 430.0;

/// A timeline strip's height, logical px.
const STRIP_HEIGHT: f32 = 14.0;

/// A marker's width, logical px.
const MARKER_WIDTH: f32 = 9.0;

/// How many tick labels sit above the timeline (`0%` … `100%`), the reference's
/// `p0`..`p4`.
const TICKS: usize = 5;

/// A keyframe marker's fill.
const MARKER_FILL: Color = Color::srgb(0.72, 0.76, 0.84);

/// The selected keyframe marker's fill.
const MARKER_SELECTED: Color = Color::srgb(0.98, 0.82, 0.35);

/// How many columns a knob page lays its controls out in.
const COLUMNS: usize = 3;

// ---------------------------------------------------------------------------
// Actions.
// ---------------------------------------------------------------------------

/// What one of the window's buttons does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DayAction {
    /// Save the cycle back onto the item it came from.
    Save,
    /// Save it as a new inventory item.
    SaveAs,
    /// Throw the edits away and go back to the cycle as loaded.
    Revert,
    /// Put a keyframe on the current track at the scrubber, holding whatever is
    /// being previewed there.
    AddFrame,
    /// Take the selected keyframe off the track.
    DeleteFrame,
    /// Put a settings asset from inventory on the track at the scrubber.
    LoadFrame,
    /// Copy another track of this cycle over the current one.
    CopyTrack,
    /// Copy the matching track of a day cycle in inventory over the current one.
    LoadTrack,
    /// Empty the current track as far as it may be emptied.
    ClearTrack,
    /// Start or stop walking the scrubber through the day.
    PlayPause,
    /// Jump the scrubber to the previous keyframe.
    SkipBack,
    /// Jump the scrubber to the next keyframe.
    SkipForward,
}

impl DayAction {
    /// The action's slug (its node name's tail) and its label's Fluent key.
    const fn labels(self) -> (&'static str, &'static str) {
        match self {
            Self::Save => ("save", "settings-editor-save"),
            Self::SaveAs => ("save-as", "settings-editor-save-as"),
            Self::Revert => ("revert", "settings-editor-revert"),
            Self::AddFrame => ("add-frame", "day-cycle-add-frame"),
            Self::DeleteFrame => ("delete-frame", "day-cycle-delete-frame"),
            Self::LoadFrame => ("load-frame", "day-cycle-load-frame"),
            Self::CopyTrack => ("copy-track", "day-cycle-copy-track"),
            Self::LoadTrack => ("load-track", "day-cycle-load-track"),
            Self::ClearTrack => ("clear-track", "day-cycle-clear-track"),
            Self::PlayPause => ("play", "day-cycle-play"),
            Self::SkipBack => ("skip-back", "day-cycle-skip-back"),
            Self::SkipForward => ("skip-forward", "day-cycle-skip-forward"),
        }
    }
}

/// The label a track button carries — the reference's own wording, with the
/// altitude tracks numbered rather than named after their breakpoints (the
/// breakpoints belong to the *region*, and this window edits an inventory item
/// that has not been given to one).
const fn track_label_key(track: DayTrack) -> &'static str {
    match track {
        DayTrack::Water => "day-cycle-track-water",
        DayTrack::Sky(0) => "day-cycle-track-ground",
        DayTrack::Sky(1) => "day-cycle-track-sky-2",
        DayTrack::Sky(2) => "day-cycle-track-sky-3",
        DayTrack::Sky(_other) => "day-cycle-track-sky-4",
    }
}

/// A track's slug, for the node names of the button that selects it.
const fn track_slug(track: DayTrack) -> &'static str {
    match track {
        DayTrack::Water => "track-water",
        DayTrack::Sky(0) => "track-ground",
        DayTrack::Sky(1) => "track-sky-2",
        DayTrack::Sky(2) => "track-sky-3",
        DayTrack::Sky(_other) => "track-sky-4",
    }
}

// ---------------------------------------------------------------------------
// Components.
// ---------------------------------------------------------------------------

/// A sky slider in this window.
#[derive(Component, Debug, Clone, Copy)]
struct DaySkySlider(SkyKnob);

/// A water slider in this window.
#[derive(Component, Debug, Clone, Copy)]
struct DayWaterSlider(WaterKnob);

/// A colour swatch in this window.
#[derive(Component, Debug, Clone, Copy)]
struct DayColorSwatch(ColorKnob);

/// A texture swatch in this window.
#[derive(Component, Debug, Clone, Copy)]
struct DayTextureSwatch(TextureKnob);

/// The name field.
#[derive(Component, Debug, Clone, Copy)]
struct DayNameField;

/// One of the five track buttons.
#[derive(Component, Debug, Clone, Copy)]
struct DayTrackButton(DayTrack);

/// One of the window's action buttons.
#[derive(Component, Debug, Clone, Copy)]
struct DayButton(DayAction);

/// Which of the two timeline strips a node is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StripKind {
    /// The scrubber: one marker, dragged to choose the time of day.
    Cursor,
    /// The keyframes of the selected track.
    Keyframes,
}

/// A timeline strip — the node a press or a drag is measured against.
#[derive(Component, Debug, Clone, Copy)]
struct DayStrip(StripKind);

/// One marker on a strip. The keyframe strip's markers are a fixed pool of
/// [`MAX_KEYFRAMES`], shown and placed rather than spawned and despawned: a
/// window that respawns its own widgets as its content changes is the shape that
/// leaves observers pointing at dead entities.
#[derive(Component, Debug, Clone, Copy)]
struct DayMarker {
    /// Which strip the marker belongs to.
    strip: StripKind,
    /// Its index in the pool — always `0` on the cursor strip.
    index: usize,
}

/// One of the tick labels above the timeline, by its position in `0..TICKS`.
#[derive(Component, Debug, Clone, Copy)]
struct DayTick(usize);

/// The combo naming which track a **Copy Track** copies from.
#[derive(Component, Debug, Clone, Copy)]
struct DayCloneSource;

/// The node families [`sync_day_chrome`] walks: the track buttons, the action
/// buttons, the timeline's tick labels, and the children a button's label is
/// found through.
type ChromeWidgets<'w, 's> = (
    Query<'w, 's, (Entity, &'static DayTrackButton)>,
    Query<'w, 's, (Entity, &'static DayButton)>,
    Query<'w, 's, (Entity, &'static DayTick)>,
    Query<'w, 's, &'static Children>,
);

/// What [`paint_button`] writes through — the shared
/// [`ButtonPaint`](crate::rows::ButtonPaint), named for this window's chrome.
type ChromePaint<'w, 's> = crate::rows::ButtonPaint<'w, 's>;

// ---------------------------------------------------------------------------
// State.
// ---------------------------------------------------------------------------

/// The window's entity handles, filled in as its content is built.
#[derive(Debug, Clone, Copy, Default)]
struct DayUi {
    /// The floater root, carrying the `UiPanelShown` the window is shown by.
    panel: Option<Entity>,
    /// The one-line status readout.
    status: Option<Entity>,
    /// The sky knob pages, shown while a sky track is selected.
    sky_pages: Option<Entity>,
    /// The water knob pages, shown while the water track is selected.
    water_pages: Option<Entity>,
    /// The "select a keyframe" hint, shown while nothing is selected.
    hint: Option<Entity>,
    /// The `NN% (HH:MM)` readout beside the timeline.
    readout: Option<Entity>,
    /// The clone-source combo.
    clone_source: Option<Entity>,
}

/// Where a picked settings asset is going once it decodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InsertTarget {
    /// A sky or water frame, onto the current track at the scrubber.
    Keyframe,
    /// A day cycle, whose matching track replaces the current one.
    Track,
}

/// A settings asset picked from inventory whose bytes have not arrived yet.
#[derive(Debug, Clone, Copy)]
struct PendingInsert {
    /// The asset being waited for.
    asset: AssetKey,
    /// What it is being fetched for.
    target: InsertTarget,
    /// The track it is going onto — recorded at the pick, so a track selected
    /// while the fetch was in flight does not redirect it.
    track: DayTrack,
    /// The position it is going at.
    position: f32,
}

/// An open whose asset has not arrived yet. Only an *inventory* open waits:
/// a land open carries its cycle in the message.
#[derive(Debug, Clone)]
struct PendingOpen {
    /// The request that started it, replayed once the asset decodes.
    request: OpenSettingsEditor,
    /// The asset being waited for.
    asset: AssetKey,
}

/// An open the window has been asked for, whichever of the two kinds it is.
///
/// One type because the *guard* in front of both is one question — is there
/// unsaved work in the window this would replace? — and an open held for a
/// confirmation has to come back as the open it was.
#[derive(Debug, Clone)]
enum DayOpen {
    /// An inventory item, whose asset is fetched first.
    Inventory(Box<OpenSettingsEditor>),
    /// A land's own cycle, already in hand.
    Land(Box<OpenLandDayCycle>),
}

/// Where the cycle this window is editing came from, and therefore what
/// **Save** means.
///
/// The reference splits the same way (`LLFloaterEditExtDayCycle`'s
/// `KEY_EDIT_CONTEXT`: `CONTEXT_INVENTORY` against `CONTEXT_REGION` /
/// `CONTEXT_PARCEL`), because a cycle that belongs to a region has no asset to
/// write onto — the environment carries it inline.
#[derive(Debug, Clone)]
enum DaySource {
    /// An inventory item. Save writes the asset back onto it; Save As files a
    /// copy beside it.
    Inventory(EditedItem),
    /// A region's or a parcel's own environment, opened from the
    /// land-environment panel ([`crate::land_environment`]).
    ///
    /// Save hands the cycle **back to the panel** rather than publishing it
    /// here: the panel owns the permission tests, the scope (`?parcelid=`) and
    /// the Apply / Revert pair, and a second path to the wire would be a
    /// second set of all three. Save As still files a copy in inventory,
    /// because a cycle worth putting on a region is worth keeping.
    Land {
        /// The panel to answer.
        panel: Entity,
        /// What to call the land in the status line.
        label: String,
        /// Where a Save As files the copy — the Settings system folder, since
        /// a land cycle has no folder of its own to sit beside. `None` on a
        /// session opened before the inventory skeleton arrived, where there
        /// is nowhere to file one yet.
        folder_id: Option<InventoryFolderKey>,
        /// Whether the agent may change this land's environment at all. A
        /// panel that is read-only opens the window read-only.
        editable: bool,
    },
}

impl DaySource {
    /// Whether the cycle may be edited at all.
    const fn editable(&self) -> bool {
        match self {
            Self::Inventory(item) => item.editable,
            Self::Land { editable, .. } => *editable,
        }
    }

    /// The folder a Save As files its copy in, or `None` when there is
    /// nowhere to file one.
    const fn save_folder(&self) -> Option<InventoryFolderKey> {
        match self {
            Self::Inventory(item) => Some(item.folder_id),
            Self::Land { folder_id, .. } => *folder_id,
        }
    }

    /// Whether a Save needs the grid to be able to store a settings asset.
    /// A land Save publishes the cycle inline and never touches inventory.
    const fn save_needs_asset_store(&self) -> bool {
        matches!(*self, Self::Inventory(_))
    }
}

/// A run of the scrubber.
#[derive(Debug, Clone, Copy)]
struct Playback {
    /// Where the scrubber was when Play was pressed.
    from: f32,
    /// How long it has been running, in seconds.
    elapsed: f32,
}

/// The day cycle this window is editing.
#[expect(
    clippy::struct_excessive_bools,
    reason = "the four are independent facts about one session with different lifetimes, not a \
              state machine an enum could replace: `dirty`, `reseed` and `relist` are one-shot \
              signals spent by the systems that read them (push the preview; re-seed the knobs; \
              rebuild the markers), while `modified` is a state that outlives a frame — and a \
              session can legitimately be all four at once"
)]
#[derive(Debug, Clone)]
struct DaySession {
    /// Where this cycle came from, and therefore what a Save does with it.
    source: DaySource,
    /// The name shown in the field and written into the asset.
    name: String,
    /// The cycle as loaded — what Revert restores.
    original: DayCycle,
    /// The cycle being edited.
    edited: DayCycle,
    /// The track whose keyframes the timeline shows.
    track: DayTrack,
    /// The scrubber, `0.0..=1.0`.
    position: f32,
    /// The selected keyframe's index in [`track`](Self::track), if any. Nothing
    /// is selected between keyframes, and the knob pages are then a read-only
    /// view of the blend — the reference's lock icon.
    selected: Option<usize>,
    /// Play is running.
    playing: Option<Playback>,
    /// Something changed: push the preview.
    dirty: bool,
    /// The cycle differs from what was loaded (or last saved).
    modified: bool,
    /// Re-seed every knob from the selected keyframe.
    reseed: bool,
    /// Rebuild the timeline's markers from the current track.
    relist: bool,
    /// A Save is in flight.
    saving: bool,
}

impl DaySession {
    /// The keyframes the timeline is showing.
    fn keyframes(&self) -> &[sl_client_bevy::DayCycleFrame] {
        self.edited.track(self.track)
    }

    /// The name of the frame the selected keyframe holds, if one is selected.
    fn selected_frame(&self) -> Option<String> {
        let index = self.selected?;
        self.keyframes()
            .get(index)
            .map(|keyframe| keyframe.name.clone())
    }

    /// The sky and water the scrubber is standing in — the blend both the
    /// preview and the unselected knob pages read.
    fn preview(&self) -> (Option<SkySettings>, Option<WaterSettings>) {
        let sky_track = self.track.sky_index().unwrap_or(0);
        (
            self.edited.blended_sky(sky_track, self.position),
            self.edited.blended_water(self.position),
        )
    }

    /// The frames the knob pages show: the selected keyframe's own frame where
    /// there is one, and the blend at the scrubber otherwise.
    fn shown_frames(&self) -> (Option<SkySettings>, Option<WaterSettings>) {
        let Some(name) = self.selected_frame() else {
            return self.preview();
        };
        match self.track.sky_index() {
            Some(_sky) => (self.edited.sky_frames.get(&name).cloned(), None),
            None => (None, self.edited.water_frames.get(&name).cloned()),
        }
    }

    /// Select whichever keyframe is under `position` (within
    /// [`KEYFRAME_SLOP`]), and move the scrubber onto it when one is —
    /// the reference's `selectFrame`.
    fn select_at(&mut self, position: f32, slop: f32) {
        self.selected = self.edited.keyframe_near(self.track, position, slop);
        self.position = self
            .selected
            .and_then(|index| self.keyframes().get(index))
            .map_or(position, |keyframe| keyframe.keyframe);
        self.dirty = true;
        self.reseed = true;
        self.relist = true;
    }
}

/// The window's whole state.
#[derive(Resource, Debug, Default)]
struct DayCycleEditorState {
    /// The window's entity handles.
    ui: DayUi,
    /// The cycle being edited, or `None` while the window has nothing open.
    session: Option<DaySession>,
    /// The item whose asset is being fetched, if the window is waiting on one.
    pending: Option<PendingOpen>,
    /// A picked settings asset being fetched for a Load Frame / Load Track.
    insert: Option<PendingInsert>,
    /// The item an in-place save is writing onto, while its reply is out.
    saving: Option<InventoryKey>,
    /// Whether a **Save As** copy is outstanding on the shared creation queue.
    saving_as: bool,
    /// An open the user is being asked about, because taking it would throw
    /// unsaved work away.
    confirm: Option<DayOpen>,
    /// The clone-source combo's row states as last published, so the list is
    /// re-sent only when it actually changes.
    clone_rows: Vec<ComboRow>,
    /// Whether the transport button's label currently says Pause, so it is
    /// rebound only on the flip.
    play_label_playing: Option<bool>,
    /// The `(percent, day length)` the readout was last written for, so the
    /// string is only formatted when it would read differently — a scrubber
    /// dragged across one percent of the day is one write, not sixty.
    shown_readout: Option<(i64, i32)>,
    /// The day length the tick labels were last written for.
    shown_ticks: Option<i32>,
}

// ---------------------------------------------------------------------------
// Plugin.
// ---------------------------------------------------------------------------

/// The plugin wiring the day-cycle editor into a host.
#[derive(Debug, Clone, Copy, Default)]
pub struct DayCycleEditorPlugin;

impl Plugin for DayCycleEditorPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<crate::rows::RowsPlugin>() {
            app.add_plugins(crate::rows::RowsPlugin);
        }
        if !app.is_plugin_added::<ComboWidgetPlugin>() {
            app.add_plugins(ComboWidgetPlugin);
        }
        app.init_resource::<DayCycleEditorState>()
            // Idempotent, for the same reason the fixed editors initialise them:
            // a host that stands this window up without the inventory (the
            // gallery) must still have somewhere for a Save As to enqueue, and a
            // `MessageWriter` for an unregistered message panics on its first
            // run rather than quietly doing nothing.
            .init_resource::<PendingSettingsCreations>()
            // Idempotent likewise: the inventory's actions plugin owns it and
            // keeps it current from the capability map; a host without one
            // reads "no settings grid", which greys the two saves.
            .init_resource::<SettingsInventorySupport>()
            .add_message::<ShowNotification>()
            .add_message::<NotificationResponse>()
            .add_message::<SettingsItemCreated>()
            .add_message::<OpenSettingsEditor>()
            .add_message::<OpenSettingsPicker>()
            .add_message::<SettingsPicked>()
            .add_message::<OpenLandDayCycle>()
            .add_message::<LandDayCycleEdited>()
            .add_systems(
                Startup,
                spawn_day_cycle_editor.after(UiScaffoldSystems::SpawnRoot),
            )
            .add_systems(
                Update,
                // Ordered as the fixed editors' are: an open has to reach the
                // widgets and the edit layer in the frame it happened, or the
                // window's first frame shows the last asset's values.
                (
                    open_day_cycle_editor.after(FloaterSystems::Commands),
                    open_land_day_cycle.after(FloaterSystems::Commands),
                    poll_pending_day_open,
                    advance_day_playback,
                    apply_day_color_picks,
                    apply_day_texture_picks,
                    take_day_settings_pick,
                    poll_day_insert,
                    read_day_name,
                    rebuild_day_markers,
                    reseed_day_widgets,
                    sync_day_chrome,
                    push_day_preview,
                    report_day_save,
                    report_day_save_as,
                    confirm_day_replace,
                    drop_day_preview_on_close,
                )
                    .chain(),
            );
    }
}

/// The window's [`FloaterSpec`] — shared with the `FLOATERS` registry, so the
/// swept window is the one the viewer spawns.
#[must_use]
pub fn day_cycle_editor_floater_spec() -> FloaterSpec {
    FloaterSpec {
        id: DAY_CYCLE_EDITOR_FLOATER_ID,
        title: "Day Cycle".to_owned(),
        position: Vec2::new(140.0, 120.0),
        // Wide enough for the track buttons beside the timeline, and for three
        // knob columns under both; tall enough for the timeline, its two rows of
        // actions, the longest knob column and the buttons.
        default_size: Some(Vec2::new(600.0, 620.0)),
        min_size: Some(Vec2::new(360.0, 300.0)),
        dock_host: None,
        caps: FloaterCaps {
            resizable: true,
            minimizable: true,
            closable: true,
            dockable: false,
        },
    }
}

/// Startup: spawn the (hidden) window, its content deferred to the first open.
fn spawn_day_cycle_editor(mut commands: Commands, root: Res<UiRoot>) {
    let handle = spawn_floater(&mut commands, root.0, day_cycle_editor_floater_spec());
    commands
        .entity(handle.title_text)
        .insert(Translated::new("day-cycle-editor-title"));
    let builder = commands.register_system(build_day_cycle_content);
    commands
        .entity(handle.root)
        .insert(DeferredFloaterContent { builder, handle })
        // Where it sits is worth remembering; that it was open is not. The
        // window is bound to one inventory item of one account.
        .insert(FloaterOpenExempt);
    commands.insert_resource(DayCycleEditorState {
        ui: DayUi {
            panel: Some(handle.root),
            ..DayUi::default()
        },
        ..DayCycleEditorState::default()
    });
}

// ---------------------------------------------------------------------------
// Content.
// ---------------------------------------------------------------------------

/// The window's content: the name row, the timeline and its actions, the knob
/// pages, and the save row.
fn build_day_cycle_content(In(handle): In<FloaterHandle>, mut commands: Commands) {
    let element = DAY_CYCLE_EDITOR_FLOATER_ID;
    let content = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                min_height: Val::Px(0.0),
                padding: UiRect::all(Val::Px(6.0)),
                ..column(Val::Px(6.0))
            },
            Name::new(format!("{element}:content")),
            ChildOf(handle.content),
        ))
        .id();
    let mut tab = 0_i32;

    spawn_name_row(&mut commands, content, &mut tab);
    let (readout, clone_source) = spawn_timeline(&mut commands, content, &mut tab);
    let hint = spawn_hint(&mut commands, content);
    let sky_pages = spawn_pages(&mut commands, content, SKY_ELEMENT, SKY_TABS, &mut tab);
    let water_pages = spawn_pages(&mut commands, content, WATER_ELEMENT, WATER_TABS, &mut tab);
    let status = spawn_save_row(&mut commands, content, &mut tab);

    commands.queue(move |world: &mut World| {
        if let Some(mut state) = world.get_resource_mut::<DayCycleEditorState>() {
            state.ui.status = Some(status);
            state.ui.sky_pages = Some(sky_pages);
            state.ui.water_pages = Some(water_pages);
            state.ui.hint = Some(hint);
            state.ui.readout = Some(readout);
            state.ui.clone_source = Some(clone_source);
            // The content is built on the window's first open, a frame after the
            // session that asked for it was installed — so the widgets that have
            // just appeared have never been seeded. Ask for it now.
            if let Some(session) = state.session.as_mut() {
                session.reseed = true;
                session.relist = true;
            }
        }
    });
}

/// The name row: a label and the field the cycle's name is edited in.
fn spawn_name_row(commands: &mut Commands, parent: Entity, tab: &mut i32) {
    let element = DAY_CYCLE_EDITOR_FLOATER_ID;
    let holder = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                align_items: AlignItems::Center,
                ..row(Val::Px(6.0))
            },
            Name::new(format!("{element}-name:row")),
            ChildOf(parent),
        ))
        .id();
    commands.spawn((
        Text::new(String::new()),
        UiFont::Sans.at(FONT_SIZE),
        TextColor(LABEL_COLOR),
        Translated::new("settings-editor-name"),
        ChildOf(holder),
    ));
    let field = spawn_text_input(
        commands,
        holder,
        &TextInputSpec {
            tab_index: *tab,
            font_size: FONT_SIZE,
            fill: true,
            max_characters: Some(63),
            ..TextInputSpec::new("day-cycle-editor-name", TextInputKind::Line)
        },
    );
    commands.entity(field).insert(DayNameField);
    *tab = tab.saturating_add(1);
}

/// The timeline block: the track buttons beside the ticks, the two strips, the
/// readout and play controls, and the track / frame action rows. Returns the
/// readout text entity and the clone-source combo.
fn spawn_timeline(commands: &mut Commands, parent: Entity, tab: &mut i32) -> (Entity, Entity) {
    let element = DAY_CYCLE_EDITOR_FLOATER_ID;
    let holder = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_shrink: 0.0,
                ..row(Val::Px(8.0))
            },
            Name::new(format!("{element}-timeline:row")),
            ChildOf(parent),
        ))
        .id();

    // The track picker, top down as the reference stacks it: the highest sky
    // track first and the water track at the bottom, under the ground.
    let tracks = commands
        .spawn((
            Node {
                flex_shrink: 0.0,
                ..column(Val::Px(2.0))
            },
            Name::new(format!("{element}-tracks:column")),
            ChildOf(holder),
        ))
        .id();
    for track in [
        DayTrack::Sky(3),
        DayTrack::Sky(2),
        DayTrack::Sky(1),
        DayTrack::GROUND,
        DayTrack::Water,
    ] {
        let button = spawn_action_button(
            commands,
            tracks,
            element,
            track_slug(track),
            track_label_key(track).to_owned(),
            tab,
        );
        commands
            .entity(button)
            .insert(DayTrackButton(track))
            .observe(on_day_track_button);
    }

    let strip_column = commands
        .spawn((
            Node {
                flex_grow: 1.0,
                min_width: Val::Px(0.0),
                ..column(Val::Px(3.0))
            },
            Name::new(format!("{element}-timeline:column")),
            ChildOf(holder),
        ))
        .id();

    // The tick labels, evenly spread over the strips' width.
    let ticks = commands
        .spawn((
            Node {
                width: Val::Px(TIMELINE_WIDTH),
                justify_content: JustifyContent::SpaceBetween,
                ..row(Val::Px(2.0))
            },
            Name::new(format!("{element}-ticks:row")),
            ChildOf(strip_column),
        ))
        .id();
    for index in 0..TICKS {
        commands.spawn((
            Text::new(String::new()),
            TextLayout {
                linebreak: LineBreak::NoWrap,
                ..TextLayout::default()
            },
            UiFont::Sans.at(FONT_SIZE),
            TextColor(DIM_LABEL_COLOR),
            DayTick(index),
            Name::new(format!("{element}-tick-{index}")),
            ChildOf(ticks),
        ));
    }

    spawn_strip(commands, strip_column, StripKind::Cursor, 1);
    spawn_strip(commands, strip_column, StripKind::Keyframes, MAX_KEYFRAMES);

    // The readout and the transport.
    let transport = commands
        .spawn((
            Node {
                width: Val::Px(TIMELINE_WIDTH),
                align_items: AlignItems::Center,
                ..row(Val::Px(6.0))
            },
            Name::new(format!("{element}-transport:row")),
            ChildOf(strip_column),
        ))
        .id();
    let readout = commands
        .spawn((
            Text::new(String::new()),
            TextLayout {
                linebreak: LineBreak::NoWrap,
                ..TextLayout::default()
            },
            UiFont::Sans.at(FONT_SIZE),
            TextColor(LABEL_COLOR),
            Node {
                min_width: Val::Px(120.0),
                ..Node::default()
            },
            Name::new(format!("{element}-time")),
            ChildOf(transport),
        ))
        .id();
    for action in [
        DayAction::SkipBack,
        DayAction::PlayPause,
        DayAction::SkipForward,
    ] {
        spawn_day_button(commands, transport, action, tab);
    }

    // The track actions: which track to copy from, and the three verbs.
    let track_row = commands
        .spawn((
            Node {
                width: Val::Px(TIMELINE_WIDTH),
                align_items: AlignItems::Center,
                flex_wrap: FlexWrap::Wrap,
                ..row(Val::Px(6.0))
            },
            Name::new(format!("{element}-track-actions:row")),
            ChildOf(strip_column),
        ))
        .id();
    let labels: Vec<String> = (0..SKY_TRACK_COUNT)
        .map(|index| track_label_key(DayTrack::Sky(index)).to_owned())
        .collect();
    let clone_source = spawn_combo(
        commands,
        track_row,
        &ComboSpec {
            element: "day-cycle-editor-clone-source",
            labels: &labels,
            active: 0,
            tab_index: *tab,
            font_size: FONT_SIZE,
            translate_labels: true,
        },
    );
    commands.entity(clone_source).insert(DayCloneSource);
    *tab = tab.saturating_add(1);
    for action in [
        DayAction::CopyTrack,
        DayAction::LoadTrack,
        DayAction::ClearTrack,
    ] {
        spawn_day_button(commands, track_row, action, tab);
    }

    // The frame actions.
    let frame_row = commands
        .spawn((
            Node {
                width: Val::Px(TIMELINE_WIDTH),
                align_items: AlignItems::Center,
                flex_wrap: FlexWrap::Wrap,
                ..row(Val::Px(6.0))
            },
            Name::new(format!("{element}-frame-actions:row")),
            ChildOf(strip_column),
        ))
        .id();
    for action in [
        DayAction::AddFrame,
        DayAction::LoadFrame,
        DayAction::DeleteFrame,
    ] {
        spawn_day_button(commands, frame_row, action, tab);
    }

    (readout, clone_source)
}

/// One timeline strip and its pool of `markers` markers.
fn spawn_strip(commands: &mut Commands, parent: Entity, kind: StripKind, markers: usize) {
    let element = DAY_CYCLE_EDITOR_FLOATER_ID;
    let slug = match kind {
        StripKind::Cursor => "cursor",
        StripKind::Keyframes => "keyframes",
    };
    let strip = commands
        .spawn((
            Node {
                width: Val::Px(TIMELINE_WIDTH),
                height: Val::Px(STRIP_HEIGHT),
                flex_shrink: 0.0,
                border: UiRect::all(Val::Px(1.0)),
                ..Node::default()
            },
            BorderColor::all(CONTROL_BORDER),
            BackgroundColor(TRACK_FILL),
            DayStrip(kind),
            Name::new(format!("{element}-{slug}:strip")),
            ChildOf(parent),
        ))
        .observe(on_day_strip_press)
        .observe(on_day_strip_drag)
        .id();
    for index in 0..markers {
        commands.spawn((
            Node {
                position_type: PositionType::Absolute,
                width: Val::Px(MARKER_WIDTH),
                height: Val::Px(STRIP_HEIGHT),
                display: Display::None,
                ..Node::default()
            },
            LogicalInset(LogicalRect {
                inline_start: Val::Px(0.0),
                ..LogicalRect::ZERO
            }),
            BackgroundColor(if kind == StripKind::Cursor {
                THUMB_FILL
            } else {
                MARKER_FILL
            }),
            // The strip owns the gesture; a marker under the pointer must not
            // swallow the press that was aimed at the track behind it.
            Pickable::IGNORE,
            DayMarker { strip: kind, index },
            ChildOf(strip),
        ));
    }
}

/// The "select a keyframe" hint line above the knob pages.
fn spawn_hint(commands: &mut Commands, parent: Entity) -> Entity {
    commands
        .spawn((
            Text::new(String::new()),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(DIM_LABEL_COLOR),
            Translated::new("day-cycle-select-a-keyframe"),
            Name::new(format!("{DAY_CYCLE_EDITOR_FLOATER_ID}-hint")),
            ChildOf(parent),
        ))
        .id()
}

/// One set of knob pages — the sky's four tabs, or the water's one. Returns the
/// container the track selection shows and hides.
fn spawn_pages(
    commands: &mut Commands,
    parent: Entity,
    element: &'static str,
    pages: &'static [TabPage],
    tab: &mut i32,
) -> Entity {
    let holder = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                display: Display::None,
                ..column(Val::Px(0.0))
            },
            Name::new(format!("{element}:pages")),
            ChildOf(parent),
        ))
        .id();
    let labels: Vec<String> = pages.iter().map(|page| page.label.to_owned()).collect();
    let tabs = spawn_tab_container(
        commands,
        holder,
        &TabSpec {
            element,
            placement: TabPlacement::BlockStart,
            labels: &labels,
            active: 0,
            tab_index: *tab,
            font_size: FONT_SIZE,
            strip_width: None,
            ellipsis: DEFAULT_ELLIPSIS,
            translate_labels: true,
        },
    );
    fill_tab_container(commands, TabPlacement::BlockStart, &tabs);
    *tab = tab.saturating_add(1);

    for (page, panel) in pages.iter().zip(&tabs.panels) {
        let columns = spawn_columns(commands, *panel, element, page.slug);
        let has_swatches = !page.colors.is_empty() || !page.textures.is_empty();
        let slider_columns = if has_swatches {
            columns.get(1..).unwrap_or_default()
        } else {
            columns.as_slice()
        };
        if let Some(swatches) = columns.first() {
            for knob in page.colors {
                let swatch = spawn_color_row(commands, *swatches, element, *knob, tab);
                commands.entity(swatch).insert(DayColorSwatch(*knob));
            }
            for knob in page.textures {
                let swatch = spawn_texture_row(commands, *swatches, element, *knob, tab);
                commands.entity(swatch).insert(DayTextureSwatch(*knob));
            }
        }
        // A body's trackball opens the column its two angle sliders are in, as
        // the reference's sun-and-moon panel opens with them.
        for (index, knobs) in page.aims.iter().enumerate() {
            let Some(column_entity) = slider_columns.get(index) else {
                continue;
            };
            let trackball = spawn_trackball_row(commands, *column_entity, element, *knobs, tab);
            commands.entity(trackball).observe(on_day_trackball);
        }
        for (index, knob) in page.sky.iter().enumerate() {
            let Some(column_entity) = slider_column(slider_columns, index, page.sky.len()) else {
                continue;
            };
            let track = spawn_slider(
                commands,
                column_entity,
                element,
                knob.slug(),
                knob.range(),
                knob.decimals(),
                tab,
            );
            commands
                .entity(track)
                .insert(DaySkySlider(*knob))
                .observe(on_day_sky_slider);
            tag_aim_slider(commands, track, element, *knob);
        }
        for (index, knob) in page.water.iter().enumerate() {
            let Some(column_entity) = slider_column(slider_columns, index, page.water.len()) else {
                continue;
            };
            let track = spawn_slider(
                commands,
                column_entity,
                element,
                knob.slug(),
                knob.range(),
                knob.decimals(),
                tab,
            );
            commands
                .entity(track)
                .insert(DayWaterSlider(*knob))
                .observe(on_day_water_slider);
        }
    }
    holder
}

/// Three columns inside a tab panel: the swatches, then two of sliders.
fn spawn_columns(
    commands: &mut Commands,
    panel: Entity,
    element: &str,
    tab_name: &str,
) -> Vec<Entity> {
    let strip = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                min_height: Val::Px(0.0),
                ..row(Val::Px(10.0))
            },
            Name::new(format!("{element}-{tab_name}:columns")),
            ChildOf(panel),
        ))
        .id();
    std::iter::repeat_with(|| {
        commands
            .spawn((
                Node {
                    min_width: Val::Px(0.0),
                    ..column(Val::Px(3.0))
                },
                ChildOf(strip),
            ))
            .id()
    })
    .take(COLUMNS)
    .collect()
}

/// The column the `index`-th of `total` sliders goes in, spreading them evenly
/// over `columns` in order.
fn slider_column(columns: &[Entity], index: usize, total: usize) -> Option<Entity> {
    if columns.is_empty() {
        return None;
    }
    let per_column = total.div_ceil(columns.len()).max(1);
    columns
        .get(index.checked_div(per_column).unwrap_or(0))
        .or_else(|| columns.last())
        .copied()
}

/// The Save / Save As / Revert row and the status line under it. Returns the
/// status text entity.
fn spawn_save_row(commands: &mut Commands, parent: Entity, tab: &mut i32) -> Entity {
    let element = DAY_CYCLE_EDITOR_FLOATER_ID;
    let holder = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                align_items: AlignItems::Center,
                ..row(Val::Px(6.0))
            },
            Name::new(format!("{element}-actions:row")),
            ChildOf(parent),
        ))
        .id();
    for action in [DayAction::Save, DayAction::SaveAs, DayAction::Revert] {
        spawn_day_button(commands, holder, action, tab);
    }
    commands
        .spawn((
            Text::new(String::new()),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(DIM_LABEL_COLOR),
            Name::new(format!("{element}-status")),
            ChildOf(parent),
        ))
        .id()
}

/// One action button, tagged and observed.
fn spawn_day_button(
    commands: &mut Commands,
    parent: Entity,
    action: DayAction,
    tab: &mut i32,
) -> Entity {
    let (slug, key) = action.labels();
    let button = spawn_action_button(
        commands,
        parent,
        DAY_CYCLE_EDITOR_FLOATER_ID,
        slug,
        key.to_owned(),
        tab,
    );
    commands
        .entity(button)
        .insert(DayButton(action))
        .observe(on_day_button);
    button
}

// ---------------------------------------------------------------------------
// Opening.
// ---------------------------------------------------------------------------

/// Handle an [`OpenSettingsEditor`] naming a day cycle: show the window and
/// start fetching the item's asset.
fn open_day_cycle_editor(
    mut opens: MessageReader<OpenSettingsEditor>,
    mut state: ResMut<DayCycleEditorState>,
    mut assets: Option<ResMut<EnvironmentAssetManager>>,
    mut notify: MessageWriter<ShowNotification>,
    mut panels: Query<&mut UiPanelShown>,
    mut raises: MessageWriter<FloaterCommand>,
    mut texts: Query<&mut Text>,
) {
    for open in opens.read() {
        if open.kind != SettingsKind::DayCycle {
            // A sky or a water frame is the fixed editors' message; they read the
            // same stream.
            continue;
        }
        // One window, because the cycle being edited is *previewed*: two previews
        // of one sky cannot both be what the user is standing under. So opening a
        // second item replaces the first, and the reference asks before throwing
        // unsaved work away (`checkAndConfirmSettingsLoss`).
        if ask_before_replacing(
            &mut state,
            &mut notify,
            DayOpen::Inventory(Box::new(open.clone())),
        ) {
            continue;
        }
        let asset = AssetKey::from(open.asset_id);
        state.pending = Some(PendingOpen {
            request: open.clone(),
            asset,
        });
        if let Some(assets) = assets.as_mut() {
            assets.request(asset);
        }
        let status = state.ui.status;
        set_status(&mut texts, status, "Loading…");
        if let Some(panel) = state.ui.panel {
            if let Ok(mut shown) = panels.get_mut(panel) {
                shown.0 = true;
            }
            // Showing a window that is already open leaves it wherever it was in
            // the z-order — which, opened from the My Environments list, is
            // *behind* the window that asked for it.
            raises.write(FloaterCommand {
                floater: panel,
                op: FloaterOp::BringToFront,
            });
        }
    }
}

/// Seed the window when the asset arrives — or give up on one that will not.
fn poll_pending_day_open(
    mut state: ResMut<DayCycleEditorState>,
    assets: Option<Res<EnvironmentAssetManager>>,
    mut texts: Query<&mut Text>,
) {
    let Some(assets) = assets else {
        return;
    };
    let Some(pending) = state.pending.clone() else {
        return;
    };
    let status = state.ui.status;
    if let Some(decoded) = assets.get(pending.asset) {
        state.pending = None;
        // The item's *flags* said it is a day cycle, but the asset's own body is
        // the authority — an item flagged day whose asset is a sky frame would
        // otherwise be edited by a window with nowhere to put it.
        let EnvironmentAsset::DayCycle(cycle) = (**decoded).clone() else {
            set_status(
                &mut texts,
                status,
                "That item is not the kind this editor edits.",
            );
            return;
        };
        let cycle = *cycle;
        let source = DaySource::Inventory(EditedItem {
            item_id: pending.request.item_id,
            folder_id: pending.request.folder_id,
            editable: pending.request.editable,
        });
        seed_day_session(&mut state, source, pending.request.name.clone(), cycle);
        set_status(&mut texts, status, "");
    } else if assets.is_unavailable(pending.asset) {
        state.pending = None;
        set_status(
            &mut texts,
            status,
            "That settings asset could not be loaded.",
        );
    }
}

/// Carry out (or drop) an open the user was asked to confirm.
fn confirm_day_replace(
    mut responses: MessageReader<NotificationResponse>,
    mut state: ResMut<DayCycleEditorState>,
    mut opens: MessageWriter<OpenSettingsEditor>,
    mut land_opens: MessageWriter<OpenLandDayCycle>,
) {
    for response in responses.read() {
        if response.template != "SettingsConfirmLoss" {
            continue;
        }
        let Some(open) = state.confirm.take() else {
            continue;
        };
        if response.button != Some("OK") {
            continue;
        }
        // Drop the session *before* replaying, or the open would find it still
        // modified and ask again.
        state.session = None;
        match open {
            DayOpen::Inventory(request) => {
                opens.write(*request);
            }
            DayOpen::Land(request) => {
                land_opens.write(*request);
            }
        }
    }
}

/// Ask before an open replaces unsaved work, holding the open until the answer
/// comes back. Returns whether the caller should stand down and wait.
///
/// Shared by both kinds of open, because the question is about the session the
/// open would *replace* and has nothing to do with where the new one comes
/// from — the reference's `checkAndConfirmSettingsLoss`.
fn ask_before_replacing(
    state: &mut DayCycleEditorState,
    notify: &mut MessageWriter<ShowNotification>,
    open: DayOpen,
) -> bool {
    let Some(name) = state
        .session
        .as_ref()
        .filter(|session| session.modified)
        .map(|session| session.name.clone())
    else {
        return false;
    };
    state.confirm = Some(open);
    notify.write(
        ShowNotification::new("SettingsConfirmLoss")
            .arg("TYPE", "day cycle")
            .arg("NAME", name),
    );
    true
}

/// Install `cycle` as the window's session, opened at midnight as the
/// reference opens on frame 0.
fn seed_day_session(
    state: &mut DayCycleEditorState,
    source: DaySource,
    name: String,
    cycle: DayCycle,
) {
    state.session = Some(DaySession {
        source,
        name,
        original: cycle.clone(),
        edited: cycle,
        track: DayTrack::GROUND,
        position: 0.0,
        selected: None,
        playing: None,
        dirty: true,
        modified: false,
        reseed: true,
        relist: true,
        saving: false,
    });
    if let Some(session) = state.session.as_mut() {
        session.select_at(0.0, KEYFRAME_SLOP);
    }
}

/// Handle an [`OpenLandDayCycle`]: show the window on a land's own cycle.
///
/// No fetch, and so no pending state — the panel had the cycle in hand, which
/// is the whole reason this message carries it.
fn open_land_day_cycle(
    mut opens: MessageReader<OpenLandDayCycle>,
    mut state: ResMut<DayCycleEditorState>,
    inventory: Option<Res<InventoryModel>>,
    mut notify: MessageWriter<ShowNotification>,
    mut panels: Query<&mut UiPanelShown>,
    mut raises: MessageWriter<FloaterCommand>,
    mut texts: Query<&mut Text>,
) {
    for open in opens.read() {
        if ask_before_replacing(
            &mut state,
            &mut notify,
            DayOpen::Land(Box::new(open.clone())),
        ) {
            continue;
        }
        // A land cycle has no folder of its own; a Save As files the copy where
        // every other freshly minted settings item goes.
        let folder_id = inventory.as_deref().and_then(crate::settings_destination);
        let source = DaySource::Land {
            panel: open.panel,
            label: open.label.clone(),
            folder_id,
            editable: open.editable,
        };
        let name = open.cycle.name.clone();
        seed_day_session(&mut state, source, name, (*open.cycle).clone());
        // A fetch is what normally clears the pending state; there is none
        // here, and a stale one would seed over this session the moment its
        // asset arrived.
        state.pending = None;
        let status = state.ui.status;
        let label = open.label.clone();
        set_status(
            &mut texts,
            status,
            &format!("Editing the environment of {label}. Save hands it back to the panel."),
        );
        if let Some(panel) = state.ui.panel {
            if let Ok(mut shown) = panels.get_mut(panel) {
                shown.0 = true;
            }
            raises.write(FloaterCommand {
                floater: panel,
                op: FloaterOp::BringToFront,
            });
        }
    }
}

// ---------------------------------------------------------------------------
// The timeline.
// ---------------------------------------------------------------------------

/// The logical-pixel rectangle of a UI node: `(top_left, size)`.
///
/// Component-wise f32 maths, per the workspace `arithmetic_side_effects`
/// convention on `glam` operators.
fn node_rect(computed: &ComputedNode, transform: &UiGlobalTransform) -> (Vec2, Vec2) {
    let scale = computed.inverse_scale_factor();
    let physical = computed.size();
    let size = Vec2::new(physical.x * scale, physical.y * scale);
    let centre = transform.translation;
    (
        Vec2::new(
            centre.x * scale - size.x / 2.0,
            centre.y * scale - size.y / 2.0,
        ),
        size,
    )
}

/// Where along a strip a pointer at `pointer` sits, as a `0.0..=1.0` fraction.
///
/// A marker is placed by its *left* edge, so the usable span is the strip minus
/// one marker — and the pointer is measured against that same span, or grabbing
/// a marker would make it jump by half its width.
fn strip_fraction(pointer: Vec2, rect: (Vec2, Vec2)) -> f32 {
    let (top_left, size) = rect;
    let span = (size.x - MARKER_WIDTH).max(1.0);
    ((pointer.x - top_left.x - MARKER_WIDTH / 2.0) / span).clamp(0.0, 1.0)
}

/// A press on either strip: move the scrubber, and on the keyframe strip pick up
/// whatever keyframe is under the pointer.
fn on_day_strip_press(
    mut press: On<Pointer<Press>>,
    strips: Query<(&DayStrip, &ComputedNode, &UiGlobalTransform)>,
    mut state: ResMut<DayCycleEditorState>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let Ok((strip, computed, transform)) = strips.get(press.entity) else {
        return;
    };
    press.propagate(false);
    let fraction = strip_fraction(
        press.pointer_location.position,
        node_rect(computed, transform),
    );
    let Some(session) = state.session.as_mut() else {
        return;
    };
    // Any hand on the timeline stops the playback, as it does in the reference:
    // the scrubber cannot be somebody's and the clock's at once.
    session.playing = None;
    match strip.0 {
        // The scrubber snaps onto a keyframe it lands near, which is what makes
        // "drag to a keyframe and edit it" one gesture rather than two.
        StripKind::Cursor | StripKind::Keyframes => session.select_at(fraction, KEYFRAME_SLOP),
    }
}

/// A drag on either strip: the cursor strip scrubs; the keyframe strip moves the
/// keyframe that was picked up.
fn on_day_strip_drag(
    mut drag: On<Pointer<Drag>>,
    strips: Query<(&DayStrip, &ComputedNode, &UiGlobalTransform)>,
    mut state: ResMut<DayCycleEditorState>,
) {
    if drag.button != PointerButton::Primary {
        return;
    }
    let Ok((strip, computed, transform)) = strips.get(drag.entity) else {
        return;
    };
    drag.propagate(false);
    let fraction = strip_fraction(
        drag.pointer_location.position,
        node_rect(computed, transform),
    );
    let Some(session) = state.session.as_mut() else {
        return;
    };
    session.playing = None;
    let dragging_keyframe =
        strip.0 == StripKind::Keyframes && session.source.editable() && session.selected.is_some();
    if !dragging_keyframe {
        session.position = fraction;
        session.dirty = true;
        session.relist = true;
        return;
    }
    let Some(index) = session.selected else {
        return;
    };
    let track = session.track;
    // A move onto another keyframe is refused rather than clamped: the day is a
    // ring, and a keyframe shoved into its neighbour is a discontinuity nobody
    // asked for. The scrubber still follows the hand, so the refusal is visible.
    if let Some(moved) = session.edited.move_keyframe(track, index, fraction) {
        session.selected = Some(moved);
        session.position = fraction;
        session.modified = true;
        session.dirty = true;
        session.relist = true;
    }
}

/// Place the timeline's markers from the current track.
///
/// Only when something moved ([`DaySession::relist`]): a `LogicalInset` written
/// every frame is a node relaid out every frame, over a window where nothing has
/// changed since the pointer left it.
fn rebuild_day_markers(
    mut state: ResMut<DayCycleEditorState>,
    mut markers: Query<(
        &DayMarker,
        &mut Node,
        &mut LogicalInset,
        &mut BackgroundColor,
    )>,
) {
    let Some(session) = state.session.as_mut() else {
        // Nothing open: every marker is hidden, including the scrubber.
        for (_marker, mut node, _inset, _color) in &mut markers {
            if node.display != Display::None {
                node.display = Display::None;
            }
        }
        return;
    };
    if !session.relist {
        return;
    }
    session.relist = false;
    let span = TIMELINE_WIDTH - MARKER_WIDTH;
    let cursor = session.position;
    let keyframes: Vec<(f32, bool)> = session
        .keyframes()
        .iter()
        .enumerate()
        .map(|(index, keyframe)| (keyframe.keyframe, Some(index) == session.selected))
        .collect();
    for (marker, mut node, mut inset, mut color) in &mut markers {
        let placed = match marker.strip {
            StripKind::Cursor => Some((cursor, false)),
            StripKind::Keyframes => keyframes.get(marker.index).copied(),
        };
        let Some((position, selected)) = placed else {
            if node.display != Display::None {
                node.display = Display::None;
            }
            continue;
        };
        if node.display != Display::Flex {
            node.display = Display::Flex;
        }
        let offset = Val::Px(position.clamp(0.0, 1.0) * span);
        if inset.0.inline_start != offset {
            inset.0.inline_start = offset;
        }
        let wanted = match (marker.strip, selected) {
            (StripKind::Cursor, _any) => THUMB_FILL,
            (StripKind::Keyframes, true) => MARKER_SELECTED,
            (StripKind::Keyframes, false) => MARKER_FILL,
        };
        if color.0 != wanted {
            color.0 = wanted;
        }
    }
}

/// Walk the scrubber while Play is running.
fn advance_day_playback(time: Res<Time>, mut state: ResMut<DayCycleEditorState>) {
    let Some(session) = state.session.as_mut() else {
        return;
    };
    let Some(playing) = session.playing.as_mut() else {
        return;
    };
    playing.elapsed += time.delta_secs();
    let (from, elapsed) = (playing.from, playing.elapsed);
    session.position = (from + elapsed / PLAY_SECONDS).rem_euclid(1.0);
    session.dirty = true;
    session.relist = true;
}

// ---------------------------------------------------------------------------
// Editing.
// ---------------------------------------------------------------------------

/// The frame the selected keyframe holds, split off first if it is shared —
/// the name a knob write should go into, or `None` when nothing is selected or
/// the item may not be changed.
fn writable_frame(session: &mut DaySession) -> Option<String> {
    if !session.source.editable() {
        return None;
    }
    let index = session.selected?;
    let track = session.track;
    session.edited.split_shared_frame(track, index)
}

/// A sky slider moved.
fn on_day_sky_slider(
    change: On<ValueChange<f32>>,
    sliders: Query<(&DaySkySlider, &SliderRange)>,
    mut state: ResMut<DayCycleEditorState>,
    mut commands: Commands,
) {
    let Ok((row_info, range)) = sliders.get(change.source) else {
        return;
    };
    let clamped = range.clamp(change.value);
    commands.entity(change.source).insert(SliderValue(clamped));
    let Some(session) = state.session.as_mut() else {
        return;
    };
    let Some(name) = writable_frame(session) else {
        return;
    };
    if let Some(sky) = session.edited.sky_frames.get_mut(&name) {
        row_info.0.write(sky, clamped);
        session.dirty = true;
        session.modified = true;
    }
}

/// A trackball was aimed: write the body's whole direction into the keyframe
/// the window is showing. The two sliders under it are put back in step by the
/// shared [`crate::rows`] systems.
fn on_day_trackball(
    change: On<ValueChange<Vec2>>,
    trackballs: Query<&AimTrackball>,
    mut state: ResMut<DayCycleEditorState>,
) {
    let Ok(trackball) = trackballs.get(change.source) else {
        return;
    };
    let aim = TrackballAim {
        azimuth: change.value.x,
        elevation: change.value.y,
    };
    let Some(session) = state.session.as_mut() else {
        return;
    };
    let Some(name) = writable_frame(session) else {
        return;
    };
    if let Some(sky) = session.edited.sky_frames.get_mut(&name) {
        trackball.knobs.write(sky, aim);
        session.dirty = true;
        session.modified = true;
    }
}

/// A water slider moved.
fn on_day_water_slider(
    change: On<ValueChange<f32>>,
    sliders: Query<(&DayWaterSlider, &SliderRange)>,
    mut state: ResMut<DayCycleEditorState>,
    mut commands: Commands,
) {
    let Ok((row_info, range)) = sliders.get(change.source) else {
        return;
    };
    let clamped = range.clamp(change.value);
    commands.entity(change.source).insert(SliderValue(clamped));
    let Some(session) = state.session.as_mut() else {
        return;
    };
    let Some(name) = writable_frame(session) else {
        return;
    };
    if let Some(water) = session.edited.water_frames.get_mut(&name) {
        row_info.0.write(water, clamped);
        session.dirty = true;
        session.modified = true;
    }
}

/// A colour came back from the picker.
fn apply_day_color_picks(
    mut picks: MessageReader<ColorPicked>,
    mut swatches: Query<(&DayColorSwatch, &mut ColorSwatchValue)>,
    mut state: ResMut<DayCycleEditorState>,
) {
    for pick in picks.read() {
        let Ok((swatch, mut value)) = swatches.get_mut(pick.requester) else {
            continue;
        };
        value.0 = pick.color;
        let Some(session) = state.session.as_mut() else {
            continue;
        };
        let Some(name) = writable_frame(session) else {
            continue;
        };
        // The knob tables read and write a sky *and* a water frame at once; a
        // keyframe is only ever one of them, so the other is a scratch the write
        // lands in and nothing reads.
        let mut scratch_sky = SkySettings::legacy_windlight_default("scratch");
        let mut scratch_water = WaterSettings::legacy_default("scratch");
        if let Some(sky) = session.edited.sky_frames.get_mut(&name) {
            swatch.0.write(sky, &mut scratch_water, pick.color);
        } else if let Some(water) = session.edited.water_frames.get_mut(&name) {
            swatch.0.write(&mut scratch_sky, water, pick.color);
        }
        session.dirty = true;
        session.modified = true;
    }
}

/// A texture came back from the picker.
fn apply_day_texture_picks(
    mut picks: MessageReader<TexturePicked>,
    mut swatches: Query<(&DayTextureSwatch, &mut TextureSwatchValue)>,
    mut state: ResMut<DayCycleEditorState>,
) {
    for pick in picks.read() {
        let Ok((swatch, mut value)) = swatches.get_mut(pick.requester) else {
            continue;
        };
        value.0 = pick.texture;
        let Some(session) = state.session.as_mut() else {
            continue;
        };
        let Some(name) = writable_frame(session) else {
            continue;
        };
        let mut scratch_sky = SkySettings::legacy_windlight_default("scratch");
        let mut scratch_water = WaterSettings::legacy_default("scratch");
        if let Some(sky) = session.edited.sky_frames.get_mut(&name) {
            swatch.0.write(sky, &mut scratch_water, pick.texture);
        } else if let Some(water) = session.edited.water_frames.get_mut(&name) {
            swatch.0.write(&mut scratch_sky, water, pick.texture);
        }
        session.dirty = true;
        session.modified = true;
    }
}

/// Keep the session's name in step with its field.
///
/// Read out of the field rather than written into the session on every
/// keystroke, and **not while a reseed is outstanding**: a freshly spawned
/// `EditableText` counts as `Changed`, so without the guard the first pass reads
/// the empty widget back over the name the asset arrived with.
fn read_day_name(
    fields: Query<&EditableText, (With<DayNameField>, Changed<EditableText>)>,
    mut state: ResMut<DayCycleEditorState>,
) {
    for editable in &fields {
        let Some(session) = state.session.as_mut() else {
            continue;
        };
        if session.reseed {
            continue;
        }
        let value = editable.value().to_string();
        if session.name != value {
            session.name = value;
            session.modified = true;
        }
    }
}

/// The two swatch kinds a re-seed paints.
///
/// One parameter rather than two because they are written identically and read
/// the same pair of frames — and because the trackballs pushed the re-seed past
/// Bevy's seven-parameter shape, which is the nudge to group what belongs
/// together rather than to raise a limit.
#[derive(SystemParam)]
struct DaySwatches<'w, 's> {
    /// The colour swatches.
    colors: Query<'w, 's, (&'static DayColorSwatch, &'static mut ColorSwatchValue)>,
    /// The texture swatches.
    textures: Query<'w, 's, (&'static DayTextureSwatch, &'static mut TextureSwatchValue)>,
}

/// Seed every knob from the frame the window is showing.
fn reseed_day_widgets(
    mut commands: Commands,
    mut state: ResMut<DayCycleEditorState>,
    sky_sliders: Query<(Entity, &DaySkySlider, &SliderRange, &SliderValue)>,
    water_sliders: Query<(Entity, &DayWaterSlider, &SliderRange, &SliderValue)>,
    mut swatches: DaySwatches,
    mut fields: Query<&mut EditableText, With<DayNameField>>,
    mut trackballs: Query<(&AimTrackball, &mut TrackballAim)>,
) {
    let Some(session) = state.session.as_mut() else {
        return;
    };
    if !session.reseed {
        return;
    }
    // The content is deferred to the window's first open, so a session can be
    // installed a frame before there is a single widget to seed. Hold the
    // request rather than spend it on an empty query.
    if sky_sliders.is_empty() {
        return;
    }
    session.reseed = false;
    let (sky, water) = session.shown_frames();
    if let Some(sky) = sky.as_ref() {
        for (entity, row_info, range, value) in &sky_sliders {
            let wanted = range.clamp(row_info.0.read(sky));
            // `SliderValue` is immutable, so a new value is *inserted* — and only
            // when it differs, since an insert marks the component changed
            // whether or not it carries a new number.
            if value.0.to_bits() != wanted.to_bits() {
                commands.entity(entity).insert(SliderValue(wanted));
            }
        }
        // The trackballs are seeded here rather than left to the slider sync,
        // which only fires on a slider that *changed*: a keyframe whose sun
        // happens to sit at its two sliders' current values would otherwise
        // leave the marker where the last keyframe put it.
        for (trackball, mut aim) in &mut trackballs {
            let wanted = trackball.knobs.read(sky);
            if *aim != wanted {
                *aim = wanted;
            }
        }
    }
    if let Some(water) = water.as_ref() {
        for (entity, row_info, range, value) in &water_sliders {
            let wanted = range.clamp(row_info.0.read(water));
            if value.0.to_bits() != wanted.to_bits() {
                commands.entity(entity).insert(SliderValue(wanted));
            }
        }
    }
    let sky_frame = sky.unwrap_or_else(|| SkySettings::legacy_windlight_default("scratch"));
    let water_frame = water.unwrap_or_else(|| WaterSettings::legacy_default("scratch"));
    for (swatch, mut value) in &mut swatches.colors {
        value.0 = swatch.0.read(&sky_frame, &water_frame);
    }
    for (swatch, mut value) in &mut swatches.textures {
        value.0 = swatch.0.read(&sky_frame, &water_frame);
    }
    for mut editable in &mut fields {
        if editable.value().to_string() != session.name {
            editable.editor.set_text(&session.name);
        }
    }
}

/// Push the blend at the scrubber into the environment's **edit** layer, so the
/// time of day being authored is the one the user is standing in.
fn push_day_preview(
    mut state: ResMut<DayCycleEditorState>,
    environment: Option<ResMut<EnvironmentState>>,
) {
    let Some(mut environment) = environment else {
        return;
    };
    let Some(session) = state.session.as_mut() else {
        return;
    };
    if !session.dirty {
        return;
    }
    session.dirty = false;
    let (sky, water) = session.preview();
    if let Some(sky) = sky {
        environment.set_edit(EnvironmentAsset::Sky(Box::new(sky)));
    }
    if let Some(water) = water {
        environment.set_edit(EnvironmentAsset::Water(water));
    }
}

/// Take the preview out of the edit layer when the window closes, and forget the
/// session — the reference's `onClose`, which clears `ENV_EDIT`.
fn drop_day_preview_on_close(
    panels: Query<(Entity, &UiPanelShown), Changed<UiPanelShown>>,
    mut state: ResMut<DayCycleEditorState>,
    environment: Option<ResMut<EnvironmentState>>,
) {
    let Some(mut environment) = environment else {
        return;
    };
    for (entity, shown) in &panels {
        if shown.0 || state.ui.panel != Some(entity) {
            continue;
        }
        state.session = None;
        state.pending = None;
        state.insert = None;
        // Both tracks: this window is the only one that previews the pair.
        environment.clear_edit(SettingsKind::Sky);
        environment.clear_edit(SettingsKind::Water);
    }
}

// ---------------------------------------------------------------------------
// The chrome: which track, which buttons, what the readout says.
// ---------------------------------------------------------------------------

/// A track button: show that track's keyframes and its kind of knobs.
fn on_day_track_button(
    mut press: On<Pointer<Press>>,
    buttons: Query<&DayTrackButton>,
    mut state: ResMut<DayCycleEditorState>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let Ok(button) = buttons.get(press.entity).copied() else {
        return;
    };
    press.propagate(false);
    let Some(session) = state.session.as_mut() else {
        return;
    };
    if session.track == button.0 {
        return;
    }
    session.track = button.0;
    session.playing = None;
    // The selection belongs to the track it was made on; re-pick whatever sits
    // under the scrubber on the new one, as the reference's `updateSlider` does.
    let position = session.position;
    session.select_at(position, KEYFRAME_SLOP);
    session.relist = true;
}

/// Keep the track buttons, the knob pages, the readout, the tick labels, the
/// clone-source combo and the enabled states in step with the session.
///
/// Everything here is written **only when it would change**. A `Node`, a
/// `BackgroundColor` or a `Translated` re-inserted every frame is a component
/// marked changed every frame, and the systems downstream of them — the
/// translation sweep, the layout gate — then have work to do sixty times a
/// second over a window where nothing moved. The remembered values are read out
/// of the resource before the session is borrowed and written back after, which
/// is what lets one system both read the session and record what it drew.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources: the state and environment it \
              reads, the translator the readout is formatted through, and the families of node it \
              reconciles — grouped into tuples by role to fit the SystemParam arity"
)]
fn sync_day_chrome(
    mut state: ResMut<DayCycleEditorState>,
    environment: Option<Res<EnvironmentState>>,
    support: Res<SettingsInventorySupport>,
    translator: Translator,
    mut nodes: Query<&mut Node>,
    mut texts: Query<&mut Text>,
    widgets: ChromeWidgets,
    mut paint: ChromePaint,
    mut options: MessageWriter<SetComboOptions>,
    mut commands: Commands,
) {
    let (tracks, buttons, ticks, labels) = widgets;
    let day_length = environment.map_or(0, |environment| environment.settings.day_length);
    let settings_supported = support.supported();
    // A locale switch re-resolves every `Translated` label through the
    // translation sweep; a hand-formatted string has to ask for it.
    let relocalised = translator.changed();
    let ui = state.ui;
    let mut shown_readout = state.shown_readout;
    let mut shown_ticks = state.shown_ticks;
    let mut play_label = state.play_label_playing;
    let mut clone_rows = std::mem::take(&mut state.clone_rows);
    let mut publish_rows = false;

    {
        let session = state.session.as_ref();

        // The knob pages: one kind at a time, as the reference toggles its two
        // layout panels.
        let water_selected = session.is_some_and(|session| session.track == DayTrack::Water);
        show(
            &mut nodes,
            ui.sky_pages,
            session.is_some() && !water_selected,
        );
        show(&mut nodes, ui.water_pages, water_selected);
        show(
            &mut nodes,
            ui.hint,
            session.is_some_and(|session| session.selected.is_none()),
        );

        // The track buttons: the selected one is lit.
        for (entity, button) in &tracks {
            let selected = session.is_some_and(|session| session.track == button.0);
            paint_button(
                &mut commands,
                &labels,
                &mut paint,
                entity,
                session.is_some(),
                selected,
            );
        }

        // The action buttons.
        for (entity, button) in &buttons {
            let enabled = action_enabled(button.0, session, settings_supported);
            paint_button(&mut commands, &labels, &mut paint, entity, enabled, false);
        }

        // The readout, and the ticks it shares its formatting with.
        let readout_key = session.map(|session| (day_percent(session.position), day_length));
        if relocalised || shown_readout != readout_key {
            shown_readout = readout_key;
            let readout = session.map_or_else(String::new, |session| {
                time_label(&translator, session.position, day_length)
            });
            set_text(&mut texts, ui.readout, &readout);
        }
        if relocalised || shown_ticks != Some(day_length) {
            shown_ticks = Some(day_length);
            for (entity, tick) in &ticks {
                let label = time_label(&translator, tick_fraction(tick.0), day_length);
                set_text(&mut texts, Some(entity), &label);
            }
        }

        // The clone source: every sky track but the current one and the empty
        // ones. A row nobody can pick is shown disabled rather than dropped, so
        // the list is the same length whatever the cycle holds.
        let rows: Vec<ComboRow> = session.map_or_else(Vec::new, |session| {
            (0..SKY_TRACK_COUNT)
                .map(|index| {
                    let track = DayTrack::Sky(index);
                    if track == session.track || session.edited.track(track).is_empty() {
                        ComboRow::Disabled
                    } else {
                        ComboRow::Selectable
                    }
                })
                .collect()
        });
        if clone_rows != rows {
            clone_rows = rows;
            publish_rows = true;
        }

        // One button whose label says what pressing it does now, where the
        // reference swaps two buttons in and out of a layout stack.
        let playing = session.is_some_and(|session| session.playing.is_some());
        if play_label != Some(playing) {
            play_label = Some(playing);
            let key = if playing {
                "day-cycle-pause"
            } else {
                "day-cycle-play"
            };
            for (entity, button) in &buttons {
                if button.0 == DayAction::PlayPause {
                    retranslate(&mut commands, &labels, entity, key);
                }
            }
        }
    }

    if publish_rows && let Some(combo) = ui.clone_source {
        let labels: Vec<String> = (0..SKY_TRACK_COUNT)
            .map(|index| track_label_key(DayTrack::Sky(index)).to_owned())
            .collect();
        options.write(SetComboOptions {
            combo,
            labels,
            rows: clone_rows.clone(),
        });
    }
    state.clone_rows = clone_rows;
    state.shown_readout = shown_readout;
    state.shown_ticks = shown_ticks;
    state.play_label_playing = play_label;
}

/// The fraction of the day tick `index` of [`TICKS`] marks.
fn tick_fraction(index: usize) -> f32 {
    #[expect(
        clippy::cast_precision_loss,
        clippy::as_conversions,
        reason = "TICKS is five; the ratio of two small counts is exact in f32"
    )]
    let fraction = index as f32 / TICKS.saturating_sub(1).max(1) as f32;
    fraction
}

/// `NN%`, and the time of day beside it when a day length is known.
///
/// The reference takes its day length from the context it was opened in — a
/// region or a parcel — and shows the percentage alone for an inventory item.
/// This window only ever edits an inventory item, so it takes the length of the
/// day the agent is standing in instead: an authored cycle is going to be worn
/// by *some* day, and the region's is the only one on hand.
fn time_label(translator: &Translator, position: f32, day_length: i32) -> String {
    let percent = day_percent(position);
    let Some((hours, minutes)) = clock_at(position, day_length) else {
        return translator.format(
            "day-cycle-time-percent",
            &TransArgs::new().int("percent", percent),
        );
    };
    translator.format(
        "day-cycle-time",
        &TransArgs::new()
            .int("percent", percent)
            .int("hours", hours)
            // Zero-padded here rather than in the bundle: Fluent has no width
            // specifier, and "3:5" is not a time.
            .text("minutes", &format!("{minutes:02}")),
    )
}

/// A normalised day position as a whole percentage.
fn day_percent(position: f32) -> i64 {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::as_conversions,
        reason = "a normalised 0..1 fraction scaled to a percentage: 0..=100, well inside i64"
    )]
    let percent = (position.clamp(0.0, 1.0) * 100.0).round() as i64;
    percent
}

/// The hours and minutes `position` is at in a `day_length`-second day, or
/// `None` when no day length is known — the reference's `updateTimeAndLabel`,
/// which says only the percentage in that case.
fn clock_at(position: f32, day_length: i32) -> Option<(i64, i64)> {
    if day_length <= 0 {
        return None;
    }
    let total = f64::from(day_length) * f64::from(position.clamp(0.0, 1.0));
    #[expect(
        clippy::cast_possible_truncation,
        clippy::as_conversions,
        reason = "a day length is an i32 count of seconds, so any fraction of one fits i64"
    )]
    let seconds = total as i64;
    Some((
        seconds.div_euclid(3600),
        seconds.div_euclid(60).rem_euclid(60),
    ))
}

/// Whether `action` can be taken on `session` — the reference's `updateButtons`.
///
/// `settings_supported` is the reference's `is_inventory_avail`
/// ([`SettingsInventorySupport`]): on a grid that cannot hold a settings asset,
/// the two saves are the actions that cannot be taken at all, whatever the
/// session says. (The reference *hides* them there rather than greying them;
/// greyed is this viewer's convention for an entry that exists but cannot be
/// used, and it is the one that tells a person the window is not broken.)
fn action_enabled(
    action: DayAction,
    session: Option<&DaySession>,
    settings_supported: bool,
) -> bool {
    let Some(session) = session else {
        return false;
    };
    // A Save As always mints an inventory item; a Save only writes one when the
    // cycle came from one. A land cycle's Save publishes inline and needs
    // nothing of the asset store, so a grid without one does not disable it.
    let needs_store = match action {
        DayAction::SaveAs => true,
        DayAction::Save => session.source.save_needs_asset_store(),
        _other => false,
    };
    if needs_store && !settings_supported {
        return false;
    }
    // Playing takes the hands off everything that edits, as the reference's
    // `can_manipulate` does; the transport itself stays live.
    let can_edit = session.source.editable() && session.playing.is_none();
    match action {
        // The transport never edits anything, Revert only undoes, and a Save As
        // mints a fresh item — none of them need the item to be writable, and
        // none of them are what Play takes the hands off.
        DayAction::PlayPause
        | DayAction::SkipBack
        | DayAction::SkipForward
        | DayAction::Revert
        | DayAction::SaveAs => true,
        DayAction::Save => session.source.editable() && !session.saving,
        DayAction::AddFrame => {
            can_edit
                && session.keyframes().len() < MAX_KEYFRAMES
                && session
                    .edited
                    .keyframe_near(session.track, session.position, KEYFRAME_SLOP)
                    .is_none()
        }
        DayAction::DeleteFrame => {
            can_edit
                && session.selected.is_some()
                && (session.track.may_be_empty() || session.keyframes().len() > 1)
        }
        DayAction::LoadFrame => can_edit && session.keyframes().len() < MAX_KEYFRAMES,
        DayAction::LoadTrack => can_edit,
        DayAction::ClearTrack => {
            can_edit
                && if session.track.may_be_empty() {
                    !session.keyframes().is_empty()
                } else {
                    session.keyframes().len() > 1
                }
        }
        // Copying needs somewhere to copy *from*: another sky track with
        // something on it. The water track has no sibling, so it never has one.
        DayAction::CopyTrack => {
            can_edit
                && session.track.sky_index().is_some()
                && (0..SKY_TRACK_COUNT).any(|index| {
                    let track = DayTrack::Sky(index);
                    track != session.track && !session.edited.track(track).is_empty()
                })
        }
    }
}

/// Show or hide one of the window's containers.
fn show(nodes: &mut Query<&mut Node>, entity: Option<Entity>, visible: bool) {
    let wanted = if visible {
        Display::Flex
    } else {
        Display::None
    };
    if let Some(entity) = entity
        && let Ok(mut node) = nodes.get_mut(entity)
        && node.display != wanted
    {
        node.display = wanted;
    }
}

/// Mark a button enabled or disabled, and lit or not — this window's colours
/// over the shared [`paint_action_button`], which owns the
/// `InteractionDisabled` half.
fn paint_button(
    commands: &mut Commands,
    labels: &Query<&Children>,
    paint: &mut ChromePaint,
    entity: Entity,
    enabled: bool,
    lit: bool,
) {
    let background = if !enabled {
        TRACK_FILL
    } else if lit {
        crate::style::SELECTED_BACKGROUND
    } else {
        crate::style::ACTION_BACKGROUND
    };
    let label = if enabled {
        LABEL_COLOR
    } else {
        DIM_LABEL_COLOR
    };
    paint_action_button(
        commands,
        labels,
        paint,
        entity,
        enabled,
        (background, label),
    );
}

/// Point a button's label at a different Fluent key (the Play / Pause swap).
fn retranslate(
    commands: &mut Commands,
    labels: &Query<&Children>,
    entity: Entity,
    key: &'static str,
) {
    if let Ok(children) = labels.get(entity) {
        for child in children.iter() {
            commands.entity(child).insert(Translated::new(key));
        }
    }
}

/// Write a one-line message into a text node, only when it would change.
fn set_text(texts: &mut Query<&mut Text>, entity: Option<Entity>, message: &str) {
    if let Some(entity) = entity
        && let Ok(mut text) = texts.get_mut(entity)
        && text.0 != message
    {
        message.clone_into(&mut text.0);
    }
}

/// Write a one-line message into the window's status readout.
fn set_status(texts: &mut Query<&mut Text>, status: Option<Entity>, message: &str) {
    set_text(texts, status, message);
}

// ---------------------------------------------------------------------------
// The actions.
// ---------------------------------------------------------------------------

/// A button press: the transport, the track and frame verbs, and the save row.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy observer's parameters are its injected resources: the button pool and its \
              disabled filter, the window's state, the clone-source combo a copy reads, the \
              creation queue a Save As writes, and the command / picker / status outputs"
)]
fn on_day_button(
    mut press: On<Pointer<Press>>,
    buttons: Query<&DayButton>,
    disabled: Query<(), With<bevy::ui::InteractionDisabled>>,
    mut state: ResMut<DayCycleEditorState>,
    support: Res<SettingsInventorySupport>,
    combos: Query<&ComboSelection, With<DayCloneSource>>,
    mut creations: ResMut<PendingSettingsCreations>,
    mut commands: MessageWriter<SlCommand>,
    mut notify: MessageWriter<ShowNotification>,
    mut pickers: MessageWriter<OpenSettingsPicker>,
    mut land_edits: MessageWriter<LandDayCycleEdited>,
    mut texts: Query<&mut Text>,
) {
    if press.button != PointerButton::Primary || disabled.contains(press.entity) {
        return;
    }
    let Ok(button) = buttons.get(press.entity).copied() else {
        return;
    };
    press.propagate(false);
    let status = state.ui.status;
    if state.session.is_none() {
        set_status(&mut texts, status, "Nothing is open in this editor.");
        return;
    }
    // The two saves are greyed on the same predicate, so this is the race a
    // region cross leaves: the press was made while the grid still did
    // settings. The reference refuses in the same place —
    // `LLSettingsVOBase::updateInventoryItem` / `createInventoryItem`, both of
    // which raise this notification and return.
    let touches_inventory = match button.0 {
        DayAction::SaveAs => true,
        DayAction::Save => state
            .session
            .as_ref()
            .is_some_and(|session| session.source.save_needs_asset_store()),
        _other => false,
    };
    if touches_inventory && !support.supported() {
        warn!("day-cycle editor: the region cannot store settings assets; refusing to save");
        notify.write(ShowNotification::new("SettingsUnsuported"));
        set_status(
            &mut texts,
            status,
            "This region cannot store settings assets.",
        );
        return;
    }
    let clone_from = combos.iter().next().map_or(0, |selection| selection.active);
    let requester = press.entity;
    match button.0 {
        DayAction::Save => {
            save_day_cycle(&mut state, &mut commands, &mut land_edits, &mut texts);
        }
        DayAction::SaveAs => {
            save_day_cycle_as(&mut state, &mut creations, &mut commands, &mut texts);
        }
        DayAction::Revert => {
            if let Some(session) = state.session.as_mut() {
                session.edited = session.original.clone();
                session.name = session.original.name.clone();
                session.modified = false;
                session.playing = None;
                session.relist = true;
                let position = session.position;
                session.select_at(position, KEYFRAME_SLOP);
            }
            set_status(&mut texts, status, "Reverted.");
        }
        DayAction::PlayPause => {
            if let Some(session) = state.session.as_mut() {
                let next = match session.playing {
                    // Stopping puts the selection back under the scrubber, as
                    // the reference's `stopPlay` does.
                    Some(_running) => {
                        let position = session.position;
                        session.select_at(position, KEYFRAME_SLOP);
                        None
                    }
                    // Starting lets go of the selection, as its `startPlay`
                    // does: the knobs cannot be editing a keyframe the scrubber
                    // is walking away from.
                    None => {
                        session.selected = None;
                        session.reseed = true;
                        session.relist = true;
                        Some(Playback {
                            from: session.position,
                            elapsed: 0.0,
                        })
                    }
                };
                session.playing = next;
            }
        }
        DayAction::SkipBack | DayAction::SkipForward => {
            if let Some(session) = state.session.as_mut() {
                session.playing = None;
                let forward = button.0 == DayAction::SkipForward;
                if let Some(next) = neighbour_keyframe(session, forward) {
                    session.select_at(next, KEYFRAME_SLOP);
                }
            }
        }
        DayAction::AddFrame => add_day_frame(&mut state, &mut texts),
        DayAction::DeleteFrame => {
            if let Some(session) = state.session.as_mut()
                && let Some(index) = session.selected
            {
                let track = session.track;
                if session.edited.remove_keyframe(track, index) {
                    session.modified = true;
                    session.relist = true;
                    let position = session.position;
                    session.select_at(position, KEYFRAME_SLOP);
                }
            }
        }
        DayAction::ClearTrack => {
            if let Some(session) = state.session.as_mut() {
                let track = session.track;
                session.edited.clear_track(track);
                session.modified = true;
                session.relist = true;
                let position = session.position;
                session.select_at(position, KEYFRAME_SLOP);
            }
        }
        DayAction::CopyTrack => copy_day_track(&mut state, clone_from, &mut texts),
        DayAction::LoadFrame | DayAction::LoadTrack => {
            let Some(session) = state.session.as_ref() else {
                return;
            };
            let kind = match button.0 {
                DayAction::LoadTrack => SettingsKind::DayCycle,
                _frame => match session.track.sky_index() {
                    Some(_sky) => SettingsKind::Sky,
                    None => SettingsKind::Water,
                },
            };
            let (slug, _key) = button.0.labels();
            pickers.write(OpenSettingsPicker {
                requester,
                field: slug.into(),
                kind,
                current: None,
            });
        }
    }
}

/// The keyframe before or after the scrubber, wrapping round the day — the
/// reference's `getLowerBoundFrame` / `getUpperBoundFrame`.
fn neighbour_keyframe(session: &DaySession, forward: bool) -> Option<f32> {
    let position = session.position;
    let keyframes = session.keyframes();
    if keyframes.is_empty() {
        return None;
    }
    let candidate = if forward {
        keyframes
            .iter()
            .filter(|keyframe| keyframe.keyframe > position + f32::EPSILON)
            .min_by(|a, b| a.keyframe.total_cmp(&b.keyframe))
            .or_else(|| {
                keyframes
                    .iter()
                    .min_by(|a, b| a.keyframe.total_cmp(&b.keyframe))
            })
    } else {
        keyframes
            .iter()
            .filter(|keyframe| keyframe.keyframe < position - f32::EPSILON)
            .max_by(|a, b| a.keyframe.total_cmp(&b.keyframe))
            .or_else(|| {
                keyframes
                    .iter()
                    .max_by(|a, b| a.keyframe.total_cmp(&b.keyframe))
            })
    };
    candidate.map(|keyframe| keyframe.keyframe)
}

/// Put a keyframe at the scrubber holding whatever is being previewed there —
/// the reference's `onAddFrame`, which clones its scratch sky or water.
fn add_day_frame(state: &mut DayCycleEditorState, texts: &mut Query<&mut Text>) {
    let status = state.ui.status;
    let Some(session) = state.session.as_mut() else {
        return;
    };
    let track = session.track;
    let position = session.position;
    let (sky, water) = session.preview();
    let filed = match track.sky_index() {
        Some(_sky) => sky.and_then(|sky| session.edited.insert_sky_keyframe(track, position, sky)),
        None => {
            water.and_then(|water| session.edited.insert_water_keyframe(track, position, water))
        }
    };
    if filed.is_none() {
        set_status(texts, status, "There is already a keyframe here.");
        return;
    }
    session.modified = true;
    session.relist = true;
    session.select_at(position, KEYFRAME_SLOP);
}

/// Copy sky track `from` over the current one.
fn copy_day_track(state: &mut DayCycleEditorState, from: usize, texts: &mut Query<&mut Text>) {
    let status = state.ui.status;
    let Some(session) = state.session.as_mut() else {
        return;
    };
    let source_track = DayTrack::Sky(from);
    let into = session.track;
    if source_track == into {
        set_status(texts, status, "A track cannot be copied onto itself.");
        return;
    }
    let source = session.edited.clone();
    if !session.edited.clone_track(&source, source_track, into) {
        set_status(texts, status, "Water and sky tracks do not mix.");
        return;
    }
    session.modified = true;
    session.relist = true;
    let position = session.position;
    session.select_at(position, KEYFRAME_SLOP);
}

/// A settings asset came back from the picker: start fetching its bytes.
fn take_day_settings_pick(
    mut picks: MessageReader<SettingsPicked>,
    buttons: Query<&DayButton>,
    mut state: ResMut<DayCycleEditorState>,
    mut assets: Option<ResMut<EnvironmentAssetManager>>,
    mut texts: Query<&mut Text>,
) {
    for pick in picks.read() {
        if !pick.final_pick {
            // The picker publishes every selection so a consumer can preview it;
            // inserting a keyframe on each of them would fill the track with the
            // ones the user scrolled past.
            continue;
        }
        let Ok(button) = buttons.get(pick.requester).copied() else {
            continue;
        };
        let target = match button.0 {
            DayAction::LoadFrame => InsertTarget::Keyframe,
            DayAction::LoadTrack => InsertTarget::Track,
            _other => continue,
        };
        let Some(chosen) = pick.chosen.as_ref() else {
            continue;
        };
        let Some((track, position)) = state
            .session
            .as_ref()
            .map(|session| (session.track, session.position))
        else {
            continue;
        };
        let asset = AssetKey::from(chosen.asset_id);
        state.insert = Some(PendingInsert {
            asset,
            target,
            track,
            position,
        });
        if let Some(assets) = assets.as_mut() {
            assets.request(asset);
        }
        let status = state.ui.status;
        set_status(&mut texts, status, "Loading…");
    }
}

/// Put a picked asset onto the track once its bytes have decoded.
fn poll_day_insert(
    mut state: ResMut<DayCycleEditorState>,
    assets: Option<Res<EnvironmentAssetManager>>,
    mut texts: Query<&mut Text>,
) {
    let Some(assets) = assets else {
        return;
    };
    let Some(insert) = state.insert else {
        return;
    };
    let status = state.ui.status;
    if assets.is_unavailable(insert.asset) {
        state.insert = None;
        set_status(
            &mut texts,
            status,
            "That settings asset could not be loaded.",
        );
        return;
    }
    let Some(decoded) = assets.get(insert.asset) else {
        return;
    };
    let asset = (**decoded).clone();
    state.insert = None;
    let Some(session) = state.session.as_mut() else {
        return;
    };
    let message = match (insert.target, asset) {
        (InsertTarget::Keyframe, EnvironmentAsset::Sky(sky)) => {
            match session
                .edited
                .insert_sky_keyframe(insert.track, insert.position, *sky)
            {
                Some(_named) => None,
                None => Some("There is already a keyframe here."),
            }
        }
        (InsertTarget::Keyframe, EnvironmentAsset::Water(water)) => {
            match session
                .edited
                .insert_water_keyframe(insert.track, insert.position, water)
            {
                Some(_named) => None,
                None => Some("There is already a keyframe here."),
            }
        }
        (InsertTarget::Track, EnvironmentAsset::DayCycle(source)) => {
            // The **matching** track, which is the reference's default when the
            // track picker is left alone: a cycle's ground track belongs over a
            // ground track, and its 3000 m track over a 3000 m one.
            if session
                .edited
                .clone_track(&source, insert.track, insert.track)
            {
                None
            } else {
                Some("Water and sky tracks do not mix.")
            }
        }
        (_target, _mismatched) => Some("That item is not the kind this action takes."),
    };
    if let Some(message) = message {
        set_status(&mut texts, status, message);
        return;
    }
    session.modified = true;
    session.relist = true;
    let position = session.position;
    session.select_at(position, KEYFRAME_SLOP);
}

// ---------------------------------------------------------------------------
// Saving.
// ---------------------------------------------------------------------------

/// Save the cycle back where it came from: onto its inventory item, or — for a
/// land cycle — into the hands of the panel that opened the window.
fn save_day_cycle(
    state: &mut DayCycleEditorState,
    commands: &mut MessageWriter<SlCommand>,
    edits: &mut MessageWriter<LandDayCycleEdited>,
    texts: &mut Query<&mut Text>,
) {
    let status = state.ui.status;
    let Some(session) = state.session.as_mut() else {
        return;
    };
    if !session.source.editable() {
        set_status(texts, status, "That day cycle may not be modified.");
        return;
    }
    // Read the destination out of the source first: both arms go on to mutate
    // the session, and a borrow of `source` held across that is one the
    // compiler will not have.
    let destination = match &session.source {
        DaySource::Land { panel, label, .. } => Err((*panel, label.clone())),
        DaySource::Inventory(item) => Ok(item.item_id),
    };
    match destination {
        Err((panel, label)) => {
            edits.write(LandDayCycleEdited {
                panel,
                cycle: Box::new(named_cycle(&session.edited, &session.name)),
            });
            // Nothing is in flight and nothing can fail: the cycle is now the
            // panel's draft, which is the new baseline a Revert goes back to.
            session.modified = false;
            session.original = session.edited.clone();
            let told = format!(
                "Handed back to the environment panel for {label} — press Apply there to \
                 publish it."
            );
            set_status(texts, status, &told);
        }
        Ok(item) => {
            let data = environment_asset_to_bytes(&named(&session.edited, &session.name));
            commands.write(SlCommand(Command::UpdateInventoryAsset {
                location: AssetUpdateLocation::AgentInventory { item_id: item },
                asset_type: UpdatableAssetType::Settings,
                data,
            }));
            session.saving = true;
            state.saving = Some(item);
            set_status(texts, status, "Saving…");
        }
    }
}

/// Save the cycle as a fresh inventory item.
fn save_day_cycle_as(
    state: &mut DayCycleEditorState,
    creations: &mut PendingSettingsCreations,
    commands: &mut MessageWriter<SlCommand>,
    texts: &mut Query<&mut Text>,
) {
    let status = state.ui.status;
    let Some(session) = state.session.as_ref() else {
        return;
    };
    let Some(folder) = session.source.save_folder() else {
        set_status(texts, status, "There is nowhere to file a copy yet.");
        return;
    };
    let name = session.name.clone();
    let data = environment_asset_to_bytes(&named(&session.edited, &name));
    // Two steps, not an upload: `NewFileAgentInventory` has no settings arm on
    // either grid, so the simulator mints the item (which is also what stamps its
    // kind) and the body is written onto it when the reply names it — the
    // reference's `createInventoryItem` → `onInventoryItemCreated` →
    // `updateInventoryItem`.
    commands.write(SlCommand(new_settings_item(
        SettingsKind::DayCycle,
        &name,
        folder,
    )));
    creations.enqueue(SettingsKind::DayCycle, Some(data));
    state.saving_as = true;
    set_status(texts, status, "Saving a copy…");
}

/// Report an **in-place** save's outcome.
fn report_day_save(
    mut events: MessageReader<SlEvent>,
    mut state: ResMut<DayCycleEditorState>,
    mut texts: Query<&mut Text>,
) {
    for event in events.read() {
        let status = state.ui.status;
        match &event.0 {
            SlSessionEvent::AssetUploaded {
                new_inventory_item: Some(replied),
                ..
            } => {
                // An in-place save names the item it wrote, so somebody else's
                // upload landing in between is not taken for this one.
                if state.saving != Some(InventoryKey::from(*replied)) {
                    continue;
                }
                state.saving = None;
                if let Some(session) = state.session.as_mut() {
                    session.saving = false;
                    // A save that landed *is* the new baseline: nothing unsaved
                    // is left, and a Revert should now go back to what was
                    // stored rather than to what was on screen before it.
                    session.modified = false;
                    session.original = session.edited.clone();
                }
                set_status(&mut texts, status, "Saved.");
            }
            SlSessionEvent::AssetUploadFailed { reason } => {
                if state.saving.is_none() {
                    continue;
                }
                state.saving = None;
                let message = format!("Save failed: {reason}");
                if let Some(session) = state.session.as_mut() {
                    session.saving = false;
                }
                set_status(&mut texts, status, &message);
            }
            _other => {}
        }
    }
}

/// Report a **Save As** landing, and follow the copy.
///
/// Following it is the reference's `onInventoryCreated`, and it is also the only
/// coherent answer: the cycle on screen is now stored in the *copy*, so a window
/// still pointed at the original would write everything just done into the wrong
/// item on the next plain Save.
fn report_day_save_as(
    mut created: MessageReader<SettingsItemCreated>,
    mut state: ResMut<DayCycleEditorState>,
    mut texts: Query<&mut Text>,
) {
    for item in created.read() {
        if !item.authored || !state.saving_as || item.kind != SettingsKind::DayCycle {
            continue;
        }
        state.saving_as = false;
        let status = state.ui.status;
        if let Some(session) = state.session.as_mut() {
            match &session.source {
                // Following the copy is the reference's `onInventoryCreated`,
                // and the only coherent answer: the cycle on screen is now
                // stored in the copy, so a window still pointed at the original
                // would write everything just done into the wrong item.
                DaySource::Inventory(_item) => {
                    session.source = DaySource::Inventory(EditedItem {
                        item_id: item.item,
                        folder_id: item.folder,
                        // Freshly minted by this agent, so modifiable by
                        // definition.
                        editable: true,
                    });
                    session.modified = false;
                    session.original = session.edited.clone();
                }
                // A land session keeps its context. Filing a copy is a *side*
                // errand — the window is still editing the region's cycle, and
                // silently turning its Save from "hand this to the panel" into
                // "write this item" would be the opposite of what was asked.
                DaySource::Land { .. } => {}
            }
        }
        set_status(&mut texts, status, "Saved a copy.");
    }
}

/// `cycle` named `name` — the cycle's name and the inventory item's are the same
/// string in the reference, which is why editing the field renames the cycle
/// rather than only the item.
fn named(cycle: &DayCycle, name: &str) -> EnvironmentAsset {
    EnvironmentAsset::DayCycle(Box::new(named_cycle(cycle, name)))
}

/// `cycle` with the name field applied, without the asset wrapper — what a
/// land publish carries, since the environment holds the cycle inline rather
/// than as an asset.
fn named_cycle(cycle: &DayCycle, name: &str) -> DayCycle {
    let mut cycle = cycle.clone();
    name.clone_into(&mut cycle.name);
    cycle
}

#[cfg(test)]
mod tests {
    use super::{
        DayAction, DaySession, DaySource, EditedItem, MARKER_WIDTH, MAX_KEYFRAMES, PLAY_SECONDS,
        Playback, TICKS, action_enabled, clock_at, day_percent, named, neighbour_keyframe,
        strip_fraction, tick_fraction,
    };
    use bevy::prelude::{Entity, Vec2};
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{
        DayCycle, DayTrack, EnvironmentAsset, EnvironmentSettings, InventoryFolderKey,
        InventoryKey, KEYFRAME_SLOP, SkySettings, Uuid, environment_asset_from_bytes,
        environment_asset_to_bytes,
    };

    /// A boxed error, so a test can `?` rather than reach for the `panic!` the
    /// workspace's lints (rightly) forbid.
    type TestError = Box<dyn core::error::Error>;

    /// A session over `cycle`, on the ground track at midnight with nothing
    /// selected — the state a freshly opened window is in before its first
    /// `select_at`.
    fn open_session(cycle: DayCycle) -> DaySession {
        DaySession {
            source: DaySource::Inventory(EditedItem {
                item_id: InventoryKey::from(Uuid::from_u128(1)),
                folder_id: InventoryFolderKey::from(Uuid::from_u128(2)),
                editable: true,
            }),
            name: cycle.name.clone(),
            original: cycle.clone(),
            edited: cycle,
            track: DayTrack::GROUND,
            position: 0.0,
            selected: None,
            playing: None,
            dirty: false,
            modified: false,
            reseed: false,
            relist: false,
            saving: false,
        }
    }

    /// The built-in cycle plus keyframes at `positions` on the ground track.
    fn cycle_with(positions: &[f32]) -> DayCycle {
        let mut cycle = EnvironmentSettings::legacy_windlight_default().day_cycle;
        for (index, position) in positions.iter().enumerate() {
            drop(cycle.insert_sky_keyframe(
                DayTrack::GROUND,
                *position,
                SkySettings::legacy_windlight_default(&format!("Frame {index}")),
            ));
        }
        cycle
    }

    /// **A marker is placed by its left edge, and the pointer is measured
    /// against the same span.** Getting this wrong is not a wrong number — it is
    /// a keyframe that jumps half a marker sideways the instant it is grabbed,
    /// every time, which looks like the drag is broken rather than the maths.
    #[test]
    fn the_timeline_maps_a_pointer_to_the_span_a_marker_is_placed_in() {
        let rect = (Vec2::new(100.0, 50.0), Vec2::new(200.0, 14.0));
        let span = 200.0 - MARKER_WIDTH;
        // The left edge of the usable span reads zero, its right edge one.
        assert!(strip_fraction(Vec2::new(100.0 + MARKER_WIDTH / 2.0, 55.0), rect).abs() < 0.001);
        assert!(
            (strip_fraction(Vec2::new(100.0 + MARKER_WIDTH / 2.0 + span, 55.0), rect) - 1.0).abs()
                < 0.001
        );
        // The middle is a half, and everything past either end clamps rather
        // than running off the day.
        assert!(
            (strip_fraction(
                Vec2::new(100.0 + MARKER_WIDTH / 2.0 + span / 2.0, 55.0),
                rect
            ) - 0.5)
                .abs()
                < 0.001
        );
        assert!(strip_fraction(Vec2::new(-500.0, 55.0), rect).abs() < f32::EPSILON);
        assert!((strip_fraction(Vec2::new(5000.0, 55.0), rect) - 1.0).abs() < f32::EPSILON);
    }

    /// The tick labels span the whole day, ends included.
    #[test]
    fn the_ticks_run_from_the_start_of_the_day_to_its_end() {
        assert!(tick_fraction(0).abs() < f32::EPSILON);
        assert!((tick_fraction(TICKS.saturating_sub(1)) - 1.0).abs() < f32::EPSILON);
        assert!((tick_fraction(2) - 0.5).abs() < f32::EPSILON);
    }

    /// **Skipping wraps round the day.** The last keyframe's Next is the first
    /// one of the next day, not nothing — a transport that stops dead at the end
    /// of a cycle that has no end is the one thing a day cycle must not do.
    #[test]
    fn skipping_walks_the_keyframes_and_wraps_at_midnight() {
        let mut session = open_session(cycle_with(&[0.25, 0.75]));
        // The built-in cycle already has a keyframe at 0.0, so the track is
        // [0.0, 0.25, 0.75].
        session.position = 0.0;
        assert_eq!(neighbour_keyframe(&session, true), Some(0.25));
        session.position = 0.75;
        assert_eq!(
            neighbour_keyframe(&session, true),
            Some(0.0),
            "wraps forward"
        );
        session.position = 0.0;
        assert_eq!(
            neighbour_keyframe(&session, false),
            Some(0.75),
            "wraps backward"
        );
        session.position = 0.5;
        assert_eq!(neighbour_keyframe(&session, false), Some(0.25));
        // A track with nothing on it has nowhere to skip to.
        let mut empty = session;
        empty.track = DayTrack::Sky(3);
        assert_eq!(neighbour_keyframe(&empty, true), None);
    }

    /// Selecting snaps the scrubber onto the keyframe it lands near, and lets go
    /// of the selection between keyframes — which is what makes the knob pages
    /// read-only there rather than writing into a frame nobody chose.
    #[test]
    fn selecting_snaps_onto_a_keyframe_and_lets_go_between_them() {
        let mut session = open_session(cycle_with(&[0.5]));
        session.select_at(0.5 + KEYFRAME_SLOP / 2.0, KEYFRAME_SLOP);
        assert_eq!(session.selected, Some(1));
        assert!((session.position - 0.5).abs() < f32::EPSILON, "snapped");
        session.select_at(0.3, KEYFRAME_SLOP);
        assert_eq!(session.selected, None);
        assert!(
            (session.position - 0.3).abs() < f32::EPSILON,
            "left where it was put"
        );
    }

    /// **What the knob pages show is the selected keyframe's own frame, and the
    /// blend at the scrubber when nothing is selected.** The second half is the
    /// reference's locked panels: they still show a sky, so the user can see
    /// what is between two keyframes, and it is nobody's stored frame.
    #[test]
    fn the_pages_show_the_selected_frame_or_the_blend() -> Result<(), TestError> {
        let mut cycle = EnvironmentSettings::legacy_windlight_default().day_cycle;
        drop(cycle.insert_sky_keyframe(
            DayTrack::GROUND,
            0.5,
            SkySettings {
                haze_density: 4.0,
                ..SkySettings::legacy_windlight_default("Noon")
            },
        ));
        let mut session = open_session(cycle);
        session.select_at(0.5, KEYFRAME_SLOP);
        let (sky, water) = session.shown_frames();
        assert_eq!(water, None, "a sky track shows no water frame");
        let sky = sky.ok_or("a selected sky keyframe shows its frame")?;
        assert!((sky.haze_density - 4.0).abs() < 0.001);

        // Between the two keyframes, the blend — neither endpoint's value.
        session.select_at(0.25, KEYFRAME_SLOP);
        assert_eq!(session.selected, None);
        let (blended, _water) = session.shown_frames();
        let blended = blended.ok_or("an unselected scrubber still shows a sky")?;
        assert!(blended.haze_density > 0.0);
        assert!((blended.haze_density - 4.0).abs() > 0.001);
        Ok(())
    }

    /// **The water track previews the ground track's sky.** A sky has to come
    /// from somewhere while the water is being edited, and the reference takes
    /// it from track 1 (`skytrack = mCurrentTrack ? mCurrentTrack : 1`).
    #[test]
    fn the_water_track_still_previews_a_sky() -> Result<(), TestError> {
        let mut session = open_session(EnvironmentSettings::legacy_windlight_default().day_cycle);
        session.track = DayTrack::Water;
        let (sky, water) = session.preview();
        assert!(sky.is_some(), "the ground track's sky stands in");
        assert!(water.is_some());
        // And the pages show the water keyframe, not the sky one.
        session.select_at(0.0, KEYFRAME_SLOP);
        let (page_sky, page_water) = session.shown_frames();
        assert_eq!(page_sky, None);
        assert!(page_water.is_some());
        Ok(())
    }

    /// **The buttons say what the cycle allows**, which is the reference's
    /// `updateButtons` and the only thing standing between a press and an
    /// operation the model would refuse.
    #[test]
    fn the_verbs_are_offered_exactly_where_they_would_work() {
        // Nothing open: nothing at all.
        for action in [
            DayAction::Save,
            DayAction::AddFrame,
            DayAction::PlayPause,
            DayAction::CopyTrack,
        ] {
            assert!(
                !action_enabled(action, None, true),
                "{action:?} with no session"
            );
        }

        let mut session = open_session(cycle_with(&[0.5]));
        session.select_at(0.5, KEYFRAME_SLOP);
        // On a keyframe there is nothing to add and something to delete.
        assert!(!action_enabled(DayAction::AddFrame, Some(&session), true));
        assert!(action_enabled(DayAction::DeleteFrame, Some(&session), true));
        // Between keyframes, the other way round.
        session.select_at(0.25, KEYFRAME_SLOP);
        assert!(action_enabled(DayAction::AddFrame, Some(&session), true));
        assert!(!action_enabled(
            DayAction::DeleteFrame,
            Some(&session),
            true
        ));

        // The ground track cannot be emptied, so with one keyframe left neither
        // Clear nor Delete is on offer.
        let mut bare = open_session(EnvironmentSettings::legacy_windlight_default().day_cycle);
        bare.select_at(0.0, KEYFRAME_SLOP);
        assert!(!action_enabled(DayAction::ClearTrack, Some(&bare), true));
        assert!(!action_enabled(DayAction::DeleteFrame, Some(&bare), true));

        // Copy needs another sky track with something on it, and the water track
        // never has a sibling to copy from.
        assert!(!action_enabled(DayAction::CopyTrack, Some(&bare), true));
        let mut copyable = bare.clone();
        drop(copyable.edited.insert_sky_keyframe(
            DayTrack::Sky(1),
            0.5,
            SkySettings::legacy_windlight_default("High"),
        ));
        assert!(action_enabled(DayAction::CopyTrack, Some(&copyable), true));
        copyable.track = DayTrack::Water;
        assert!(!action_enabled(DayAction::CopyTrack, Some(&copyable), true));

        // A read-only item is previewed, not edited — but it can still be saved
        // as a copy, which is the whole point of Save As.
        let mut locked = open_session(cycle_with(&[0.5]));
        locked.source = DaySource::Inventory(EditedItem {
            item_id: InventoryKey::from(Uuid::from_u128(1)),
            folder_id: InventoryFolderKey::from(Uuid::from_u128(2)),
            editable: false,
        });
        locked.select_at(0.5, KEYFRAME_SLOP);
        assert!(!action_enabled(DayAction::Save, Some(&locked), true));
        assert!(!action_enabled(DayAction::DeleteFrame, Some(&locked), true));
        assert!(action_enabled(DayAction::SaveAs, Some(&locked), true));

        // A **land** session's Save publishes the cycle inline and never
        // touches inventory, so a grid that cannot store a settings asset
        // takes away its Save As and leaves its Save alone. Getting this
        // backwards would make the whole feature unusable on exactly the
        // grids where editing a region's inline cycle is the only way to
        // change it.
        let mut land = open_session(cycle_with(&[0.5]));
        land.source = DaySource::Land {
            panel: Entity::from_raw_u32(1).unwrap_or(Entity::PLACEHOLDER),
            label: "this region".to_owned(),
            folder_id: Some(InventoryFolderKey::from(Uuid::from_u128(2))),
            editable: true,
        };
        land.select_at(0.5, KEYFRAME_SLOP);
        assert!(action_enabled(DayAction::Save, Some(&land), false));
        assert!(!action_enabled(DayAction::SaveAs, Some(&land), false));
        assert!(action_enabled(DayAction::SaveAs, Some(&land), true));
        // A panel the agent may not publish from opens the window read-only,
        // exactly as a no-modify item does.
        let mut read_only = land.clone();
        read_only.source = DaySource::Land {
            panel: Entity::from_raw_u32(1).unwrap_or(Entity::PLACEHOLDER),
            label: "this parcel".to_owned(),
            folder_id: None,
            editable: false,
        };
        assert!(!action_enabled(DayAction::Save, Some(&read_only), true));
        assert!(!action_enabled(
            DayAction::DeleteFrame,
            Some(&read_only),
            true
        ));

        // A grid that cannot hold a settings asset takes both saves away —
        // including the Save As a read-only item could otherwise still do,
        // since a copy has to be filed somewhere too. Everything that only
        // edits the open day is untouched: the window keeps working, it simply
        // has nowhere to put the result.
        let mut editable = open_session(cycle_with(&[0.5]));
        editable.select_at(0.5, KEYFRAME_SLOP);
        assert!(action_enabled(DayAction::Save, Some(&editable), true));
        assert!(!action_enabled(DayAction::Save, Some(&editable), false));
        assert!(!action_enabled(DayAction::SaveAs, Some(&editable), false));
        assert!(action_enabled(DayAction::Revert, Some(&editable), false));
        assert!(action_enabled(DayAction::PlayPause, Some(&editable), false));
        assert!(action_enabled(
            DayAction::DeleteFrame,
            Some(&editable),
            false
        ));

        // Playing takes the hands off the editing verbs and leaves the
        // transport alone.
        let mut playing = open_session(cycle_with(&[0.5]));
        playing.playing = Some(Playback {
            from: 0.0,
            elapsed: 0.0,
        });
        assert!(!action_enabled(DayAction::AddFrame, Some(&playing), true));
        assert!(!action_enabled(DayAction::ClearTrack, Some(&playing), true));
        assert!(action_enabled(DayAction::PlayPause, Some(&playing), true));
    }

    /// A full track offers no more room — the reference's `canAddSliders`, which
    /// is what actually bounds a track there.
    #[test]
    fn a_full_track_stops_offering_more_keyframes() {
        // Evenly spread, far enough apart to clear the slop.
        #[expect(
            clippy::cast_precision_loss,
            clippy::as_conversions,
            reason = "MAX_KEYFRAMES is twenty; the ratio of two small counts is exact in f32"
        )]
        let positions: Vec<f32> = (1..MAX_KEYFRAMES)
            .map(|index| index as f32 / MAX_KEYFRAMES as f32)
            .collect();
        let mut session = open_session(cycle_with(&positions));
        assert_eq!(session.keyframes().len(), MAX_KEYFRAMES);
        // Somewhere with room, so only the count can be refusing.
        session.position = 1.0 / f32::from(2_u8) / f32::from(20_u8);
        assert!(!action_enabled(DayAction::AddFrame, Some(&session), true));
        assert!(!action_enabled(DayAction::LoadFrame, Some(&session), true));
    }

    /// **Play walks a whole day in the reference's minute.** The number is the
    /// feel of the feature: at ten seconds it is a strobe, at ten minutes it is
    /// not a preview.
    #[test]
    fn play_covers_the_whole_day_in_the_reference_s_minute() {
        assert!((PLAY_SECONDS - 60.0).abs() < f32::EPSILON);
        let from = 0.75_f32;
        // Half the play time from three-quarters through wraps past midnight.
        let travelled = (PLAY_SECONDS / 2.0) / PLAY_SECONDS;
        assert!(((from + travelled).rem_euclid(1.0) - 0.25).abs() < 0.001);
    }

    /// The readout says the percentage always, and a clock only when a day
    /// length is known — the reference shows the percentage alone for an
    /// inventory item, and this window is only ever editing one.
    #[test]
    fn the_readout_reads_a_clock_only_where_there_is_a_day_to_read_it_from() {
        assert_eq!(day_percent(0.0), 0);
        assert_eq!(day_percent(0.255), 26);
        assert_eq!(day_percent(1.5), 100, "a position past the day clamps");
        assert_eq!(clock_at(0.5, 0), None);
        assert_eq!(clock_at(0.5, -1), None);
        // A four-hour day, the reference's default, is two hours in at the half.
        assert_eq!(clock_at(0.5, 4 * 60 * 60), Some((2, 0)));
        assert_eq!(clock_at(0.25, 4 * 60 * 60), Some((1, 0)));
        // And a day long enough for the hours to run past a clock face still
        // reads as hours, as the reference's does.
        assert_eq!(clock_at(1.0, 7 * 24 * 60 * 60), Some((168, 0)));
        assert_eq!(clock_at(0.1, 60 * 60), Some((0, 6)));
    }

    /// **A save renames the cycle, not only the item**, and re-encodes the whole
    /// thing: the frames a track no longer names are gone, and the ones it does
    /// come back with their keyframes.
    #[test]
    fn a_save_writes_the_whole_cycle_under_the_name_in_the_field() -> Result<(), TestError> {
        let cycle = cycle_with(&[0.25, 0.75]);
        let asset = named(&cycle, "A day of my own");
        let bytes = environment_asset_to_bytes(&asset);
        let Some(EnvironmentAsset::DayCycle(read_back)) =
            environment_asset_from_bytes("A day of my own", &bytes)
        else {
            return Err("a saved day cycle decodes back as a day cycle".into());
        };
        assert_eq!(read_back.name, "A day of my own");
        assert_eq!(read_back.track(DayTrack::GROUND).len(), 3);
        // Every keyframe still resolves to a frame the asset carries.
        for keyframe in read_back.track(DayTrack::GROUND) {
            assert!(
                read_back.sky_frames.contains_key(&keyframe.name),
                "{} is named but not defined",
                keyframe.name
            );
        }
        Ok(())
    }
}
