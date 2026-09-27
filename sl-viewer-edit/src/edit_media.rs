//! **Per-face media editing** (`viewer-media-prim-browser`, the media-settings
//! addendum): the build floater Texture tab's **Media** mode and the **Media
//! Settings** window it opens — the write half of media-on-a-prim, whose read
//! half (`sl_viewer_world_view::media_prim`) has rendered other people's media
//! all along.
//!
//! # The Texture tab's Media mode
//!
//! The third entry of the tab's material-mode strip (the reference's `combobox
//! matmedia` "Media" item). It shows the selection's media line — the home page
//! of the one configuration the selected faces carry, *Multiple Media*, or
//! nothing — and three actions:
//!
//! - **Choose…** opens the Media Settings window for the selected faces. With
//!   more than one face selected it first asks (`MultipleFacesSelected`), since
//!   an Apply then writes every one of them.
//! - **Remove** asks (`DeleteMedia`) and then takes the media off the selected
//!   faces.
//! - **Align** fits the face's texture repeats and offset to the part of the
//!   texture the media surface draws into — "must load first", so it is live
//!   only while a selected face's surface is running.
//!
//! The tab opens in Media mode on its own when the selected face carries media
//! ([`crate::edit_texture`]'s `auto_mode_for_face`).
//!
//! # The Media Settings window
//!
//! The reference's `floater_media_settings.xml`, its three tabs as they are
//! there:
//!
//! - **General** — the home page (warning when the white-list would refuse it),
//!   a small muted preview of it, the current page with **Reset** (back to the
//!   home page), the auto-loop / first-click-interacts / auto-zoom / auto-play /
//!   auto-scale switches and the surface size, which auto-scale greys.
//! - **Customize** — the control-bar style and the six permission boxes (owner,
//!   the object's group, anyone × interact, show controls).
//! - **Security** — the white-list switch, the list with the entries the home
//!   page fails marked, **Add…** (the small `Whitelist Entry` window) and
//!   **Delete**. A home page the whole list refuses turns the switch off and
//!   greys it, as the reference does, until an entry lets it through.
//!
//! **OK** / **Apply** write the form to every selected face of every selected
//! object the agent may modify: the texture-entry media flag goes out first as
//! an `ObjectImage`, then the object's whole per-face media over the
//! `ObjectMedia` capability — `selectionSetMedia`'s order, and the one the
//! simulator expects (OpenSim sets the flag itself on the update, but a
//! removal is *only* the texture-entry update). Which fields an Apply carries,
//! and how a mixed selection is left alone, is `edit_media_model`'s.
//!
//! The window follows the selection while it is open, re-reading its fields
//! when the selected faces' media changes — unless the user has edited the
//! form, which a re-read would throw away. It closes with the build tools.
//!
//! Reference (Firestorm, read-only): `llpanelface.cpp` (`refreshMedia`,
//! `onClickBtnAddMedia` / `DeleteMedia`, `LLPanelFaceSetMediaFunctor`),
//! `llfloatermediasettings.cpp`, `llpanelmediasettings{general,permissions,
//! security}.cpp`, `llfloaterwhitelistentry.cpp`, `llselectmgr.cpp`
//! (`selectionSetMedia`).

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::text::{EditableText, FontCx, LayoutCx};
use bevy::ui::{Checked, InteractionDisabled};
use bevy_flair::style::components::ClassList;
use sl_client_bevy::{
    CAP_OBJECT_MEDIA, Command, MediaEntry, ObjectKey, PrimFaceId, ScopedObjectId, SlCapabilities,
    SlCommand, TextureEntry, TextureFace,
};
use sl_viewer_media::browser_widget::{
    BrowserView, BrowserViewSpec, SurfaceTrust, ValidatedMediaUrl, spawn_browser_view,
};
use sl_viewer_media::media_engine::MediaSurfaces;
use sl_viewer_notifications::{NotificationResponse, ShowNotification};
use sl_viewer_ui_core::skin::{TEXT_CLASS, WARN_TEXT_CLASS, text_meaning, text_role};
use sl_viewer_world_view::media_prim::{MediaData, MediaPrimState, MediaStartRequests};

use crate::edit_media_model::{
    FaceMedia, FormError, MediaEdit, MediaForm, MediaSelectionView, MediaSummary, UrlEdit,
    edit_object_media, form_edit, gather_media, home_passes_whitelist, media_alignment,
    parse_media_url, remove_object_media,
};
use crate::edit_params::set_disabled_class;
use crate::edit_texture::{OwnGate, PrimFaceLookup, ShowWhen, node_face_indices, spawn_row};
use crate::edit_tool::VALUE_CLASS;
use crate::floater::{
    DeferredFloaterContent, Floater, FloaterCaps, FloaterHandle, FloaterSpec, spawn_floater,
};
use crate::i18n::{Translated, Translator};
use crate::skin_palette::SkinPalette;
use crate::social::GroupsModel;
use crate::ui::{UiPanelShown, UiRoot, UiScaffoldSystems, column, row};
use crate::ui_checkbox::{CheckboxSpec, spawn_checkbox};
use crate::ui_combo::{ComboSelection, ComboSpec, spawn_combo};
use crate::ui_font::UiFont;
use crate::ui_spawn::{self, ButtonKind, ButtonSpec, SpawnedButton, UiLabel};
use crate::ui_tab::{DEFAULT_ELLIPSIS, TabPlacement, TabSpec, spawn_tab_container};
use crate::ui_text::set_editor_text;
use crate::ui_text_input::{TextInputKind, TextInputSpec, spawn_text_input};
use crate::virtual_list::{VirtualList, VirtualRow, layout_virtual_lists};
use crate::world_api::{EditToolState, MediaTarget, ObjectState, SelectionSet};
use sl_viewer_ui_widgets::ui_table::{
    TableAlign, TableColumn, TableColumnKind, TableColumnWidth, TableHandle, TableRowCells,
    TableSelectionMode, TableSpec, TableState, set_table_cell, spawn_table, spawn_table_row,
};

/// The Media Settings window's stable [`Floater::id`].
pub const MEDIA_SETTINGS_FLOATER_ID: &str = "media-settings";

/// The Whitelist Entry window's stable [`Floater::id`].
pub const WHITELIST_ENTRY_FLOATER_ID: &str = "media-whitelist-entry";

/// The windows' body font size, in logical pixels.
const FONT_SIZE: f32 = 13.0;

/// A label / value text colour.
const LABEL_COLOR: Color = SkinPalette::FALLBACK.text_primary;

/// A dim label / note colour.
const DIM_COLOR: Color = SkinPalette::FALLBACK.text_muted;

/// The warning colour (a home page the white-list refuses).
const WARN_COLOR: Color = Color::srgb(0.95, 0.62, 0.30);

/// An action button's background.
const BUTTON_BACKGROUND: Color = Color::srgb(0.13, 0.15, 0.20);

/// An action button's border.
const BUTTON_BORDER: Color = Color::srgb(0.34, 0.40, 0.52);

/// The home-page preview's edge, in logical pixels (the reference's 128×128
/// `preview_media`).
const PREVIEW_SIZE: f32 = 128.0;

/// The white-list's bounded height, in logical pixels.
const WHITELIST_HEIGHT: f32 = 160.0;

/// One white-list row's height, in logical pixels.
const ROW_HEIGHT: f32 = 20.0;

/// The width of a URL field, in `"0"`-glyph advances — what fits a tab panel's
/// capped, padded width (the field scrolls a longer URL).
const URL_FIELD_GLYPHS: f32 = 28.0;

/// The width of a size field, in `"0"`-glyph advances.
const SIZE_FIELD_GLYPHS: f32 = 6.0;

/// The mark on a white-list entry the home page fails (the reference's
/// `Parcel_Exp_Color` icon, as a glyph a skin can recolour).
const FAILS_MARK: &str = "⚠";

/// The Fluent key of the text a mixed home page shows.
const MULTIPLE_KEY: &str = "media-settings-multiple";

/// The white-list table: the fails-the-home-page mark and the pattern.
const WHITELIST_TABLE: TableSpec = TableSpec {
    element: "media-settings-whitelist",
    selection: TableSelectionMode::Single,
    columns: &[
        TableColumn {
            header_key: "media-settings-whitelist-mark",
            token: "mark",
            kind: TableColumnKind::Text,
            width: TableColumnWidth::Fixed { default: 22.0 },
            align: TableAlign::Center,
            sortable: false,
        },
        TableColumn {
            header_key: "media-settings-whitelist-entry",
            token: "entry",
            kind: TableColumnKind::Text,
            width: TableColumnWidth::Flex(1.0),
            align: TableAlign::Start,
            sortable: false,
        },
    ],
    default_sort: &[],
    builtin_sort: false,
    row_height: ROW_HEIGHT,
    font_size: FONT_SIZE,
    header_color: DIM_COLOR,
    cell_color: LABEL_COLOR,
    column_gap: 6.0,
    row_padding: 4.0,
    sort_setting: None,
    widths_setting: None,
};

// ---------------------------------------------------------------------------
// Messages.
// ---------------------------------------------------------------------------

/// Open (or raise) the Media Settings window on the current selection.
#[derive(Message, Debug, Clone, Copy, Default)]
pub struct OpenMediaSettings;

/// A press on one of the Texture tab's Media-mode buttons.
#[derive(Message, Component, Debug, Clone, Copy, PartialEq, Eq)]
enum MediaSectionAction {
    /// Choose… — open the settings (asking first for several faces).
    Choose,
    /// Remove — ask, then take the media off the selected faces.
    Remove,
    /// Align — fit the texture to the running surface.
    Align,
}

/// A press on one of the Media Settings window's buttons.
#[derive(Message, Component, Debug, Clone, Copy, PartialEq, Eq)]
enum MediaSettingsAction {
    /// Apply the form and close.
    Ok,
    /// Close without applying.
    Cancel,
    /// Apply the form and stay open.
    Apply,
    /// Send the selected faces back to their home page.
    Reset,
    /// Open the Whitelist Entry window.
    WhitelistAdd,
    /// Drop the selected white-list entry.
    WhitelistDelete,
}

/// A press on one of the Whitelist Entry window's buttons.
#[derive(Message, Component, Debug, Clone, Copy, PartialEq, Eq)]
enum WhitelistEntryAction {
    /// Add the typed entry and close.
    Ok,
    /// Close.
    Cancel,
}

// ---------------------------------------------------------------------------
// State.
// ---------------------------------------------------------------------------

/// Whether the agent's region serves the `ObjectMedia` capability — the
/// reference's `refreshMedia` gate, without which **Choose…** stays dead
/// (there would be nowhere to send the media).
#[derive(Resource, Debug, Default, Clone, Copy)]
struct ObjectMediaSupport {
    /// Whether the last capability map carried the cap.
    supported: bool,
}

/// Which confirmation this panel is waiting on.
#[derive(Resource, Debug, Default)]
struct PendingMediaConfirm {
    /// A `DeleteMedia` question is out.
    delete: bool,
    /// A `MultipleFacesSelected` question is out.
    choose: bool,
}

/// The Texture tab's Media-mode widgets.
#[derive(Resource, Debug, Clone, Copy)]
pub(crate) struct MediaSectionUi {
    /// The media line (home page / *Multiple Media* / nothing).
    info: Entity,
    /// Choose….
    choose: SpawnedButton,
    /// Remove.
    remove: SpawnedButton,
    /// Align.
    align: SpawnedButton,
}

/// The Media Settings window's widgets, for the in-place updates.
#[derive(Resource, Debug, Clone)]
struct MediaSettingsUi {
    /// The floater root.
    panel: Entity,
    /// Every field the form is read from and written to.
    fields: MediaFormFields,
    /// The home page's "fails the white-list" warning.
    home_warning: Entity,
    /// The preview browser view.
    preview: Entity,
    /// The current page (a read-only field).
    current_url: Entity,
    /// The object's group name, on the Customize tab.
    group_name: Entity,
    /// The Security tab's "home page fails this white-list" warning.
    whitelist_warning: Entity,
    /// The white-list table.
    whitelist: TableHandle,
    /// The error line over the buttons.
    status: Entity,
    /// The controls greyed while the selection may not be edited.
    controls: Vec<Entity>,
}

/// The form's widgets, one per [`MediaForm`] field.
#[derive(Debug, Clone)]
struct MediaFormFields {
    /// The home page field.
    home_url: Entity,
    /// Auto loop.
    auto_loop: Entity,
    /// First click interacts.
    first_click_interact: Entity,
    /// Auto zoom.
    auto_zoom: Entity,
    /// Auto play.
    auto_play: Entity,
    /// Auto scale.
    auto_scale: Entity,
    /// The width field.
    width_pixels: Entity,
    /// The height field.
    height_pixels: Entity,
    /// The controls combo.
    controls: Entity,
    /// The interact boxes, in [`crate::edit_media_model::MEDIA_PERM_BITS`] order.
    perms_interact: [Entity; 3],
    /// The show-controls boxes, in [`crate::edit_media_model::MEDIA_PERM_BITS`] order.
    perms_control: [Entity; 3],
    /// The white-list switch.
    whitelist_enable: Entity,
}

/// What the Media Settings window is showing.
#[derive(Resource, Debug, Default)]
struct MediaSettingsState {
    /// The selection the form was last filled from.
    view: MediaSelectionView,
    /// The form as it was filled — a form that still equals it has not been
    /// edited, so the window may re-read the selection into it.
    filled: MediaForm,
    /// The white-list as the form holds it (the table's rows).
    whitelist: Vec<String>,
    /// Whether the table needs rebinding.
    whitelist_dirty: bool,
    /// Whether the form has ever been filled since the window opened.
    loaded: bool,
    /// Whether the selection may be edited.
    editable: bool,
    /// Why the last Apply was refused.
    error: Option<FormError>,
    /// The home page the preview was last pointed at.
    previewed: Option<String>,
}

/// The Whitelist Entry window's widgets.
#[derive(Resource, Debug, Clone, Copy)]
struct WhitelistEntryUi {
    /// The floater root.
    panel: Entity,
    /// The pattern field.
    field: Entity,
}

// ---------------------------------------------------------------------------
// Plugin.
// ---------------------------------------------------------------------------

/// The plugin wiring per-face media editing into the viewer. Added by
/// [`crate::edit_texture::EditTexturePlugin`], whose tab it extends.
#[derive(Debug, Clone, Copy, Default)]
pub struct EditMediaPlugin;

impl Plugin for EditMediaPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<OpenMediaSettings>()
            .add_message::<MediaSectionAction>()
            .add_message::<MediaSettingsAction>()
            .add_message::<WhitelistEntryAction>()
            .add_message::<SlCapabilities>()
            .add_message::<ShowNotification>()
            .add_message::<NotificationResponse>()
            .init_resource::<ObjectMediaSupport>()
            .init_resource::<PendingMediaConfirm>()
            .init_resource::<MediaSettingsState>()
            .add_systems(
                Startup,
                spawn_media_floaters.after(UiScaffoldSystems::SpawnRoot),
            )
            .add_systems(
                Update,
                (
                    ingest_media_capability,
                    sync_media_section,
                    handle_media_section_actions,
                    answer_media_confirmations,
                    open_media_settings,
                    close_with_the_build_tools,
                    fill_media_settings,
                    sync_media_settings_controls,
                    handle_media_settings_actions,
                    handle_whitelist_entry_actions,
                    sync_whitelist_rows,
                    populate_whitelist_rows,
                    bind_whitelist_rows,
                    drive_media_preview,
                )
                    .chain()
                    .before(layout_virtual_lists),
            );
    }
}

/// Fold each capability map into [`ObjectMediaSupport`].
fn ingest_media_capability(
    mut capabilities: MessageReader<SlCapabilities>,
    mut support: ResMut<ObjectMediaSupport>,
) {
    for SlCapabilities(map) in capabilities.read() {
        let supported = map.contains_key(CAP_OBJECT_MEDIA);
        if supported != support.supported {
            info!(
                "this region {} media on a prim",
                if supported {
                    "serves"
                } else {
                    "does not serve"
                }
            );
        }
        support.supported = supported;
    }
}

// ---------------------------------------------------------------------------
// The selection's media.
// ---------------------------------------------------------------------------

/// One selected object's faces, as a media edit sees them.
struct SelectedMedia {
    /// The object's region-scoped id (what the `ObjectImage` addresses).
    scoped: ScopedObjectId,
    /// The object's key (what the `ObjectMedia` capability addresses).
    full: ObjectKey,
    /// Whether the agent may modify it.
    editable: bool,
    /// Every face's rendered texture entry.
    texture: Vec<TextureFace>,
    /// Every face's media, where its texture entry carries the flag.
    media: Vec<Option<MediaEntry>>,
    /// The selected faces.
    selected: Vec<usize>,
}

/// Everything a media edit reads about the selection, and where it writes.
#[derive(SystemParam)]
struct MediaSelection<'w, 's> {
    /// What is selected.
    selection: Res<'w, SelectionSet>,
    /// Object properties, for the modify-permission check.
    objects: Res<'w, ObjectState>,
    /// The faces' rendered texture entries.
    prim_faces: PrimFaceLookup<'w, 's>,
    /// The faces' media, as the capability reported it.
    media: Option<Res<'w, MediaData>>,
}

impl MediaSelection<'_, '_> {
    /// The selected objects' faces.
    fn objects(&self) -> Vec<SelectedMedia> {
        self.selection
            .iter()
            .filter_map(|node| {
                let texture = self.prim_faces.current_faces(node.entity);
                if texture.is_empty() {
                    return None;
                }
                let reported = self
                    .media
                    .as_ref()
                    .and_then(|media| media.faces(node.full))
                    .unwrap_or(&[]);
                let media = texture
                    .iter()
                    .enumerate()
                    .map(|(index, face)| {
                        if face.media_enabled() {
                            reported.get(index).cloned().flatten()
                        } else {
                            None
                        }
                    })
                    .collect();
                Some(SelectedMedia {
                    scoped: node.scoped,
                    full: node.full,
                    editable: self.objects.agent_can_modify(&node.scoped),
                    selected: node_face_indices(node, texture.len()),
                    texture,
                    media,
                })
            })
            .collect()
    }

    /// The selected faces, gathered into the form.
    fn view(&self) -> MediaSelectionView {
        let objects = self.objects();
        let faces: Vec<FaceMedia<'_>> = objects
            .iter()
            .flat_map(|object| {
                object.selected.iter().map(|&index| FaceMedia {
                    flagged: object
                        .texture
                        .get(index)
                        .is_some_and(|face| face.media_enabled()),
                    entry: object.media.get(index).and_then(Option::as_ref),
                })
            })
            .collect();
        gather_media(&faces)
    }

    /// Whether the selection may be edited: the primary object may be modified
    /// (the reference's `first_object->permModify()`).
    fn editable(&self) -> bool {
        self.selection
            .primary()
            .is_some_and(|node| self.objects.agent_can_modify(&node.scoped))
    }

    /// How many faces are selected, across every object.
    fn face_count(&self) -> usize {
        self.objects()
            .iter()
            .map(|object| object.selected.len())
            .sum()
    }

    /// Every selected face, as the media surfaces key them.
    fn targets(&self) -> Vec<MediaTarget> {
        self.objects()
            .iter()
            .flat_map(|object| {
                object.selected.iter().filter_map(|&index| {
                    u16::try_from(index).ok().map(|face| MediaTarget {
                        object: object.full,
                        face: PrimFaceId::new(face),
                    })
                })
            })
            .collect()
    }
}

/// Send `object`'s texture entry with the media flag set (`flag`) or cleared
/// on `faces` — the first half of an edit, and the whole of a removal. Sends
/// nothing when no flag changes.
fn send_media_flags(
    object: &SelectedMedia,
    faces: &[usize],
    flag: bool,
    objects: &ObjectState,
    commands: &mut MessageWriter<SlCommand>,
) {
    let mut entry = TextureEntry {
        faces: object.texture.clone(),
    };
    let mut changed = false;
    for &index in faces {
        if let Some(face) = entry.faces.get_mut(index)
            && face.media_enabled() != flag
        {
            if flag {
                face.media_flags |= 0x01;
            } else {
                face.media_flags &= !0x01;
            }
            changed = true;
        }
    }
    if changed {
        commands.write(SlCommand(Command::SetObjectImage {
            local_id: object.scoped,
            media_url: objects.media_url_of(&object.scoped),
            texture_entry: entry,
        }));
    }
}

/// Apply `edit` to every selected face of every selected object the agent may
/// modify: the flags, then the object's media. Returns the faces that now
/// carry media.
fn apply_media_edit(
    selection: &MediaSelection,
    edit: &MediaEdit,
    commands: &mut MessageWriter<SlCommand>,
) -> Vec<MediaTarget> {
    let mut edited = Vec::new();
    for object in selection.objects() {
        if !object.editable {
            continue;
        }
        let result = edit_object_media(&object.media, &object.selected, edit);
        if result.flagged.is_empty() {
            continue;
        }
        send_media_flags(&object, &result.flagged, true, &selection.objects, commands);
        edited.extend(result.flagged.iter().filter_map(|&index| {
            u16::try_from(index).ok().map(|face| MediaTarget {
                object: object.full,
                face: PrimFaceId::new(face),
            })
        }));
        commands.write(SlCommand(Command::SetObjectMedia {
            object_id: object.full,
            faces: result.faces,
        }));
    }
    edited
}

/// Take the media off every selected face of every selected object the agent
/// may modify: the flags, then — when faces keep media — the rest.
fn remove_selected_media(selection: &MediaSelection, commands: &mut MessageWriter<SlCommand>) {
    for object in selection.objects() {
        if !object.editable {
            continue;
        }
        send_media_flags(
            &object,
            &object.selected,
            false,
            &selection.objects,
            commands,
        );
        if let Some(faces) = remove_object_media(&object.media, &object.selected) {
            commands.write(SlCommand(Command::SetObjectMedia {
                object_id: object.full,
                faces,
            }));
        }
    }
}

// ---------------------------------------------------------------------------
// The Texture tab's Media mode.
// ---------------------------------------------------------------------------

/// Spawn the Media-mode rows into the Texture tab's `page`: the media line and
/// the Choose… / Remove / Align buttons, all shown only in Media mode.
pub(crate) fn spawn_media_section(
    commands: &mut Commands,
    page: Entity,
    tab_index: &mut i32,
    font_size: f32,
) -> MediaSectionUi {
    // The buttons' observers write a message a specimen host has no plugin to
    // register; registering it here keeps a click in the gallery harmless.
    commands.init_resource::<Messages<MediaSectionAction>>();
    let info_row = spawn_row(commands, page, "build-tex-media-label", font_size);
    commands.entity(info_row).insert(ShowWhen::Media);
    let info = commands
        .spawn((
            Text::default(),
            UiFont::Sans.at(font_size),
            TextColor(SkinPalette::FALLBACK.text_primary),
            ClassList::new_with_classes([VALUE_CLASS]),
            Name::new("build-tex-media:info"),
            ChildOf(info_row),
        ))
        .id();
    let buttons = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                flex_wrap: FlexWrap::Wrap,
                row_gap: Val::Px(4.0),
                ..row(Val::Px(6.0))
            },
            ShowWhen::Media,
            // These buttons follow their own rule (Choose… is live on a face
            // without media, when the rest of the tab is refused), so the
            // panel-wide greying must not reach them.
            OwnGate,
            ChildOf(page),
        ))
        .id();
    let mut button = |key: &'static str, action: MediaSectionAction| {
        let spawned = ui_spawn::spawn_button(
            commands,
            buttons,
            ButtonSpec::bordered(UiLabel::key(key), format!("build-tex:{key}"))
                .kind(ButtonKind::Headless)
                .tab_from(tab_index)
                .padding(10.0, 2.0)
                .colors(
                    Color::srgba(0.18, 0.18, 0.2, 1.0),
                    Color::srgba(0.4, 0.4, 0.45, 1.0),
                )
                .label_color(SkinPalette::FALLBACK.text_primary)
                .label_class(VALUE_CLASS)
                .font_size(font_size),
        );
        commands
            .entity(spawned.button)
            .insert(action)
            .observe(on_media_section_press);
        spawned
    };
    let choose = button("build-tex-media-choose", MediaSectionAction::Choose);
    let remove = button("build-tex-media-remove", MediaSectionAction::Remove);
    let align = button("build-tex-media-align", MediaSectionAction::Align);
    MediaSectionUi {
        info,
        choose,
        remove,
        align,
    }
}

/// A press on a Media-mode button: hand it to [`handle_media_section_actions`]
/// unless the button is refused.
fn on_media_section_press(
    press: On<Pointer<Press>>,
    buttons: Query<&MediaSectionAction, Without<InteractionDisabled>>,
    mut actions: MessageWriter<MediaSectionAction>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    if let Ok(action) = buttons.get(press.entity) {
        actions.write(*action);
    }
}

/// Grey (or un-grey) one Media-mode button.
fn gate_section_button(
    commands: &mut Commands,
    classes: &mut Query<&mut ClassList>,
    disabled: &Query<(), With<InteractionDisabled>>,
    button: SpawnedButton,
    enabled: bool,
) {
    if disabled.contains(button.button) != enabled {
        return;
    }
    if enabled {
        commands
            .entity(button.button)
            .remove::<InteractionDisabled>();
    } else {
        commands.entity(button.button).insert(InteractionDisabled);
    }
    if let Ok(mut class_list) = classes.get_mut(button.label) {
        set_disabled_class(&mut class_list, !enabled);
    }
}

/// Whether any selected face's media surface is running (Align needs one).
fn any_live_surface(targets: &[MediaTarget], live: Option<&MediaPrimState>) -> bool {
    live.is_some_and(|live| {
        targets
            .iter()
            .any(|target| live.active.contains_key(target))
    })
}

/// Keep the Media-mode line and buttons in step with the selection.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its data access; bundling these \
              eight into a SystemParam would only move the list"
)]
fn sync_media_section(
    tool: Res<EditToolState>,
    ui: Option<Res<MediaSectionUi>>,
    selection: MediaSelection,
    support: Res<ObjectMediaSupport>,
    live: Option<Res<MediaPrimState>>,
    translator: Translator,
    mut texts: Query<&mut Text>,
    mut gate: (
        Commands,
        Query<&mut ClassList>,
        Query<(), With<InteractionDisabled>>,
    ),
) {
    if !tool.active {
        return;
    }
    let Some(ui) = ui.as_deref() else {
        return;
    };
    let view = selection.view();
    let line = match &view.summary {
        MediaSummary::None => String::new(),
        MediaSummary::Single(home) => home.clone(),
        MediaSummary::Multiple => translator.get(MULTIPLE_KEY),
    };
    if let Ok(mut text) = texts.get_mut(ui.info)
        && text.0 != line
    {
        text.0 = line;
    }
    let selected = selection.selection.primary().is_some();
    let editable = selected && selection.editable();
    let live_surface = any_live_surface(&selection.targets(), live.as_deref());
    let (commands, classes, disabled) = &mut gate;
    for (button, enabled) in [
        (ui.choose, editable && support.supported),
        (ui.remove, editable && view.any_media),
        (ui.align, editable && live_surface),
    ] {
        gate_section_button(commands, classes, disabled, button, enabled);
    }
}

/// Act on a Media-mode press.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its data access"
)]
fn handle_media_section_actions(
    mut actions: MessageReader<MediaSectionAction>,
    selection: MediaSelection,
    mut pending: ResMut<PendingMediaConfirm>,
    mut notify: MessageWriter<ShowNotification>,
    mut opens: MessageWriter<OpenMediaSettings>,
    live: Option<Res<MediaPrimState>>,
    surfaces: Option<NonSend<MediaSurfaces>>,
    images: Res<Assets<Image>>,
    mut commands: MessageWriter<SlCommand>,
) {
    for action in actions.read() {
        match action {
            MediaSectionAction::Choose => {
                if selection.face_count() > 1 {
                    pending.choose = true;
                    notify.write(ShowNotification::new("MultipleFacesSelected"));
                } else {
                    opens.write(OpenMediaSettings);
                }
            }
            MediaSectionAction::Remove => {
                pending.delete = true;
                notify.write(ShowNotification::new("DeleteMedia"));
            }
            MediaSectionAction::Align => {
                align_media(
                    &selection,
                    live.as_deref(),
                    surfaces.as_deref(),
                    &images,
                    &mut commands,
                );
            }
        }
    }
}

/// Align: set each selected face with a running surface to the repeats and
/// offset that fit the surface into its texture, and send each object's entry.
fn align_media(
    selection: &MediaSelection,
    live: Option<&MediaPrimState>,
    surfaces: Option<&MediaSurfaces>,
    images: &Assets<Image>,
    commands: &mut MessageWriter<SlCommand>,
) {
    let (Some(live), Some(surfaces)) = (live, surfaces) else {
        return;
    };
    for object in selection.objects() {
        if !object.editable {
            continue;
        }
        let mut entry = TextureEntry {
            faces: object.texture.clone(),
        };
        let mut changed = false;
        for &index in &object.selected {
            let Ok(face_id) = u16::try_from(index) else {
                continue;
            };
            let target = MediaTarget {
                object: object.full,
                face: PrimFaceId::new(face_id),
            };
            let Some(slot) = live
                .active
                .get(&target)
                .and_then(|active| surfaces.get(active.surface))
            else {
                continue;
            };
            let texture = images
                .get(&slot.image)
                .map_or((0, 0), |image| (image.width(), image.height()));
            let Some([scale_s, scale_t, offset_s, offset_t]) =
                media_alignment((slot.size.x, slot.size.y), texture)
            else {
                continue;
            };
            if let Some(face) = entry.faces.get_mut(index) {
                face.scale_s = scale_s;
                face.scale_t = scale_t;
                face.offset_s = offset_s;
                face.offset_t = offset_t;
                changed = true;
            }
        }
        if changed {
            commands.write(SlCommand(Command::SetObjectImage {
                local_id: object.scoped,
                media_url: selection.objects.media_url_of(&object.scoped),
                texture_entry: entry,
            }));
        }
    }
}

/// Act on the answers to the two questions this panel asks.
fn answer_media_confirmations(
    mut responses: MessageReader<NotificationResponse>,
    mut pending: ResMut<PendingMediaConfirm>,
    selection: MediaSelection,
    mut opens: MessageWriter<OpenMediaSettings>,
    ui: Option<Res<MediaSettingsUi>>,
    mut panels: Query<&mut UiPanelShown>,
    mut commands: MessageWriter<SlCommand>,
) {
    for response in responses.read() {
        match response.template {
            "DeleteMedia" if pending.delete => {
                pending.delete = false;
                // `YES_NO_FORM`'s Yes button is *named* "OK" (only its label
                // says Yes), like every OK / Cancel pair in the catalogue.
                if response.button == Some("OK") {
                    remove_selected_media(&selection, &mut commands);
                    // The reference closes the settings window over a removal:
                    // it would otherwise show media the faces no longer carry.
                    if let Some(ui) = ui.as_deref()
                        && let Ok(mut shown) = panels.get_mut(ui.panel)
                        && shown.0
                    {
                        shown.0 = false;
                    }
                }
            }
            "MultipleFacesSelected" if pending.choose => {
                pending.choose = false;
                if response.button == Some("OK") {
                    opens.write(OpenMediaSettings);
                }
            }
            _other => {}
        }
    }
}

// ---------------------------------------------------------------------------
// The Media Settings window: spawn.
// ---------------------------------------------------------------------------

/// The Media Settings window's [`FloaterSpec`] — shared with the `FLOATERS`
/// registry, so the swept window is the one the viewer spawns.
#[must_use]
pub fn media_settings_floater_spec() -> FloaterSpec {
    FloaterSpec {
        id: MEDIA_SETTINGS_FLOATER_ID,
        title: "Media Settings".to_owned(),
        // Clear of the build tools on the right, which it is worked beside.
        position: Vec2::new(180.0, 120.0),
        default_size: None,
        min_size: None,
        dock_host: None,
        caps: FloaterCaps {
            resizable: false,
            minimizable: true,
            closable: true,
            dockable: false,
        },
    }
}

/// The Whitelist Entry window's [`FloaterSpec`].
#[must_use]
pub fn whitelist_entry_floater_spec() -> FloaterSpec {
    FloaterSpec {
        id: WHITELIST_ENTRY_FLOATER_ID,
        title: "Whitelist Entry".to_owned(),
        position: Vec2::new(220.0, 260.0),
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

/// Startup: spawn both windows' (hidden) chrome, their content deferred to the
/// first open.
fn spawn_media_floaters(mut commands: Commands, root: Res<UiRoot>) {
    let handle = spawn_floater(&mut commands, root.0, media_settings_floater_spec());
    commands
        .entity(handle.title_text)
        .insert(Translated::new("media-settings-title"));
    let builder = commands.register_system(build_media_settings_content);
    commands
        .entity(handle.root)
        .insert(DeferredFloaterContent { builder, handle });

    let handle = spawn_floater(&mut commands, root.0, whitelist_entry_floater_spec());
    commands
        .entity(handle.title_text)
        .insert(Translated::new("media-whitelist-entry-title"));
    let builder = commands.register_system(build_whitelist_entry_content);
    commands
        .entity(handle.root)
        .insert(DeferredFloaterContent { builder, handle });
}

/// First-open content build of the Media Settings window.
fn build_media_settings_content(
    In(handle): In<FloaterHandle>,
    mut commands: Commands,
    mut state: ResMut<MediaSettingsState>,
) {
    let parts = spawn_media_settings_content(&mut commands, handle.content, FONT_SIZE);
    commands.insert_resource(MediaSettingsUi {
        panel: handle.root,
        fields: parts.fields,
        home_warning: parts.home_warning,
        preview: parts.preview,
        current_url: parts.current_url,
        group_name: parts.group_name,
        whitelist_warning: parts.whitelist_warning,
        whitelist: parts.whitelist,
        status: parts.status,
        controls: parts.controls,
    });
    // The first fill happens now that there is something to fill.
    state.loaded = false;
}

/// First-open content build of the Whitelist Entry window.
fn build_whitelist_entry_content(In(handle): In<FloaterHandle>, mut commands: Commands) {
    let field = spawn_whitelist_entry_content(&mut commands, handle.content, FONT_SIZE);
    commands.insert_resource(WhitelistEntryUi {
        panel: handle.root,
        field,
    });
}

/// The nodes of a built Media Settings window.
struct MediaSettingsParts {
    /// The form's fields.
    fields: MediaFormFields,
    /// The home page's white-list warning.
    home_warning: Entity,
    /// The preview.
    preview: Entity,
    /// The current page.
    current_url: Entity,
    /// The group name.
    group_name: Entity,
    /// The Security tab's warning.
    whitelist_warning: Entity,
    /// The white-list table.
    whitelist: TableHandle,
    /// The error line.
    status: Entity,
    /// The controls the editable gate greys.
    controls: Vec<Entity>,
}

/// A text line under `parent`, in `color`, the caller updates in place.
fn spawn_line(commands: &mut Commands, parent: Entity, color: Color, font_size: f32) -> Entity {
    commands
        .spawn((
            Text::default(),
            UiFont::Sans.at(font_size),
            text_role(color),
            Pickable::IGNORE,
            ChildOf(parent),
        ))
        .id()
}

/// A translated label under `parent`.
fn spawn_label(
    commands: &mut Commands,
    parent: Entity,
    key: &'static str,
    color: Color,
    font_size: f32,
) -> Entity {
    ui_spawn::spawn_label(commands, parent, UiLabel::key(key), color, font_size)
}

/// A translated warning line under `parent`, hidden until the sync shows it.
fn spawn_warning(
    commands: &mut Commands,
    parent: Entity,
    key: &'static str,
    font_size: f32,
) -> Entity {
    commands
        .spawn((
            Node {
                display: Display::None,
                max_width: Val::Px(340.0),
                ..default()
            },
            Text::default(),
            Translated::new(key),
            UiFont::Sans.at(font_size),
            text_meaning(WARN_COLOR, WARN_TEXT_CLASS),
            Pickable::IGNORE,
            ChildOf(parent),
        ))
        .id()
}

/// A wrapping row under `parent`.
fn spawn_flow_row(commands: &mut Commands, parent: Entity, gap: f32) -> Entity {
    commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                flex_wrap: FlexWrap::Wrap,
                row_gap: Val::Px(4.0),
                ..row(Val::Px(gap))
            },
            ChildOf(parent),
        ))
        .id()
}

/// A translated checkbox under `parent`.
fn spawn_box(
    commands: &mut Commands,
    parent: Entity,
    key: &'static str,
    tab_index: &mut i32,
    font_size: f32,
) -> Entity {
    let index = *tab_index;
    *tab_index = tab_index.saturating_add(1);
    spawn_checkbox(
        commands,
        parent,
        &CheckboxSpec {
            element: key,
            label: key.to_owned(),
            tab_index: index,
            font_size,
            translate_label: true,
        },
    )
    .checkbox
}

/// A translated window button carrying `action`; the caller observes its
/// press.
fn spawn_window_button<A: Component + Copy>(
    commands: &mut Commands,
    parent: Entity,
    key: &'static str,
    action: A,
    tab_index: &mut i32,
    font_size: f32,
) -> Entity {
    let index = *tab_index;
    *tab_index = tab_index.saturating_add(1);
    let button = ui_spawn::spawn_button(
        commands,
        parent,
        ButtonSpec::bordered(UiLabel::key(key), format!("media-settings-button:{key}"))
            .tab_index(index)
            .colors(BUTTON_BACKGROUND, BUTTON_BORDER)
            .label_color(LABEL_COLOR)
            .font_size(font_size)
            // The greyed look of a refused button is the skin's
            // (`.sk-button:disabled .sk-text`).
            .label_class(TEXT_CLASS),
    )
    .button;
    commands.entity(button).insert(action);
    button
}

/// A Media Settings button dispatching `action`.
fn spawn_settings_button(
    commands: &mut Commands,
    parent: Entity,
    key: &'static str,
    action: MediaSettingsAction,
    tab_index: &mut i32,
    font_size: f32,
) -> Entity {
    let button = spawn_window_button(commands, parent, key, action, tab_index, font_size);
    commands.entity(button).observe(on_media_settings_press);
    button
}

/// A Whitelist Entry button dispatching `action`.
fn spawn_entry_button(
    commands: &mut Commands,
    parent: Entity,
    key: &'static str,
    action: WhitelistEntryAction,
    tab_index: &mut i32,
    font_size: f32,
) -> Entity {
    let button = spawn_window_button(commands, parent, key, action, tab_index, font_size);
    commands.entity(button).observe(on_whitelist_entry_press);
    button
}

/// Build the Media Settings window's content into `parent` at `font_size`: the
/// three tabs, the error line and the OK / Cancel / Apply row. Shared by the
/// live window and its specimen.
fn spawn_media_settings_content(
    commands: &mut Commands,
    parent: Entity,
    font_size: f32,
) -> MediaSettingsParts {
    commands.init_resource::<Messages<MediaSettingsAction>>();
    let mut tab_index = 0_i32;
    let body = commands
        .spawn((
            Node {
                padding: UiRect::all(Val::Px(8.0)),
                ..column(Val::Px(8.0))
            },
            Name::new("media-settings:body"),
            ChildOf(parent),
        ))
        .id();
    let labels = [
        "media-settings-tab-general".to_owned(),
        "media-settings-tab-customize".to_owned(),
        "media-settings-tab-security".to_owned(),
    ];
    let tabs = spawn_tab_container(
        commands,
        body,
        &TabSpec {
            element: "media-settings-tabs",
            placement: TabPlacement::BlockStart,
            labels: &labels,
            active: 0,
            tab_index,
            font_size,
            strip_width: None,
            ellipsis: DEFAULT_ELLIPSIS,
            translate_labels: true,
        },
    );
    tab_index = tab_index.saturating_add(1);
    let mut controls = Vec::new();
    let [general, customize, security] =
        [0, 1, 2].map(|index| tabs.panels.get(index).copied().unwrap_or(tabs.panel_area));

    let general = spawn_general_tab(commands, general, &mut tab_index, font_size, &mut controls);
    let customize = spawn_customize_tab(
        commands,
        customize,
        &mut tab_index,
        font_size,
        &mut controls,
    );
    let security = spawn_security_tab(commands, security, &mut tab_index, font_size, &mut controls);

    let status = commands
        .spawn((
            Node {
                max_width: Val::Px(340.0),
                ..default()
            },
            Text::default(),
            UiFont::Sans.at(font_size),
            text_meaning(WARN_COLOR, WARN_TEXT_CLASS),
            Pickable::IGNORE,
            ChildOf(body),
        ))
        .id();
    let buttons = spawn_flow_row(commands, body, 8.0);
    let ok = spawn_settings_button(
        commands,
        buttons,
        "media-settings-ok",
        MediaSettingsAction::Ok,
        &mut tab_index,
        font_size,
    );
    spawn_settings_button(
        commands,
        buttons,
        "media-settings-cancel",
        MediaSettingsAction::Cancel,
        &mut tab_index,
        font_size,
    );
    let apply = spawn_settings_button(
        commands,
        buttons,
        "media-settings-apply",
        MediaSettingsAction::Apply,
        &mut tab_index,
        font_size,
    );
    controls.extend([ok, apply]);

    MediaSettingsParts {
        fields: MediaFormFields {
            home_url: general.home_url,
            auto_loop: general.auto_loop,
            first_click_interact: general.first_click_interact,
            auto_zoom: general.auto_zoom,
            auto_play: general.auto_play,
            auto_scale: general.auto_scale,
            width_pixels: general.width_pixels,
            height_pixels: general.height_pixels,
            controls: customize.controls,
            perms_interact: customize.perms_interact,
            perms_control: customize.perms_control,
            whitelist_enable: security.whitelist_enable,
        },
        home_warning: general.home_warning,
        preview: general.preview,
        current_url: general.current_url,
        group_name: customize.group_name,
        whitelist_warning: security.warning,
        whitelist: security.table,
        status,
        controls,
    }
}

/// The General tab's nodes.
struct GeneralParts {
    /// The home page field.
    home_url: Entity,
    /// The home page's white-list warning.
    home_warning: Entity,
    /// The preview.
    preview: Entity,
    /// The current page.
    current_url: Entity,
    /// Auto loop.
    auto_loop: Entity,
    /// First click interacts.
    first_click_interact: Entity,
    /// Auto zoom.
    auto_zoom: Entity,
    /// Auto play.
    auto_play: Entity,
    /// Auto scale.
    auto_scale: Entity,
    /// Width.
    width_pixels: Entity,
    /// Height.
    height_pixels: Entity,
}

/// Build the General tab into `panel`.
fn spawn_general_tab(
    commands: &mut Commands,
    panel: Entity,
    tab_index: &mut i32,
    font_size: f32,
    controls: &mut Vec<Entity>,
) -> GeneralParts {
    let home_row = spawn_flow_row(commands, panel, 8.0);
    spawn_label(
        commands,
        home_row,
        "media-settings-home-label",
        DIM_COLOR,
        font_size,
    );
    let home_warning = spawn_warning(
        commands,
        home_row,
        "media-settings-home-fails-whitelist",
        font_size,
    );
    let home_url = spawn_text_input(
        commands,
        panel,
        &TextInputSpec {
            font_size,
            width_glyphs: URL_FIELD_GLYPHS,
            tab_index: *tab_index,
            max_characters: Some(1024),
            ..TextInputSpec::new("media-settings-home-url", TextInputKind::Line)
        },
    );
    *tab_index = tab_index.saturating_add(1);
    controls.push(home_url);

    // The preview: the home page, small and muted, watch-only.
    let preview_column = commands
        .spawn((
            Node {
                align_self: AlignSelf::Center,
                align_items: AlignItems::Center,
                ..column(Val::Px(2.0))
            },
            ChildOf(panel),
        ))
        .id();
    let preview_frame = commands
        .spawn((
            Node {
                width: Val::Px(PREVIEW_SIZE),
                height: Val::Px(PREVIEW_SIZE),
                ..default()
            },
            ChildOf(preview_column),
        ))
        .id();
    let preview = spawn_browser_view(
        commands,
        preview_frame,
        &BrowserViewSpec {
            initial_url: ValidatedMediaUrl::blank(),
            trust: SurfaceTrust::InWorld,
            tab_index: -1,
            fixed_height: Some(PREVIEW_SIZE),
        },
    );
    commands.entity(preview).insert(InteractionDisabled);
    spawn_label(
        commands,
        preview_column,
        "media-settings-preview",
        DIM_COLOR,
        font_size,
    );

    spawn_label(
        commands,
        panel,
        "media-settings-current-label",
        DIM_COLOR,
        font_size,
    );
    let current_row = spawn_flow_row(commands, panel, 8.0);
    let current_url = spawn_text_input(
        commands,
        current_row,
        &TextInputSpec {
            font_size,
            width_glyphs: URL_FIELD_GLYPHS - 10.0,
            tab_index: *tab_index,
            read_only: true,
            ..TextInputSpec::new("media-settings-current-url", TextInputKind::Line)
        },
    );
    *tab_index = tab_index.saturating_add(1);
    let reset = spawn_settings_button(
        commands,
        current_row,
        "media-settings-reset",
        MediaSettingsAction::Reset,
        tab_index,
        font_size,
    );
    controls.push(reset);

    let auto_loop = spawn_box(
        commands,
        panel,
        "media-settings-auto-loop",
        tab_index,
        font_size,
    );
    let first_click_interact = spawn_box(
        commands,
        panel,
        "media-settings-first-click-interact",
        tab_index,
        font_size,
    );
    let auto_zoom = spawn_box(
        commands,
        panel,
        "media-settings-auto-zoom",
        tab_index,
        font_size,
    );
    let auto_play = spawn_box(
        commands,
        panel,
        "media-settings-auto-play",
        tab_index,
        font_size,
    );
    spawn_label(
        commands,
        panel,
        "media-settings-auto-play-note",
        DIM_COLOR,
        font_size,
    );
    let auto_scale = spawn_box(
        commands,
        panel,
        "media-settings-auto-scale",
        tab_index,
        font_size,
    );
    let size_row = spawn_flow_row(commands, panel, 6.0);
    spawn_label(
        commands,
        size_row,
        "media-settings-size-label",
        DIM_COLOR,
        font_size,
    );
    let size_field = |commands: &mut Commands, element: &'static str, tab_index: &mut i32| {
        let field = spawn_text_input(
            commands,
            size_row,
            &TextInputSpec {
                font_size,
                width_glyphs: SIZE_FIELD_GLYPHS,
                tab_index: *tab_index,
                ..TextInputSpec::new(element, TextInputKind::Integer)
            },
        );
        *tab_index = tab_index.saturating_add(1);
        field
    };
    let width_pixels = size_field(commands, "media-settings-width", tab_index);
    spawn_label(
        commands,
        size_row,
        "media-settings-size-by",
        DIM_COLOR,
        font_size,
    );
    let height_pixels = size_field(commands, "media-settings-height", tab_index);
    controls.extend([
        auto_loop,
        first_click_interact,
        auto_zoom,
        auto_play,
        auto_scale,
        width_pixels,
        height_pixels,
    ]);
    GeneralParts {
        home_url,
        home_warning,
        preview,
        current_url,
        auto_loop,
        first_click_interact,
        auto_zoom,
        auto_play,
        auto_scale,
        width_pixels,
        height_pixels,
    }
}

/// The Customize tab's nodes.
struct CustomizeParts {
    /// The controls combo.
    controls: Entity,
    /// The interact boxes.
    perms_interact: [Entity; 3],
    /// The show-controls boxes.
    perms_control: [Entity; 3],
    /// The group's name.
    group_name: Entity,
}

/// Build the Customize tab into `panel`.
fn spawn_customize_tab(
    commands: &mut Commands,
    panel: Entity,
    tab_index: &mut i32,
    font_size: f32,
    controls: &mut Vec<Entity>,
) -> CustomizeParts {
    let controls_row = spawn_flow_row(commands, panel, 8.0);
    spawn_label(
        commands,
        controls_row,
        "media-settings-controls-label",
        DIM_COLOR,
        font_size,
    );
    let labels = [
        "media-settings-controls-standard".to_owned(),
        "media-settings-controls-mini".to_owned(),
    ];
    let combo = spawn_combo(
        commands,
        controls_row,
        &ComboSpec {
            element: "media-settings-controls",
            labels: &labels,
            active: 0,
            tab_index: *tab_index,
            font_size,
            translate_labels: true,
        },
    );
    *tab_index = tab_index.saturating_add(1);
    controls.push(combo);

    let mut group_name = Entity::PLACEHOLDER;
    let mut interact = [Entity::PLACEHOLDER; 3];
    let mut control = [Entity::PLACEHOLDER; 3];
    let headings = [
        "media-settings-perms-owner",
        "media-settings-perms-group",
        "media-settings-perms-anyone",
    ];
    for (index, heading) in headings.into_iter().enumerate() {
        let heading_row = spawn_flow_row(commands, panel, 8.0);
        spawn_label(commands, heading_row, heading, DIM_COLOR, font_size);
        if index == 1 {
            group_name = spawn_line(commands, heading_row, LABEL_COLOR, font_size);
        }
        let boxes = commands
            .spawn((
                Node {
                    margin: UiRect::left(Val::Px(18.0)),
                    ..column(Val::Px(4.0))
                },
                ChildOf(panel),
            ))
            .id();
        let interact_box = spawn_box(
            commands,
            boxes,
            "media-settings-perms-interact",
            tab_index,
            font_size,
        );
        let control_box = spawn_box(
            commands,
            boxes,
            "media-settings-perms-control",
            tab_index,
            font_size,
        );
        if let Some(slot) = interact.get_mut(index) {
            *slot = interact_box;
        }
        if let Some(slot) = control.get_mut(index) {
            *slot = control_box;
        }
        controls.extend([interact_box, control_box]);
    }
    CustomizeParts {
        controls: combo,
        perms_interact: interact,
        perms_control: control,
        group_name,
    }
}

/// The Security tab's nodes.
struct SecurityParts {
    /// The white-list switch.
    whitelist_enable: Entity,
    /// The white-list table.
    table: TableHandle,
    /// The "home page fails this list" warning.
    warning: Entity,
}

/// Build the Security tab into `panel`.
fn spawn_security_tab(
    commands: &mut Commands,
    panel: Entity,
    tab_index: &mut i32,
    font_size: f32,
    controls: &mut Vec<Entity>,
) -> SecurityParts {
    let whitelist_enable = spawn_box(
        commands,
        panel,
        "media-settings-whitelist-enable",
        tab_index,
        font_size,
    );
    let wrapper = commands
        .spawn((
            Node {
                // The panel's width, not a fixed one: a tab panel is capped
                // (`PANEL_MAX_WIDTH`) and padded, and a fixed-width list pokes
                // out of it.
                width: Val::Percent(100.0),
                height: Val::Px(WHITELIST_HEIGHT),
                flex_shrink: 0.0,
                ..default()
            },
            ChildOf(panel),
        ))
        .id();
    let table = spawn_table(commands, wrapper, &WHITELIST_TABLE);
    let note_row = spawn_flow_row(commands, panel, 6.0);
    spawn_label(
        commands,
        note_row,
        "media-settings-whitelist-note",
        DIM_COLOR,
        font_size,
    );
    commands.spawn((
        Text::new(FAILS_MARK),
        UiFont::Sans.at(font_size),
        text_meaning(WARN_COLOR, WARN_TEXT_CLASS),
        Pickable::IGNORE,
        ChildOf(note_row),
    ));
    let buttons = spawn_flow_row(commands, panel, 8.0);
    let add = spawn_settings_button(
        commands,
        buttons,
        "media-settings-whitelist-add",
        MediaSettingsAction::WhitelistAdd,
        tab_index,
        font_size,
    );
    let delete = spawn_settings_button(
        commands,
        buttons,
        "media-settings-whitelist-delete",
        MediaSettingsAction::WhitelistDelete,
        tab_index,
        font_size,
    );
    let warning = spawn_warning(commands, panel, "media-settings-whitelist-fails", font_size);
    controls.extend([whitelist_enable, table.root, add, delete]);
    SecurityParts {
        whitelist_enable,
        table,
        warning,
    }
}

/// Build the Whitelist Entry window into `parent`; returns the pattern field.
fn spawn_whitelist_entry_content(
    commands: &mut Commands,
    parent: Entity,
    font_size: f32,
) -> Entity {
    commands.init_resource::<Messages<WhitelistEntryAction>>();
    let body = commands
        .spawn((
            Node {
                padding: UiRect::all(Val::Px(8.0)),
                max_width: Val::Px(380.0),
                ..column(Val::Px(8.0))
            },
            ChildOf(parent),
        ))
        .id();
    spawn_label(
        commands,
        body,
        "media-whitelist-entry-help",
        LABEL_COLOR,
        font_size,
    );
    let field = spawn_text_input(
        commands,
        body,
        &TextInputSpec {
            font_size,
            width_glyphs: URL_FIELD_GLYPHS,
            tab_index: 0,
            max_characters: Some(1024),
            ..TextInputSpec::new("media-whitelist-entry-field", TextInputKind::Line)
        },
    );
    let buttons = spawn_flow_row(commands, body, 8.0);
    let mut tab_index = 1;
    spawn_entry_button(
        commands,
        buttons,
        "media-whitelist-entry-ok",
        WhitelistEntryAction::Ok,
        &mut tab_index,
        font_size,
    );
    spawn_entry_button(
        commands,
        buttons,
        "media-whitelist-entry-cancel",
        WhitelistEntryAction::Cancel,
        &mut tab_index,
        font_size,
    );
    field
}

/// A press on a Media Settings button.
fn on_media_settings_press(
    press: On<Pointer<Press>>,
    buttons: Query<&MediaSettingsAction, Without<InteractionDisabled>>,
    mut actions: MessageWriter<MediaSettingsAction>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    if let Ok(action) = buttons.get(press.entity) {
        actions.write(*action);
    }
}

/// A press on a Whitelist Entry button.
fn on_whitelist_entry_press(
    press: On<Pointer<Press>>,
    buttons: Query<&WhitelistEntryAction, Without<InteractionDisabled>>,
    mut actions: MessageWriter<WhitelistEntryAction>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    if let Ok(action) = buttons.get(press.entity) {
        actions.write(*action);
    }
}

// ---------------------------------------------------------------------------
// The Media Settings window: gallery specimens.
// ---------------------------------------------------------------------------

/// The Media Settings window's gallery / `ui_test` specimen: the live content,
/// built by the same `spawn_media_settings_content` the viewer's window is,
/// filled with a sample configuration through the same form writer the window
/// fills itself with — a home page, auto-play on, the owner's rights only, and
/// a two-entry white-list whose second entry the home page fails.
pub fn spawn_media_settings_specimen(
    commands: &mut Commands,
    parent: Entity,
    cx: crate::ui_element::ElementCx,
) -> Entity {
    let parts = spawn_media_settings_content(commands, parent, cx.font_size);
    let form = MediaForm {
        home_url: "https://example.com/welcome".to_owned(),
        auto_play: true,
        auto_zoom: true,
        width_pixels: "1024".to_owned(),
        height_pixels: "768".to_owned(),
        perms_interact: [true, false, false],
        perms_control: [true, true, false],
        whitelist_enable: true,
        whitelist: vec!["*.example.com".to_owned(), "secondlife.com".to_owned()],
        ..MediaForm::default()
    };
    let rows: Vec<Vec<String>> = form
        .whitelist
        .iter()
        .map(|entry| vec![whitelist_mark(&form.home_url, entry), entry.clone()])
        .collect();
    sl_viewer_ui_widgets::ui_table::spawn_specimen_table_rows(
        commands,
        sl_viewer_ui_widgets::ui_table::SpecimenTable::from(&parts.whitelist),
        &WHITELIST_TABLE,
        &rows
            .iter()
            .map(|row| {
                row.iter()
                    .map(|value| (value.clone(), WHITELIST_TABLE.cell_color))
                    .collect()
            })
            .collect::<Vec<_>>(),
    );
    commands
        .entity(parts.group_name)
        .insert(Text::new(cx.text("Sample Group")));
    let fields = parts.fields;
    let current_url = parts.current_url;
    commands.queue(move |world: &mut World| {
        if let Err(error) = world.run_system_cached_with(write_form_system, (fields, form)) {
            warn!("media settings specimen: the sample form was not written: {error}");
        }
        let text = (current_url, "https://example.com/welcome".to_owned());
        if let Err(error) = world.run_system_cached_with(write_text_system, text) {
            warn!("media settings specimen: the current page was not written: {error}");
        }
    });
    parent
}

/// The Whitelist Entry window's specimen: the live content, a sample pattern
/// typed in.
pub fn spawn_whitelist_entry_specimen(
    commands: &mut Commands,
    parent: Entity,
    cx: crate::ui_element::ElementCx,
) -> Entity {
    let field = spawn_whitelist_entry_content(commands, parent, cx.font_size);
    commands.queue(move |world: &mut World| {
        let text = (field, "*.example.com".to_owned());
        if let Err(error) = world.run_system_cached_with(write_text_system, text) {
            warn!("whitelist entry specimen: the sample pattern was not written: {error}");
        }
    });
    parent
}

/// The one-shot a specimen writes one field's sample text through.
fn write_text_system(In((field, text)): In<(Entity, String)>, mut writer: FormWriter) {
    writer.text(field, &text);
}

/// The one-shot a specimen writes its sample form through.
fn write_form_system(In((fields, form)): In<(MediaFormFields, MediaForm)>, mut writer: FormWriter) {
    writer.write(&fields, &form);
}

// ---------------------------------------------------------------------------
// The Media Settings window: the form.
// ---------------------------------------------------------------------------

/// Reads a form off its widgets.
#[derive(SystemParam)]
struct FormReader<'w, 's> {
    /// The text fields.
    editors: Query<'w, 's, &'static EditableText>,
    /// The checkboxes.
    boxes: Query<'w, 's, Has<Checked>>,
    /// The combo.
    combos: Query<'w, 's, &'static ComboSelection>,
}

impl FormReader<'_, '_> {
    /// The text of field `entity`.
    fn text(&self, entity: Entity) -> String {
        self.editors
            .get(entity)
            .map(|editor| editor.value().to_string())
            .unwrap_or_default()
    }

    /// Whether box `entity` is ticked.
    fn ticked(&self, entity: Entity) -> bool {
        self.boxes.get(entity).unwrap_or(false)
    }

    /// The form the widgets hold, with `whitelist` as its list.
    fn read(&self, fields: &MediaFormFields, whitelist: &[String]) -> MediaForm {
        MediaForm {
            home_url: self.text(fields.home_url),
            auto_loop: self.ticked(fields.auto_loop),
            first_click_interact: self.ticked(fields.first_click_interact),
            auto_zoom: self.ticked(fields.auto_zoom),
            auto_play: self.ticked(fields.auto_play),
            auto_scale: self.ticked(fields.auto_scale),
            width_pixels: self.text(fields.width_pixels),
            height_pixels: self.text(fields.height_pixels),
            controls: self
                .combos
                .get(fields.controls)
                .map_or(0, |combo| i32::try_from(combo.active).unwrap_or(0)),
            perms_interact: fields.perms_interact.map(|entity| self.ticked(entity)),
            perms_control: fields.perms_control.map(|entity| self.ticked(entity)),
            whitelist_enable: self.ticked(fields.whitelist_enable),
            whitelist: whitelist.to_vec(),
        }
    }
}

/// Writes a form into its widgets.
#[derive(SystemParam)]
struct FormWriter<'w, 's> {
    /// The text fields.
    editors: Query<'w, 's, &'static mut EditableText>,
    /// The checkboxes' state.
    boxes: Query<'w, 's, Has<Checked>>,
    /// The combo.
    combos: Query<'w, 's, &'static mut ComboSelection>,
    /// The fonts a text rewrite relays through.
    font_cx: ResMut<'w, FontCx>,
    /// The layout context a text rewrite relays through.
    layout_cx: ResMut<'w, LayoutCx>,
    /// Where a tick goes on or off.
    commands: Commands<'w, 's>,
}

impl FormWriter<'_, '_> {
    /// The form the widgets hold, with `whitelist` as its list — the
    /// [`FormReader::read`] of a system that also writes.
    fn read(&self, fields: &MediaFormFields, whitelist: &[String]) -> MediaForm {
        let text = |entity: Entity| {
            self.editors
                .get(entity)
                .map(|editor| editor.value().to_string())
                .unwrap_or_default()
        };
        let ticked = |entity: Entity| self.boxes.get(entity).unwrap_or(false);
        MediaForm {
            home_url: text(fields.home_url),
            auto_loop: ticked(fields.auto_loop),
            first_click_interact: ticked(fields.first_click_interact),
            auto_zoom: ticked(fields.auto_zoom),
            auto_play: ticked(fields.auto_play),
            auto_scale: ticked(fields.auto_scale),
            width_pixels: text(fields.width_pixels),
            height_pixels: text(fields.height_pixels),
            controls: self
                .combos
                .get(fields.controls)
                .map_or(0, |combo| i32::try_from(combo.active).unwrap_or(0)),
            perms_interact: fields.perms_interact.map(ticked),
            perms_control: fields.perms_control.map(ticked),
            whitelist_enable: ticked(fields.whitelist_enable),
            whitelist: whitelist.to_vec(),
        }
    }

    /// Write `text` into field `entity`, when it differs.
    fn text(&mut self, entity: Entity, text: &str) {
        if let Ok(mut editor) = self.editors.get_mut(entity)
            && editor.value() != text
        {
            set_editor_text(&mut editor, text, &mut self.font_cx, &mut self.layout_cx);
        }
    }

    /// Tick or untick box `entity`, when it differs.
    fn tick(&mut self, entity: Entity, on: bool) {
        if self.boxes.get(entity).is_ok_and(|ticked| ticked != on) {
            if on {
                self.commands.entity(entity).insert(Checked);
            } else {
                self.commands.entity(entity).remove::<Checked>();
            }
        }
    }

    /// Write `form` into the widgets.
    fn write(&mut self, fields: &MediaFormFields, form: &MediaForm) {
        self.text(fields.home_url, &form.home_url);
        self.tick(fields.auto_loop, form.auto_loop);
        self.tick(fields.first_click_interact, form.first_click_interact);
        self.tick(fields.auto_zoom, form.auto_zoom);
        self.tick(fields.auto_play, form.auto_play);
        self.tick(fields.auto_scale, form.auto_scale);
        self.text(fields.width_pixels, &form.width_pixels);
        self.text(fields.height_pixels, &form.height_pixels);
        let controls = usize::try_from(form.controls).unwrap_or(0);
        if let Ok(mut combo) = self.combos.get_mut(fields.controls)
            && combo.active != controls
        {
            combo.active = controls;
        }
        for (entity, on) in fields.perms_interact.iter().zip(form.perms_interact) {
            self.tick(*entity, on);
        }
        for (entity, on) in fields.perms_control.iter().zip(form.perms_control) {
            self.tick(*entity, on);
        }
        self.tick(fields.whitelist_enable, form.whitelist_enable);
    }
}

/// The mark a white-list row shows: [`FAILS_MARK`] when the home page fails
/// that one entry, nothing otherwise (or with no home page).
fn whitelist_mark(home_url: &str, entry: &str) -> String {
    if home_passes_whitelist(home_url, std::slice::from_ref(&entry.to_owned())) {
        String::new()
    } else {
        FAILS_MARK.to_owned()
    }
}

// ---------------------------------------------------------------------------
// The Media Settings window: systems.
// ---------------------------------------------------------------------------

/// Show the Media Settings window on an [`OpenMediaSettings`], filling it from
/// the selection afresh.
fn open_media_settings(
    mut opens: MessageReader<OpenMediaSettings>,
    floaters: Query<(Entity, &Floater)>,
    mut panels: Query<&mut UiPanelShown>,
    mut state: ResMut<MediaSettingsState>,
) {
    if opens.read().count() == 0 {
        return;
    }
    let panel = floaters
        .iter()
        .find(|(_entity, floater)| floater.id == MEDIA_SETTINGS_FLOATER_ID)
        .map(|(entity, _floater)| entity);
    if let Some(panel) = panel
        && let Ok(mut shown) = panels.get_mut(panel)
    {
        shown.0 = true;
        state.loaded = false;
    }
}

/// Close both windows when the build tools close (the reference's
/// `LLFloaterTools::onClose`), and the Whitelist Entry window with the Media
/// Settings one.
fn close_with_the_build_tools(
    tool: Res<EditToolState>,
    ui: Option<Res<MediaSettingsUi>>,
    entry: Option<Res<WhitelistEntryUi>>,
    mut panels: Query<&mut UiPanelShown>,
) {
    let settings_open = ui
        .as_deref()
        .and_then(|ui| panels.get(ui.panel).ok())
        .is_some_and(|shown| shown.0);
    if !tool.active
        && settings_open
        && let Some(ui) = ui.as_deref()
        && let Ok(mut shown) = panels.get_mut(ui.panel)
    {
        shown.0 = false;
    }
    let settings_open = settings_open && tool.active;
    if !settings_open
        && let Some(entry) = entry.as_deref()
        && let Ok(mut shown) = panels.get_mut(entry.panel)
        && shown.0
    {
        shown.0 = false;
    }
}

/// Fill the form from the selection: on opening, and whenever the selected
/// faces' media changes while the form is unedited.
fn fill_media_settings(
    ui: Option<Res<MediaSettingsUi>>,
    panels: Query<&UiPanelShown>,
    selection: MediaSelection,
    mut state: ResMut<MediaSettingsState>,
    mut writer: FormWriter,
    translator: Translator,
    groups: (Option<Res<GroupsModel>>, MessageWriter<SlCommand>),
) {
    let Some(ui) = ui.as_deref() else {
        return;
    };
    if !panels.get(ui.panel).is_ok_and(|shown| shown.0) {
        return;
    }
    let view = selection.view();
    let editable = selection.editable();
    let multiple = translator.get(MULTIPLE_KEY);
    let current = writer.read(&ui.fields, &state.whitelist);
    let edited = state.loaded && current != state.filled;
    if state.loaded && (view == state.view || edited) {
        state.editable = editable;
        return;
    }
    let form = MediaForm::from_view(&view, &multiple);
    writer.write(&ui.fields, &form);
    writer.text(ui.current_url, &view.current_url.value);
    let group = selection
        .selection
        .primary()
        .and_then(|node| node.properties.as_ref())
        .and_then(|properties| properties.group);
    let (groups, mut names) = groups;
    let group_label = match (group, groups.as_deref()) {
        (Some(group), Some(groups)) => {
            groups.request_name(group, &mut names);
            groups
                .group_name(group)
                .map_or_else(|| group.uuid().to_string(), str::to_owned)
        }
        (Some(group), None) => group.uuid().to_string(),
        (None, _) => String::new(),
    };
    writer
        .commands
        .entity(ui.group_name)
        .insert(Text::new(group_label));
    state.whitelist.clone_from(&form.whitelist);
    state.whitelist_dirty = true;
    state.filled = form;
    state.view = view;
    state.editable = editable;
    state.error = None;
    state.loaded = true;
}

/// The widget state the controls sync writes.
#[derive(SystemParam)]
struct ControlGate<'w, 's> {
    /// Which controls are refused now.
    disabled: Query<'w, 's, (), With<InteractionDisabled>>,
    /// Display toggles for the warnings.
    nodes: Query<'w, 's, &'static mut Node>,
    /// The status line.
    texts: Query<'w, 's, &'static mut Text>,
    /// Where the gates are inserted and removed.
    commands: Commands<'w, 's>,
}

impl ControlGate<'_, '_> {
    /// Refuse or allow `entity`.
    fn allow(&mut self, entity: Entity, enabled: bool) {
        if self.disabled.contains(entity) == enabled {
            if enabled {
                self.commands.entity(entity).remove::<InteractionDisabled>();
            } else {
                self.commands.entity(entity).insert(InteractionDisabled);
            }
        }
    }

    /// Show or hide `entity`.
    fn show(&mut self, entity: Entity, shown: bool) {
        let display = if shown { Display::Flex } else { Display::None };
        if let Ok(mut node) = self.nodes.get_mut(entity)
            && node.display != display
        {
            node.display = display;
        }
    }
}

/// Keep the window's gates in step with the form: every control refused while
/// the selection may not be edited, the size fields while auto-scale is on, the
/// white-list switch (and the warnings) while the home page fails the list, and
/// the error line.
fn sync_media_settings_controls(
    ui: Option<Res<MediaSettingsUi>>,
    panels: Query<&UiPanelShown>,
    mut state: ResMut<MediaSettingsState>,
    reader: FormReader,
    translator: Translator,
    mut gate: ControlGate,
) {
    let Some(ui) = ui.as_deref() else {
        return;
    };
    if !panels.get(ui.panel).is_ok_and(|shown| shown.0) {
        return;
    }
    let form = reader.read(&ui.fields, &state.whitelist);
    for &control in &ui.controls {
        gate.allow(control, state.editable);
    }
    let sized = state.editable && !form.auto_scale;
    gate.allow(ui.fields.width_pixels, sized);
    gate.allow(ui.fields.height_pixels, sized);

    // A mixed white-list cannot be checked against: pass it, as the
    // reference's `urlPassesWhiteList` does for a tentative list.
    let passes = state.view.whitelist.mixed && form.whitelist == state.filled.whitelist
        || home_passes_whitelist(&form.home_url, &form.whitelist);
    gate.show(ui.home_warning, !passes && form.whitelist_enable);
    gate.show(ui.whitelist_warning, !passes);
    gate.allow(ui.fields.whitelist_enable, state.editable && passes);
    if !passes && form.whitelist_enable {
        // The reference turns the switch off while the home page fails the
        // list ("it has been disabled until a valid entry has been added").
        gate.commands
            .entity(ui.fields.whitelist_enable)
            .remove::<Checked>();
    }
    let status = match state.error {
        None => String::new(),
        Some(FormError::HomeUrl) => translator.get("media-settings-error-home-url"),
        Some(FormError::Size) => translator.get("media-settings-error-size"),
    };
    if let Ok(mut text) = gate.texts.get_mut(ui.status)
        && text.0 != status
    {
        text.0 = status;
    }
    // A mark per row follows the home page as it is typed.
    if form.home_url != state.filled.home_url && !state.whitelist.is_empty() {
        state.whitelist_dirty = true;
    }
}

/// Everything an Apply writes to.
#[derive(SystemParam)]
struct MediaWrites<'w> {
    /// The protocol commands.
    commands: MessageWriter<'w, SlCommand>,
    /// The running surfaces (Reset navigates them home).
    live: Option<Res<'w, MediaPrimState>>,
    /// Where an applied face is asked to start playing.
    starts: Option<ResMut<'w, MediaStartRequests>>,
}

/// Act on the Media Settings buttons.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its data access"
)]
fn handle_media_settings_actions(
    mut actions: MessageReader<MediaSettingsAction>,
    ui: Option<Res<MediaSettingsUi>>,
    entry: Option<Res<WhitelistEntryUi>>,
    selection: MediaSelection,
    mut state: ResMut<MediaSettingsState>,
    reader: FormReader,
    translator: Translator,
    mut writes: MediaWrites,
    surfaces: Option<NonSend<MediaSurfaces>>,
    mut panels: Query<&mut UiPanelShown>,
    mut tables: Query<&mut TableState>,
    floaters: Query<(Entity, &Floater)>,
) {
    let Some(ui) = ui.as_deref() else {
        actions.clear();
        return;
    };
    for action in actions.read() {
        match action {
            MediaSettingsAction::Ok | MediaSettingsAction::Apply => {
                let form = reader.read(&ui.fields, &state.whitelist);
                let multiple = translator.get(MULTIPLE_KEY);
                match form_edit(&state.view, &form, &multiple) {
                    Ok(edit) => {
                        let edited = apply_media_edit(&selection, &edit, &mut writes.commands);
                        // Show the media just set up — while the build tools
                        // are open a click on the face selects it instead.
                        if let Some(starts) = writes.starts.as_mut() {
                            for target in edited {
                                starts.request(target);
                            }
                        }
                        state.error = None;
                        // The applied form is the new baseline: the reply that
                        // follows re-reads the selection into it.
                        state.filled = form;
                        state.loaded = false;
                        if *action == MediaSettingsAction::Ok
                            && let Ok(mut shown) = panels.get_mut(ui.panel)
                        {
                            shown.0 = false;
                        }
                    }
                    Err(error) => state.error = Some(error),
                }
            }
            MediaSettingsAction::Cancel => {
                if let Ok(mut shown) = panels.get_mut(ui.panel) {
                    shown.0 = false;
                }
            }
            MediaSettingsAction::Reset => {
                reset_to_home(
                    &selection,
                    writes.live.as_deref(),
                    surfaces.as_deref(),
                    &mut writes.commands,
                );
            }
            MediaSettingsAction::WhitelistAdd => {
                let panel = entry.as_deref().map(|entry| entry.panel).or_else(|| {
                    floaters
                        .iter()
                        .find(|(_entity, floater)| floater.id == WHITELIST_ENTRY_FLOATER_ID)
                        .map(|(entity, _floater)| entity)
                });
                if let Some(panel) = panel
                    && let Ok(mut shown) = panels.get_mut(panel)
                {
                    shown.0 = true;
                }
            }
            MediaSettingsAction::WhitelistDelete => {
                if let Ok(mut table) = tables.get_mut(ui.whitelist.root)
                    && let Some(&index) = table.selected().first()
                    && index < state.whitelist.len()
                {
                    state.whitelist.remove(index);
                    state.whitelist_dirty = true;
                    table.clear_selection();
                }
            }
        }
    }
}

/// Reset: clear the selected faces' current page — so they start again from
/// the home page — and send each running surface there now (the reference's
/// `navigateHomeSelectedFace(false)`).
fn reset_to_home(
    selection: &MediaSelection,
    live: Option<&MediaPrimState>,
    surfaces: Option<&MediaSurfaces>,
    commands: &mut MessageWriter<SlCommand>,
) {
    let edit = MediaEdit {
        current_url: UrlEdit::Write(None),
        ..MediaEdit::default()
    };
    for object in selection.objects() {
        if !object.editable {
            continue;
        }
        let result = edit_object_media(&object.media, &object.selected, &edit);
        if result.flagged.is_empty() {
            continue;
        }
        commands.write(SlCommand(Command::SetObjectMedia {
            object_id: object.full,
            faces: result.faces,
        }));
        let (Some(live), Some(surfaces)) = (live, surfaces) else {
            continue;
        };
        for index in result.flagged {
            let Some(home) = object
                .media
                .get(index)
                .and_then(Option::as_ref)
                .and_then(|entry| entry.home_url.as_ref())
                .and_then(|home| ValidatedMediaUrl::from_url(home).ok())
            else {
                continue;
            };
            let Ok(face) = u16::try_from(index) else {
                continue;
            };
            let target = MediaTarget {
                object: object.full,
                face: PrimFaceId::new(face),
            };
            if let Some(slot) = live
                .active
                .get(&target)
                .and_then(|active| surfaces.get(active.surface))
            {
                slot.surface.navigate(&home);
            }
        }
    }
}

/// Act on the Whitelist Entry buttons: OK adds the typed pattern to the form's
/// list (the reference's `addWhiteListEntry`) and closes; Cancel closes.
fn handle_whitelist_entry_actions(
    mut actions: MessageReader<WhitelistEntryAction>,
    entry: Option<Res<WhitelistEntryUi>>,
    mut state: ResMut<MediaSettingsState>,
    mut editors: Query<&mut EditableText>,
    mut panels: Query<&mut UiPanelShown>,
    mut text_cx: (ResMut<FontCx>, ResMut<LayoutCx>),
) {
    let Some(entry) = entry.as_deref() else {
        actions.clear();
        return;
    };
    for action in actions.read() {
        if *action == WhitelistEntryAction::Ok
            && let Ok(mut editor) = editors.get_mut(entry.field)
        {
            let pattern = editor.value().to_string().trim().to_owned();
            if !pattern.is_empty() {
                state.whitelist.push(pattern);
                state.whitelist_dirty = true;
            }
            let (font_cx, layout_cx) = &mut text_cx;
            set_editor_text(&mut editor, "", font_cx, layout_cx);
        }
        if let Ok(mut shown) = panels.get_mut(entry.panel) {
            shown.0 = false;
        }
    }
}

/// Keep the white-list table's row count in step with the form's list.
fn sync_whitelist_rows(
    ui: Option<Res<MediaSettingsUi>>,
    state: Res<MediaSettingsState>,
    mut lists: Query<&mut VirtualList>,
) {
    let Some(ui) = ui.as_deref() else {
        return;
    };
    let count = state.whitelist.len();
    if let Ok(mut list) = lists.get_mut(ui.whitelist.viewport)
        && list.item_count != count
    {
        list.item_count = count;
    }
}

/// Give each pooled white-list row its cells the first time it is spawned.
fn populate_whitelist_rows(
    mut commands: Commands,
    ui: Option<Res<MediaSettingsUi>>,
    new_rows: Query<(Entity, &ChildOf), Added<VirtualRow>>,
) {
    let Some(ui) = ui.as_deref() else {
        return;
    };
    for (row_entity, child_of) in &new_rows {
        if child_of.parent() == ui.whitelist.viewport {
            spawn_table_row(
                &mut commands,
                row_entity,
                ui.whitelist.root,
                &WHITELIST_TABLE,
            );
        }
    }
}

/// Write each visible white-list row's mark and pattern into its cells.
fn bind_whitelist_rows(
    ui: Option<Res<MediaSettingsUi>>,
    mut state: ResMut<MediaSettingsState>,
    reader: FormReader,
    rows: Query<(Ref<VirtualRow>, &ChildOf, &TableRowCells)>,
    mut texts: Query<(&mut Text, &mut TextColor, Option<&mut ClassList>)>,
) {
    let Some(ui) = ui.as_deref() else {
        return;
    };
    let refresh = state.whitelist_dirty;
    state.whitelist_dirty = false;
    let home_url = reader.text(ui.fields.home_url);
    for (row, child_of, cells) in &rows {
        if child_of.parent() != ui.whitelist.viewport || (!refresh && !row.is_changed()) {
            continue;
        }
        let entry = row
            .index
            .and_then(|index| state.whitelist.get(index))
            .cloned()
            .unwrap_or_default();
        let mark = if entry.is_empty() {
            String::new()
        } else {
            whitelist_mark(&home_url, &entry)
        };
        if let Some(cell) = cells.cell(0) {
            set_table_cell(&mut texts, cell, &mark, WARN_COLOR);
        }
        if let Some(cell) = cells.cell(1) {
            set_table_cell(&mut texts, cell, &entry, LABEL_COLOR);
        }
    }
}

/// Point the preview at the home page as it stands, muted — the reference's
/// `updateMediaPreview`, which sets the preview's volume to zero so a page
/// with a movie does not play into the room.
fn drive_media_preview(
    ui: Option<Res<MediaSettingsUi>>,
    panels: Query<&UiPanelShown>,
    mut state: ResMut<MediaSettingsState>,
    reader: FormReader,
    views: Query<&BrowserView>,
    surfaces: Option<NonSend<MediaSurfaces>>,
    translator: Translator,
) {
    let (Some(ui), Some(surfaces)) = (ui.as_deref(), surfaces) else {
        return;
    };
    if !panels.get(ui.panel).is_ok_and(|shown| shown.0) {
        return;
    }
    let Some(slot) = views
        .get(ui.preview)
        .ok()
        .and_then(|view| view.surface)
        .and_then(|id| surfaces.get(id))
    else {
        return;
    };
    let home = reader.text(ui.fields.home_url);
    if state.previewed.as_deref() == Some(home.as_str()) {
        return;
    }
    if !slot.surface.muted() {
        slot.surface.set_muted(true);
    }
    let target = if home == translator.get(MULTIPLE_KEY) {
        None
    } else {
        parse_media_url(&home)
            .ok()
            .flatten()
            .and_then(|url| ValidatedMediaUrl::from_url(&url).ok())
    };
    slot.surface
        .navigate(&target.unwrap_or_else(ValidatedMediaUrl::blank));
    state.previewed = Some(home);
}

#[cfg(test)]
mod tests {
    use bevy::app::App;
    use bevy::ecs::message::Messages;
    use bevy::prelude::{ChildOf, Entity, MessageWriter, Update};
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{
        CircuitId, Command, MediaEntry, ObjectKey, PrimFaceId, RegionLocalObjectId, ScopedObjectId,
        SlCommand, TextureFace, TextureKey, Uuid,
    };

    use super::{MediaSelection, UrlEdit, apply_media_edit, remove_selected_media};
    use crate::edit_media_model::MediaEdit;
    use crate::objects::{FaceTextureDebug, PrimFaceEntity};
    use crate::world_api::{ObjectState, SelectionSet};

    /// The test object's face count.
    const FACE_COUNT: u16 = 3;

    /// The test object's region-scoped id.
    const fn scoped() -> ScopedObjectId {
        ScopedObjectId {
            circuit: CircuitId::new(1),
            id: RegionLocalObjectId(7),
        }
    }

    /// The test object's key.
    fn full() -> ObjectKey {
        ObjectKey::from(Uuid::from_u128(7))
    }

    /// An app holding one three-face object whose face `flagged` (if any)
    /// carries the texture-entry media flag, with face `selected` selected.
    fn app_with_object(flagged: Option<u16>, selected: u16) -> App {
        let mut app = App::new();
        app.add_message::<SlCommand>()
            .init_resource::<ObjectState>();
        let object = app.world_mut().spawn_empty().id();
        let geometry = app.world_mut().spawn(ChildOf(object)).id();
        for face_id in 0..FACE_COUNT {
            let mut face = TextureFace::new(TextureKey::from(Uuid::nil()));
            if Some(face_id) == flagged {
                face.media_flags |= 0x01;
            }
            let _face: Entity = app
                .world_mut()
                .spawn((
                    PrimFaceEntity {
                        face_id: PrimFaceId::new(face_id),
                    },
                    FaceTextureDebug(face),
                    ChildOf(geometry),
                ))
                .id();
        }
        let mut selection = SelectionSet::default();
        selection.select_only_face(scoped(), full(), object, PrimFaceId::new(selected));
        app.insert_resource(selection);
        app
    }

    /// The commands the last update sent.
    fn sent(app: &App) -> Vec<Command> {
        let messages = app.world().resource::<Messages<SlCommand>>();
        let mut cursor = messages.get_cursor();
        cursor
            .read(messages)
            .map(|command| command.0.clone())
            .collect()
    }

    /// An Apply that sets a home page on a bare face flags that face in the
    /// texture entry **first**, then sends the object's whole media — the
    /// reference's order, the one a removal-by-texture-update also relies on.
    #[test]
    fn an_apply_flags_the_face_then_sends_the_objects_media() {
        /// Apply a home page to the selection.
        fn apply(selection: MediaSelection, mut commands: MessageWriter<SlCommand>) {
            let edit = MediaEdit {
                home_url: UrlEdit::Write(url::Url::parse("https://example.com/").ok()),
                ..MediaEdit::default()
            };
            let _edited = apply_media_edit(&selection, &edit, &mut commands);
        }
        let mut app = app_with_object(None, 1);
        app.add_systems(Update, apply);
        app.update();
        let sent = sent(&app);
        let flags: Vec<Vec<bool>> = sent
            .iter()
            .filter_map(|command| match command {
                Command::SetObjectImage { texture_entry, .. } => Some(
                    texture_entry
                        .faces
                        .iter()
                        .map(|face| face.media_enabled())
                        .collect(),
                ),
                _other => None,
            })
            .collect();
        assert_eq!(flags, vec![vec![false, true, false]]);
        let media: Vec<(ObjectKey, Vec<Option<MediaEntry>>)> = sent
            .iter()
            .filter_map(|command| match command {
                Command::SetObjectMedia { object_id, faces } => Some((*object_id, faces.clone())),
                _other => None,
            })
            .collect();
        assert_eq!(
            media,
            vec![(
                full(),
                vec![
                    None,
                    Some(MediaEntry {
                        home_url: url::Url::parse("https://example.com/").ok(),
                        ..MediaEntry::default()
                    }),
                    None,
                ]
            )]
        );
        assert!(
            matches!(sent.first(), Some(Command::SetObjectImage { .. })),
            "the flag goes out before the media"
        );
    }

    /// An edit that does not set a home page gives a bare face nothing, so
    /// nothing is sent at all.
    #[test]
    fn an_edit_without_a_home_page_leaves_a_bare_face_alone() {
        /// Apply an auto-play change to the selection.
        fn apply(selection: MediaSelection, mut commands: MessageWriter<SlCommand>) {
            let edit = MediaEdit {
                auto_play: Some(true),
                ..MediaEdit::default()
            };
            let _edited = apply_media_edit(&selection, &edit, &mut commands);
        }
        let mut app = app_with_object(None, 0);
        app.add_systems(Update, apply);
        app.update();
        assert!(sent(&app).is_empty(), "nothing is sent");
    }

    /// Removing the last face's media is the texture-entry update alone: the
    /// flag is cleared and no media update follows, since there is nothing left
    /// to send.
    #[test]
    fn removing_the_last_media_is_the_texture_update_alone() {
        /// Remove the selection's media.
        fn remove(selection: MediaSelection, mut commands: MessageWriter<SlCommand>) {
            remove_selected_media(&selection, &mut commands);
        }
        let mut app = app_with_object(Some(2), 2);
        app.add_systems(Update, remove);
        app.update();
        let sent = sent(&app);
        assert_eq!(sent.len(), 1, "one ObjectImage and nothing else");
        let cleared = sent.first().is_some_and(|command| match command {
            Command::SetObjectImage { texture_entry, .. } => {
                texture_entry.faces.iter().all(|face| !face.media_enabled())
            }
            _other => false,
        });
        assert!(cleared, "the face's media flag is cleared");
    }
}
