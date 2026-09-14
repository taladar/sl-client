//! The **notecard viewer & editor** floater (`viewer-notecard-editor`): open a
//! notecard from inventory, read it, edit its text when permitted, and save it
//! back to agent inventory.
//!
//! A notecard is *not* plain text — the asset is a Linden-text container
//! carrying the prose **plus embedded inventory items** (landmarks, objects,
//! other notecards a resident drops inline). The pure [`sl_notecard`] crate
//! decodes and re-encodes that container; this module is the widget over it.
//!
//! # One body, items and all
//!
//! The reference viewer draws each embedded item as a **clickable box in the
//! text flow** (`llviewertexteditor`'s embedded-item machinery), whether the
//! notecard is being read or written. So does this editor, on the rich-text
//! field ([`sl_viewer_ui_widgets::ui_rich_text`]): the buffer keeps each item's
//! private-use marker code point — it is what a deletion deletes and what
//! [`sl_notecard::Notecard::with_edited_text`] reconciles the item table
//! against on save — the field takes the marker off the screen, and the box for
//! that item is laid out in the flow exactly where the marker sits.
//!
//! There used to be a **toggle** here, between a plain edit field that showed
//! the markers as placeholder glyphs and a separate read-only preview that
//! showed the items. That was the shape of the thing while Bevy's editable text
//! was `parley::PlainEditor` with one style for the whole buffer and no inline
//! boxes; the workspace's parley fork now lets a field carry both, so the two
//! views are one and there is nothing to switch between.
//!
//! What the editor does:
//!
//! - **reads** a notecard as flowing text with its embedded items inline and
//!   clickable ([`crate::notecard_render`] fills each box, and a click copies,
//!   opens a profile or previews a texture) and its prose URLs / SLURLs
//!   linkified;
//! - **edits** a modifiable notecard's text in that same body, so an item stays
//!   visible, clickable and in place while the prose around it is typed;
//! - lets a resident **drag an inventory item onto the editor to add it** as an
//!   embedded item (`crate::inventory_drag`'s notecard drop target);
//! - **saves** back to **agent** inventory over `UpdateNotecardAgentInventory`
//!   or, for a notecard opened from a prim's contents, to that object's **task**
//!   inventory over `UpdateNotecardTaskInventory` — one
//!   [`Command::UpdateInventoryAsset`] whose [`NotecardSource`] picks the
//!   capability and the "opened-from-task" provenance the reference carries.
//!
//! Still deferred (it needs the field to report where the caret is): dropping an
//! item **at the caret** rather than appending its marker to the end.
//!
//! # Read-only when you cannot modify
//!
//! Editability is gated on the item's owner mask carrying `MODIFY` (the
//! reference's `LLPreviewNotecard::canModify`). A no-modify notecard — a freebie
//! someone handed you — opens with a note and no Save button, and its body
//! refuses the keystrokes that would change it while staying selectable,
//! copyable and clickable, so its text is never presented as editable when a
//! save would be refused.
//!
//! # One window per notecard
//!
//! A notecard editor is a **keyed floater** ([`FloaterKey`]): opening a second
//! notecard opens a second window rather than re-pointing the first, which is
//! what the reference does (`LLPreviewNotecard` is registered per item id) and
//! what keeps a window's **unsaved text** from vanishing because someone opened
//! another notecard. Each window's state — the notecard it shows, its in-flight
//! load / save, its field entities — is a component on that window, and closing
//! one ends it; the body's own state (its item table, its boxes) is a component
//! on the field.
//!
//! Reference (Firestorm, read-only): `llpreviewnotecard`, `llfloaternotecard`,
//! `llviewertexteditor`.

use std::collections::HashMap;

use bevy::prelude::*;
use bevy::text::EditableText;
use sl_client_bevy::{
    AssetKey, AssetType, Command, InventoryKey, InventoryType, ItemInfo, OwnerKey, Permissions,
    SaleType, SlCommand, SlEvent, SlSessionEvent, UpdatableAssetType, Uuid,
};

use crate::asset_editor::{
    DIM_COLOR, ERROR_COLOR, EditedText, FONT_SIZE, SaveEditorWindow, UnsavedWork, set_status,
    spawn_note, spawn_save_button, spawn_status, tear_down,
};
use crate::floater::{
    FloaterCaps, FloaterHandle, FloaterKey, FloaterSpec, FloaterSystems, KeyedFloaterOpen,
    KeyedFloaters,
};
use sl_viewer_ui_widgets::ui_rich_text::{
    RichTextClass, RichTextContent, RichTextObject, RichTextRange, RichTextRangeActivated,
    RichTextSpec, RichTextStyle, spawn_rich_text, spawn_rich_text_object,
};

use crate::inventory::AddEmbeddedItem;
use crate::linkified_text::{LinkActivated, LinkTextStyle, populate_linkified_text};
use crate::notecard_render::spawn_embedded_item_box;
use crate::ui::{column, row};
use crate::ui_element::{ElementCx, TextMayClip};
use crate::ui_font::UiFont;
use crate::url_linkify::{TextRun, linkify};
use crate::world_api::{NotecardDropTarget, NotecardSource, OpenNotecard};

/// The body field's height, in visible text lines — what the window opens at,
/// being content-driven. It is a *starting* size, not a cap: the body fills, so
/// a resized window gives it every line that fits (`ui_text_input`'s `fill`).
const BODY_VISIBLE_LINES: f32 = 18.0;

// ---------------------------------------------------------------------------
// Messages.
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Plugin, resources.
// ---------------------------------------------------------------------------

/// The plugin owning the notecard editor floater.
#[derive(Debug)]
pub struct EditNotecardPlugin;

impl Plugin for EditNotecardPlugin {
    /// Register the open message and the open / ingest / report systems.
    ///
    /// Nothing spawns at `Startup`: a notecard window exists only while that
    /// notecard is open, so `open_notecard` spawns the instance and builds its
    /// content. The per-window systems are gated on there being a window.
    fn build(&self, app: &mut App) {
        // The preview's focus drop needs `InputFocus`, which normally arrives
        // with `bevy_input_focus`'s plugin; `init_resource` is a no-op when it
        // is already there and keeps an app without it (the gallery) from
        // failing the system's parameter validation.
        app.init_resource::<bevy::input_focus::InputFocus>()
            .add_message::<OpenNotecard>()
            .add_message::<AddEmbeddedItem>()
            .add_systems(
                Update,
                (
                    // After the manager's command pass — see `FloaterSystems`:
                    // the click that opens a notecard (an inventory row, a
                    // link) also raises the window it landed in, and the later
                    // raise wins.
                    open_notecard.after(FloaterSystems::Commands),
                    (
                        ingest_notecard_asset,
                        ingest_added_items,
                        save_notecard,
                        report_notecard_save,
                    )
                        .chain()
                        .run_if(any_with_component::<NotecardEditorState>),
                    // Not gated on a window: a gallery specimen has a body and
                    // no window, and its items are part of what the layout
                    // sweeps measure.
                    (rebuild_notecard_body, activate_notecard_link)
                        .chain()
                        .run_if(any_with_component::<NotecardBody>),
                )
                    .chain(),
            );
    }
}

/// One open notecard window's entities and live state — a **component on the
/// window**, since notecard editors open per notecard ([`FloaterKey`]).
#[derive(Component, Debug)]
struct NotecardEditorState {
    /// The content column this window's editor is built under.
    content: Entity,
    /// Where this window's notecard lives (Save target), fixed for its life.
    source: NotecardSource,
    /// Whether this window's notecard is editable, fixed for its life.
    editable: bool,
    /// The asset id awaited (`FetchAsset` sent), matched on `AssetReceived`.
    pending_load: Option<Uuid>,
    /// The save in flight, matched on the upload result; `None` when none is.
    pending_save: Option<NotecardSaveInFlight>,
    /// The editable body field, when the notecard is modifiable.
    body_field: Option<Entity>,
    /// The status text node (loading / saving / result), when present.
    status: Option<Entity>,
}

/// One notecard save on the wire: which item it wrote, and the text it wrote.
///
/// The text is what makes the unsaved-work mark honest — see
/// [`save_notecard`].
#[derive(Debug, Clone)]
struct NotecardSaveInFlight {
    /// The inventory item the save named, matched against the reply's.
    item: Uuid,
    /// The text that went out, which becomes the new baseline when it lands.
    text: String,
}

/// The notecard editor floater's [`FloaterSpec`] — shared with the `FLOATERS`
/// registry, so the swept window is the one the viewer spawns.
#[must_use]
pub fn notecard_editor_floater_spec() -> FloaterSpec {
    FloaterSpec {
        id: "notecard-editor",
        title: "Notecard".to_owned(),
        position: Vec2::new(400.0, 100.0),
        // Content-driven (the scaffold's convention 2), and here that is a
        // correction rather than a preference. The window used to open at a
        // rect measured against the *read-only* block — narrower and shorter
        // than the editable body it actually opens with — and a floater's
        // content slot clips, so the Save row underneath was cut off the bottom
        // of the window at the default font size and further at every larger
        // one. The field's declared lines size the window instead. The grip
        // resizes it from there, and the body **grows with it**: the field
        // fills, so the slot's spare height is the body's (`ui_text_input`'s
        // `fill`).
        default_size: None,
        min_size: Some(Vec2::new(260.0, 160.0)),
        dock_host: None,
        caps: FloaterCaps {
            resizable: true,
            minimizable: true,
            closable: true,
            dockable: false,
        },
    }
}

/// The [`FloaterKey`] of the window showing the notecard at `source`.
///
/// A task-held notecard is keyed by its **object and item**, not the item
/// alone: two rezzed copies of one object carry the same item ids, and they are
/// two different notecards.
fn notecard_key(source: NotecardSource) -> FloaterKey {
    match source {
        NotecardSource::Agent { item_id } => FloaterKey::subject(&item_id),
        NotecardSource::Task { task_id, item_id } => {
            FloaterKey::subject(&format!("{task_id}/{item_id}"))
        }
    }
}

/// Open a notecard **per notecard** (`viewer-keyed-floater-audit`): raise this
/// notecard's window when it is already up, and otherwise spawn one, hang its
/// state off it and fetch the asset.
///
/// Every open of the frame is honoured, not just the last. A window already up
/// is only raised — never re-fetched and never rebuilt — because rebuilding it
/// would throw away exactly what a second open must not touch: the resident's
/// unsaved edits.
fn open_notecard(
    mut opens: MessageReader<OpenNotecard>,
    mut floaters: KeyedFloaters,
    mut commands: Commands,
    mut sl_commands: MessageWriter<SlCommand>,
) {
    for open in opens.read().cloned() {
        let opened = floaters.open(notecard_editor_floater_spec(), notecard_key(open.source));
        let KeyedFloaterOpen::Spawned(handle) = opened else {
            continue;
        };
        build_notecard_window(&mut commands, handle, &open);
        sl_commands.write(SlCommand(Command::FetchAsset {
            asset_id: AssetKey::from(open.asset_id),
            asset_type: AssetType::Notecard,
            byte_range: None,
        }));
    }
}

/// Furnish a freshly spawned notecard window: its title, its drop target, a
/// loading line, and the [`NotecardEditorState`] the per-window systems find it
/// by.
fn build_notecard_window(commands: &mut Commands, handle: FloaterHandle, open: &OpenNotecard) {
    // The title is the notecard's name (the title node carries no Fluent key,
    // so a direct text set is not fought by the translator).
    commands
        .entity(handle.title_text)
        .insert(Text::new(open.name.clone()));
    // A modifiable notecard accepts a dragged item as a new embedded item; the
    // drop names this window (`AddEmbeddedItem::editor`).
    commands.entity(handle.root).insert(NotecardDropTarget {
        editable: open.editable,
    });
    let status = spawn_status(
        commands,
        handle.content,
        "notecard-status-loading",
        DIM_COLOR,
    );
    commands.entity(handle.root).insert((
        UnsavedWork::default(),
        NotecardEditorState {
            content: handle.content,
            source: open.source,
            editable: open.editable,
            pending_load: Some(open.asset_id),
            pending_save: None,
            body_field: None,
            status: Some(status),
        },
    ));
}

// ---------------------------------------------------------------------------
// Asset received → build the editor.
// ---------------------------------------------------------------------------

/// Fold the fetched notecard asset into the editor once it arrives: decode it,
/// then build the read-only or editable body and the embedded-item list.
fn ingest_notecard_asset(
    mut events: MessageReader<SlEvent>,
    mut windows: Query<(Entity, &mut NotecardEditorState)>,
    children: Query<&Children>,
    mut commands: Commands,
) {
    // Collected once and replayed per window: a reader is consumed by the first
    // pass over it, so with two notecards open the second would see nothing.
    let frame: Vec<&SlEvent> = events.read().collect();
    if frame.is_empty() {
        return;
    }
    for (window, mut state) in &mut windows {
        for event in &frame {
            let SlSessionEvent::AssetReceived(asset) = &event.0 else {
                continue;
            };
            if state.pending_load != Some(asset.id) {
                continue;
            }
            state.pending_load = None;
            let (content, source) = (state.content, state.source);

            let notecard = match decode_notecard_asset(&asset.data) {
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
                FONT_SIZE,
            );
            state.body_field = built.body_field;
            state.status = built.status;
            // The text as it arrived is the baseline the unsaved-work guard
            // measures against; a read-only window has no buffer to lose and so
            // never gets one.
            if let Some(field) = built.body_field {
                commands.entity(window).insert(EditedText {
                    field,
                    saved: notecard.text.clone(),
                });
            }
        }
    }
}

/// Decode a fetched notecard asset, reading an **empty** payload as an empty
/// notecard rather than a malformed one.
///
/// A notecard the resident just created has no body yet, and grids write that
/// differently: Second Life stores a valid empty Linden-text container, while
/// OpenSim's server-side `CreateInventoryItem` stores a **single `0x00` byte**.
/// Both mean "nothing in it", and both must open as an empty editable notecard
/// — otherwise every notecard made on OpenSim reads as unreadable and can never
/// be written into, since the first Save needs an editor to type in.
///
/// This is deliberately narrow: only an all-zero (or zero-length) payload takes
/// the empty path. A valid notecard always starts with `Linden text version `,
/// so no truncation of a real one can be mistaken for empty, and any other
/// malformed asset still fails, logs and shows the unreadable status.
fn decode_notecard_asset(
    data: &[u8],
) -> Result<sl_notecard::Notecard, sl_notecard::decode::NotecardError> {
    if data.iter().all(|byte| *byte == 0) {
        return Ok(sl_notecard::Notecard {
            source_version: sl_notecard::NotecardVersion::V2,
            items: Vec::new(),
            text: String::new(),
        });
    }
    sl_notecard::Notecard::decode(data)
}

/// The entities [`populate_editor`] hands back to the live state.
#[derive(Debug, Default, Clone, Copy)]
struct BuiltEditor {
    /// The editable body field, when the notecard is modifiable.
    body_field: Option<Entity>,
    /// The status text node, when the editor offers a Save button.
    status: Option<Entity>,
}

/// Build the editor's content under `content`: the rich-text body, and — when
/// the notecard is modifiable — a Save button and a status line.
///
/// **One body, both modes.** The embedded items are drawn in the flow of the
/// text itself ([`crate::notecard_render`] fills the boxes the rich-text field
/// reserves), so a modifiable notecard shows them exactly where a read-only one
/// does and there is nothing to toggle between. What the two modes differ in is
/// the prose's links: a read-only body draws each as the *resolved* chip the
/// rest of the viewer draws (an avatar's name, a location's pin), while an
/// editable one leaves the URL text alone — it is text the resident is editing —
/// and styles it in place.
///
/// `source` locates the notecard, so a copied embedded item names the right
/// notecard / holding prim. Where a save goes is not a parameter: the Save
/// button names the **window** it sits in and [`save_notecard`] reads that
/// window's own source, so a specimen — which sits in no window — shows the
/// button for layout and saves nothing.
fn populate_editor(
    commands: &mut Commands,
    content: Entity,
    notecard: &sl_notecard::Notecard,
    editable: bool,
    source: NotecardSource,
    font_size: f32,
) -> BuiltEditor {
    let style = LinkTextStyle::at(font_size);

    // A no-modify notecard says so before its body, and then reads the same.
    if !editable {
        spawn_note(commands, content, "notecard-readonly-note", font_size);
    }

    let handle = spawn_rich_text(
        commands,
        content,
        &RichTextSpec {
            initial: notecard.text.clone(),
            tab_index: 1,
            font_size,
            visible_lines: BODY_VISIBLE_LINES,
            read_only: !editable,
            // The body is what a bigger notecard window should make bigger.
            fill: true,
            classes: vec![RichTextClass {
                color: style.link_color,
                underline: true,
                clickable: true,
            }],
            ..RichTextSpec::new("notecard-body")
        },
    );
    // The model is seeded here rather than left to the first rebuild pass, so
    // the body's items are drawn on the frame it is built — and so a **specimen**,
    // which no system ever visits, still shows them.
    let mut body = NotecardBody {
        overlay: handle.overlay,
        baseline: notecard.clone(),
        source,
        read_only: !editable,
        font_size,
        shown_text: None,
        objects: HashMap::new(),
    };
    let model = build_body_model(commands, &mut body, &notecard.text);
    body.shown_text = Some(notecard.text.clone());
    commands.entity(handle.field).insert((body, model));

    if !editable {
        return BuiltEditor::default();
    }

    let bar = commands
        .spawn((
            Node {
                ..row(Val::Px(8.0))
            },
            ChildOf(content),
        ))
        .id();
    let _save = spawn_save_button(commands, bar, "notecard-save", "notecard-save", font_size);
    // The status node sits after the Save button, empty until a save runs.
    let status = commands
        .spawn((
            Text::default(),
            UiFont::Sans.at(font_size),
            TextColor(DIM_COLOR),
            ChildOf(bar),
        ))
        .id();
    BuiltEditor {
        body_field: Some(handle.field),
        status: Some(status),
    }
}

// ---------------------------------------------------------------------------
// The body: what the buffer's markers and URLs are drawn as.
// ---------------------------------------------------------------------------

/// The style class index links wear in the body — the single class
/// [`populate_editor`] declares.
const LINK_CLASS: usize = 0;

/// One notecard body's live state, on the rich-text field itself.
///
/// The **baseline** lives here rather than on the window because it is the
/// body's: it is the item table the buffer's markers resolve against while the
/// resident types, and the table [`sl_notecard::Notecard::with_edited_text`]
/// reconciles against when the Save button reads the field.
#[derive(Component, Debug)]
struct NotecardBody {
    /// The rich-text field's overlay, which the item and link boxes are spawned
    /// under.
    overlay: Entity,
    /// The notecard as it was loaded, plus every item dropped in since — the
    /// table a marker code point in the buffer names an item from.
    baseline: sl_notecard::Notecard,
    /// Where the notecard lives, so a copied embedded item names the right
    /// notecard and holding prim.
    source: NotecardSource,
    /// Whether the body is read-only, which is what decides whether a link is
    /// drawn as a resolved chip or styled in place.
    read_only: bool,
    /// The font size the boxes are built at.
    font_size: f32,
    /// The buffer the current model was built from. `None` only while the body
    /// is being built — a sentinel rather than an empty string, because an empty
    /// notecard would otherwise look like one that had already been drawn.
    shown_text: Option<String>,
    /// The boxes currently spawned, by what they stand for, so typing prose
    /// around them moves them rather than rebuilding them.
    objects: HashMap<NotecardObjectKey, Entity>,
}

/// What a box in the body stands for — the key a box is reused by.
///
/// Reuse is the point: a resident typing a word before an item must not cost a
/// despawn and respawn of that item's box (the floaters' build-once rule), and a
/// link whose label is still resolving must not lose the request it made.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum NotecardObjectKey {
    /// The `ordinal`-th occurrence of the marker for the embedded item at
    /// `index` in the notecard's table. An ordinal, because the reference lets a
    /// resident copy-paste a marker and get the item twice.
    Item {
        /// The item's index in the notecard's table.
        index: u32,
        /// Which occurrence of that marker this box is, from the top.
        ordinal: usize,
    },
    /// The `ordinal`-th link to `url` in the body (read-only bodies only).
    Link {
        /// The link's canonical URL.
        url: String,
        /// Which occurrence of that URL this box is, from the top.
        ordinal: usize,
    },
}

/// Rebuild a body's model whenever its buffer has moved: which box sits at which
/// byte offset, and which ranges are hidden or styled as links.
///
/// The buffer is compared before anything is built, so this is user-paced work
/// on a real edit rather than per-frame churn — and boxes are matched by
/// [`NotecardObjectKey`], so the common edit (typing prose) spawns and despawns
/// nothing at all and only moves what is already there.
fn rebuild_notecard_body(
    mut bodies: Query<(&mut NotecardBody, &EditableText, &mut RichTextContent)>,
    mut commands: Commands,
) {
    for (mut body, editable, mut content) in &mut bodies {
        let text = editable.value().to_string();
        if body.shown_text.as_deref() == Some(text.as_str()) {
            continue;
        }
        let model = build_body_model(&mut commands, &mut body, &text);
        content.set_if_neq(model);
        body.shown_text = Some(text);
    }
}

/// Build the body's model for `text`: one box per embedded-item marker, one per
/// link when the body is read-only, and the ranges that hide or colour them.
///
/// Boxes already standing for the same thing are reused; the ones nothing in the
/// new text stands for are despawned.
fn build_body_model(
    commands: &mut Commands,
    body: &mut NotecardBody,
    text: &str,
) -> RichTextContent {
    let style = LinkTextStyle::at(body.font_size);
    let (overlay, source, read_only) = (body.overlay, body.source, body.read_only);
    let mut previous = core::mem::take(&mut body.objects);
    let mut kept: HashMap<NotecardObjectKey, Entity> = HashMap::new();
    let mut model = RichTextContent::default();

    // The embedded items: a marker code point stands where each one sits.
    let mut ordinals: HashMap<u32, usize> = HashMap::new();
    for (offset, character) in text.char_indices() {
        let Some(index) = sl_notecard::embedded_char_index(character) else {
            continue;
        };
        let Some(item) = body.baseline.item_by_index(index) else {
            continue;
        };
        let ordinal = ordinals.entry(index).or_insert(0);
        let key = NotecardObjectKey::Item {
            index,
            ordinal: *ordinal,
        };
        *ordinal = ordinal.saturating_add(1);
        let object = previous.remove(&key).unwrap_or_else(|| {
            let object = spawn_rich_text_object(commands, overlay);
            spawn_embedded_item_box(commands, object, item, source, style);
            object
        });
        kept.insert(key, object);
        model.objects.push(RichTextObject {
            entity: object,
            index: offset,
        });
        // The marker itself is kept in the buffer and taken off the screen: it
        // is what a deletion deletes and what a save reconciles against.
        model.ranges.push(RichTextRange {
            range: offset..offset.saturating_add(character.len_utf8()),
            style: RichTextStyle::Hidden,
        });
    }

    // The prose's links.
    let mut offset = 0_usize;
    let mut link_ordinals: HashMap<String, usize> = HashMap::new();
    for run in linkify(text) {
        let link = match run {
            TextRun::Plain(plain) => {
                offset = offset.saturating_add(plain.len());
                continue;
            }
            TextRun::Link(link) => link,
        };
        let range = offset..offset.saturating_add(link.matched.len());
        offset = range.end;
        // The segmenter's runs are the source string in order, and this is what
        // says so: a run that does not sit where it claims is dropped rather
        // than styling a range of somebody else's text.
        if text.get(range.clone()) != Some(link.matched.as_str()) {
            continue;
        }
        if !read_only {
            model.ranges.push(RichTextRange {
                range,
                style: RichTextStyle::Class(LINK_CLASS),
            });
            continue;
        }
        // A read-only body draws the link the way the rest of the viewer does —
        // an agent link as that avatar's name, with its icon and its tooltip —
        // which means drawing a box over the hidden URL text.
        let ordinal = link_ordinals.entry(link.url.clone()).or_insert(0);
        let key = NotecardObjectKey::Link {
            url: link.url.clone(),
            ordinal: *ordinal,
        };
        *ordinal = ordinal.saturating_add(1);
        let object = previous.remove(&key).unwrap_or_else(|| {
            let object = spawn_rich_text_object(commands, overlay);
            populate_linkified_text(commands, object, &link.matched, style);
            object
        });
        kept.insert(key, object);
        model.objects.push(RichTextObject {
            entity: object,
            index: range.start,
        });
        model.ranges.push(RichTextRange {
            range,
            style: RichTextStyle::Hidden,
        });
    }

    for (_key, object) in previous {
        commands.entity(object).despawn();
    }
    body.objects = kept;
    model
}

/// Open a link the resident pressed in an **editable** body.
///
/// A read-only body's links are real link nodes and dispatch themselves; an
/// editable body's are ranges of the text being edited, so the press arrives as
/// a [`RichTextRangeActivated`] naming the range and the URL is re-read from the
/// buffer — which is also what keeps a half-typed URL from opening the link it
/// used to be.
fn activate_notecard_link(
    mut activations: MessageReader<RichTextRangeActivated>,
    bodies: Query<&EditableText, With<NotecardBody>>,
    mut links: MessageWriter<LinkActivated>,
) {
    for activation in activations.read() {
        if activation.class != LINK_CLASS {
            continue;
        }
        let Ok(editable) = bodies.get(activation.field) else {
            continue;
        };
        let text = editable.value().to_string();
        let Some(pressed) = text.get(activation.range.clone()) else {
            continue;
        };
        for run in linkify(pressed) {
            if let TextRun::Link(link) = run {
                links.write(LinkActivated {
                    target: link.target,
                    url: link.url,
                });
                break;
            }
        }
    }
}

/// Reconcile one window's edited text against its baseline and write it back
/// over the source's `Update*Inventory` capability (agent or task, per
/// [`NotecardSource`]).
///
/// The Save button's press and the "Save" answer to the unsaved-work
/// confirmation both arrive here as a [`SaveEditorWindow`], so the two cannot
/// come to save different things.
fn save_notecard(
    mut requests: MessageReader<SaveEditorWindow>,
    mut windows: Query<&mut NotecardEditorState>,
    fields: Query<(&EditableText, &NotecardBody)>,
    mut sl_commands: MessageWriter<SlCommand>,
    mut commands: Commands,
) {
    for request in requests.read() {
        let Ok(mut state) = windows.get_mut(request.window) else {
            continue;
        };
        let (Some(field_entity), true) = (state.body_field, state.editable) else {
            continue;
        };
        let Ok((field, body)) = fields.get(field_entity) else {
            continue;
        };
        let edited = field.value().to_string();
        let data = body.baseline.with_edited_text(&edited).encode();
        let source = state.source;
        sl_commands.write(SlCommand(Command::UpdateInventoryAsset {
            location: source.location(),
            asset_type: UpdatableAssetType::Notecard,
            data,
        }));
        state.pending_save = Some(NotecardSaveInFlight {
            item: source.item_id().uuid(),
            // Held rather than re-read from the field when the reply lands: a
            // resident who keeps typing while the save is in flight has not
            // saved *that* text, and clearing the unsaved-work mark against the
            // buffer as it then stands would claim they had.
            text: edited,
        });
        if let Some(status) = state.status {
            set_status(&mut commands, status, "notecard-status-saving", DIM_COLOR);
        }
    }
}

// ---------------------------------------------------------------------------
// Save result.
// ---------------------------------------------------------------------------

/// Report the outcome of an in-flight notecard save. A notecard save is
/// user-triggered one at a time, so the next terminal upload event while a save
/// is pending is treated as its result.
fn report_notecard_save(
    mut events: MessageReader<SlEvent>,
    mut windows: Query<(&mut NotecardEditorState, Option<&mut EditedText>)>,
    mut inventory: Option<ResMut<crate::inventory::InventoryModel>>,
    mut commands: Commands,
) {
    let frame: Vec<&SlEvent> = events.read().collect();
    if frame.is_empty() {
        return;
    }
    for (mut state, mut edited) in &mut windows {
        for event in &frame {
            let Some(pending) = state.pending_save.clone() else {
                continue;
            };
            match &event.0 {
                SlSessionEvent::AssetUploaded {
                    new_asset,
                    new_inventory_item,
                    // A save replaces the asset of an item that already
                    // exists; only an upload that creates one carries it.
                    created: _,
                } => {
                    // Prefer the returned item id; a save reports the item it wrote,
                    // so a mismatching upload (a baked texture, another floater's
                    // asset) is not ours. A `None` item id is accepted as ours
                    // rather than leaving the status stuck on "saving".
                    if matches!(new_inventory_item, Some(id) if *id != pending.item) {
                        continue;
                    }
                    state.pending_save = None;
                    // What went out is now what is stored, so it is the new
                    // baseline the unsaved-work guard measures against — and a
                    // window whose close was waiting on this save can close.
                    if let Some(edited) = edited.as_mut() {
                        edited.saved = pending.text;
                    }
                    // The save wrote a **new** asset and the grid rebound the
                    // item to it. Point the inventory model at it too, or the
                    // next open of this notecard fetches the asset it had
                    // *before* the save — which is what made a saved notecard
                    // read back blank. The rebind comes from the item this
                    // window saved rather than from the reply, because a grid
                    // may answer with the new asset alone (OpenSim's
                    // `UpdateNotecardAgentInventory` does).
                    if let NotecardSource::Agent { item_id } = state.source
                        && let Some(model) = inventory.as_mut()
                    {
                        let _folder = model.rebind_asset(item_id, *new_asset);
                    }
                    if let Some(status) = state.status {
                        set_status(&mut commands, status, "notecard-status-saved", DIM_COLOR);
                    }
                }
                SlSessionEvent::AssetUploadFailed { reason } => {
                    warn!("notecard save failed: {reason}");
                    // Nothing was stored, so the baseline stands and the work is
                    // still unsaved: a window that was closing on this save
                    // stays up with the failure on screen.
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
}

// ---------------------------------------------------------------------------
// Drag-add: a dropped inventory item becomes an embedded item.
// ---------------------------------------------------------------------------

/// Whether an item may be embedded in a notecard at all.
///
/// The reference refuses the drop outright unless the item's **next-owner**
/// mask is `PERM_ITEM_UNRESTRICTED` — copy, modify and transfer together
/// (`LLViewerTextEditor::handleDragAndDrop`, which answers `ACCEPT_NO` with the
/// "owner restricted" tooltip otherwise). The reason is what happens at the
/// other end: an item copied back out of a notecard is a transfer, so the grid
/// hands the copier the *next-owner* permissions, and embedding a restricted
/// item would promise a copy that arrives stripped of what the resident saw.
const fn may_embed(item: &ItemInfo) -> bool {
    item.permissions
        .next_owner
        .contains(Permissions::ITEM_UNRESTRICTED)
}

/// Fold each dropped inventory item into the open notecard: add it to the
/// baseline item table with a fresh index and append its marker code point to
/// the edit buffer, so a Save reconciles it in via
/// [`sl_notecard::Notecard::with_edited_text`]. The marker renders as a
/// placeholder glyph in the plain field until the inline-box editor widget
/// draws it inline; the read-only preview shows it as a clickable item at once.
///
/// An item whose next-owner permissions are restricted is refused, as the
/// reference refuses it — see [`may_embed`].
fn ingest_added_items(
    mut adds: MessageReader<AddEmbeddedItem>,
    windows: Query<&NotecardEditorState>,
    mut fields: Query<(&mut EditableText, &mut NotecardBody)>,
    mut commands: Commands,
) {
    for add in adds.read() {
        // The drop names the window it landed on, so the item joins *that*
        // notecard rather than whichever one was opened last.
        let Ok(state) = windows.get(add.editor) else {
            continue;
        };
        // Only a modifiable notecard with a live body can take an added item.
        if !state.editable {
            continue;
        }
        if !may_embed(&add.item) {
            warn!(
                "item {} has restricted next-owner permissions; not embedded",
                add.item.item_id
            );
            if let Some(status) = state.status {
                set_status(
                    &mut commands,
                    status,
                    "notecard-status-drop-restricted",
                    ERROR_COLOR,
                );
            }
            continue;
        }
        let Some(field_entity) = state.body_field else {
            continue;
        };
        let Ok((mut editable, mut body)) = fields.get_mut(field_entity) else {
            continue;
        };
        // The item's index is its position in the table, so appending it gives
        // it the next index — which no marker already in the text can alias
        // (`with_edited_text` resolves markers by position).
        let next_index = u32::try_from(body.baseline.items.len()).unwrap_or(u32::MAX);
        let Some(marker) = sl_notecard::embedded_char(next_index) else {
            warn!("notecard already holds the maximum embedded items; drop ignored");
            continue;
        };
        body.baseline.items.push(to_embedded_item(&add.item));
        let mut value = editable.value().to_string();
        value.push(marker);
        editable.editor_mut().set_text(&value);
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
    let (sale_type, sale_price) = (
        notecard_sale_type(item.sale.sale_type),
        i32::try_from(item.sale.price.0).unwrap_or(0),
    );
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
/// `crate::ui_test` sweeps its layout across every script, scale and font.
pub fn spawn_notecard_editor_specimen(
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
    populate_editor(commands, col, &notecard, true, source, cx.font_size);
    col
}

/// Spawn the rich read-only reader specimen: prose with a linkified URL and an
/// inline embedded item, built with no floater / session so `crate::ui_test`
/// sweeps the interleaved prose-run / item-box layout across every script,
/// scale and font.
pub fn spawn_notecard_reader_specimen(
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
        items: vec![item],
        text: format!(
            "{welcome} https://example.com\n{visit} {marker}",
            welcome = cx.text("Welcome! See"),
            visit = cx.text("or drop by"),
        ),
    };
    let source = NotecardSource::Agent {
        item_id: InventoryKey::from(Uuid::nil()),
    };
    populate_editor(commands, col, &notecard, false, source, cx.font_size);
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
        items: vec![item],
        text: format!("{text} {marker}"),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        decode_notecard_asset, proto_asset_type_name, proto_inv_type_name, to_embedded_item,
    };
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
            sale: sl_client_bevy::SaleInfo::default(),
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
            items: vec![to_embedded_item(&item)],
            text: format!("See {marker}"),
        };
        let decoded =
            sl_notecard::Notecard::decode(&notecard.encode()).map_err(|error| error.to_string())?;
        let survivor = decoded.items.first().ok_or("no embedded item")?;
        assert_eq!(survivor.asset_type, sl_notecard::AssetType::Landmark);
        assert_eq!(
            survivor.inventory_type,
            sl_notecard::InventoryType::Landmark
        );
        assert_eq!(survivor.name, "My Landmark");
        assert_eq!(survivor.permissions.owner_mask.0, 0x7fff_ffff);
        assert_eq!(survivor.permissions.creator_id.0, Uuid::from_u128(0x60));
        assert_eq!(survivor.asset_id.0, Uuid::from_u128(0x30));
        Ok(())
    }

    /// A brand-new notecard's asset opens as an **empty** notecard, whichever
    /// way the grid represents "nothing in it" — Second Life's empty container
    /// or OpenSim's single `0x00` byte. Anything else malformed still fails, so
    /// this is a reading of the grid's empty, not a swallowed error.
    #[test]
    fn an_empty_asset_reads_as_an_empty_notecard() -> TestResult {
        for empty in [b"".as_slice(), b"\0".as_slice(), b"\0\0\0\0".as_slice()] {
            let notecard =
                decode_notecard_asset(empty).map_err(|error| format!("empty asset: {error}"))?;
            assert_eq!(notecard.text, "");
            assert!(notecard.items.is_empty());
        }
        // A real notecard still decodes as itself.
        let encoded = sl_notecard::Notecard {
            source_version: sl_notecard::NotecardVersion::V2,
            items: Vec::new(),
            text: "hello".to_owned(),
        }
        .encode();
        let decoded =
            decode_notecard_asset(&encoded).map_err(|error| format!("round trip: {error}"))?;
        assert_eq!(decoded.text, "hello");
        // And a non-empty, non-notecard blob is still an error.
        assert_eq!(
            decode_notecard_asset(b"not a notecard at all").ok(),
            None,
            "a malformed asset must still be refused, not read as empty"
        );
        Ok(())
    }

    /// **One window per notecard** (`viewer-keyed-floater-audit`): the open
    /// path, driven by the message an inventory row's double-click writes.
    ///
    /// What the keying buys is not two windows for their own sake — it is that
    /// opening a second notecard cannot touch the first window's **unsaved
    /// text**, which the singleton editor tore down on every open.
    mod instances {
        use super::super::{NotecardEditorState, notecard_key, open_notecard};
        use crate::floater::{Floater, FloaterCommand, FloaterOp, FloaterPlugin};
        use crate::ui::UiRoot;
        use crate::world_api::{NotecardDropTarget, NotecardSource, OpenNotecard};
        use bevy::prelude::*;
        use pretty_assertions::assert_eq;
        use sl_client_bevy::{Command, InventoryKey, SlCommand, Uuid};

        /// A boxed error so tests can use `?` rather than the disallowed
        /// `unwrap` / `expect`.
        type TestError = Box<dyn core::error::Error>;

        /// Two notecards in the agent's own inventory.
        fn notecards() -> (NotecardSource, NotecardSource) {
            (
                NotecardSource::Agent {
                    item_id: InventoryKey::from(Uuid::from_u128(0xA1)),
                },
                NotecardSource::Agent {
                    item_id: InventoryKey::from(Uuid::from_u128(0xB2)),
                },
            )
        }

        /// An app with the floater manager and this module's open path. The
        /// systems that fill a window need the asset pipeline and a live
        /// session; the open is where the singleton bug lived.
        fn editor_app() -> App {
            let mut app = App::new();
            app.add_message::<SlCommand>()
                .add_message::<OpenNotecard>()
                .init_resource::<UiScale>()
                .init_resource::<ButtonInput<KeyCode>>()
                .add_plugins(FloaterPlugin)
                .add_systems(Update, open_notecard);
            let root = app.world_mut().spawn(Node::default()).id();
            app.insert_resource(UiRoot(root));
            app.update();
            app
        }

        /// Open a notecard the way every caller does — by writing the message.
        fn open(app: &mut App, source: NotecardSource, asset: u128, editable: bool) {
            app.world_mut().write_message(OpenNotecard {
                name: "A notecard".to_owned(),
                asset_id: Uuid::from_u128(asset),
                editable,
                source,
            });
            app.update();
        }

        /// Every live notecard window, as (entity, source) pairs.
        fn windows(app: &mut App) -> Vec<(Entity, NotecardSource)> {
            app.world_mut()
                .query::<(Entity, &NotecardEditorState)>()
                .iter(app.world())
                .map(|(entity, state)| (entity, state.source))
                .collect()
        }

        /// How many asset fetches the session was asked for.
        fn fetches(app: &App) -> usize {
            app.world()
                .resource::<Messages<SlCommand>>()
                .iter_current_update_messages()
                .filter(|command| matches!(command.0, Command::FetchAsset { .. }))
                .count()
        }

        /// Two notecards are two windows, each with its own source, its own
        /// content column and its own drop target.
        #[test]
        fn two_notecards_open_two_windows() -> Result<(), TestError> {
            let (first, second) = notecards();
            let mut app = editor_app();
            open(&mut app, first, 0xA1_00, true);
            open(&mut app, second, 0xB2_00, true);

            let open_windows = windows(&mut app);
            assert_eq!(
                open_windows.len(),
                2,
                "the second notecard reused the first window"
            );
            let sources: Vec<NotecardSource> = open_windows
                .iter()
                .map(|(_entity, source)| *source)
                .collect();
            assert!(sources.contains(&first) && sources.contains(&second));

            let world = app.world();
            let contents: Vec<Entity> = open_windows
                .iter()
                .filter_map(|(window, _source)| world.get::<NotecardEditorState>(*window))
                .map(|state| state.content)
                .collect();
            assert!(
                contents.first() != contents.get(1),
                "both windows build into one content column"
            );
            for (window, _source) in &open_windows {
                assert!(
                    world.get::<NotecardDropTarget>(*window).is_some(),
                    "a notecard window is not its own drop target"
                );
            }
            let keys: Vec<Option<&crate::floater::FloaterKey>> = open_windows
                .iter()
                .map(|(window, _source)| world.get::<Floater>(*window).and_then(Floater::key))
                .collect();
            assert!(keys.contains(&Some(&notecard_key(first))));
            assert!(keys.contains(&Some(&notecard_key(second))));
            Ok(())
        }

        /// **Re-opening a notecard that is already up never re-fetches it.**
        /// That is the unsaved-edit guarantee: a second open of the same
        /// notecard must raise its window, not replace the resident's text with
        /// the grid's copy.
        #[test]
        fn reopening_a_notecard_does_not_refetch_it() -> Result<(), TestError> {
            let (first, _second) = notecards();
            let mut app = editor_app();
            open(&mut app, first, 0xA1_00, true);
            assert_eq!(fetches(&app), 1, "the first open must fetch the asset");
            assert_eq!(windows(&mut app).len(), 1);

            open(&mut app, first, 0xA1_00, true);
            assert_eq!(
                fetches(&app),
                0,
                "a re-open re-fetched the asset, which would overwrite unsaved edits"
            );
            assert_eq!(
                windows(&mut app).len(),
                1,
                "a re-open spawned a second window"
            );
            Ok(())
        }

        /// Closing one notecard ends that window — its baseline, its field and
        /// its pending save go with it — and leaves the other open.
        #[test]
        fn closing_one_notecard_leaves_the_other() -> Result<(), TestError> {
            let (first, second) = notecards();
            let mut app = editor_app();
            open(&mut app, first, 0xA1_00, true);
            open(&mut app, second, 0xB2_00, true);
            let target = windows(&mut app)
                .into_iter()
                .find_map(|(window, source)| (source == first).then_some(window))
                .ok_or("the first notecard has no window")?;

            app.world_mut()
                .resource_mut::<Messages<FloaterCommand>>()
                .write(FloaterCommand {
                    floater: target,
                    op: FloaterOp::Close,
                });
            app.update();

            let left = windows(&mut app);
            assert_eq!(left.len(), 1);
            assert_eq!(left.first().map(|(_window, source)| *source), Some(second));
            Ok(())
        }
    }

    /// **The body is one flowing text with its items in it**
    /// (`viewer-notecard-inline-items`): the model the rich-text field lays out
    /// — a box per embedded item, at the byte offset of the marker that names
    /// it, with that marker taken off the screen.
    ///
    /// These tests replaced a set that drove a **toggle** between a plain edit
    /// field and a separate read-only preview. The bug that set was written for
    /// (a preview showing the notecard as loaded rather than as typed) cannot
    /// occur here: there is one buffer, and the boxes are placed against it.
    mod body {
        use super::super::{
            NotecardBody, NotecardEditorState, ingest_added_items, ingest_notecard_asset,
            open_notecard, rebuild_notecard_body,
        };
        use crate::floater::FloaterPlugin;
        use crate::inventory::AddEmbeddedItem;
        use crate::ui::UiRoot;
        use crate::world_api::{NotecardSource, OpenNotecard};
        use bevy::input_focus::InputFocus;
        use bevy::prelude::*;
        use bevy::text::EditableText;
        use pretty_assertions::assert_eq;
        use sl_client_bevy::{
            AgentKey, Asset, AssetType, InventoryFolderKey, InventoryKey, InventoryType, ItemInfo,
            OwnerKey, Permissions, Permissions5, SaleInfo, SlCommand, SlEvent, SlSessionEvent,
            Uuid,
        };
        use sl_viewer_ui_widgets::ui_rich_text::{RichTextContent, RichTextStyle};

        /// A boxed error so tests can use `?` rather than the disallowed
        /// `unwrap` / `expect`.
        type TestError = Box<dyn core::error::Error>;

        /// The notecard the window is opened on.
        const NOTECARD_ITEM: u128 = 0xC3;

        /// The asset id its body arrives under.
        const NOTECARD_ASSET: u128 = 0xC3_00;

        /// The prose the notecard arrives with.
        const LOADED_TEXT: &str = "as it was saved";

        /// An app with the floater manager, the open path and the per-window
        /// pass that fills a window: the asset ingest that builds the body, the
        /// drop that adds an embedded item, and the rebuild that turns the
        /// buffer into the field's model.
        fn editor_app() -> App {
            let mut app = App::new();
            app.add_message::<SlCommand>()
                .add_message::<SlEvent>()
                .add_message::<OpenNotecard>()
                .add_message::<AddEmbeddedItem>()
                .init_resource::<UiScale>()
                .init_resource::<InputFocus>()
                .init_resource::<ButtonInput<KeyCode>>()
                .add_plugins(FloaterPlugin)
                .add_systems(
                    Update,
                    (
                        open_notecard,
                        ingest_notecard_asset,
                        ingest_added_items,
                        rebuild_notecard_body,
                    )
                        .chain(),
                );
            let root = app.world_mut().spawn(Node::default()).id();
            app.insert_resource(UiRoot(root));
            app.update();
            app
        }

        /// Open the notecard and hand it a body carrying `text` and `items`.
        fn open_with(
            app: &mut App,
            editable: bool,
            text: &str,
            items: Vec<sl_notecard::InventoryItem>,
        ) {
            app.world_mut().write_message(OpenNotecard {
                name: "A notecard".to_owned(),
                asset_id: Uuid::from_u128(NOTECARD_ASSET),
                editable,
                source: NotecardSource::Agent {
                    item_id: InventoryKey::from(Uuid::from_u128(NOTECARD_ITEM)),
                },
            });
            app.update();
            let data = sl_notecard::Notecard {
                source_version: sl_notecard::NotecardVersion::V2,
                items,
                text: text.to_owned(),
            }
            .encode();
            app.world_mut()
                .write_message(SlEvent(SlSessionEvent::AssetReceived(Box::new(Asset {
                    id: Uuid::from_u128(NOTECARD_ASSET),
                    asset_type: AssetType::Notecard,
                    data,
                }))));
            app.update();
            // One more pass: the body is spawned by the ingest's commands, so
            // its first model is built on the frame after.
            app.update();
        }

        /// The one open window.
        fn window(app: &mut App) -> Result<Entity, TestError> {
            app.world_mut()
                .query_filtered::<Entity, With<NotecardEditorState>>()
                .iter(app.world())
                .next()
                .ok_or_else(|| TestError::from("no notecard window is open"))
        }

        /// The body field of the one open window.
        fn body_field(app: &mut App) -> Result<Entity, TestError> {
            app.world_mut()
                .query_filtered::<Entity, With<NotecardBody>>()
                .iter(app.world())
                .next()
                .ok_or_else(|| TestError::from("the window has no body"))
        }

        /// The body's model, as the rich-text field will lay it out.
        fn model(app: &mut App) -> Result<RichTextContent, TestError> {
            let field = body_field(app)?;
            app.world()
                .get::<RichTextContent>(field)
                .cloned()
                .ok_or_else(|| TestError::from("the body has no model"))
        }

        /// Set the body's buffer the way typing does.
        fn type_into_body(app: &mut App, text: &str) -> Result<(), TestError> {
            let field = body_field(app)?;
            let mut editable = app
                .world_mut()
                .get_mut::<EditableText>(field)
                .ok_or("the body field is not editable")?;
            editable.editor_mut().set_text(text);
            app.update();
            Ok(())
        }

        /// Every text run under `entity`, in tree order — what a box reads as.
        fn text_under(app: &App, entity: Entity) -> Vec<String> {
            let mut runs = Vec::new();
            let mut stack = vec![entity];
            while let Some(entity) = stack.pop() {
                if let Some(text) = app.world().get::<Text>(entity) {
                    runs.push(text.0.clone());
                }
                if let Some(children) = app.world().get::<Children>(entity) {
                    stack.extend(children.iter());
                }
            }
            runs
        }

        /// A landmark embedded in the notecard's table.
        fn embedded_landmark(name: &str) -> sl_notecard::InventoryItem {
            sl_notecard::InventoryItem {
                item_id: sl_types::key::Key(Uuid::from_u128(0xE1)),
                parent_id: sl_types::key::NULL_KEY,
                permissions: sl_notecard::Permissions::default(),
                metadata: None,
                asset_id: sl_types::key::Key(Uuid::from_u128(0xE2)),
                asset_id_encoding: sl_notecard::AssetIdEncoding::Plain,
                asset_type: sl_notecard::AssetType::Landmark,
                inventory_type: sl_notecard::InventoryType::Landmark,
                flags: 0,
                sale_info: sl_notecard::SaleInfo::default(),
                name: name.to_owned(),
                description: String::new(),
                creation_date: 0,
                unknown_fields: Vec::new(),
            }
        }

        /// An inventory item a resident drags onto the open notecard.
        fn dropped_item(name: &str) -> ItemInfo {
            dropped_item_with_next_owner(name, Permissions::from_bits(0x0008_e000))
        }

        /// The same dragged item at a chosen **next-owner** mask, which is what
        /// decides whether a notecard may hold it at all (see `may_embed`).
        fn dropped_item_with_next_owner(name: &str, next_owner: Permissions) -> ItemInfo {
            ItemInfo {
                item_id: InventoryKey::from(Uuid::from_u128(0xD1)),
                folder_id: InventoryFolderKey::from(Uuid::from_u128(0xD2)),
                name: name.to_owned(),
                description: String::new(),
                asset_id: Uuid::from_u128(0xD3),
                asset_type: AssetType::Landmark,
                inv_type: InventoryType::Landmark,
                flags: 0,
                sale: SaleInfo::default(),
                creation_date: 0,
                owner: OwnerKey::Agent(AgentKey::from(Uuid::from_u128(0xD4))),
                last_owner_id: Uuid::from_u128(0xD4),
                creator_id: AgentKey::from(Uuid::from_u128(0xD5)),
                group: None,
                permissions: Permissions5 {
                    base: Permissions::from_bits(0x7fff_ffff),
                    owner: Permissions::from_bits(0x7fff_ffff),
                    group: Permissions::empty(),
                    everyone: Permissions::empty(),
                    next_owner,
                },
            }
        }

        /// An embedded item is a box at its marker's byte offset, and the marker
        /// is hidden — the editable body included, which is what the toggle
        /// used to stand in for.
        #[test]
        fn an_item_is_a_box_where_its_marker_sits() -> Result<(), TestError> {
            let marker = sl_notecard::embedded_char(0).ok_or("no marker")?;
            let text = format!("visit {marker} today");
            let offset = text.find(marker).ok_or("no marker in the fixture")?;
            let mut app = editor_app();
            open_with(&mut app, true, &text, vec![embedded_landmark("Our Home")]);

            let model = model(&mut app)?;
            let object = model
                .objects
                .first()
                .ok_or("the body drew no box for the embedded item")?;
            assert_eq!(object.index, offset, "the box is not where the marker is");
            assert!(
                model
                    .ranges
                    .iter()
                    .any(|styled| styled.style == RichTextStyle::Hidden
                        && styled.range.start == offset),
                "the marker was left on the screen: {:?}",
                model.ranges
            );
            assert!(
                text_under(&app, object.entity)
                    .iter()
                    .any(|run| run == "Our Home"),
                "the box does not name the item"
            );
            Ok(())
        }

        /// Typing prose around an item **moves** its box rather than rebuilding
        /// it: the same entity, at its new offset. A rebuild per keystroke is
        /// what the floaters' build-once rule forbids, and it would also throw
        /// away a link box's in-flight name lookup.
        #[test]
        fn typing_moves_a_box_it_does_not_rebuild_it() -> Result<(), TestError> {
            let marker = sl_notecard::embedded_char(0).ok_or("no marker")?;
            let mut app = editor_app();
            open_with(
                &mut app,
                true,
                &format!("a{marker}"),
                vec![embedded_landmark("Our Home")],
            );
            let before = model(&mut app)?
                .objects
                .first()
                .copied()
                .ok_or("no box for the item")?;

            type_into_body(&mut app, &format!("abcd{marker}"))?;

            let after = model(&mut app)?
                .objects
                .first()
                .copied()
                .ok_or("the box vanished when prose was typed")?;
            assert_eq!(
                after.entity, before.entity,
                "the item's box was despawned and respawned for a keystroke"
            );
            assert!(
                after.index > before.index,
                "the box did not follow the text that was typed before it"
            );
            Ok(())
        }

        /// An item dropped **since the load** joins the table and appears as its
        /// own box, because the drop appends its marker to the buffer the model
        /// is built from.
        #[test]
        fn an_item_dropped_since_the_load_appears_as_a_box() -> Result<(), TestError> {
            let mut app = editor_app();
            open_with(&mut app, true, LOADED_TEXT, Vec::new());
            let editor = window(&mut app)?;
            assert!(model(&mut app)?.objects.is_empty());

            app.world_mut().write_message(AddEmbeddedItem {
                item: dropped_item("Our Home"),
                editor,
            });
            app.update();
            app.update();

            let model = model(&mut app)?;
            let object = model
                .objects
                .first()
                .ok_or("the dropped item drew no box")?;
            assert!(
                text_under(&app, object.entity)
                    .iter()
                    .any(|run| run == "Our Home"),
                "the box does not name the dropped item"
            );
            Ok(())
        }

        /// An item whose **next-owner** permissions are restricted is refused,
        /// as the reference refuses it: a copy taken back out of a notecard is
        /// handed the next-owner permissions, so embedding one of these would
        /// promise a copy that arrives stripped of what the resident saw.
        #[test]
        fn a_next_owner_restricted_item_is_not_embedded() -> Result<(), TestError> {
            let mut app = editor_app();
            open_with(&mut app, true, LOADED_TEXT, Vec::new());
            let editor = window(&mut app)?;

            // Move and transfer, but neither copy nor modify for the next
            // owner — a "no copy" item.
            app.world_mut().write_message(AddEmbeddedItem {
                item: dropped_item_with_next_owner(
                    "Borrowed Thing",
                    Permissions::from_bits(0x0008_2000),
                ),
                editor,
            });
            app.update();
            app.update();

            assert!(
                model(&mut app)?.objects.is_empty(),
                "a next-owner-restricted item was embedded anyway"
            );
            Ok(())
        }

        /// A **read-only** body draws each link as the resolved chip the rest of
        /// the viewer draws, over the hidden URL text; an **editable** one
        /// leaves the URL as the text it is and styles it in place.
        #[test]
        fn a_link_is_a_chip_to_read_and_a_styled_range_to_edit() -> Result<(), TestError> {
            let text = "see https://example.com now";

            let mut reading = editor_app();
            open_with(&mut reading, false, text, Vec::new());
            let read_model = model(&mut reading)?;
            assert_eq!(
                read_model.objects.len(),
                1,
                "a read-only body should draw the link as a box"
            );
            assert!(
                read_model
                    .ranges
                    .iter()
                    .all(|styled| styled.style == RichTextStyle::Hidden),
                "the URL under the box should be hidden: {:?}",
                read_model.ranges
            );

            let mut editing = editor_app();
            open_with(&mut editing, true, text, Vec::new());
            let edit_model = model(&mut editing)?;
            assert!(
                edit_model.objects.is_empty(),
                "an editable body should leave the URL as text, not draw a box over it"
            );
            let styled = edit_model
                .ranges
                .first()
                .ok_or("the editable body did not style its link")?;
            assert_eq!(styled.style, RichTextStyle::Class(super::super::LINK_CLASS));
            assert_eq!(
                text.get(styled.range.clone()),
                Some("https://example.com"),
                "the styled range is not the link"
            );
            Ok(())
        }
    }
}
