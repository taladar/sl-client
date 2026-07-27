//! The **notecard viewer & editor** floater (`viewer-notecard-editor`): open a
//! notecard from inventory, read it, edit its text when permitted, and save it
//! back to agent inventory.
//!
//! A notecard is *not* plain text — the asset is a Linden-text container
//! carrying the prose **plus embedded inventory items** (landmarks, objects,
//! other notecards a resident drops inline). The pure [`sl_notecard`] crate
//! decodes and re-encodes that container; this module is the widget over it.
//!
//! # What this surface does today, and what waits on the rich-text widget
//!
//! The reference viewer renders each embedded item as a **clickable inline
//! box** in the text flow (`llviewertexteditor`'s embedded-item machinery). That
//! needs a rich-text editor with inline boxes and per-range brushes — the parley
//! `PlainEditor` fork tracked by [`viewer-lsl-editor-widget`], which is not yet
//! built. Until it lands, this editor:
//!
//! - edits the notecard **text** in the reusable multi-line field
//!   ([`crate::ui_text_input`], a stock `EditableText`), preserving each embedded
//!   item's private-use marker code point in the buffer so a round-trip never
//!   corrupts or orphans an item ([`sl_notecard::Notecard::with_edited_text`]
//!   reconciles the item table against the markers on save);
//! - lists the embedded items below the body (icon + name + type) so the reader
//!   sees what the notecard carries, rather than rendering them inline;
//! - saves back to **agent** inventory over `UpdateNotecardAgentInventory` or,
//!   for a notecard opened from a prim's contents, to that object's **task**
//!   inventory over `UpdateNotecardTaskInventory` — one
//!   [`Command::UpdateInventoryAsset`] whose [`NotecardSource`] picks the
//!   capability and the "opened-from-task" provenance the reference carries.
//!
//! Deferred to their own tasks (all needing machinery this task does not build):
//! inline clickable items and drag-and-drop *adding* of items
//! ([`viewer-lsl-editor-widget`]'s inline boxes + the inventory folder tree) and
//! URL / SLURL linkification ([`viewer-url-linkification`]).
//!
//! # Read-only when you cannot modify
//!
//! Editability is gated on the item's owner mask carrying `MODIFY` (the
//! reference's `LLPreviewNotecard::canModify`). A no-modify notecard — a freebie
//! someone handed you — opens as a read-only text block with a note and no Save
//! button, so its text is never presented as editable when a save would be
//! refused.
//!
//! Reference (Firestorm, read-only): `llpreviewnotecard`, `llfloaternotecard`,
//! `llviewertexteditor`.

use bevy::prelude::*;
use bevy::text::EditableText;
use sl_client_bevy::{
    AssetKey, AssetType, AssetUpdateLocation, Command, InventoryKey, ObjectKey, SlCommand, SlEvent,
    SlSessionEvent, UpdatableAssetType, Uuid,
};

use crate::floater::{FloaterCaps, FloaterSpec, spawn_floater};
use crate::ui::{UiPanelShown, UiRoot, UiScaffoldSystems, column, row};
use crate::ui_element::ElementCx;
use crate::ui_font::UiFont;

/// The editor's text font size, in logical pixels.
const FONT_SIZE: f32 = 14.0;

/// A general-purpose light label colour.
const LABEL_COLOR: Color = Color::srgb(0.90, 0.92, 0.96);

/// A dimmer colour for secondary text (the read-only note, the item list).
const DIM_COLOR: Color = Color::srgb(0.62, 0.66, 0.74);

/// A red-tinted colour for a failed-save status.
const ERROR_COLOR: Color = Color::srgb(0.92, 0.55, 0.50);

/// The body field's height, in visible text lines.
const BODY_VISIBLE_LINES: f32 = 18.0;

/// The read-only body block's viewport height, in logical pixels.
const READONLY_BODY_HEIGHT: f32 = 320.0;

/// The read-only body block's width bound, in logical pixels.
const READONLY_BODY_WIDTH: f32 = 460.0;

// ---------------------------------------------------------------------------
// Messages.
// ---------------------------------------------------------------------------

/// Where the notecard being edited lives — the agent's own inventory, or an
/// in-world object's task inventory. Carried through the editor so Save writes
/// back to the right place (the reference's "opened-from-task" provenance).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NotecardSource {
    /// A notecard in the agent's own inventory.
    Agent {
        /// The agent-inventory item.
        item_id: InventoryKey,
    },
    /// A notecard inside an in-world object's task inventory.
    Task {
        /// The object (task) holding the notecard.
        task_id: ObjectKey,
        /// The notecard item within that object's inventory.
        item_id: InventoryKey,
    },
}

impl NotecardSource {
    /// The notecard item's own id, whichever inventory it lives in.
    const fn item_id(self) -> InventoryKey {
        match self {
            Self::Agent { item_id } | Self::Task { item_id, .. } => item_id,
        }
    }

    /// The asset-update location this source saves back to.
    const fn location(self) -> AssetUpdateLocation {
        match self {
            Self::Agent { item_id } => AssetUpdateLocation::AgentInventory { item_id },
            Self::Task { task_id, item_id } => {
                AssetUpdateLocation::TaskInventory { task_id, item_id }
            }
        }
    }
}

/// Open the notecard editor on a notecard. Written by the inventory **Open**
/// action (routed here from [`crate::inventory_properties`]) and by the Object
/// Contents floater's Open ([`crate::edit_contents`]) for a task-inventory
/// notecard.
#[derive(Message, Debug, Clone)]
pub(crate) struct OpenNotecard {
    /// The notecard's name, shown as the floater title.
    pub(crate) name: String,
    /// The notecard asset to fetch and show.
    pub(crate) asset_id: Uuid,
    /// Whether the notecard is editable (the caller applies the right
    /// permission rule: an agent item's own modify bit, or an object's modify
    /// **and** the item's modify bit for a task notecard).
    pub(crate) editable: bool,
    /// Where the notecard lives, so Save writes back to the right place.
    pub(crate) source: NotecardSource,
}

// ---------------------------------------------------------------------------
// Plugin, resources.
// ---------------------------------------------------------------------------

/// The plugin owning the notecard editor floater.
pub(crate) struct EditNotecardPlugin;

impl Plugin for EditNotecardPlugin {
    /// Register the open message and state, and spawn the (hidden) floater.
    fn build(&self, app: &mut App) {
        app.init_resource::<NotecardEditorState>()
            .add_message::<OpenNotecard>()
            .add_systems(
                Startup,
                spawn_notecard_floater.after(UiScaffoldSystems::SpawnRoot),
            )
            .add_systems(
                Update,
                (open_notecard, ingest_notecard_asset, report_notecard_save).chain(),
            );
    }
}

/// The notecard editor floater's entities and live state.
#[derive(Resource, Debug, Default)]
struct NotecardEditorState {
    /// The floater root (carries [`UiPanelShown`]).
    panel: Option<Entity>,
    /// The rebuilt-per-open content column.
    content: Option<Entity>,
    /// The title-bar text node (set to the notecard's name on open).
    title_text: Option<Entity>,
    /// Where the notecard currently shown lives (Save target), set on open.
    source: Option<NotecardSource>,
    /// Whether the notecard currently shown is editable, set on open.
    editable: bool,
    /// The originally decoded notecard, kept as the baseline the edited text's
    /// embedded-item markers resolve against on save.
    baseline: Option<sl_notecard::Notecard>,
    /// The asset id awaited (`FetchAsset` sent), matched on `AssetReceived`.
    pending_load: Option<Uuid>,
    /// The item id of an in-flight save, matched on the upload result.
    pending_save: Option<Uuid>,
    /// The editable body field, when the notecard is modifiable.
    body_field: Option<Entity>,
    /// The status text node (loading / saving / result), when present.
    status: Option<Entity>,
}

/// Spawn the notecard editor floater, hidden, and stash its handles.
fn spawn_notecard_floater(mut commands: Commands, root: Res<UiRoot>) {
    let handle = spawn_floater(
        &mut commands,
        root.0,
        FloaterSpec {
            id: "notecard-editor",
            title: "Notecard".to_owned(),
            position: Vec2::new(400.0, 100.0),
            default_size: Some(Vec2::new(READONLY_BODY_WIDTH, READONLY_BODY_HEIGHT + 60.0)),
            min_size: Some(Vec2::new(260.0, 160.0)),
            dock_host: None,
            caps: FloaterCaps {
                resizable: true,
                minimizable: true,
                closable: true,
                dockable: false,
            },
        },
    );
    // Subject-bound: the shown notecard is not persisted, so neither is the
    // floater's position (matching the previews / properties floater).
    commands
        .entity(handle.root)
        .insert(crate::floater_persist::FloaterPersistExempt);
    commands.insert_resource(NotecardEditorState {
        panel: Some(handle.root),
        content: Some(handle.content),
        title_text: Some(handle.title_text),
        ..NotecardEditorState::default()
    });
}

// ---------------------------------------------------------------------------
// Open → fetch.
// ---------------------------------------------------------------------------

/// Open the editor on the newest [`OpenNotecard`]: clear the content, show a
/// loading line, request the asset, and reveal the floater.
fn open_notecard(
    mut opens: MessageReader<OpenNotecard>,
    mut state: ResMut<NotecardEditorState>,
    children: Query<&Children>,
    mut texts: Query<&mut Text>,
    mut panels: Query<&mut UiPanelShown>,
    mut commands: Commands,
    mut sl_commands: MessageWriter<SlCommand>,
) {
    let Some(open) = opens.read().last().cloned() else {
        return;
    };
    let (Some(content), Some(panel)) = (state.content, state.panel) else {
        return;
    };

    // Set the title to the notecard's name (the title node carries no Fluent
    // key, so a direct text set is not fought by the translator).
    if let Some(title) = state.title_text
        && let Ok(mut text) = texts.get_mut(title)
    {
        open.name.clone_into(&mut text.0);
    }

    tear_down(&mut commands, &children, content);
    let status = spawn_status(&mut commands, content, "notecard-status-loading", DIM_COLOR);

    state.pending_load = Some(open.asset_id);
    state.pending_save = None;
    state.body_field = None;
    state.status = Some(status);
    state.baseline = None;
    state.source = Some(open.source);
    state.editable = open.editable;

    sl_commands.write(SlCommand(Command::FetchAsset {
        asset_id: AssetKey::from(open.asset_id),
        asset_type: AssetType::Notecard,
        byte_range: None,
    }));
    if let Ok(mut shown) = panels.get_mut(panel) {
        shown.0 = true;
    }
}

// ---------------------------------------------------------------------------
// Asset received → build the editor.
// ---------------------------------------------------------------------------

/// Fold the fetched notecard asset into the editor once it arrives: decode it,
/// then build the read-only or editable body and the embedded-item list.
fn ingest_notecard_asset(
    mut events: MessageReader<SlEvent>,
    mut state: ResMut<NotecardEditorState>,
    children: Query<&Children>,
    mut commands: Commands,
) {
    for event in events.read() {
        let SlSessionEvent::AssetReceived(asset) = &event.0 else {
            continue;
        };
        if state.pending_load != Some(asset.id) {
            continue;
        }
        state.pending_load = None;
        let (Some(content), Some(source)) = (state.content, state.source) else {
            continue;
        };

        let notecard = match sl_notecard::Notecard::decode(&asset.data) {
            Ok(notecard) => notecard,
            Err(error) => {
                warn!("failed to decode notecard {}: {error}", asset.id);
                tear_down(&mut commands, &children, content);
                let status = spawn_status(
                    &mut commands,
                    content,
                    "notecard-status-decode-failed",
                    ERROR_COLOR,
                );
                state.status = Some(status);
                state.body_field = None;
                state.baseline = None;
                continue;
            }
        };

        let editable = state.editable;
        tear_down(&mut commands, &children, content);
        let built = populate_editor(
            &mut commands,
            content,
            &notecard,
            editable,
            editable.then_some(source),
            FONT_SIZE,
        );
        state.body_field = built.body_field;
        state.status = built.status;
        state.baseline = Some(notecard);
    }
}

/// The entities [`populate_editor`] hands back to the live state.
#[derive(Debug, Default, Clone, Copy)]
struct BuiltEditor {
    /// The editable body field, when the notecard is modifiable.
    body_field: Option<Entity>,
    /// The status text node, when the editor offers a Save button.
    status: Option<Entity>,
}

/// Build the editor's content under `content`: the body (editable field or
/// read-only block), the embedded-item list, and — when editable — a Save
/// button wired to `save_target` plus a status line.
///
/// `save_target` is where to write back to, or `None` for a specimen that has
/// no live notecard (the Save button is shown for layout but does nothing).
fn populate_editor(
    commands: &mut Commands,
    content: Entity,
    notecard: &sl_notecard::Notecard,
    editable: bool,
    save_target: Option<NotecardSource>,
    font_size: f32,
) -> BuiltEditor {
    if !editable {
        spawn_note(commands, content, "notecard-readonly-note", font_size);
    }

    let body_field = if editable {
        Some(spawn_body_field(
            commands,
            content,
            &notecard.text,
            font_size,
        ))
    } else {
        spawn_readonly_body(commands, content, &notecard.text, font_size);
        None
    };

    spawn_embedded_list(commands, content, notecard, font_size);

    let status = editable.then(|| {
        let bar = commands
            .spawn((
                Node {
                    ..row(Val::Px(8.0))
                },
                ChildOf(content),
            ))
            .id();
        let save = spawn_save_button(commands, bar, font_size);
        // The status node sits after the Save button, empty until a save runs.
        let status = commands
            .spawn((
                Text::default(),
                UiFont::Sans.at(font_size),
                TextColor(DIM_COLOR),
                ChildOf(bar),
            ))
            .id();
        if let (Some(target), Some(field)) = (save_target, body_field) {
            attach_save(commands, save, target, field, status);
        }
        status
    });

    BuiltEditor { body_field, status }
}

/// Wire a Save button to reconcile the edited text against the baseline and
/// write it back over the source's `Update*Inventory` capability (agent or
/// task, per [`NotecardSource`]).
fn attach_save(
    commands: &mut Commands,
    button: Entity,
    source: NotecardSource,
    body_field: Entity,
    status: Entity,
) {
    commands.entity(button).observe(
        move |press: On<Pointer<Press>>,
              fields: Query<&EditableText>,
              mut state: ResMut<NotecardEditorState>,
              mut sl_commands: MessageWriter<SlCommand>,
              mut commands: Commands| {
            if press.button != PointerButton::Primary {
                return;
            }
            let Ok(field) = fields.get(body_field) else {
                return;
            };
            // Borrow the baseline just long enough to reconcile, then release it
            // before mutating the state below.
            let edited = field.value().to_string();
            let data = {
                let Some(baseline) = state.baseline.as_ref() else {
                    return;
                };
                baseline.with_edited_text(&edited).encode()
            };
            sl_commands.write(SlCommand(Command::UpdateInventoryAsset {
                location: source.location(),
                asset_type: UpdatableAssetType::Notecard,
                data,
            }));
            state.pending_save = Some(source.item_id().uuid());
            set_status(&mut commands, status, "notecard-status-saving", DIM_COLOR);
        },
    );
}

// ---------------------------------------------------------------------------
// Save result.
// ---------------------------------------------------------------------------

/// Report the outcome of an in-flight notecard save. A notecard save is
/// user-triggered one at a time, so the next terminal upload event while a save
/// is pending is treated as its result.
fn report_notecard_save(
    mut events: MessageReader<SlEvent>,
    mut state: ResMut<NotecardEditorState>,
    mut commands: Commands,
) {
    for event in events.read() {
        let Some(pending) = state.pending_save else {
            continue;
        };
        match &event.0 {
            SlSessionEvent::AssetUploaded {
                new_inventory_item, ..
            } => {
                // Prefer the returned item id; a save reports the item it wrote,
                // so a mismatching upload (a baked texture, another floater's
                // asset) is not ours. A `None` item id is accepted as ours
                // rather than leaving the status stuck on "saving".
                if matches!(new_inventory_item, Some(id) if *id != pending) {
                    continue;
                }
                state.pending_save = None;
                if let Some(status) = state.status {
                    set_status(&mut commands, status, "notecard-status-saved", DIM_COLOR);
                }
            }
            SlSessionEvent::AssetUploadFailed { reason } => {
                warn!("notecard save failed: {reason}");
                state.pending_save = None;
                if let Some(status) = state.status {
                    set_status(
                        &mut commands,
                        status,
                        "notecard-status-save-failed",
                        ERROR_COLOR,
                    );
                }
            }
            _other => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Content builders.
// ---------------------------------------------------------------------------

/// Despawn every child of `parent`.
fn tear_down(commands: &mut Commands, children: &Query<&Children>, parent: Entity) {
    if let Ok(existing) = children.get(parent) {
        for child in existing.iter().collect::<Vec<_>>() {
            commands.entity(child).despawn();
        }
    }
}

/// Spawn a fresh status line under `parent`, driven by a Fluent key.
fn spawn_status(
    commands: &mut Commands,
    parent: Entity,
    key: &'static str,
    color: Color,
) -> Entity {
    commands
        .spawn((
            Text::default(),
            crate::i18n::Translated::new(key),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(color),
            ChildOf(parent),
        ))
        .id()
}

/// Repoint an existing status node at a new Fluent key and colour.
fn set_status(commands: &mut Commands, status: Entity, key: &'static str, color: Color) {
    commands
        .entity(status)
        .insert((crate::i18n::Translated::new(key), TextColor(color)));
}

/// Spawn the read-only note shown above a no-modify notecard's text.
fn spawn_note(commands: &mut Commands, parent: Entity, key: &'static str, font_size: f32) {
    commands.spawn((
        Text::default(),
        crate::i18n::Translated::new(key),
        UiFont::Sans.at(font_size),
        TextColor(DIM_COLOR),
        ChildOf(parent),
    ));
}

/// Spawn the editable multi-line body field, returning its entity.
fn spawn_body_field(commands: &mut Commands, parent: Entity, text: &str, font_size: f32) -> Entity {
    crate::ui_text_input::spawn_text_input(
        commands,
        parent,
        &crate::ui_text_input::TextInputSpec {
            initial: text.to_owned(),
            font_size,
            visible_lines: BODY_VISIBLE_LINES,
            tab_index: 1,
            ..crate::ui_text_input::TextInputSpec::new(
                "notecard-body",
                crate::ui_text_input::TextInputKind::Multiline,
            )
        },
    )
}

/// Spawn the read-only body: a bounded, clipped block showing the notecard
/// text (no caret, no edit).
fn spawn_readonly_body(commands: &mut Commands, parent: Entity, text: &str, font_size: f32) {
    commands
        .spawn((
            Node {
                max_width: Val::Px(READONLY_BODY_WIDTH),
                max_height: Val::Px(READONLY_BODY_HEIGHT),
                overflow: Overflow::clip(),
                ..column(Val::Px(2.0))
            },
            ChildOf(parent),
        ))
        .with_child((
            Text::new(text.to_owned()),
            UiFont::Sans.at(font_size),
            TextColor(LABEL_COLOR),
        ));
}

/// Spawn the embedded-item list, one row per item (icon + name + type). Emits
/// nothing when the notecard carries no embedded items.
fn spawn_embedded_list(
    commands: &mut Commands,
    parent: Entity,
    notecard: &sl_notecard::Notecard,
    font_size: f32,
) {
    if notecard.items.is_empty() {
        return;
    }
    commands.spawn((
        Text::default(),
        crate::i18n::Translated::new("notecard-embedded-header"),
        UiFont::Sans.at(font_size),
        TextColor(DIM_COLOR),
        ChildOf(parent),
    ));
    for embedded in &notecard.items {
        let label = format!(
            "{}  {}  ({})",
            embedded_icon(&embedded.item.asset_type),
            embedded.item.name,
            embedded.item.asset_type.type_name(),
        );
        commands.spawn((
            Text::new(label),
            UiFont::Sans.at(font_size),
            TextColor(DIM_COLOR),
            ChildOf(parent),
        ));
    }
}

/// Spawn the Save button, returning its entity for the caller to wire.
fn spawn_save_button(commands: &mut Commands, parent: Entity, font_size: f32) -> Entity {
    commands
        .spawn((
            Button,
            bevy::input_focus::tab_navigation::TabIndex(2),
            Node {
                padding: UiRect::axes(Val::Px(8.0), Val::Px(3.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(Color::srgb(0.32, 0.36, 0.44)),
            BackgroundColor(Color::srgb(0.13, 0.15, 0.20)),
            Pickable::default(),
            Name::new("notecard-save"),
            ChildOf(parent),
        ))
        .with_child((
            Text::default(),
            crate::i18n::Translated::new("notecard-save"),
            UiFont::Sans.at(font_size),
            TextColor(LABEL_COLOR),
            Pickable::IGNORE,
        ))
        .id()
}

/// The emoji glyph for an embedded item, keyed on its asset class — matching
/// [`crate::inventory::item_icon`]'s vocabulary, but over [`sl_notecard`]'s own
/// asset-type enum.
const fn embedded_icon(asset_type: &sl_notecard::AssetType) -> &'static str {
    use sl_notecard::AssetType as A;
    match asset_type {
        A::Landmark => "\u{1f4cd}",
        A::Notecard => "\u{1f4c4}",
        A::Texture | A::TextureTga | A::ImageTga | A::ImageJpeg => "\u{1f5bc}\u{fe0f}",
        A::Sound | A::SoundWav => "\u{1f50a}",
        A::CallingCard => "\u{1f4c7}",
        A::Object => "\u{1f4e6}",
        A::Clothing | A::Bodypart => "\u{1f455}",
        A::Animation => "\u{1f3c3}",
        A::Gesture => "\u{1f44b}",
        A::Script | A::LslText | A::LslBytecode => "\u{1f4dc}",
        A::Mesh => "\u{1f4d0}",
        A::Settings => "\u{2699}\u{fe0f}",
        A::Material => "\u{1f3a8}",
        _other => "\u{2753}",
    }
}

// ---------------------------------------------------------------------------
// Gallery specimen.
// ---------------------------------------------------------------------------

/// The sample prose the specimen's body shows.
const SPECIMEN_TEXT: &str = "Welcome! Drop the landmark below to visit us.";

/// Spawn the notecard editor's content specimen: an editable body with one
/// embedded item and a Save button, built with no floater / session so
/// [`crate::ui_test`] sweeps its layout across every script, scale and font.
pub(crate) fn spawn_notecard_editor_specimen(
    commands: &mut Commands,
    parent: Entity,
    cx: ElementCx,
) -> Entity {
    let col = commands
        .spawn((
            Node {
                ..column(Val::Px(6.0))
            },
            ChildOf(parent),
        ))
        .id();
    let notecard = specimen_notecard(&cx.text(SPECIMEN_TEXT));
    populate_editor(commands, col, &notecard, true, None, cx.font_size);
    col
}

/// A one-embedded-item notecard for the specimen, so the item-list row is swept
/// alongside the body.
fn specimen_notecard(text: &str) -> sl_notecard::Notecard {
    let item = sl_notecard::InventoryItem {
        item_id: sl_types::key::NULL_KEY,
        parent_id: sl_types::key::NULL_KEY,
        permissions: sl_notecard::Permissions::default(),
        metadata: None,
        asset_id: sl_types::key::NULL_KEY,
        asset_id_encoding: sl_notecard::AssetIdEncoding::Plain,
        asset_type: sl_notecard::AssetType::Landmark,
        inventory_type: sl_notecard::InventoryType::Landmark,
        flags: 0,
        sale_info: sl_notecard::SaleInfo::default(),
        name: "Our Home".to_owned(),
        description: String::new(),
        creation_date: 0,
        unknown_fields: Vec::new(),
    };
    let marker = sl_notecard::embedded_char(0).unwrap_or(' ');
    sl_notecard::Notecard {
        source_version: sl_notecard::NotecardVersion::V2,
        embedded_items_version: 1,
        items: vec![sl_notecard::EmbeddedItem {
            char_index: 0,
            item,
        }],
        text: format!("{text} {marker}"),
    }
}
