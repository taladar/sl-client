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
//! box** in the *editable* text flow (`llviewertexteditor`'s embedded-item
//! machinery). Rendering them inline *while editing* needs a rich-text editor
//! with inline boxes and per-range brushes — the parley `PlainEditor` fork
//! tracked by [`viewer-lsl-editor-widget`], which is not yet built. A
//! **read-only** view needs no caret, so this editor:
//!
//! - shows a no-modify notecard as the **rich read-only reader**
//!   ([`crate::notecard_render`]): its embedded items drawn inline as clickable
//!   icon-and-name boxes, its prose URLs / SLURLs linkified — the reference's
//!   reader, minus editing;
//! - edits a modifiable notecard's **text** in the reusable multi-line field
//!   ([`crate::ui_text_input`], a stock `EditableText`), preserving each embedded
//!   item's private-use marker code point in the buffer so a round-trip never
//!   corrupts or orphans an item ([`sl_notecard::Notecard::with_edited_text`]
//!   reconciles the item table against the markers on save) — and offers a
//!   **toggle to that same rich read-only preview**, so its embedded items stay
//!   reachable and clickable until the inline-box editor widget lands (in the
//!   plain field the markers render as placeholder glyphs);
//! - lets a resident **drag an inventory item onto the editor to add it** as an
//!   embedded item ([`crate::inventory_drag`]'s notecard drop target);
//! - saves back to **agent** inventory over `UpdateNotecardAgentInventory` or,
//!   for a notecard opened from a prim's contents, to that object's **task**
//!   inventory over `UpdateNotecardTaskInventory` — one
//!   [`Command::UpdateInventoryAsset`] whose [`NotecardSource`] picks the
//!   capability and the "opened-from-task" provenance the reference carries.
//!
//! Deferred to [`viewer-lsl-editor-widget`] (needing its inline boxes): drawing
//! embedded items **inline in the editable flow** and dropping an item **at the
//! caret** rather than appended.
//!
//! # Read-only when you cannot modify
//!
//! Editability is gated on the item's owner mask carrying `MODIFY` (the
//! reference's `LLPreviewNotecard::canModify`). A no-modify notecard — a freebie
//! someone handed you — opens as the rich read-only reader with a note and no
//! Save button, so its text is never presented as editable when a save would be
//! refused.
//!
//! Reference (Firestorm, read-only): `llpreviewnotecard`, `llfloaternotecard`,
//! `llviewertexteditor`.

use bevy::prelude::*;
use bevy::text::EditableText;
use sl_client_bevy::{
    AssetKey, AssetType, Command, InventoryKey, InventoryType, ItemInfo, OwnerKey, SaleType,
    SlCommand, SlEvent, SlSessionEvent, UpdatableAssetType, Uuid,
};

use crate::floater::{FloaterCaps, FloaterSpec, spawn_floater};
use crate::inventory::AddEmbeddedItem;
use crate::linkified_text::LinkTextStyle;
use crate::notecard_render::spawn_notecard_body;
use crate::ui::{UiPanelShown, UiRoot, UiScaffoldSystems, column, row};
use crate::ui_element::{ElementCx, TextMayClip};
use crate::ui_font::UiFont;
use crate::world_api::{NotecardDropTarget, NotecardSource, OpenNotecard};

/// The editor's text font size, in logical pixels.
const FONT_SIZE: f32 = 14.0;

/// A general-purpose light label colour.
const LABEL_COLOR: Color = Color::srgb(0.90, 0.92, 0.96);

/// A dimmer colour for secondary text (the read-only note, the status line).
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
            .add_message::<AddEmbeddedItem>()
            .add_systems(
                Startup,
                spawn_notecard_floater.after(UiScaffoldSystems::SpawnRoot),
            )
            .add_systems(
                Update,
                (
                    open_notecard,
                    ingest_notecard_asset,
                    ingest_added_items,
                    report_notecard_save,
                )
                    .chain(),
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
    // floater's position (matching the previews / properties floater). The root
    // is the inventory-drag drop target (no drop accepted until a modifiable
    // notecard is shown).
    commands.entity(handle.root).insert((
        crate::floater_persist::FloaterPersistExempt,
        NotecardDropTarget::default(),
    ));
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

    // A modifiable notecard accepts a dragged item as a new embedded item.
    commands.entity(panel).insert(NotecardDropTarget {
        editable: open.editable,
    });

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
            source,
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

/// Build the editor's content under `content`. A no-modify notecard is the
/// **rich read-only reader** ([`crate::notecard_render`]) — its embedded items
/// drawn inline and clickable, its prose linkified. An editable notecard shows
/// the plain text field by default, with a **toggle to that same read-only
/// preview** so its embedded items stay reachable until the inline-box editor
/// widget lands, plus a Save button wired to `save_target` and a status line.
///
/// `source` locates the notecard (so a copied embedded item names the right
/// notecard / holding prim); `save_target` is where to write edits back, or
/// `None` for a specimen with no live notecard (the Save button is shown for
/// layout but does nothing).
fn populate_editor(
    commands: &mut Commands,
    content: Entity,
    notecard: &sl_notecard::Notecard,
    editable: bool,
    source: NotecardSource,
    save_target: Option<NotecardSource>,
    font_size: f32,
) -> BuiltEditor {
    let style = LinkTextStyle::at(font_size);

    // A no-modify notecard: the note, then the rich reader (items inline).
    if !editable {
        spawn_note(commands, content, "notecard-readonly-note", font_size);
        spawn_reader_block(commands, content, notecard, source, style, true);
        return BuiltEditor::default();
    }

    // Editable: a view toggle, the plain edit field (shown) and the rich reader
    // (hidden), built once — the toggle flips which is displayed. The plain
    // field keeps each embedded item's private-use marker in the buffer so a
    // round-trip never corrupts an item; the preview is where those items become
    // legible and clickable meanwhile.
    let (toggle_button, toggle_label) = spawn_view_toggle(commands, content, font_size);
    let body_field = spawn_body_field(commands, content, &notecard.text, font_size);
    let reader = spawn_reader_block(commands, content, notecard, source, style, false);
    commands.entity(toggle_button).insert(NotecardViewToggle {
        edit_field: body_field,
        reader,
        label: toggle_label,
        preview: false,
    });

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
    if let Some(target) = save_target {
        attach_save(commands, save, target, body_field, status);
    }

    BuiltEditor {
        body_field: Some(body_field),
        status: Some(status),
    }
}

/// A view-mode toggle on an editable notecard: which of the plain edit field or
/// the rich read-only preview is shown. Carried by the toggle button so its
/// observer can flip the two `display`s and its own label.
#[derive(Component, Debug, Clone, Copy)]
struct NotecardViewToggle {
    /// The plain multi-line edit field (shown when not previewing).
    edit_field: Entity,
    /// The rich read-only reader (shown when previewing).
    reader: Entity,
    /// The toggle button's own label node (retitled on flip).
    label: Entity,
    /// Whether the read-only preview is currently shown.
    preview: bool,
}

/// Spawn the view-mode toggle button, returning `(button, label)`.
fn spawn_view_toggle(commands: &mut Commands, parent: Entity, font_size: f32) -> (Entity, Entity) {
    let button = commands
        .spawn((
            Button,
            bevy::input_focus::tab_navigation::TabIndex(0),
            Node {
                padding: UiRect::axes(Val::Px(8.0), Val::Px(2.0)),
                border: UiRect::all(Val::Px(1.0)),
                align_self: AlignSelf::FlexStart,
                ..default()
            },
            BorderColor::all(Color::srgb(0.32, 0.36, 0.44)),
            BackgroundColor(Color::srgb(0.13, 0.15, 0.20)),
            Pickable::default(),
            Name::new("notecard-view-toggle"),
            ChildOf(parent),
        ))
        .id();
    let label = commands
        .spawn((
            Text::default(),
            crate::i18n::Translated::new("notecard-view-preview"),
            UiFont::Sans.at(font_size),
            TextColor(LABEL_COLOR),
            Pickable::IGNORE,
            ChildOf(button),
        ))
        .id();
    commands.entity(button).observe(on_toggle_view);
    (button, label)
}

/// Flip an editable notecard between the plain edit field and the rich
/// read-only preview on a primary press.
fn on_toggle_view(
    press: On<Pointer<Press>>,
    mut toggles: Query<&mut NotecardViewToggle>,
    mut nodes: Query<&mut Node>,
    mut commands: Commands,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let Ok(mut toggle) = toggles.get_mut(press.entity) else {
        return;
    };
    toggle.preview = !toggle.preview;
    let (preview, edit_field, reader, label) = (
        toggle.preview,
        toggle.edit_field,
        toggle.reader,
        toggle.label,
    );
    if let Ok(mut node) = nodes.get_mut(edit_field) {
        node.display = if preview {
            Display::None
        } else {
            Display::Flex
        };
    }
    if let Ok(mut node) = nodes.get_mut(reader) {
        node.display = if preview {
            Display::Flex
        } else {
            Display::None
        };
    }
    let key = if preview {
        "notecard-view-edit"
    } else {
        "notecard-view-preview"
    };
    commands
        .entity(label)
        .insert(crate::i18n::Translated::new(key));
}

/// Spawn the rich read-only reader in a bounded, wheel-scrollable block. Shown
/// or hidden per `visible` (an editable notecard builds it hidden behind the
/// edit field).
fn spawn_reader_block(
    commands: &mut Commands,
    parent: Entity,
    notecard: &sl_notecard::Notecard,
    source: NotecardSource,
    style: LinkTextStyle,
    visible: bool,
) -> Entity {
    let block = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                max_height: Val::Px(READONLY_BODY_HEIGHT),
                overflow: Overflow::scroll_y(),
                flex_direction: FlexDirection::Column,
                display: if visible {
                    Display::Flex
                } else {
                    Display::None
                },
                ..default()
            },
            ScrollPosition::default(),
            Pickable::default(),
            Name::new("notecard-reader"),
            ChildOf(parent),
        ))
        .id();
    commands.entity(block).observe(on_reader_scroll);
    spawn_notecard_body(commands, block, notecard, source, style);
    block
}

/// Scroll the reader block with the mouse wheel (its own `ScrollPosition`),
/// matching the search / world-map result lists.
fn on_reader_scroll(mut event: On<Pointer<Scroll>>, mut positions: Query<&mut ScrollPosition>) {
    /// Logical pixels one wheel notch scrolls.
    const LINE_SCROLL_PIXELS: f32 = 24.0;
    if let Ok(mut position) = positions.get_mut(event.entity) {
        position.0.y = (position.0.y - event.y * LINE_SCROLL_PIXELS).max(0.0);
    }
    event.propagate(false);
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
// Drag-add: a dropped inventory item becomes an embedded item.
// ---------------------------------------------------------------------------

/// Fold each dropped inventory item into the open notecard: add it to the
/// baseline item table with a fresh index and append its marker code point to
/// the edit buffer, so a Save reconciles it in via
/// [`sl_notecard::Notecard::with_edited_text`]. The marker renders as a
/// placeholder glyph in the plain field until the inline-box editor widget
/// draws it inline; the read-only preview shows it as a clickable item at once.
fn ingest_added_items(
    mut adds: MessageReader<AddEmbeddedItem>,
    mut state: ResMut<NotecardEditorState>,
    mut fields: Query<&mut EditableText>,
) {
    for add in adds.read() {
        // Only a modifiable notecard with a live edit field and baseline can
        // take an added item.
        if !state.editable {
            continue;
        }
        let Some(field_entity) = state.body_field else {
            continue;
        };
        if state.baseline.is_none() {
            continue;
        }
        // A fresh index past every existing one, so it never aliases a marker
        // already in the text (`with_edited_text` resolves markers by index).
        let next_index = state.baseline.as_ref().map_or(0, |notecard| {
            notecard
                .items
                .iter()
                .map(|embedded| embedded.char_index)
                .max()
                .map_or(0, |max| max.saturating_add(1))
        });
        let Some(marker) = sl_notecard::embedded_char(next_index) else {
            warn!("notecard already holds the maximum embedded items; drop ignored");
            continue;
        };
        let embedded = to_embedded_item(&add.item);
        if let Some(baseline) = state.baseline.as_mut() {
            baseline.items.push(sl_notecard::EmbeddedItem {
                char_index: next_index,
                item: embedded,
            });
        }
        if let Ok(mut editable) = fields.get_mut(field_entity) {
            let mut value = editable.value().to_string();
            value.push(marker);
            editable.editor_mut().set_text(&value);
        }
    }
}

/// Convert a viewer inventory-item snapshot into the notecard's embedded-item
/// model, so a dropped item round-trips through the Linden-text encoder faithful
/// to its ids, type, permissions and sale terms (the reference embeds the whole
/// `LLInventoryItem`).
fn to_embedded_item(item: &ItemInfo) -> sl_notecard::InventoryItem {
    let mask = |bits: u32| sl_notecard::PermissionMask(bits);
    let (owner_id, group_owned) = match item.owner {
        OwnerKey::Agent(agent) => (agent.0, false),
        OwnerKey::Group(group) => (group.0, true),
    };
    let permissions = sl_notecard::Permissions {
        base_mask: mask(item.permissions.base.bits()),
        owner_mask: mask(item.permissions.owner.bits()),
        group_mask: mask(item.permissions.group.bits()),
        everyone_mask: mask(item.permissions.everyone.bits()),
        next_owner_mask: mask(item.permissions.next_owner.bits()),
        creator_id: item.creator_id.0,
        owner_id,
        last_owner_id: sl_types::key::Key(item.last_owner_id),
        group_id: item.group.map_or(sl_types::key::NULL_KEY, |group| group.0),
        group_owned,
    };
    let (sale_type, sale_price) = match &item.sale {
        Some((sale_type, amount)) => (
            notecard_sale_type(*sale_type),
            i32::try_from(amount.0).unwrap_or(0),
        ),
        None => (sl_notecard::SaleType::NotForSale, 0),
    };
    sl_notecard::InventoryItem {
        item_id: item.item_id.0,
        parent_id: item.folder_id.0,
        permissions,
        metadata: None,
        // Store the asset id in the clear; the encoder re-obfuscates only what
        // was decoded as a shadow id.
        asset_id: sl_types::key::Key(item.asset_id),
        asset_id_encoding: sl_notecard::AssetIdEncoding::Plain,
        asset_type: sl_notecard::AssetType::from_type_name(proto_asset_type_name(item.asset_type)),
        inventory_type: sl_notecard::InventoryType::from_type_name(proto_inv_type_name(
            item.inv_type,
        )),
        flags: item.flags,
        sale_info: sl_notecard::SaleInfo {
            sale_type,
            sale_price,
        },
        name: item.name.clone(),
        description: item.description.clone(),
        creation_date: i64::from(item.creation_date),
        unknown_fields: Vec::new(),
    }
}

/// The Linden-text short type name for a viewer [`AssetType`] — the inverse of
/// the shared `from_type_name` vocabulary, so the notecard encoder writes the
/// name the simulator expects. An unrecognised class falls back to `object`.
const fn proto_asset_type_name(asset_type: AssetType) -> &'static str {
    match asset_type {
        AssetType::Texture => "texture",
        AssetType::Sound => "sound",
        AssetType::CallingCard => "callcard",
        AssetType::Landmark => "landmark",
        AssetType::Clothing => "clothing",
        AssetType::Object => "object",
        AssetType::Notecard => "notecard",
        AssetType::ScriptText => "lsltext",
        AssetType::ScriptBytecode => "lslbyte",
        AssetType::TextureTga => "txtr_tga",
        AssetType::Bodypart => "bodypart",
        AssetType::SoundWav => "snd_wav",
        AssetType::ImageTga => "img_tga",
        AssetType::ImageJpeg => "jpeg",
        AssetType::Animation => "animatn",
        AssetType::Gesture => "gesture",
        AssetType::Mesh => "mesh",
        AssetType::Settings => "settings",
        AssetType::Material => "material",
        AssetType::Gltf => "gltf",
        AssetType::GltfBin => "glbin",
        AssetType::Folder => "category",
        // `Other`, and any future non-exhaustive variant, embeds as an object.
        _other => "object",
    }
}

/// The Linden-text short inventory-type name for a viewer [`InventoryType`].
const fn proto_inv_type_name(inv_type: InventoryType) -> &'static str {
    match inv_type {
        InventoryType::Texture => "texture",
        InventoryType::Sound => "sound",
        InventoryType::CallingCard => "callcard",
        InventoryType::Landmark => "landmark",
        InventoryType::Object => "object",
        InventoryType::Notecard => "notecard",
        InventoryType::Category => "category",
        InventoryType::Script => "script",
        InventoryType::Snapshot => "snapshot",
        InventoryType::Attachment => "attach",
        InventoryType::Wearable => "wearable",
        InventoryType::Animation => "animation",
        InventoryType::Gesture => "gesture",
        InventoryType::Mesh => "mesh",
        InventoryType::Settings => "settings",
        InventoryType::Material => "material",
        _other => "object",
    }
}

/// Map a viewer [`SaleType`] to the notecard's sale-type model.
const fn notecard_sale_type(sale_type: SaleType) -> sl_notecard::SaleType {
    match sale_type {
        SaleType::NotForSale => sl_notecard::SaleType::NotForSale,
        SaleType::Original => sl_notecard::SaleType::Original,
        SaleType::Copy => sl_notecard::SaleType::Copy,
        SaleType::Contents => sl_notecard::SaleType::Contents,
        _other => sl_notecard::SaleType::NotForSale,
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
/// asset-type enum. Shared with the rich reader ([`crate::notecard_render`]).
pub(crate) const fn embedded_icon(asset_type: &sl_notecard::AssetType) -> &'static str {
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
    // A specimen has no live notecard: a nil source (the reader's copy dispatch
    // never fires without a session) and no Save target.
    let source = NotecardSource::Agent {
        item_id: InventoryKey::from(Uuid::nil()),
    };
    populate_editor(commands, col, &notecard, true, source, None, cx.font_size);
    col
}

/// Spawn the rich read-only reader specimen: prose with a linkified URL and an
/// inline embedded item, built with no floater / session so [`crate::ui_test`]
/// sweeps the interleaved prose-run / item-box layout across every script,
/// scale and font.
pub(crate) fn spawn_notecard_reader_specimen(
    commands: &mut Commands,
    parent: Entity,
    cx: ElementCx,
) -> Entity {
    let col = commands
        .spawn((
            Node {
                ..column(Val::Px(6.0))
            },
            TextMayClip {
                reason: "a linkified URL is a single unbreakable node and may exceed the width",
            },
            ChildOf(parent),
        ))
        .id();
    let marker = sl_notecard::embedded_char(0).unwrap_or(' ');
    // The connective prose runs through the cell's string transform (so the
    // matrix sweeps translations); the URL stays native — a mangled URL would
    // not linkify, which is not what this specimen tests.
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
    let notecard = sl_notecard::Notecard {
        source_version: sl_notecard::NotecardVersion::V2,
        embedded_items_version: 1,
        items: vec![sl_notecard::EmbeddedItem {
            char_index: 0,
            item,
        }],
        text: format!(
            "{welcome} https://example.com\n{visit} {marker}",
            welcome = cx.text("Welcome! See"),
            visit = cx.text("or drop by"),
        ),
    };
    let source = NotecardSource::Agent {
        item_id: InventoryKey::from(Uuid::nil()),
    };
    populate_editor(commands, col, &notecard, false, source, None, cx.font_size);
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

#[cfg(test)]
mod tests {
    use super::{proto_asset_type_name, proto_inv_type_name, to_embedded_item};
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{
        AgentKey, AssetType, InventoryFolderKey, InventoryKey, InventoryType, ItemInfo, OwnerKey,
        Permissions, Permissions5, Uuid,
    };

    /// A boxed-error result so the round-trip test can `?` decode failures.
    type TestResult = Result<(), String>;

    /// The viewer type names map onto the shared Linden-text vocabulary the
    /// notecard decoder classifies (rather than falling through to `Other`).
    #[test]
    fn type_names_match_the_notecard_vocabulary() {
        assert_eq!(proto_asset_type_name(AssetType::Landmark), "landmark");
        assert_eq!(proto_asset_type_name(AssetType::Notecard), "notecard");
        assert_eq!(proto_inv_type_name(InventoryType::Attachment), "attach");
        assert_eq!(
            sl_notecard::AssetType::from_type_name(proto_asset_type_name(AssetType::Object)),
            sl_notecard::AssetType::Object
        );
        assert_eq!(
            sl_notecard::InventoryType::from_type_name(proto_inv_type_name(
                InventoryType::Landmark
            )),
            sl_notecard::InventoryType::Landmark
        );
    }

    /// A dropped inventory item survives conversion + a notecard encode/decode
    /// round-trip with its type, name and permissions intact.
    #[test]
    fn dropped_item_round_trips_through_the_notecard() -> TestResult {
        let item = ItemInfo {
            item_id: InventoryKey::from(Uuid::from_u128(0x10)),
            folder_id: InventoryFolderKey::from(Uuid::from_u128(0x20)),
            name: "My Landmark".to_owned(),
            description: "A place".to_owned(),
            asset_id: Uuid::from_u128(0x30),
            asset_type: AssetType::Landmark,
            inv_type: InventoryType::Landmark,
            flags: 7,
            sale: None,
            creation_date: 1_700_000_000,
            owner: OwnerKey::Agent(AgentKey::from(Uuid::from_u128(0x40))),
            last_owner_id: Uuid::from_u128(0x50),
            creator_id: AgentKey::from(Uuid::from_u128(0x60)),
            group: None,
            permissions: Permissions5 {
                base: Permissions::from_bits(0x7fff_ffff),
                owner: Permissions::from_bits(0x7fff_ffff),
                group: Permissions::empty(),
                everyone: Permissions::empty(),
                next_owner: Permissions::from_bits(0x0008_2000),
            },
        };
        let marker = sl_notecard::embedded_char(0).ok_or("no marker")?;
        let notecard = sl_notecard::Notecard {
            source_version: sl_notecard::NotecardVersion::V2,
            embedded_items_version: 1,
            items: vec![sl_notecard::EmbeddedItem {
                char_index: 0,
                item: to_embedded_item(&item),
            }],
            text: format!("See {marker}"),
        };
        let decoded =
            sl_notecard::Notecard::decode(&notecard.encode()).map_err(|error| error.to_string())?;
        let survivor = decoded.items.first().ok_or("no embedded item")?;
        assert_eq!(survivor.item.asset_type, sl_notecard::AssetType::Landmark);
        assert_eq!(
            survivor.item.inventory_type,
            sl_notecard::InventoryType::Landmark
        );
        assert_eq!(survivor.item.name, "My Landmark");
        assert_eq!(survivor.item.permissions.owner_mask.0, 0x7fff_ffff);
        assert_eq!(
            survivor.item.permissions.creator_id.0,
            Uuid::from_u128(0x60)
        );
        assert_eq!(survivor.item.asset_id.0, Uuid::from_u128(0x30));
        Ok(())
    }
}
