//! The **fixed-environment editors** (`viewer-environment-fixed-editor`): the
//! windows that author a sky or a water frame as an EEP settings **asset** in
//! inventory.
//!
//! Two windows, one per kind, as the reference has them
//! (`LLFloaterFixedEnvironmentSky` / `LLFloaterFixedEnvironmentWater`): a name
//! field, the knobs on tabs, and Save / Save As / Revert. What they edit is an
//! *asset* — the item opened from inventory, saved back over itself with
//! `UpdateSettingsAgentInventory`, or copied into a fresh item the simulator
//! mints and the same capability then fills in.
//!
//! # The preview is a layer, not a write
//!
//! While an editor is open, the frame it is holding renders — the reference's
//! `ENV_EDIT`, installed on open and cleared on close. Here that is
//! [`EnvironmentState::set_edit`] / [`EnvironmentState::clear_edit`], which sit
//! *above* the local layer rather than in it. So a personal environment the
//! user built in the Personal Lighting window is still there, untouched, when
//! the editor closes — whether the edit was saved or thrown away.
//!
//! The two windows own separate tracks, so both can be open at once and each
//! previews its own half of the environment.
//!
//! # What a save writes
//!
//! The whole asset, re-encoded from the frame in hand
//! (`environment_asset_to_bytes`). That is why the atmospheric-density profiles
//! are carried through `SkySettings` even though nothing here renders from them
//! (see `sl_proto::DensityLayer`): a save that emitted only the fields this
//! window shows would quietly replace the author's atmosphere with the
//! reference's defaults for everyone who opens the item afterwards.
//!
//! # The Density tab edits what this viewer does not draw
//!
//! The fourth sky tab is the reference's Density panel: the Rayleigh, Mie and
//! ozone-absorption profiles. This viewer's sky is the legacy WindLight formula
//! and reads none of them, so those sixteen sliders change the **asset** and
//! not the preview — they are there because a settings asset is authored for
//! the grid, not only for the window it was authored in, and every other viewer
//! does read them. The renderer half is
//! `viewer-environment-density-profiles`.
//!
//! Two deliberate divergences in that tab. Editing a term writes **layer 0 in
//! place**, where the reference replaces the whole profile with a single layer
//! (`createSingleLayerDensityProfile`) and so silently discards the second
//! layer of the ozone ramp its own default ships. And a term written into a
//! frame that carries no profile at all materialises the reference's default
//! profile first, rather than storing one term beside four zeroes.
//!
//! # Import reads a preset that was never an asset
//!
//! **Import** puts the host's file chooser up
//! ([`sl_viewer_platform::file_dialog`]) and converts the legacy WindLight
//! `.xml` preset it comes back with
//! ([`legacy_preset_from_bytes`]). What lands in the window is a frame with
//! **no inventory item behind it** — the reference's
//! `loadInventoryItem(LLUUID::null)` — so the session's `item` is `None`, Save
//! has nothing to write onto and says so, and a **Save As** is what files it
//! (in the Settings folder, where a brand-new settings item goes). The session
//! starts *modified*, because the frame on screen exists nowhere else.
//!
//! The confirmation for throwing unsaved work away is raised **before** the
//! dialog, as the reference's `onButtonImport` does: being asked whether you
//! meant it after picking a file is the wrong order.
//!
//! Reference (Firestorm, read-only): `llfloaterfixedenvironment.cpp`,
//! `panel_settings_sky_atmos.xml`, `panel_settings_sky_clouds.xml`,
//! `panel_settings_sky_sunmoon.xml`, `panel_settings_water.xml`,
//! `llsettingsvo.cpp` (the asset serialisation and its upload).

use std::collections::VecDeque;

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui_widgets::{SliderRange, SliderValue, ValueChange};
use sl_client_bevy::{
    AssetKey, AssetUpdateLocation, Command, EnvironmentAsset, FolderType, InventoryFolderKey,
    InventoryKey, SettingsKind, SkySettings, SlCommand, SlEvent, SlSessionEvent, TextureKey,
    UpdatableAssetType, WaterSettings, environment_asset_to_bytes, legacy_preset_from_bytes,
    legacy_preset_name,
};
use sl_viewer_inventory::inventory::InventoryModel;
use sl_viewer_inventory::inventory_actions::new_settings_item;
use sl_viewer_notifications::{NotificationResponse, ShowNotification};
use sl_viewer_pickers::ui_texture_picker::TextureSwatchValue;
use sl_viewer_platform::environment_assets::EnvironmentAssetManager;
use sl_viewer_platform::file_dialog::{
    FileDialogClosed, FileDialogFilter, FileDialogOutcome, OpenFileDialog,
};
use sl_viewer_ui_core::i18n::{Translated, Translator};
use sl_viewer_ui_core::ui::{UiPanelShown, UiRoot, UiScaffoldSystems, column, row};
use sl_viewer_ui_core::ui_font::UiFont;
use sl_viewer_ui_widgets::floater::{
    DeferredFloaterContent, FloaterCaps, FloaterCommand, FloaterHandle, FloaterOp, FloaterSpec,
    FloaterSystems, spawn_floater,
};
use sl_viewer_ui_widgets::floater_persist::FloaterOpenExempt;
use sl_viewer_ui_widgets::ui_color_picker::{ColorPicked, ColorSwatchValue};
use sl_viewer_ui_widgets::ui_tab::{
    DEFAULT_ELLIPSIS, TabPlacement, TabSpec, fill_tab_container, spawn_tab_container,
};
use sl_viewer_ui_widgets::ui_text_input::{TextInputKind, TextInputSpec, spawn_text_input};
use sl_viewer_ui_widgets::ui_trackball::TrackballAim;
use sl_viewer_world_api::{
    OpenSettingsEditor, PendingSettingsCreations, SettingsItemCreated, TexturePicked,
};
use sl_viewer_world_scene::environment::EnvironmentState;

use crate::knobs::{ColorKnob, SkyKnob, TextureKnob, WaterKnob};
use crate::rows::{
    AimTrackball, spawn_action_button, spawn_color_row, spawn_slider, spawn_texture_row,
    spawn_trackball_row, tag_aim_slider,
};
use crate::style::{DIM_LABEL_COLOR, FONT_SIZE, LABEL_COLOR};
use crate::tabs::{SKY_TABS, TabPage, WATER_TABS};

/// The sky editor's floater id.
pub const SKY_EDITOR_FLOATER_ID: &str = "settings-editor-sky";

/// The water editor's floater id.
pub const WATER_EDITOR_FLOATER_ID: &str = "settings-editor-water";

/// How many columns a tab panel lays its controls out in.
const COLUMNS: usize = 3;

// ---------------------------------------------------------------------------
// Which editor.
// ---------------------------------------------------------------------------

/// Which of the two editors a window, a control or a session belongs to.
///
/// A settings asset can also be a whole day cycle, which is a third editor with
/// a timeline rather than a knob list ([[viewer-environment-day-cycle-editor]]);
/// this type is deliberately the two *frame* kinds only, so a day cycle cannot
/// be routed into a window that has nowhere to put it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EditorKind {
    /// The sky editor.
    Sky,
    /// The water editor.
    Water,
}

impl EditorKind {
    /// The settings kind this editor authors.
    #[must_use]
    pub const fn settings_kind(self) -> SettingsKind {
        match self {
            Self::Sky => SettingsKind::Sky,
            Self::Water => SettingsKind::Water,
        }
    }

    /// The editor a settings item of `kind` opens in, or `None` for a day
    /// cycle, which neither of these windows can show.
    #[must_use]
    pub const fn of_settings(kind: SettingsKind) -> Option<Self> {
        match kind {
            SettingsKind::Sky => Some(Self::Sky),
            SettingsKind::Water => Some(Self::Water),
            SettingsKind::DayCycle => None,
        }
    }

    /// The floater id — and the element-id prefix every control in the window
    /// is named by.
    #[must_use]
    pub const fn element(self) -> &'static str {
        match self {
            Self::Sky => SKY_EDITOR_FLOATER_ID,
            Self::Water => WATER_EDITOR_FLOATER_ID,
        }
    }

    /// The window's title, and its Fluent key.
    const fn title(self) -> (&'static str, &'static str) {
        match self {
            Self::Sky => ("Sky Settings", "settings-editor-sky-title"),
            Self::Water => ("Water Settings", "settings-editor-water-title"),
        }
    }

    /// The tabs this editor shows — declared below, after the knob tables they
    /// are built out of.
    const fn tabs(self) -> &'static [TabPage] {
        match self {
            Self::Sky => SKY_TABS,
            Self::Water => WATER_TABS,
        }
    }
}

// ---------------------------------------------------------------------------
// Components.
// ---------------------------------------------------------------------------

/// A sky slider in an editor window.
#[derive(Component, Debug, Clone, Copy)]
struct EditorSkySlider(SkyKnob);

/// A water slider in an editor window.
#[derive(Component, Debug, Clone, Copy)]
struct EditorWaterSlider(WaterKnob);

/// A colour swatch in an editor window, with the window it belongs to (the
/// water fog colour is the water editor's; the rest are the sky editor's).
#[derive(Component, Debug, Clone, Copy)]
struct EditorColorSwatch {
    /// Which window the swatch is in, and so which session it writes.
    editor: EditorKind,
    /// The colour it edits.
    knob: ColorKnob,
}

/// A texture swatch in an editor window.
#[derive(Component, Debug, Clone, Copy)]
struct EditorTextureSwatch {
    /// Which window the swatch is in.
    editor: EditorKind,
    /// The texture it edits.
    knob: TextureKnob,
}

/// What a chrome button does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditorAction {
    /// Read a legacy WindLight preset off disk into this window.
    Import,
    /// Save the asset back onto the item it came from.
    Save,
    /// Save a copy as a new inventory item.
    SaveAs,
    /// Throw the edits away and go back to the frame as loaded.
    Revert,
}

/// A chrome button: which window it is in and what it does.
#[derive(Component, Debug, Clone, Copy)]
struct EditorButton {
    /// Which window the button is in.
    editor: EditorKind,
    /// What pressing it does.
    action: EditorAction,
}

/// The name field of an editor window.
#[derive(Component, Debug, Clone, Copy)]
struct EditorNameField(EditorKind);

// ---------------------------------------------------------------------------
// State.
// ---------------------------------------------------------------------------

/// One window's entity handles, filled in as its content is built.
#[derive(Debug, Clone, Copy, Default)]
struct EditorUi {
    /// The floater root, carrying the `UiPanelShown` the window is shown by.
    panel: Option<Entity>,
    /// The one-line status readout under the buttons.
    status: Option<Entity>,
}

/// The frame one window is editing.
#[expect(
    clippy::struct_excessive_bools,
    reason = "the four are independent facts about one session with different lifetimes, not a \
              state machine an enum could replace: `dirty` and `reseed` are one-shot signals \
              spent by the systems that read them (push the preview; re-seed the widgets), \
              while `modified` and `saving` are states that outlive a frame — and a session \
              can legitimately be all four at once, which is exactly why `modified` could not \
              simply reuse `dirty`"
)]
#[derive(Debug, Clone)]
struct EditSession {
    /// The inventory item this asset came from, or `None` for a frame that has
    /// never been filed.
    ///
    /// Almost always there is one: a settings frame reaches these windows by
    /// being opened, and a frame created from nothing is the inventory's New Sky
    /// / New Water, which files the item first and opens it after. The exception
    /// is an **imported** legacy WindLight preset, which is a frame off disk
    /// with nothing behind it — the reference's `loadInventoryItem(LLUUID::null)`
    /// — so Save has nothing to write onto and a Save As is what files it.
    item: Option<EditedItem>,
    /// The name shown in the field and written into the asset.
    name: String,
    /// The frame as loaded — what Revert restores.
    original: EnvironmentAsset,
    /// The frame being edited.
    edited: EnvironmentAsset,
    /// A control changed the frame: push it to the edit layer. Spent every
    /// frame, so it cannot answer "are there unsaved changes".
    dirty: bool,
    /// The frame differs from what was loaded (or last saved) — the
    /// reference's `isDirty`, and what a confirmation has to ask about.
    modified: bool,
    /// The frame was replaced (opened, or reverted): re-seed every widget.
    reseed: bool,
    /// A Save is in flight, so its reply is this window's.
    saving: bool,
}

/// The inventory item an open session came from.
///
/// Shared with the day-cycle editor, which has exactly the same three facts to
/// remember about the item it is holding: what a Save writes onto, where a Save
/// As files the copy, and whether either is allowed at all.
#[derive(Debug, Clone, Copy)]
pub(crate) struct EditedItem {
    /// The item whose asset a Save replaces.
    pub(crate) item_id: InventoryKey,
    /// The folder a Save As files the copy in.
    pub(crate) folder_id: InventoryFolderKey,
    /// Whether the item may be written back to at all.
    pub(crate) editable: bool,
}

impl EditSession {
    /// The sky being edited, if this is a sky session.
    fn sky_mut(&mut self) -> Option<&mut SkySettings> {
        match &mut self.edited {
            EnvironmentAsset::Sky(sky) => Some(&mut **sky),
            _other => None,
        }
    }

    /// The water being edited, if this is a water session.
    const fn water_mut(&mut self) -> Option<&mut WaterSettings> {
        match &mut self.edited {
            EnvironmentAsset::Water(water) => Some(water),
            _other => None,
        }
    }
}

/// Both windows' state.
#[derive(Resource, Debug, Default)]
struct SettingsEditors {
    /// The sky window.
    sky: EditorState,
    /// The water window.
    water: EditorState,
    /// The in-place saves whose reply has not arrived, oldest first, matched
    /// by the item each names.
    ///
    /// A Save As is **not** here: minting an item is the whole viewer's shared
    /// FIFO ([`PendingItemCreations`]), because the flags stamp that follows it
    /// must not be popped by somebody else's upload.
    saves: VecDeque<PendingSave>,
}

impl SettingsEditors {
    /// One window's state, mutably.
    const fn get_mut(&mut self, editor: EditorKind) -> &mut EditorState {
        match editor {
            EditorKind::Sky => &mut self.sky,
            EditorKind::Water => &mut self.water,
        }
    }
}

/// One window: its chrome, its session, and whatever fetch it is waiting on.
#[derive(Debug, Default)]
struct EditorState {
    /// The window's entity handles.
    ui: EditorUi,
    /// The frame being edited, or `None` while the window has nothing open.
    session: Option<EditSession>,
    /// The item whose asset is being fetched, if the window is waiting on one.
    pending: Option<PendingOpen>,
}

/// An in-place save whose reply has not arrived yet.
#[derive(Debug, Clone, Copy)]
struct PendingSave {
    /// The window that asked for it, so the outcome lands in its status line.
    editor: EditorKind,
    /// The item the save writes onto — the reply names it, which is what tells
    /// this save apart from every other upload in flight.
    item: InventoryKey,
}

/// An open whose asset has not arrived yet.
#[derive(Debug, Clone)]
struct PendingOpen {
    /// The request that started it, replayed once the asset decodes.
    request: OpenSettingsEditor,
    /// The asset being waited for.
    asset: AssetKey,
}

// ---------------------------------------------------------------------------
// Plugin.
// ---------------------------------------------------------------------------

/// The plugin wiring both settings editors into a host.
#[derive(Debug, Clone, Copy, Default)]
pub struct SettingsEditorPlugin;

impl Plugin for SettingsEditorPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<crate::rows::RowsPlugin>() {
            app.add_plugins(crate::rows::RowsPlugin);
        }
        app.init_resource::<SettingsEditors>()
            // Idempotent: the inventory owns this queue and its consumer, but
            // a host that stands these windows up without the inventory (the
            // gallery) must still have somewhere for a Save As to enqueue.
            .init_resource::<PendingSettingsCreations>()
            .init_resource::<PendingEditorSaveAs>()
            .init_resource::<PendingEditorReplace>()
            // The confirmation channels. Registered here too (idempotent) so
            // these windows stand up in a host that brought no notification
            // plugin — a `MessageWriter` for an unregistered message is a system
            // that panics on its first run, not a quiet no-op.
            .add_message::<ShowNotification>()
            .add_message::<NotificationResponse>()
            .add_message::<SettingsItemCreated>()
            .add_message::<OpenSettingsEditor>()
            // The Import half. Idempotent for the same reason as the
            // confirmation channels above: a host that stands these windows up
            // without the platform layer's dialog service (the gallery) must
            // still have somewhere for the request to go and somewhere for the
            // reply to be read from.
            .add_message::<OpenFileDialog>()
            .add_message::<FileDialogClosed>()
            .add_systems(
                Startup,
                spawn_settings_editors.after(UiScaffoldSystems::SpawnRoot),
            )
            .add_systems(
                Update,
                // Ordered for the same reason the Personal Lighting window's
                // are: an open has to reach the widgets and the edit layer in
                // the frame it happened, or the window's first frame shows the
                // last asset's values.
                (
                    // After the manager's command pass, so the raise this open
                    // performs outlives the raise the opening press performed on
                    // the window the row was clicked in.
                    open_settings_editor.after(FloaterSystems::Commands),
                    poll_pending_open,
                    // Before the re-seed and the preview push below, so an
                    // imported frame reaches the widgets and the edit layer in
                    // the frame the dialog closed.
                    apply_imported_preset,
                    apply_editor_color_picks,
                    apply_editor_texture_picks,
                    read_editor_names,
                    reseed_editor_widgets,
                    push_editor_preview,
                    report_editor_save,
                    report_editor_save_as,
                    confirm_editor_replace,
                    drop_preview_on_close,
                )
                    .chain(),
            );
    }
}

/// The sky editor's [`FloaterSpec`] — shared with the `FLOATERS` registry, so
/// the swept window is the one the viewer spawns.
#[must_use]
pub fn sky_settings_editor_floater_spec() -> FloaterSpec {
    editor_floater_spec(EditorKind::Sky)
}

/// The water editor's [`FloaterSpec`].
#[must_use]
pub fn water_settings_editor_floater_spec() -> FloaterSpec {
    editor_floater_spec(EditorKind::Water)
}

/// One editor window's spec.
fn editor_floater_spec(editor: EditorKind) -> FloaterSpec {
    let (title, _key) = editor.title();
    FloaterSpec {
        id: editor.element(),
        title: title.to_owned(),
        position: Vec2::new(160.0, 140.0),
        // Wide enough for three knob columns beside each other, tall enough for
        // the longest column plus the name row and the buttons.
        default_size: Some(Vec2::new(560.0, 520.0)),
        min_size: Some(Vec2::new(320.0, 260.0)),
        dock_host: None,
        caps: FloaterCaps {
            resizable: true,
            minimizable: true,
            closable: true,
            dockable: false,
        },
    }
}

/// Startup: spawn both (hidden) editor windows, their content deferred to the
/// first open.
fn spawn_settings_editors(mut commands: Commands, root: Res<UiRoot>) {
    let mut editors = SettingsEditors::default();
    for editor in [EditorKind::Sky, EditorKind::Water] {
        let handle = spawn_floater(&mut commands, root.0, editor_floater_spec(editor));
        let (_title, key) = editor.title();
        commands
            .entity(handle.title_text)
            .insert(Translated::new(key));
        let builder = match editor {
            EditorKind::Sky => commands.register_system(build_sky_editor_content),
            EditorKind::Water => commands.register_system(build_water_editor_content),
        };
        commands
            .entity(handle.root)
            .insert(DeferredFloaterContent { builder, handle })
            // Where it sits is worth remembering; that it was open is not. The
            // window is bound to one inventory item of one account and the
            // session does not outlive the run, so restoring it open would
            // restore an empty shell — chrome and knobs over no session at all.
            .insert(FloaterOpenExempt);
        editors.get_mut(editor).ui.panel = Some(handle.root);
    }
    commands.insert_resource(editors);
}

// ---------------------------------------------------------------------------
// Content.
// ---------------------------------------------------------------------------

/// The sky editor's content: the name row, three knob tabs, and the buttons.
fn build_sky_editor_content(In(handle): In<FloaterHandle>, mut commands: Commands) {
    build_editor_content(EditorKind::Sky, handle, &mut commands);
}

/// The water editor's content.
fn build_water_editor_content(In(handle): In<FloaterHandle>, mut commands: Commands) {
    build_editor_content(EditorKind::Water, handle, &mut commands);
}

/// Both windows' content build, which differs only in what goes on the tabs.
fn build_editor_content(editor: EditorKind, handle: FloaterHandle, commands: &mut Commands) {
    let element = editor.element();
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

    spawn_name_row(commands, content, editor, &mut tab);

    let pages = editor.tabs();
    let labels: Vec<String> = pages.iter().map(|page| page.label.to_owned()).collect();
    let tabs = spawn_tab_container(
        commands,
        content,
        &TabSpec {
            element,
            placement: TabPlacement::BlockStart,
            labels: &labels,
            active: 0,
            tab_index: tab,
            font_size: FONT_SIZE,
            strip_width: None,
            ellipsis: DEFAULT_ELLIPSIS,
            translate_labels: true,
        },
    );
    // The window is resizable, so the panels take the room the floater gives
    // them and scroll what does not fit, rather than the widget sizing to its
    // largest panel and overflowing the window.
    fill_tab_container(commands, TabPlacement::BlockStart, &tabs);
    tab = tab.saturating_add(1);

    for (page, panel) in pages.iter().zip(&tabs.panels) {
        let columns = spawn_columns(commands, *panel, element, page.slug);
        // The swatches take the first column; the sliders share what is left —
        // all three when a page has no swatches at all, which is the density
        // page and its sixteen terms.
        let has_swatches = !page.colors.is_empty() || !page.textures.is_empty();
        let slider_columns = if has_swatches {
            columns.get(1..).unwrap_or_default()
        } else {
            columns.as_slice()
        };
        if let Some(swatches) = columns.first() {
            for knob in page.colors {
                let swatch = spawn_color_row(commands, *swatches, element, *knob, &mut tab);
                commands.entity(swatch).insert(EditorColorSwatch {
                    editor,
                    knob: *knob,
                });
            }
            for knob in page.textures {
                spawn_texture_knob(commands, *swatches, editor, *knob, &mut tab);
            }
        }
        // A body's trackball opens the column its two angle sliders are in, as
        // the reference's sun-and-moon panel opens with them.
        for (index, knobs) in page.aims.iter().enumerate() {
            let Some(parent) = slider_columns.get(index) else {
                continue;
            };
            let trackball = spawn_trackball_row(commands, *parent, element, *knobs, &mut tab);
            commands.entity(trackball).observe(on_editor_trackball);
        }
        // The sliders fill the columns in order, evenly — the reference's own
        // multi-column sky panels, and the only arrangement that does not need
        // a per-tab layout table to go with the knob table.
        for (index, knob) in page.sky.iter().enumerate() {
            let Some(parent) = slider_column(slider_columns, index, page.sky.len()) else {
                continue;
            };
            spawn_sky_knob(commands, parent, editor, *knob, &mut tab);
        }
        for (index, knob) in page.water.iter().enumerate() {
            let Some(parent) = slider_column(slider_columns, index, page.water.len()) else {
                continue;
            };
            let track = spawn_slider(
                commands,
                parent,
                element,
                knob.slug(),
                knob.range(),
                knob.decimals(),
                &mut tab,
            );
            commands
                .entity(track)
                .insert(EditorWaterSlider(*knob))
                .observe(on_editor_water_slider);
        }
    }

    let status = spawn_button_row(commands, content, editor, &mut tab);

    commands.queue(move |world: &mut World| {
        if let Some(mut editors) = world.get_resource_mut::<SettingsEditors>() {
            let state = editors.get_mut(editor);
            state.ui.status = Some(status);
            // The content is built on the window's first open, a frame after
            // the session that asked for it was installed — so the widgets that
            // have just appeared have never been seeded. Ask for it now.
            if let Some(session) = state.session.as_mut() {
                session.reseed = true;
            }
        }
    });
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
/// over `columns` in order (so a column reads top to bottom and the next one
/// continues it, which is how the reference's panels are laid out).
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

/// One sky slider, tagged for this window.
fn spawn_sky_knob(
    commands: &mut Commands,
    parent: Entity,
    editor: EditorKind,
    knob: SkyKnob,
    tab: &mut i32,
) {
    let track = spawn_slider(
        commands,
        parent,
        editor.element(),
        knob.slug(),
        knob.range(),
        knob.decimals(),
        tab,
    );
    commands
        .entity(track)
        .insert(EditorSkySlider(knob))
        .observe(on_editor_sky_slider);
    tag_aim_slider(commands, track, editor.element(), knob);
}

/// One texture swatch, tagged for this window.
fn spawn_texture_knob(
    commands: &mut Commands,
    parent: Entity,
    editor: EditorKind,
    knob: TextureKnob,
    tab: &mut i32,
) {
    let swatch = spawn_texture_row(commands, parent, editor.element(), knob, tab);
    commands
        .entity(swatch)
        .insert(EditorTextureSwatch { editor, knob });
}

/// The name row: a label and the field the asset's name is edited in.
///
/// The field is not stashed anywhere: it carries an [`EditorNameField`], which
/// is what the read-back and the re-seed find it by — a stored handle would be
/// a second way to reach the same widget and one more thing to keep in step.
fn spawn_name_row(commands: &mut Commands, parent: Entity, editor: EditorKind, tab: &mut i32) {
    let row_entity = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                align_items: AlignItems::Center,
                ..row(Val::Px(6.0))
            },
            Name::new(format!("{}-name:row", editor.element())),
            ChildOf(parent),
        ))
        .id();
    commands.spawn((
        Text::new(String::new()),
        UiFont::Sans.at(FONT_SIZE),
        TextColor(LABEL_COLOR),
        Translated::new("settings-editor-name"),
        ChildOf(row_entity),
    ));
    let field = spawn_text_input(
        commands,
        row_entity,
        &TextInputSpec {
            tab_index: *tab,
            font_size: FONT_SIZE,
            fill: true,
            max_characters: Some(63),
            ..TextInputSpec::new(
                match editor {
                    EditorKind::Sky => "settings-editor-sky-name",
                    EditorKind::Water => "settings-editor-water-name",
                },
                TextInputKind::Line,
            )
        },
    );
    commands.entity(field).insert(EditorNameField(editor));
    *tab = tab.saturating_add(1);
}

/// The Import / Save / Save As / Revert row and the status line under it.
/// Returns the status text entity.
fn spawn_button_row(
    commands: &mut Commands,
    parent: Entity,
    editor: EditorKind,
    tab: &mut i32,
) -> Entity {
    let row_entity = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                align_items: AlignItems::Center,
                ..row(Val::Px(6.0))
            },
            Name::new(format!("{}-actions:row", editor.element())),
            ChildOf(parent),
        ))
        .id();
    for (action, slug, key) in [
        // Import leads, as it does in the reference's own button row: it is the
        // one action that does not need something already open.
        (EditorAction::Import, "import", "settings-editor-import"),
        (EditorAction::Save, "save", "settings-editor-save"),
        (EditorAction::SaveAs, "save-as", "settings-editor-save-as"),
        (EditorAction::Revert, "revert", "settings-editor-revert"),
    ] {
        let button = spawn_action_button(
            commands,
            row_entity,
            editor.element(),
            slug,
            key.to_owned(),
            tab,
        );
        commands
            .entity(button)
            .insert(EditorButton { editor, action })
            .observe(on_editor_button);
    }
    commands
        .spawn((
            Text::new(String::new()),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(DIM_LABEL_COLOR),
            Name::new(format!("{}-status", editor.element())),
            ChildOf(parent),
        ))
        .id()
}

// ---------------------------------------------------------------------------
// Opening.
// ---------------------------------------------------------------------------

/// Handle an [`OpenSettingsEditor`]: show the right window and start fetching
/// the item's asset.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources: the open stream, the \
              editors and the asset store an open drives, the confirmation stash and channel a \
              modified session needs, and the panel / raise / status outputs"
)]
fn open_settings_editor(
    mut opens: MessageReader<OpenSettingsEditor>,
    mut editors: ResMut<SettingsEditors>,
    mut assets: Option<ResMut<EnvironmentAssetManager>>,
    mut confirm: ResMut<PendingEditorReplace>,
    mut notify: MessageWriter<ShowNotification>,
    mut panels: Query<&mut UiPanelShown>,
    mut raises: MessageWriter<FloaterCommand>,
    mut texts: Query<&mut Text>,
) {
    for open in opens.read() {
        let Some(editor) = EditorKind::of_settings(open.kind) else {
            // A day cycle is a settings item too, and neither of these windows
            // can show one — `crate::day_cycle_editor` reads the same stream and
            // takes it. Not a warning: it is somebody else's message, not a
            // dropped one.
            continue;
        };
        // These windows are singletons — one per kind — because the frame being
        // edited is *previewed*, and two previews of one track cannot both be
        // what the user is standing under. So opening a second item replaces the
        // first, and the reference asks before throwing unsaved work away
        // (`checkAndConfirmSettingsLoss`, which guards its own load-from-
        // inventory the same way).
        if editors
            .get_mut(editor)
            .session
            .as_ref()
            .is_some_and(|session| session.modified)
        {
            let held = editors.get_mut(editor);
            let name = held
                .session
                .as_ref()
                .map_or_else(String::new, |session| session.name.clone());
            confirm.0 = Some(HeldReplacement::Open(open.clone()));
            notify.write(
                ShowNotification::new("SettingsConfirmLoss")
                    .arg("TYPE", settings_kind_word(editor.settings_kind()))
                    .arg("NAME", name),
            );
            continue;
        }
        let asset = AssetKey::from(open.asset_id);
        let state = editors.get_mut(editor);
        state.pending = Some(PendingOpen {
            request: open.clone(),
            asset,
        });
        if let Some(assets) = assets.as_mut() {
            assets.request(asset);
        }
        set_status(&mut texts, state.ui.status, "Loading…");
        if let Some(panel) = state.ui.panel {
            if let Ok(mut shown) = panels.get_mut(panel) {
                shown.0 = true;
            }
            // Showing a window that is already open leaves it wherever it was in
            // the z-order — which, opened from the My Environments list, is
            // *behind* the window that asked for it. The press that picked the
            // row raised that one, so the editor has to be raised after it.
            raises.write(FloaterCommand {
                floater: panel,
                op: FloaterOp::BringToFront,
            });
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
struct EditorSwatches<'w, 's> {
    /// The colour swatches.
    colors: Query<'w, 's, (&'static EditorColorSwatch, &'static mut ColorSwatchValue)>,
    /// The texture swatches.
    textures: Query<
        'w,
        's,
        (
            &'static EditorTextureSwatch,
            &'static mut TextureSwatchValue,
        ),
    >,
}

/// Seed a window whose asset has arrived — or give up on one that will not.
fn poll_pending_open(
    mut editors: ResMut<SettingsEditors>,
    assets: Option<Res<EnvironmentAssetManager>>,
    mut texts: Query<&mut Text>,
) {
    let Some(assets) = assets else {
        return;
    };
    for editor in [EditorKind::Sky, EditorKind::Water] {
        let state = editors.get_mut(editor);
        let Some(pending) = state.pending.clone() else {
            continue;
        };
        if let Some(decoded) = assets.get(pending.asset) {
            let asset = (**decoded).clone();
            // A settings item's *flags* said which kind it is, but the asset's
            // own body is the authority — an item flagged sky whose asset is a
            // water frame would otherwise be edited by the wrong window with
            // every control a no-op.
            if EditorKind::of_settings(asset.kind()) != Some(editor) {
                state.pending = None;
                set_status(
                    &mut texts,
                    state.ui.status,
                    "That item is not the kind this editor edits.",
                );
                continue;
            }
            state.pending = None;
            state.session = Some(EditSession {
                item: Some(EditedItem {
                    item_id: pending.request.item_id,
                    folder_id: pending.request.folder_id,
                    editable: pending.request.editable,
                }),
                name: pending.request.name.clone(),
                original: asset.clone(),
                edited: asset,
                dirty: true,
                modified: false,
                reseed: true,
                saving: false,
            });
            set_status(&mut texts, state.ui.status, "");
        } else if assets.is_unavailable(pending.asset) {
            state.pending = None;
            set_status(
                &mut texts,
                state.ui.status,
                "That settings asset could not be loaded.",
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Importing a legacy WindLight preset.
// ---------------------------------------------------------------------------

/// The file-dialog purpose an editor's Import asks under — also the key its
/// last-used directory is remembered by, so the sky editor reopens where the
/// skies are and the water editor where the water is (`windlight/skies` and
/// `windlight/water` are siblings, and a shared memory would keep dragging each
/// one into the other's folder).
const fn import_purpose(editor: EditorKind) -> &'static str {
    match editor {
        EditorKind::Sky => "settings-editor-import-sky",
        EditorKind::Water => "settings-editor-import-water",
    }
}

/// The file-open dialog an editor's Import puts up: legacy WindLight presets are
/// LLSD-XML files, so `.xml` and "everything" are the two filters, as in the
/// reference (`FFLOAD_XML`).
fn import_dialog_request(editor: EditorKind, translator: &Translator) -> OpenFileDialog {
    OpenFileDialog {
        purpose: import_purpose(editor).into(),
        title: translator.get(match editor {
            EditorKind::Sky => "settings-editor-import-sky-title",
            EditorKind::Water => "settings-editor-import-water-title",
        }),
        filters: vec![
            FileDialogFilter {
                label: translator.get("settings-editor-import-filter-preset"),
                extensions: vec!["xml".to_owned()],
            },
            FileDialogFilter {
                label: translator.get("settings-editor-import-filter-all"),
                extensions: vec!["*".to_owned()],
            },
        ],
        start_dir: None,
    }
}

/// Start an Import: ask the desktop for a file, or ask the user first if what is
/// on screen would be thrown away.
///
/// The confirmation comes **before** the dialog, not after the file is chosen —
/// the reference's `onButtonImport` wraps the whole of `doImportFromDisk` in
/// `checkAndConfirmSettingsLoss`, and the other order would have the user pick a
/// file only to be asked whether they meant it.
fn begin_import(
    editor: EditorKind,
    editors: &mut SettingsEditors,
    confirm: &mut PendingEditorReplace,
    notify: &mut MessageWriter<ShowNotification>,
    dialogs: &mut MessageWriter<OpenFileDialog>,
    translator: &Translator,
) {
    let state = editors.get_mut(editor);
    if let Some(session) = state.session.as_ref()
        && session.modified
    {
        let name = session.name.clone();
        confirm.0 = Some(HeldReplacement::Import(editor));
        notify.write(
            ShowNotification::new("SettingsConfirmLoss")
                .arg("TYPE", settings_kind_word(editor.settings_kind()))
                .arg("NAME", name),
        );
        return;
    }
    dialogs.write(import_dialog_request(editor, translator));
}

/// Take the file an Import dialog came back with: read it, convert it, and make
/// it the frame this window is editing.
///
/// The read is on the frame thread. A WindLight preset is a few kilobytes of XML
/// — the reference reads one with a plain `llifstream` on its own main thread —
/// and the expensive, unbounded part of picking a file (the user deciding) has
/// already happened out of process by the time this runs.
fn apply_imported_preset(
    mut closed: MessageReader<FileDialogClosed>,
    mut editors: ResMut<SettingsEditors>,
    mut notify: MessageWriter<ShowNotification>,
    mut texts: Query<&mut Text>,
) {
    for reply in closed.read() {
        let Some(editor) = [EditorKind::Sky, EditorKind::Water]
            .into_iter()
            .find(|&editor| *reply.purpose == *import_purpose(editor))
        else {
            // Somebody else's dialog.
            continue;
        };
        let FileDialogOutcome::Picked(ref path) = reply.outcome else {
            // Cancelled, or refused because another dialog was up: either way
            // the window keeps whatever it was holding, and says nothing.
            continue;
        };
        let state = editors.get_mut(editor);
        let status = state.ui.status;
        let file = path.display().to_string();
        // The preset's *name* is its filename, percent-unescaped — the old
        // viewer escaped a name to make it a filename, and nothing inside the
        // file records what it was called.
        let name = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .map_or_else(String::new, legacy_preset_name);
        let imported = fs_err::read(path)
            .map_err(|error| error.to_string())
            .and_then(|bytes| {
                legacy_preset_from_bytes(editor.settings_kind(), &name, &bytes)
                    .map_err(|error| error.to_string())
            });
        match imported {
            Ok(asset) => {
                state.session = Some(EditSession {
                    // Nothing behind it: a Save As is what files it.
                    item: None,
                    name: name.clone(),
                    // A Revert goes back to the preset as imported, which is the
                    // only baseline there is — there is no stored asset to
                    // return to.
                    original: asset.clone(),
                    edited: asset,
                    dirty: true,
                    // Dirty from the start, as the reference's `setDirtyFlag()`
                    // after an import: the frame on screen exists nowhere else,
                    // so replacing it really would lose something.
                    modified: true,
                    reseed: true,
                    saving: false,
                });
                set_status(&mut texts, status, &format!("Imported {name}."));
            }
            Err(reason) => {
                set_status(&mut texts, status, &format!("Import failed: {reason}"));
                notify.write(
                    ShowNotification::new("WLImportFail")
                        .arg("NAME", name)
                        .arg("FILE", file)
                        .arg("REASONS", reason),
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Editing.
// ---------------------------------------------------------------------------

/// A sky slider moved.
fn on_editor_sky_slider(
    change: On<ValueChange<f32>>,
    sliders: Query<(&EditorSkySlider, &SliderRange)>,
    mut editors: ResMut<SettingsEditors>,
    mut commands: Commands,
) {
    let Ok((row_info, range)) = sliders.get(change.source) else {
        return;
    };
    let clamped = range.clamp(change.value);
    commands.entity(change.source).insert(SliderValue(clamped));
    let state = editors.get_mut(EditorKind::Sky);
    if let Some(session) = state.session.as_mut()
        && let Some(sky) = session.sky_mut()
    {
        row_info.0.write(sky, clamped);
        session.dirty = true;
        session.modified = true;
    }
}

/// A trackball was aimed: write the body's whole direction into the sky the
/// window is editing. Only the sky window has one, and the two sliders under it
/// are put back in step by the shared [`crate::rows`] systems.
fn on_editor_trackball(
    change: On<ValueChange<Vec2>>,
    trackballs: Query<&AimTrackball>,
    mut editors: ResMut<SettingsEditors>,
) {
    let Ok(trackball) = trackballs.get(change.source) else {
        return;
    };
    let aim = TrackballAim {
        azimuth: change.value.x,
        elevation: change.value.y,
    };
    let state = editors.get_mut(EditorKind::Sky);
    if let Some(session) = state.session.as_mut()
        && let Some(sky) = session.sky_mut()
    {
        trackball.knobs.write(sky, aim);
        session.dirty = true;
        session.modified = true;
    }
}

/// A water slider moved.
fn on_editor_water_slider(
    change: On<ValueChange<f32>>,
    sliders: Query<(&EditorWaterSlider, &SliderRange)>,
    mut editors: ResMut<SettingsEditors>,
    mut commands: Commands,
) {
    let Ok((row_info, range)) = sliders.get(change.source) else {
        return;
    };
    let clamped = range.clamp(change.value);
    commands.entity(change.source).insert(SliderValue(clamped));
    let state = editors.get_mut(EditorKind::Water);
    if let Some(session) = state.session.as_mut()
        && let Some(water) = session.water_mut()
    {
        row_info.0.write(water, clamped);
        session.dirty = true;
        session.modified = true;
    }
}

/// A colour came back from the picker (live or committed, as the Personal
/// Lighting window takes them).
fn apply_editor_color_picks(
    mut picks: MessageReader<ColorPicked>,
    mut swatches: Query<(&EditorColorSwatch, &mut ColorSwatchValue)>,
    mut editors: ResMut<SettingsEditors>,
) {
    for pick in picks.read() {
        let Ok((swatch, mut value)) = swatches.get_mut(pick.requester) else {
            continue;
        };
        value.0 = pick.color;
        let state = editors.get_mut(swatch.editor);
        let Some(session) = state.session.as_mut() else {
            continue;
        };
        write_color(session, swatch.knob, pick.color);
        session.dirty = true;
        session.modified = true;
    }
}

/// A texture came back from the picker.
fn apply_editor_texture_picks(
    mut picks: MessageReader<TexturePicked>,
    mut swatches: Query<(&EditorTextureSwatch, &mut TextureSwatchValue)>,
    mut editors: ResMut<SettingsEditors>,
) {
    for pick in picks.read() {
        let Ok((swatch, mut value)) = swatches.get_mut(pick.requester) else {
            continue;
        };
        value.0 = pick.texture;
        let state = editors.get_mut(swatch.editor);
        let Some(session) = state.session.as_mut() else {
            continue;
        };
        write_texture(session, swatch.knob, pick.texture);
        session.dirty = true;
        session.modified = true;
    }
}

/// Keep each session's name in step with its field.
///
/// Read out of the field rather than written into the session on every
/// keystroke: the field owns its own text (it is a `bevy_text` editor), and a
/// session that mirrored it per key would be a second copy to keep in step.
///
/// **Not while a reseed is outstanding.** The window's content is built the
/// first time it opens, which is a *later* frame than the open that asked for
/// it and can be the very frame the fetched asset installs the session. A
/// freshly spawned `EditableText` counts as `Changed`, so without this guard the
/// first pass reads the empty widget back over the name the asset arrived with,
/// and the reseed a moment later then writes that emptiness into the field —
/// leaving every opened item nameless. Until the reseed has pushed the session's
/// name *into* the field, what the field holds is not the user's typing.
fn read_editor_names(
    fields: Query<(&EditorNameField, &EditableText), Changed<EditableText>>,
    mut editors: ResMut<SettingsEditors>,
) {
    for (field, editable) in &fields {
        let state = editors.get_mut(field.0);
        if let Some(session) = state.session.as_mut() {
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
}

/// Seed every widget of a window whose session was just opened or reverted.
fn reseed_editor_widgets(
    mut commands: Commands,
    mut editors: ResMut<SettingsEditors>,
    sky_sliders: Query<(Entity, &EditorSkySlider, &SliderRange, &SliderValue)>,
    water_sliders: Query<(Entity, &EditorWaterSlider, &SliderRange, &SliderValue)>,
    mut swatches: EditorSwatches,
    mut fields: Query<(&EditorNameField, &mut EditableText)>,
    mut trackballs: Query<(&AimTrackball, &mut TrackballAim)>,
) {
    for editor in [EditorKind::Sky, EditorKind::Water] {
        let state = editors.get_mut(editor);
        let Some(session) = state.session.as_mut() else {
            continue;
        };
        if !session.reseed {
            continue;
        }
        // The content is deferred to the window's first open, so a session can
        // be installed a frame before there is a single widget to seed. Hold
        // the request rather than spend it on an empty query.
        let built = match editor {
            EditorKind::Sky => !sky_sliders.is_empty(),
            EditorKind::Water => !water_sliders.is_empty(),
        };
        if !built {
            continue;
        }
        session.reseed = false;
        let (sky, water) = frames_of(&session.edited);
        // `SliderValue` is immutable, so a new value is *inserted* — and only
        // when it differs, since an insert marks the component changed whether
        // or not it carries a new number.
        if let Some(sky) = sky.as_ref() {
            for (entity, row_info, range, value) in &sky_sliders {
                let wanted = range.clamp(row_info.0.read(sky));
                if value.0.to_bits() != wanted.to_bits() {
                    commands.entity(entity).insert(SliderValue(wanted));
                }
            }
            // The trackballs are seeded here rather than left to the slider
            // sync, which only fires on a slider that *changed*: a sky whose sun
            // happens to sit at its two sliders' current values would otherwise
            // leave the marker where the last item put it.
            for (trackball, mut aim) in &mut trackballs {
                if trackball.scope != editor.element() {
                    continue;
                }
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
            if swatch.editor == editor {
                value.0 = swatch.knob.read(&sky_frame, &water_frame);
            }
        }
        for (swatch, mut value) in &mut swatches.textures {
            if swatch.editor == editor {
                value.0 = swatch.knob.read(&sky_frame, &water_frame);
            }
        }
        for (field, mut editable) in &mut fields {
            if field.0 == editor && editable.value().to_string() != session.name {
                editable.editor.set_text(&session.name);
            }
        }
    }
}

/// Push each open session's frame into the environment's **edit** layer, so the
/// sky or water the user is authoring is the one they are standing under.
fn push_editor_preview(
    mut editors: ResMut<SettingsEditors>,
    environment: Option<ResMut<EnvironmentState>>,
) {
    let Some(mut environment) = environment else {
        return;
    };
    for editor in [EditorKind::Sky, EditorKind::Water] {
        let state = editors.get_mut(editor);
        let Some(session) = state.session.as_mut() else {
            continue;
        };
        if !session.dirty {
            continue;
        }
        session.dirty = false;
        environment.set_edit(named(&session.edited, &session.name));
    }
}

/// Take a window's preview out of the edit layer when it closes, and forget the
/// session — the reference's `onClose`, which clears `ENV_EDIT`.
fn drop_preview_on_close(
    panels: Query<(Entity, &UiPanelShown), Changed<UiPanelShown>>,
    mut editors: ResMut<SettingsEditors>,
    environment: Option<ResMut<EnvironmentState>>,
) {
    let Some(mut environment) = environment else {
        return;
    };
    for (entity, shown) in &panels {
        if shown.0 {
            continue;
        }
        for editor in [EditorKind::Sky, EditorKind::Water] {
            let state = editors.get_mut(editor);
            if state.ui.panel != Some(entity) {
                continue;
            }
            state.session = None;
            state.pending = None;
            environment.clear_edit(editor.settings_kind());
        }
    }
}

// ---------------------------------------------------------------------------
// Saving.
// ---------------------------------------------------------------------------

/// A chrome button press: Import (a preset off disk), Save (over the item),
/// Save As (a new item), Revert.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy observer's parameters are its injected resources: the button pool and its \
              disabled filter, the session state, the two creation queues a Save As writes, the \
              inventory mirror an unfiled frame needs a folder out of, the confirmation stash and \
              file-dialog channel an Import uses, and the command and status channels"
)]
fn on_editor_button(
    press: On<Pointer<Press>>,
    buttons: Query<&EditorButton>,
    disabled: Query<(), With<bevy::ui::InteractionDisabled>>,
    mut editors: ResMut<SettingsEditors>,
    mut settings_creations: ResMut<PendingSettingsCreations>,
    mut saving_as: ResMut<PendingEditorSaveAs>,
    mut confirm: ResMut<PendingEditorReplace>,
    mut notify: MessageWriter<ShowNotification>,
    mut dialogs: MessageWriter<OpenFileDialog>,
    inventory: Option<Res<InventoryModel>>,
    translator: Translator,
    mut commands: MessageWriter<SlCommand>,
    mut texts: Query<&mut Text>,
) {
    if press.button != PointerButton::Primary || disabled.contains(press.entity) {
        return;
    }
    let Ok(button) = buttons.get(press.entity).copied() else {
        return;
    };
    // Import is the one action that does not need something open — it is how a
    // window with nothing in it gets a frame — so it is handled before the
    // session guard rather than inside it.
    if button.action == EditorAction::Import {
        begin_import(
            button.editor,
            &mut editors,
            &mut confirm,
            &mut notify,
            &mut dialogs,
            &translator,
        );
        return;
    }
    let state = editors.get_mut(button.editor);
    let status = state.ui.status;
    let Some(session) = state.session.as_mut() else {
        set_status(&mut texts, status, "Nothing is open in this editor.");
        return;
    };
    // Recorded here and pushed after the session borrow ends.
    let mut queued: Option<PendingSave> = None;
    match button.action {
        // Handled above, before the session guard.
        EditorAction::Import => {}
        EditorAction::Save => {
            // An imported preset has no item to write onto; the reference greys
            // its Save for the same reason (`mInventoryId.isNull()`).
            let Some(item) = session.item else {
                set_status(
                    &mut texts,
                    status,
                    "This preset is not in your inventory yet — use Save As.",
                );
                return;
            };
            if !item.editable {
                set_status(&mut texts, status, "That item may not be modified.");
                return;
            }
            let data = environment_asset_to_bytes(&named(&session.edited, &session.name));
            commands.write(SlCommand(Command::UpdateInventoryAsset {
                location: AssetUpdateLocation::AgentInventory {
                    item_id: item.item_id,
                },
                asset_type: UpdatableAssetType::Settings,
                data,
            }));
            session.saving = true;
            set_status(&mut texts, status, "Saving…");
            queued = Some(PendingSave {
                editor: button.editor,
                item: item.item_id,
            });
        }
        EditorAction::SaveAs => {
            // An imported preset has no folder of its own, so the copy goes
            // where a brand-new settings item goes: the Settings system folder,
            // falling back to the agent's root — the reference's
            // `findCategoryUUIDForType(FT_SETTINGS)`.
            let folder_id = match session.item {
                Some(item) => Some(item.folder_id),
                None => inventory.as_ref().and_then(|model| {
                    model
                        .folder_by_type(FolderType::Settings)
                        .or_else(|| model.agent_root())
                }),
            };
            let Some(folder_id) = folder_id else {
                set_status(
                    &mut texts,
                    status,
                    "There is no Settings folder to file a copy in yet.",
                );
                return;
            };
            let name = session.name.clone();
            let data = environment_asset_to_bytes(&named(&session.edited, &name));
            let kind = button.editor.settings_kind();
            // **Two steps, not an upload.** `NewFileAgentInventory` has no
            // settings arm on either grid — OpenSim files the item as a Texture
            // and Second Life creates nothing — so the simulator mints the item
            // (which is also what stamps its kind), and the body is written onto
            // it when the reply names it. That is the reference's
            // `createInventoryItem` → `onInventoryItemCreated` →
            // `updateInventoryItem`, and the second half is the same
            // `UpdateSettingsAgentInventory` an in-place Save already uses.
            commands.write(SlCommand(new_settings_item(kind, &name, folder_id)));
            settings_creations.enqueue(kind, Some(data));
            saving_as.0 = saving_as.0.saturating_add(1);
            set_status(&mut texts, status, "Saving a copy…");
        }
        EditorAction::Revert => {
            session.edited = session.original.clone();
            session.name = frame_name(&session.original);
            session.dirty = true;
            session.modified = false;
            session.reseed = true;
            set_status(&mut texts, status, "Reverted.");
        }
    }
    if let Some(queued) = queued {
        editors.saves.push_back(queued);
    }
}

/// Report an **in-place** save's outcome.
///
/// A Save As is finished elsewhere: the flags stamp its fresh item needs rides
/// the viewer's shared creation queue, which the inventory consumes.
fn report_editor_save(
    mut events: MessageReader<SlEvent>,
    mut editors: ResMut<SettingsEditors>,
    mut texts: Query<&mut Text>,
) {
    for event in events.read() {
        match &event.0 {
            SlSessionEvent::AssetUploaded {
                new_inventory_item: Some(replied),
                ..
            } => {
                // An in-place save names the item it wrote, so somebody else's
                // upload landing in between is not taken for this one.
                let item = InventoryKey::from(*replied);
                let Some(pending) = editors.saves.front().copied() else {
                    continue;
                };
                if pending.item != item {
                    continue;
                }
                let _taken = editors.saves.pop_front();
                finish_save(&mut editors, pending.editor, "Saved.", true, &mut texts);
            }
            SlSessionEvent::AssetUploadFailed { reason } => {
                let Some(pending) = editors.saves.pop_front() else {
                    continue;
                };
                let message = format!("Save failed: {reason}");
                finish_save(&mut editors, pending.editor, &message, false, &mut texts);
            }
            _other => {}
        }
    }
}

/// The open a window is holding until the user says the unsaved changes it
/// would discard can go.
///
/// One slot, and the reply is only acted on while it is full — the notification
/// response carries the template name rather than the raise's own id, so this is
/// what tells our confirmation from anybody else's.
#[derive(Resource, Debug, Default)]
struct PendingEditorReplace(Option<HeldReplacement>);

/// What a window is waiting for permission to replace its unsaved frame with.
///
/// Both arms raise the same `SettingsConfirmLoss` — they are the same question
/// (may the work on screen go?) asked by the two things that would throw it
/// away, which is exactly how the reference groups them: `onButtonImport` and
/// its load-from-inventory both go through `checkAndConfirmSettingsLoss`.
#[derive(Debug, Clone)]
enum HeldReplacement {
    /// An inventory item the user asked to open, replayed once confirmed.
    Open(OpenSettingsEditor),
    /// An Import the user pressed: the dialog has not been opened yet, because
    /// asking for a file and *then* asking whether the answer may be used gets
    /// the order wrong.
    Import(EditorKind),
}

/// The reference's `getSettingsType()` word, for the confirmation's `[TYPE]`.
const fn settings_kind_word(kind: SettingsKind) -> &'static str {
    match kind {
        SettingsKind::Sky => "sky",
        SettingsKind::Water => "water",
        SettingsKind::DayCycle => "day cycle",
    }
}

/// Carry out (or drop) the replacement the user was asked to confirm.
///
/// Replaying the original [`OpenSettingsEditor`] rather than opening by hand
/// keeps one route into these windows: the confirmed open takes the same path an
/// unconfirmed one does, and the session it replaces is gone by the time it runs.
/// A confirmed Import goes the same way — the session is dropped and the file
/// dialog is asked for, which is what an unconfirmed press does.
fn confirm_editor_replace(
    mut responses: MessageReader<NotificationResponse>,
    mut confirm: ResMut<PendingEditorReplace>,
    mut editors: ResMut<SettingsEditors>,
    mut opens: MessageWriter<OpenSettingsEditor>,
    mut dialogs: MessageWriter<OpenFileDialog>,
    translator: Translator,
) {
    for response in responses.read() {
        if response.template != "SettingsConfirmLoss" {
            continue;
        }
        let Some(held) = confirm.0.take() else {
            continue;
        };
        if response.button != Some("OK") {
            continue;
        }
        match held {
            HeldReplacement::Open(open) => {
                // Drop the session *before* replaying, or the open would find it
                // still modified and ask again.
                if let Some(editor) = EditorKind::of_settings(open.kind) {
                    editors.get_mut(editor).session = None;
                }
                opens.write(open);
            }
            HeldReplacement::Import(editor) => {
                editors.get_mut(editor).session = None;
                dialogs.write(import_dialog_request(editor, &translator));
            }
        }
    }
}

/// How many **Save As** copies this crate's editors are waiting on.
///
/// The shared settings-creation queue publishes every creation in order, so a
/// count is enough to know that the next [`SettingsItemCreated`] is one of ours
/// rather than the library window's or the inventory's.
#[derive(Resource, Debug, Default)]
struct PendingEditorSaveAs(usize);

/// Report a **Save As** landing: the item exists, its body has been written, and
/// the window now edits the copy.
///
/// Following the copy is the reference's `onInventoryCreated`, which clears the
/// dirty flag and then `loadInventoryItem`s the item it just made. It is also
/// the only coherent answer: the frame on screen is now stored in the *copy*, so
/// a window still pointed at the original would write everything you just did
/// into the wrong item on the next plain Save, and would keep asking about
/// "unsaved" changes that are, in fact, saved.
///
/// Re-pointing rather than re-fetching — the reference re-loads the item, but
/// the bytes it would fetch are the ones just uploaded, and they are already in
/// hand.
fn report_editor_save_as(
    mut created: MessageReader<SettingsItemCreated>,
    mut saving_as: ResMut<PendingEditorSaveAs>,
    mut editors: ResMut<SettingsEditors>,
    mut texts: Query<&mut Text>,
) {
    for item in created.read() {
        if !item.authored || saving_as.0 == 0 {
            continue;
        }
        saving_as.0 = saving_as.0.saturating_sub(1);
        let Some(editor) = EditorKind::of_settings(item.kind) else {
            continue;
        };
        let state = editors.get_mut(editor);
        let status = state.ui.status;
        if let Some(session) = state.session.as_mut() {
            session.item = Some(EditedItem {
                item_id: item.item,
                folder_id: item.folder,
                // Freshly minted by this agent, so modifiable by definition.
                editable: true,
            });
            session.modified = false;
            session.original = session.edited.clone();
        }
        set_status(&mut texts, status, "Saved a copy.");
    }
}

/// Clear a window's in-flight-save flag and tell it how the save went.
fn finish_save(
    editors: &mut SettingsEditors,
    editor: EditorKind,
    message: &str,
    saved: bool,
    texts: &mut Query<&mut Text>,
) {
    let state = editors.get_mut(editor);
    let status = state.ui.status;
    if let Some(session) = state.session.as_mut() {
        session.saving = false;
        // A save that landed *is* the new baseline — nothing unsaved is left,
        // and a Revert should now go back to what was stored rather than to
        // what was on screen before the save. A failed one changes neither: the
        // work is still unsaved and the next open must still ask about it.
        if saved {
            session.modified = false;
            session.original = session.edited.clone();
        }
    }
    set_status(texts, status, message);
}

// ---------------------------------------------------------------------------
// Helpers.
// ---------------------------------------------------------------------------

/// The frames an asset holds, as owned copies: a sky asset has a sky and no
/// water, and the other way round.
fn frames_of(asset: &EnvironmentAsset) -> (Option<SkySettings>, Option<WaterSettings>) {
    match asset {
        EnvironmentAsset::Sky(sky) => (Some((**sky).clone()), None),
        EnvironmentAsset::Water(water) => (None, Some(water.clone())),
        // Neither window edits a day cycle; `poll_pending_open` refuses one
        // before a session is ever built from it.
        EnvironmentAsset::DayCycle(_) => (None, None),
    }
}

/// The asset's own frame name — what a Revert puts back in the name field.
fn frame_name(asset: &EnvironmentAsset) -> String {
    match asset {
        EnvironmentAsset::Sky(sky) => sky.name.clone(),
        EnvironmentAsset::Water(water) => water.name.clone(),
        EnvironmentAsset::DayCycle(day) => day.name.clone(),
    }
}

/// `asset` with its frame named `name` — the frame name and the inventory
/// item's name are the same string in the reference, which is why editing the
/// field renames the frame rather than only the item.
fn named(asset: &EnvironmentAsset, name: &str) -> EnvironmentAsset {
    match asset {
        EnvironmentAsset::Sky(sky) => {
            let mut sky = sky.clone();
            name.clone_into(&mut sky.name);
            EnvironmentAsset::Sky(sky)
        }
        EnvironmentAsset::Water(water) => {
            let mut water = water.clone();
            name.clone_into(&mut water.name);
            EnvironmentAsset::Water(water)
        }
        EnvironmentAsset::DayCycle(day) => {
            let mut day = day.clone();
            name.clone_into(&mut day.name);
            EnvironmentAsset::DayCycle(day)
        }
    }
}

/// Write a picked colour into whichever frame the session holds.
fn write_color(session: &mut EditSession, knob: ColorKnob, color: Color) {
    let mut sky = SkySettings::legacy_windlight_default("scratch");
    let mut water = WaterSettings::legacy_default("scratch");
    match &mut session.edited {
        EnvironmentAsset::Sky(frame) => {
            knob.write(frame, &mut water, color);
        }
        EnvironmentAsset::Water(frame) => {
            knob.write(&mut sky, frame, color);
        }
        EnvironmentAsset::DayCycle(_) => {}
    }
}

/// Write a picked texture into whichever frame the session holds.
fn write_texture(session: &mut EditSession, knob: TextureKnob, texture: TextureKey) {
    let mut sky = SkySettings::legacy_windlight_default("scratch");
    let mut water = WaterSettings::legacy_default("scratch");
    match &mut session.edited {
        EnvironmentAsset::Sky(frame) => {
            knob.write(frame, &mut water, texture);
        }
        EnvironmentAsset::Water(frame) => {
            knob.write(&mut sky, frame, texture);
        }
        EnvironmentAsset::DayCycle(_) => {}
    }
}

/// Write a one-line message into a window's status readout.
fn set_status(texts: &mut Query<&mut Text>, status: Option<Entity>, message: &str) {
    if let Some(status) = status
        && let Ok(mut text) = texts.get_mut(status)
        && text.0 != message
    {
        message.clone_into(&mut text.0);
    }
}

#[cfg(test)]
mod tests {
    use bevy::ecs::system::RunSystemOnce as _;
    use bevy::prelude::*;
    use sl_viewer_platform::file_dialog::{FileDialogClosed, FileDialogOutcome};

    use super::{
        EditSession, EditorKind, HeldReplacement, OpenFileDialog, PendingEditorReplace,
        SettingsEditors, Translator, apply_imported_preset, begin_import, frame_name,
        import_purpose, named, settings_kind_word,
    };
    use crate::knobs::SkyKnob;
    use pretty_assertions::{assert_eq, assert_ne};
    use sl_client_bevy::{
        DensityLayer, EnvironmentAsset, SettingsKind, SkySettings, WaterSettings,
        environment_asset_from_bytes, environment_asset_to_bytes,
    };

    /// A boxed error, so a test can `?` rather than reach for the `panic!` the
    /// workspace's lints (rightly) forbid.
    type TestError = Box<dyn core::error::Error>;

    /// **The confirmation this editor routes on is the one the catalogue
    /// holds.** The template name and the button name are plain strings on both
    /// sides, so a rename in the catalogue would leave the open path raising a
    /// notification nothing answers — and the editor would then silently
    /// discard unsaved work again, which is the bug the confirmation exists to
    /// fix.
    #[test]
    fn the_loss_confirmation_is_catalogued_with_the_button_we_route_on() -> Result<(), TestError> {
        let template = sl_viewer_notifications::template("SettingsConfirmLoss")
            .ok_or("the reference's SettingsConfirmLoss is in the catalogue")?;
        assert!(
            template.form.iter().any(|button| button.name == "OK"),
            "the confirm arm routes on the stable reference functor name"
        );
        assert!(
            template.form.iter().any(|button| button.name == "Cancel"),
            "and so does the refusal"
        );
        Ok(())
    }

    /// Each settings kind gets its own `[TYPE]` word, so the confirmation says
    /// what is about to be lost rather than "settings".
    #[test]
    fn every_kind_names_itself_in_the_confirmation() {
        let words = [
            settings_kind_word(SettingsKind::Sky),
            settings_kind_word(SettingsKind::Water),
            settings_kind_word(SettingsKind::DayCycle),
        ];
        let mut unique = words;
        unique.sort_unstable();
        let mut deduped = unique.to_vec();
        deduped.dedup();
        assert_eq!(deduped.len(), words.len(), "{words:?}");
    }

    /// A settings item opens in the editor its kind names, and a day cycle in
    /// neither of them.
    #[test]
    fn each_settings_kind_routes_to_its_editor() {
        assert_eq!(
            EditorKind::of_settings(SettingsKind::Sky),
            Some(EditorKind::Sky)
        );
        assert_eq!(
            EditorKind::of_settings(SettingsKind::Water),
            Some(EditorKind::Water)
        );
        assert_eq!(EditorKind::of_settings(SettingsKind::DayCycle), None);
        assert_eq!(EditorKind::Sky.settings_kind(), SettingsKind::Sky);
        assert_eq!(EditorKind::Water.settings_kind(), SettingsKind::Water);
    }

    /// Renaming in the name field renames the *frame*, and nothing else — the
    /// reference keeps the item's name and the frame's the same string.
    #[test]
    fn renaming_a_frame_touches_only_its_name() -> Result<(), TestError> {
        let sky = SkySettings::legacy_windlight_default("Before");
        let renamed = named(&EnvironmentAsset::Sky(Box::new(sky.clone())), "After");
        assert_eq!(frame_name(&renamed), "After");
        let EnvironmentAsset::Sky(renamed) = renamed else {
            return Err("a renamed sky is still a sky".into());
        };
        assert_eq!(
            SkySettings {
                name: "Before".to_owned(),
                ..*renamed
            },
            sky
        );

        let water = WaterSettings::legacy_default("Before");
        let renamed = named(&EnvironmentAsset::Water(water.clone()), "After");
        assert_eq!(frame_name(&renamed), "After");
        Ok(())
    }

    /// **A save keeps what this window cannot show.** The scattering profiles
    /// are carried through `SkySettings` and edited by nothing here; the whole
    /// point is that they survive the encode a Save writes, because the
    /// reference substitutes its own defaults for an absent one and the item's
    /// atmosphere would silently change for everybody who opened it next.
    #[test]
    fn a_save_keeps_the_fields_the_editor_cannot_show() -> Result<(), TestError> {
        let mut sky = SkySettings::legacy_windlight_default("Author's sky");
        sky.rayleigh_config = vec![DensityLayer {
            width: 0.0,
            exp_term: 1.0,
            exp_scale: -0.125,
            linear_term: 0.0,
            constant_term: 0.0,
            anisotropy: None,
        }];
        sky.mie_config = vec![DensityLayer {
            width: 0.0,
            exp_term: 1.0,
            exp_scale: -0.25,
            linear_term: 0.0,
            constant_term: 0.0,
            anisotropy: Some(0.8),
        }];
        // An edit the window *can* make, so the save is a real one.
        SkyKnob::HazeDensity.write(&mut sky, 1.5);

        let asset = named(&EnvironmentAsset::Sky(Box::new(sky)), "Renamed sky");
        let bytes = environment_asset_to_bytes(&asset);
        let Some(EnvironmentAsset::Sky(read_back)) =
            environment_asset_from_bytes("Renamed sky", &bytes)
        else {
            return Err("a saved sky decodes back as a sky".into());
        };

        assert_eq!(read_back.name, "Renamed sky");
        assert!((SkyKnob::HazeDensity.read(&read_back) - 1.5).abs() < 0.001);
        assert_eq!(read_back.rayleigh_config.len(), 1);
        assert_eq!(
            read_back
                .mie_config
                .first()
                .and_then(|layer| layer.anisotropy),
            Some(0.8)
        );
        Ok(())
    }

    /// A unique throwaway directory under the system temp dir (the crate has no
    /// `tempfile` dependency; this mirrors the helper sl-settings' tests use).
    fn tempdir(label: &str) -> Result<std::path::PathBuf, TestError> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "{}-{label}-{nanos}-{:?}",
            env!("CARGO_PKG_NAME"),
            std::thread::current().id()
        ));
        fs_err::create_dir_all(&dir)?;
        Ok(dir)
    }

    /// The stock pre-EEP `Default.xml` water preset, enough of it to convert.
    const LEGACY_WATER: &str = r"<llsd>
    <map>
    <key>blurMultiplier</key>
        <real>0.25</real>
    <key>fresnelOffset</key>
        <real>0.75</real>
    <key>waterFogDensity</key>
        <real>16</real>
    </map>
</llsd>
";

    /// An app with just the pieces `apply_imported_preset` reads and writes —
    /// the window state it seeds and the notification channel it reports a
    /// failure on. The window's widgets are not needed: the session is what an
    /// import produces, and the re-seed that paints it is tested elsewhere.
    fn import_app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<SettingsEditors>()
            .add_message::<FileDialogClosed>()
            .add_message::<sl_viewer_notifications::ShowNotification>()
            .add_systems(Update, apply_imported_preset);
        app
    }

    /// Announce a closed dialog as if the platform layer had.
    fn picked(app: &mut App, purpose: &str, path: &std::path::Path) {
        app.world_mut().write_message(FileDialogClosed {
            purpose: purpose.into(),
            outcome: FileDialogOutcome::Picked(path.to_path_buf()),
        });
        app.update();
    }

    /// **The two editors' dialogs are told apart.** They share one reply
    /// stream, so a sky import must not be answered with the file the water
    /// editor asked for — and the purposes are also the keys the remembered
    /// directories hang on.
    #[test]
    fn each_editor_imports_under_its_own_purpose() {
        assert_ne!(
            import_purpose(EditorKind::Sky),
            import_purpose(EditorKind::Water),
            "one reply stream, two askers"
        );
    }

    /// **An imported preset has no item behind it, and is dirty from the
    /// start.** Those two facts are what make Save refuse, Save As the way to
    /// file it, and a following Import ask before throwing it away.
    #[test]
    fn importing_a_preset_seeds_an_unfiled_modified_session() -> Result<(), TestError> {
        let dir = tempdir("import")?;
        let file = dir.join("%5BTOR%5D%20Bayouette.xml");
        fs_err::write(&file, LEGACY_WATER)?;
        let mut app = import_app();
        picked(&mut app, import_purpose(EditorKind::Water), &file);

        let editors = app.world().resource::<SettingsEditors>();
        let session = editors
            .water
            .session
            .as_ref()
            .ok_or("the import seeds the water window")?;
        assert_eq!(
            session.name, "[TOR] Bayouette",
            "the frame is named after the unescaped file stem"
        );
        assert!(session.item.is_none(), "nothing filed it yet");
        assert!(session.modified, "the frame exists nowhere but this window");
        assert!(session.reseed, "and the widgets have to be repainted");
        let EnvironmentAsset::Water(ref water) = session.edited else {
            return Err("a water editor imports water".into());
        };
        assert!(
            (water.fresnel_offset - 0.75).abs() < 0.001,
            "the preset's values, not the defaults"
        );
        assert_eq!(
            frame_name(&session.original),
            session.name,
            "a revert goes back to the preset as imported"
        );
        fs_err::remove_dir_all(&dir)?;
        Ok(())
    }

    /// **A file that is not a preset of this kind is refused, loudly.** The
    /// window keeps what it had and the reference's `WLImportFail` is raised —
    /// silently leaving the old frame on screen would read as "imported".
    #[test]
    fn a_file_that_is_not_this_kind_of_preset_is_refused() -> Result<(), TestError> {
        let dir = tempdir("import-wrong")?;
        let file = dir.join("Not a preset.xml");
        fs_err::write(
            &file,
            "<llsd><map><key>nope</key><real>1</real></map></llsd>",
        )?;
        let mut app = import_app();
        picked(&mut app, import_purpose(EditorKind::Sky), &file);

        assert!(
            app.world()
                .resource::<SettingsEditors>()
                .sky
                .session
                .is_none(),
            "the window is left as it was"
        );
        let raised: Vec<sl_viewer_notifications::ShowNotification> = app
            .world_mut()
            .resource_mut::<Messages<sl_viewer_notifications::ShowNotification>>()
            .drain()
            .collect();
        let first = raised.first().ok_or("a failure is reported")?;
        assert_eq!(first.template, "WLImportFail", "the reference's own alert");
        fs_err::remove_dir_all(&dir)?;
        Ok(())
    }

    /// **A reply for somebody else's dialog is left alone.** Every file-open
    /// reply in the viewer comes down one stream, so the uploaders (when they
    /// land) must not be able to seed a settings editor by accident.
    #[test]
    fn another_window_s_file_is_not_imported() -> Result<(), TestError> {
        let dir = tempdir("import-other")?;
        let file = dir.join("Water.xml");
        fs_err::write(&file, LEGACY_WATER)?;
        let mut app = import_app();
        picked(&mut app, "upload-image", &file);

        let editors = app.world().resource::<SettingsEditors>();
        assert!(editors.sky.session.is_none(), "the sky window is untouched");
        assert!(
            editors.water.session.is_none(),
            "and so is the water window"
        );
        fs_err::remove_dir_all(&dir)?;
        Ok(())
    }

    /// Drive [`begin_import`] for the sky window inside a real system, since
    /// its outputs are message writers.
    fn run_begin_import(app: &mut App) {
        app.world_mut()
            .run_system_once(
                |mut editors: ResMut<SettingsEditors>,
                 mut confirm: ResMut<PendingEditorReplace>,
                 mut notify: MessageWriter<sl_viewer_notifications::ShowNotification>,
                 mut dialogs: MessageWriter<OpenFileDialog>,
                 translator: Translator| {
                    begin_import(
                        EditorKind::Sky,
                        &mut editors,
                        &mut confirm,
                        &mut notify,
                        &mut dialogs,
                        &translator,
                    );
                },
            )
            .ok();
    }

    /// An app with the i18n resources `begin_import` reads its dialog title
    /// out of, and the two channels it writes to. The strings resolve to their
    /// own keys (`install_untranslated`), which is all these tests need — what
    /// they assert is the purpose and the confirmation, not the wording.
    fn begin_import_app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        sl_viewer_ui_core::i18n::install_untranslated(&mut app);
        app.init_resource::<SettingsEditors>()
            .init_resource::<PendingEditorReplace>()
            .add_message::<OpenFileDialog>()
            .add_message::<sl_viewer_notifications::ShowNotification>();
        app
    }

    /// **Import needs nothing open.** It is how an empty window gets a frame at
    /// all, so pressing it with no session must reach the file chooser rather
    /// than the "nothing is open" refusal every other button gets.
    #[test]
    fn importing_with_nothing_open_goes_straight_to_the_chooser() -> Result<(), TestError> {
        let mut app = begin_import_app();
        run_begin_import(&mut app);
        let asked: Vec<OpenFileDialog> = app
            .world_mut()
            .resource_mut::<Messages<OpenFileDialog>>()
            .drain()
            .collect();
        let first = asked.first().ok_or("the chooser is asked for")?;
        assert_eq!(
            &*first.purpose,
            import_purpose(EditorKind::Sky),
            "under the sky window's own purpose"
        );
        assert!(
            app.world().resource::<PendingEditorReplace>().0.is_none(),
            "and nothing was held back for a confirmation"
        );
        Ok(())
    }

    /// **Unsaved work is asked about before the file is picked, not after.**
    /// The reference wraps the whole of `doImportFromDisk` in
    /// `checkAndConfirmSettingsLoss`; asking afterwards would have the user
    /// choose a file only to be told it might not be used.
    #[test]
    fn importing_over_unsaved_work_asks_before_opening_the_chooser() -> Result<(), TestError> {
        let mut app = begin_import_app();
        let sky = SkySettings::legacy_windlight_default("Held sky");
        let asset = EnvironmentAsset::Sky(Box::new(sky));
        app.world_mut()
            .resource_mut::<SettingsEditors>()
            .sky
            .session = Some(EditSession {
            item: None,
            name: "Held sky".to_owned(),
            original: asset.clone(),
            edited: asset,
            dirty: false,
            modified: true,
            reseed: false,
            saving: false,
        });
        run_begin_import(&mut app);

        assert!(
            app.world()
                .resource::<Messages<OpenFileDialog>>()
                .is_empty(),
            "no chooser until the user says the work can go"
        );
        let raised: Vec<sl_viewer_notifications::ShowNotification> = app
            .world_mut()
            .resource_mut::<Messages<sl_viewer_notifications::ShowNotification>>()
            .drain()
            .collect();
        let first = raised.first().ok_or("the loss confirmation is raised")?;
        assert_eq!(first.template, "SettingsConfirmLoss");
        let held = app.world().resource::<PendingEditorReplace>();
        assert!(
            matches!(held.0, Some(HeldReplacement::Import(EditorKind::Sky))),
            "and the import is what a yes will carry out: {:?}",
            held.0
        );
        Ok(())
    }
}
