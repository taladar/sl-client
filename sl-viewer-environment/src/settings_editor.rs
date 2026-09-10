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
//! # Not done here
//!
//! - **No Import.** The reference's `Import` reads a legacy WindLight `.xml`
//!   preset off disk. That is the legacy-preset importer's job, not this
//!   window's (`viewer-environment-import-legacy-presets`).
//!
//! Reference (Firestorm, read-only): `llfloaterfixedenvironment.cpp`,
//! `panel_settings_sky_atmos.xml`, `panel_settings_sky_clouds.xml`,
//! `panel_settings_sky_sunmoon.xml`, `panel_settings_water.xml`,
//! `llsettingsvo.cpp` (the asset serialisation and its upload).

use std::collections::VecDeque;

use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui_widgets::{SliderRange, SliderValue, ValueChange};
use sl_client_bevy::{
    AssetKey, AssetUpdateLocation, Command, EnvironmentAsset, InventoryFolderKey, InventoryKey,
    SettingsKind, SkySettings, SlCommand, SlEvent, SlSessionEvent, TextureKey, UpdatableAssetType,
    WaterSettings, environment_asset_to_bytes,
};
use sl_viewer_inventory::inventory_actions::new_settings_item;
use sl_viewer_notifications::{NotificationResponse, ShowNotification};
use sl_viewer_pickers::ui_texture_picker::TextureSwatchValue;
use sl_viewer_platform::environment_assets::EnvironmentAssetManager;
use sl_viewer_ui_core::i18n::Translated;
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
use sl_viewer_world_api::{
    OpenSettingsEditor, PendingSettingsCreations, SettingsItemCreated, TexturePicked,
};
use sl_viewer_world_scene::environment::EnvironmentState;

use crate::knobs::{ColorKnob, SkyKnob, TextureKnob, WaterKnob};
use crate::rows::{spawn_action_button, spawn_color_row, spawn_slider, spawn_texture_row};
use crate::style::{DIM_LABEL_COLOR, FONT_SIZE, LABEL_COLOR};

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
// The tabs.
// ---------------------------------------------------------------------------

/// One tab of an editor: its label and the knobs on it.
///
/// A table rather than three hand-written panels, so "every knob is on exactly
/// one tab" is a property a test can check — a knob added to
/// [`SkyKnob::ALL`] and forgotten here would
/// otherwise be a control nobody can reach.
#[derive(Debug, Clone, Copy)]
struct TabPage {
    /// The tab label's Fluent key.
    label: &'static str,
    /// The tab's short name, for the element ids of the nodes on it.
    slug: &'static str,
    /// The colour swatches, in the first column.
    colors: &'static [ColorKnob],
    /// The texture swatches, under them.
    textures: &'static [TextureKnob],
    /// The sky sliders, split over the other two columns (empty in the water
    /// editor).
    sky: &'static [SkyKnob],
    /// The water sliders (empty in the sky editor).
    water: &'static [WaterKnob],
}

/// The sky editor's three tabs, in the reference's order.
const SKY_TABS: &[TabPage] = &[
    TabPage {
        label: "settings-editor-tab-atmosphere",
        slug: "atmosphere",
        colors: &[
            ColorKnob::Ambient,
            ColorKnob::BlueHorizon,
            ColorKnob::BlueDensity,
        ],
        textures: &[],
        sky: &[
            SkyKnob::HazeHorizon,
            SkyKnob::HazeDensity,
            SkyKnob::MoistureLevel,
            SkyKnob::DropletRadius,
            SkyKnob::IceLevel,
            SkyKnob::DensityMultiplier,
            SkyKnob::DistanceMultiplier,
            SkyKnob::MaxAltitude,
            SkyKnob::ProbeAmbiance,
            SkyKnob::Gamma,
        ],
        water: &[],
    },
    TabPage {
        label: "settings-editor-tab-clouds",
        slug: "clouds",
        colors: &[ColorKnob::CloudColor],
        textures: &[TextureKnob::CloudImage],
        sky: &[
            SkyKnob::CloudCoverage,
            SkyKnob::CloudScale,
            SkyKnob::CloudVariance,
            SkyKnob::CloudScrollX,
            SkyKnob::CloudScrollY,
            SkyKnob::CloudDensityX,
            SkyKnob::CloudDensityY,
            SkyKnob::CloudDensityD,
            SkyKnob::CloudDetailX,
            SkyKnob::CloudDetailY,
            SkyKnob::CloudDetailD,
        ],
        water: &[],
    },
    TabPage {
        label: "settings-editor-tab-sun-moon",
        slug: "sun-moon",
        colors: &[ColorKnob::SunColor],
        textures: &[
            TextureKnob::SunImage,
            TextureKnob::MoonImage,
            TextureKnob::BloomImage,
            TextureKnob::HaloImage,
            TextureKnob::RainbowImage,
        ],
        sky: &[
            SkyKnob::SunAzimuth,
            SkyKnob::SunElevation,
            SkyKnob::SunScale,
            SkyKnob::GlowFocus,
            SkyKnob::GlowSize,
            SkyKnob::StarBrightness,
            SkyKnob::MoonAzimuth,
            SkyKnob::MoonElevation,
            SkyKnob::MoonScale,
            SkyKnob::MoonBrightness,
            SkyKnob::SunArcRadians,
        ],
        water: &[],
    },
    TabPage {
        label: "settings-editor-tab-density",
        slug: "density",
        colors: &[],
        textures: &[],
        sky: &[
            SkyKnob::RayleighExpTerm,
            SkyKnob::RayleighExpScale,
            SkyKnob::RayleighLinear,
            SkyKnob::RayleighConstant,
            SkyKnob::RayleighWidth,
            SkyKnob::MieExpTerm,
            SkyKnob::MieExpScale,
            SkyKnob::MieLinear,
            SkyKnob::MieConstant,
            SkyKnob::MieAnisotropy,
            SkyKnob::MieWidth,
            SkyKnob::AbsorptionExpTerm,
            SkyKnob::AbsorptionExpScale,
            SkyKnob::AbsorptionLinear,
            SkyKnob::AbsorptionConstant,
            SkyKnob::AbsorptionWidth,
            // The atmosphere's geometry, which the same scattering model reads
            // and the reference's panel does not offer. Their ranges are its
            // validator's.
            SkyKnob::PlanetRadius,
            SkyKnob::SkyBottomRadius,
            SkyKnob::SkyTopRadius,
        ],
        water: &[],
    },
];

/// The water editor's one tab. The reference's water panel is a single page
/// too, and every water knob fits on it.
const WATER_TABS: &[TabPage] = &[TabPage {
    label: "settings-editor-tab-water",
    slug: "water",
    colors: &[ColorKnob::WaterFogColor],
    textures: &[
        TextureKnob::WaterNormalMap,
        TextureKnob::WaterTransparentTexture,
    ],
    sky: &[],
    water: &[
        WaterKnob::FogDensity,
        WaterKnob::UnderwaterModifier,
        WaterKnob::FresnelScale,
        WaterKnob::FresnelOffset,
        WaterKnob::NormalScaleX,
        WaterKnob::NormalScaleY,
        WaterKnob::NormalScaleZ,
        WaterKnob::ScaleAbove,
        WaterKnob::ScaleBelow,
        WaterKnob::BlurMultiplier,
        WaterKnob::LargeWaveX,
        WaterKnob::LargeWaveY,
        WaterKnob::SmallWaveX,
        WaterKnob::SmallWaveY,
    ],
}];

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
    /// The inventory item this asset came from. Always one: a settings frame
    /// reaches these windows by being opened, and a frame created from nothing
    /// is the inventory's New Sky / New Water, which files the item first and
    /// opens it after.
    item: EditedItem,
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
#[derive(Debug, Clone, Copy)]
struct EditedItem {
    /// The item whose asset a Save replaces.
    item_id: InventoryKey,
    /// The folder a Save As files the copy in.
    folder_id: InventoryFolderKey,
    /// Whether the item may be written back to at all.
    editable: bool,
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

/// The Save / Save As / Revert row and the status line under it. Returns the
/// status text entity.
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
            // can show one; the day-cycle editor is its own task.
            warn!("no settings editor for a day cycle yet: {}", open.name);
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
            confirm.0 = Some(open.clone());
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
                item: EditedItem {
                    item_id: pending.request.item_id,
                    folder_id: pending.request.folder_id,
                    editable: pending.request.editable,
                },
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
    mut colors: Query<(&EditorColorSwatch, &mut ColorSwatchValue)>,
    mut textures: Query<(&EditorTextureSwatch, &mut TextureSwatchValue)>,
    mut fields: Query<(&EditorNameField, &mut EditableText)>,
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
        for (swatch, mut value) in &mut colors {
            if swatch.editor == editor {
                value.0 = swatch.knob.read(&sky_frame, &water_frame);
            }
        }
        for (swatch, mut value) in &mut textures {
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

/// A chrome button press: Save (over the item), Save As (a new item), Revert.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy observer's parameters are its injected resources: the button pool and its \
              disabled filter, the session state, the two creation queues a Save As writes, and \
              the command and status channels"
)]
fn on_editor_button(
    press: On<Pointer<Press>>,
    buttons: Query<&EditorButton>,
    disabled: Query<(), With<bevy::ui::InteractionDisabled>>,
    mut editors: ResMut<SettingsEditors>,
    mut settings_creations: ResMut<PendingSettingsCreations>,
    mut saving_as: ResMut<PendingEditorSaveAs>,
    mut commands: MessageWriter<SlCommand>,
    mut texts: Query<&mut Text>,
) {
    if press.button != PointerButton::Primary || disabled.contains(press.entity) {
        return;
    }
    let Ok(button) = buttons.get(press.entity).copied() else {
        return;
    };
    let state = editors.get_mut(button.editor);
    let status = state.ui.status;
    let Some(session) = state.session.as_mut() else {
        set_status(&mut texts, status, "Nothing is open in this editor.");
        return;
    };
    // Recorded here and pushed after the session borrow ends.
    let mut queued: Option<PendingSave> = None;
    match button.action {
        EditorAction::Save => {
            let item = session.item;
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
            let folder_id = session.item.folder_id;
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
struct PendingEditorReplace(Option<OpenSettingsEditor>);

/// The reference's `getSettingsType()` word, for the confirmation's `[TYPE]`.
const fn settings_kind_word(kind: SettingsKind) -> &'static str {
    match kind {
        SettingsKind::Sky => "sky",
        SettingsKind::Water => "water",
        SettingsKind::DayCycle => "day cycle",
    }
}

/// Carry out (or drop) an open the user was asked to confirm.
///
/// Replaying the original [`OpenSettingsEditor`] rather than opening by hand
/// keeps one route into these windows: the confirmed open takes the same path an
/// unconfirmed one does, and the session it replaces is gone by the time it runs.
fn confirm_editor_replace(
    mut responses: MessageReader<NotificationResponse>,
    mut confirm: ResMut<PendingEditorReplace>,
    mut editors: ResMut<SettingsEditors>,
    mut opens: MessageWriter<OpenSettingsEditor>,
) {
    for response in responses.read() {
        if response.template != "SettingsConfirmLoss" {
            continue;
        }
        let Some(open) = confirm.0.take() else {
            continue;
        };
        if response.button != Some("OK") {
            continue;
        }
        // Drop the session *before* replaying, or the open would find it still
        // modified and ask again.
        if let Some(editor) = EditorKind::of_settings(open.kind) {
            editors.get_mut(editor).session = None;
        }
        opens.write(open);
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
            session.item = EditedItem {
                item_id: item.item,
                folder_id: item.folder,
                // Freshly minted by this agent, so modifiable by definition.
                editable: true,
            };
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
    use super::{EditorKind, SKY_TABS, WATER_TABS, frame_name, named, settings_kind_word};
    use crate::knobs::{ColorKnob, SkyKnob, TextureKnob, WaterKnob};
    use pretty_assertions::assert_eq;
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

    /// **Every knob is on exactly one tab.** The knob tables and the tab tables
    /// are two lists that have to agree, and the failure is silent in both
    /// directions: a knob missing from every tab is a value nobody can edit, and
    /// one on two tabs is two controls writing the same field with only the last
    /// re-seed deciding which is right.
    #[test]
    fn every_knob_is_on_exactly_one_tab() {
        let sky: Vec<SkyKnob> = SKY_TABS
            .iter()
            .chain(WATER_TABS)
            .flat_map(|page| page.sky.iter().copied())
            .collect();
        assert_eq!(
            sky.len(),
            SkyKnob::ALL.len(),
            "a sky knob is missing or twice over"
        );
        for knob in SkyKnob::ALL {
            assert_eq!(
                sky.iter().filter(|shown| *shown == knob).count(),
                1,
                "{knob:?} is not on exactly one tab"
            );
        }

        let water: Vec<WaterKnob> = SKY_TABS
            .iter()
            .chain(WATER_TABS)
            .flat_map(|page| page.water.iter().copied())
            .collect();
        assert_eq!(water.len(), WaterKnob::ALL.len());
        for knob in WaterKnob::ALL {
            assert_eq!(water.iter().filter(|shown| *shown == knob).count(), 1);
        }

        let colors: Vec<ColorKnob> = SKY_TABS
            .iter()
            .chain(WATER_TABS)
            .flat_map(|page| page.colors.iter().copied())
            .collect();
        assert_eq!(colors.len(), ColorKnob::ALL.len());
        for knob in ColorKnob::ALL {
            assert_eq!(colors.iter().filter(|shown| *shown == knob).count(), 1);
        }

        let textures: Vec<TextureKnob> = SKY_TABS
            .iter()
            .chain(WATER_TABS)
            .flat_map(|page| page.textures.iter().copied())
            .collect();
        assert_eq!(textures.len(), TextureKnob::ALL.len());
        for knob in TextureKnob::ALL {
            assert_eq!(textures.iter().filter(|shown| *shown == knob).count(), 1);
        }
    }

    /// **A tab shows one kind of slider.** The sky editor's panels hold sky
    /// knobs and the water editor's water ones; a knob on the wrong window's tab
    /// would spawn a control whose write-back looks in a frame that window's
    /// session never holds, and do nothing at all.
    #[test]
    fn a_tab_holds_only_its_own_editor_s_knobs() {
        for page in SKY_TABS {
            assert!(page.water.is_empty(), "{} shows water knobs", page.label);
        }
        for page in WATER_TABS {
            assert!(page.sky.is_empty(), "{} shows sky knobs", page.label);
        }
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
}
