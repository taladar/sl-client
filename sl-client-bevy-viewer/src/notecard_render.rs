//! The **rich read-only rendering of a notecard body** (part of
//! [`viewer-notecard-editor`]): draws a decoded [`sl_notecard::Notecard`] as
//! flowing text with its **embedded inventory items** shown *inline* as
//! clickable icon-and-name boxes, and its prose URLs / SLURLs / `secondlife://`
//! app links linkified — the reference `llviewertexteditor`'s embedded-item
//! reader, minus the *editing* half.
//!
//! # Why this exists as its own path
//!
//! Bevy 0.19's editable text field is `parley::PlainEditor`: one style for the
//! whole buffer, no inline boxes. Rendering embedded items *inline while
//! editing* waits on the inline-box rich-text widget
//! ([`viewer-lsl-editor-widget`], not yet built). But a **read-only** reader
//! needs no caret, so it can lay the body out the way [`crate::linkified_text`]
//! already does: **discrete pickable nodes** in a wrapping row — each embedded
//! item a real clickable box, each prose run linkified natively — rather than
//! hit-testing glyph rects over one laid-out block. That is faithful to the
//! reference's *matching* semantics and is what a notecard reader needs.
//!
//! # Layout
//!
//! The body is a column of **lines** (split on `\n`); each line is a wrapping
//! row that interleaves linkified prose runs with the embedded-item boxes the
//! text references positionally (a `FIRST_EMBEDDED_CHAR + index` code point
//! stands where each item sits). A blank line keeps its height with a spacer.
//!
//! # Clicking an embedded item (reference `openEmbeddedItem`)
//!
//! - a **calling card** opens the named avatar's profile (the reference uses
//!   the card's description-uuid, else its creator);
//! - a **texture / snapshot** opens the texture preview;
//! - **every other type** copies the embedded item into the agent's inventory
//!   over [`Command::CopyInventoryFromNotecard`], behind the reference
//!   `ConfirmItemCopy` confirmation ("Copy this item to your inventory?") — the
//!   universal "keep this item" action for a landmark, object, notecard,
//!   wearable, … a resident dropped into the body.
//!
//! Reference (Firestorm, read-only): `llviewertexteditor` (the embedded-item
//! segment rendering + `openEmbeddedItem`).

use std::collections::VecDeque;

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::ui_widgets::Button;
use sl_client_bevy::{
    AgentKey, AssetType, Command, InventoryFolderKey, InventoryKey, InventoryType, ItemInfo,
    ObjectKey, OwnerKey, Permissions5, SlCommand, Uuid,
};

use crate::edit_notecard::{NotecardSource, embedded_icon};
use crate::inventory_properties::OpenItemPreview;
use crate::linkified_text::{LinkTextStyle, populate_linkified_text};
use crate::notifications::{NotificationResponse, ShowNotification};
use crate::ui_font::UiFont;
use crate::world_api::OpenAvatarProfile;

/// The catalogue template for the copy-embedded-item confirmation — the
/// reference `ConfirmItemCopy` alertmodal ("Copy this item to your inventory?").
const CONFIRM_ITEM_COPY_TEMPLATE: &str = "ConfirmItemCopy";

/// The affirmative button's stable functor name on `ConfirmItemCopy`.
const CONFIRM_ITEM_COPY_BUTTON: &str = "OK";

/// The embedded-item box's resting background — a faint pill so an inline item
/// reads as a distinct, clickable object in the prose.
const ITEM_BACKGROUND: Color = Color::srgba(0.20, 0.24, 0.32, 0.55);

/// The embedded-item box's hovered background (brighter, to signal the click).
const ITEM_BACKGROUND_HOVER: Color = Color::srgba(0.28, 0.36, 0.52, 0.85);

// ---------------------------------------------------------------------------
// The public builder.
// ---------------------------------------------------------------------------

/// Build the read-only rich body of `notecard` under `parent`, returning the
/// column entity. `source` locates the notecard so a copied embedded item names
/// the right notecard / holding prim; `style` sets the font size and colours
/// (prose vs. link).
pub(crate) fn spawn_notecard_body(
    commands: &mut Commands,
    parent: Entity,
    notecard: &sl_notecard::Notecard,
    source: NotecardSource,
    style: LinkTextStyle,
) -> Entity {
    let column = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                width: Val::Percent(100.0),
                ..default()
            },
            Name::new("notecard-body"),
            ChildOf(parent),
        ))
        .id();
    // Split on `\n`: each line lays out as its own wrapping row, so a paragraph
    // break is a real line break rather than a run continuing on the flex row.
    for line in notecard.text.split('\n') {
        spawn_line(commands, column, line, notecard, source, style);
    }
    column
}

/// Spawn one line as a wrapping row that interleaves linkified prose runs with
/// the embedded-item boxes the line's marker code points reference. A line with
/// no rendered content (a blank paragraph line) keeps its height with a spacer.
fn spawn_line(
    commands: &mut Commands,
    column: Entity,
    line: &str,
    notecard: &sl_notecard::Notecard,
    source: NotecardSource,
    style: LinkTextStyle,
) {
    let row = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Row,
                flex_wrap: FlexWrap::Wrap,
                align_items: AlignItems::Center,
                width: Val::Percent(100.0),
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(column),
        ))
        .id();
    let mut prose = String::new();
    let mut rendered = false;
    for character in line.chars() {
        if let Some(index) = sl_notecard::embedded_char_index(character) {
            // Flush the prose before this marker, then draw the item inline.
            if !prose.is_empty() {
                populate_linkified_text(commands, row, &prose, style);
                prose.clear();
                rendered = true;
            }
            if let Some(embedded) = notecard.item_by_index(index) {
                spawn_embedded_box(commands, row, embedded, source, style);
                rendered = true;
            }
        } else {
            prose.push(character);
        }
    }
    if !prose.is_empty() {
        populate_linkified_text(commands, row, &prose, style);
        rendered = true;
    }
    if !rendered {
        // A blank line: a single space keeps the row's line height so paragraph
        // spacing survives.
        commands.spawn((
            Text::new(" ".to_owned()),
            UiFont::Sans.at(style.font_size),
            TextColor(style.plain_color),
            Pickable::IGNORE,
            ChildOf(row),
        ));
    }
}

// ---------------------------------------------------------------------------
// The embedded-item box.
// ---------------------------------------------------------------------------

/// A rendered inline embedded-item box; its click observer runs [`action`].
#[derive(Component, Debug, Clone)]
struct EmbeddedItemBox {
    /// What clicking the item does (copy to inventory, open profile, preview).
    action: EmbeddedAction,
}

/// A copy-embedded-item-into-inventory target (the reference default action).
#[derive(Debug, Clone, Copy)]
struct CopyTarget {
    /// The notecard's own inventory item.
    notecard: InventoryKey,
    /// The prim holding the notecard, or `None` for an agent-inventory one.
    holder: Option<ObjectKey>,
    /// The embedded item to copy.
    item: InventoryKey,
}

/// The click action resolved for an embedded item from its asset class — the
/// reference `openEmbeddedItem` switch. The payloads are boxed so no one
/// variant dwarfs the others.
#[derive(Debug, Clone)]
enum EmbeddedAction {
    /// Copy the item into the agent's inventory (the reference default for a
    /// landmark, object, notecard, wearable, animation, gesture, sound, …).
    Copy(Box<CopyTarget>),
    /// Open an avatar's profile — a calling card.
    Profile(AgentKey),
    /// Open the texture preview — a texture / snapshot.
    Texture(Box<ItemInfo>),
}

/// Resolve the click action for `item`, given where the notecard lives.
fn resolve_action(item: &sl_notecard::InventoryItem, source: NotecardSource) -> EmbeddedAction {
    use sl_notecard::AssetType as A;
    match &item.asset_type {
        // A calling card opens its avatar's profile: the reference reads the
        // agent id from the card's description, falling back to its creator.
        A::CallingCard => {
            let agent = Uuid::parse_str(item.description.trim()).map_or_else(
                |_invalid| AgentKey::from(item.permissions.creator_id.0),
                AgentKey::from,
            );
            EmbeddedAction::Profile(agent)
        }
        // A texture / snapshot opens the texture preview.
        A::Texture | A::TextureTga | A::ImageTga | A::ImageJpeg => {
            EmbeddedAction::Texture(Box::new(texture_item_info(item)))
        }
        // Everything else copies into inventory.
        _other => EmbeddedAction::Copy(Box::new(CopyTarget {
            notecard: source.item_id(),
            holder: source.object_id(),
            item: InventoryKey::from(item.item_id.0),
        })),
    }
}

/// A minimal [`ItemInfo`] for opening a texture-class embedded item in the
/// texture preview: the preview reads only the asset id + name + inventory
/// type, so the ownership / permission fields are left nil.
fn texture_item_info(item: &sl_notecard::InventoryItem) -> ItemInfo {
    ItemInfo {
        item_id: InventoryKey::from(item.item_id.0),
        folder_id: InventoryFolderKey::from(Uuid::nil()),
        name: item.name.clone(),
        description: item.description.clone(),
        asset_id: item.asset_id.0,
        asset_type: AssetType::Texture,
        inv_type: InventoryType::Texture,
        flags: 0,
        sale: None,
        creation_date: 0,
        owner: OwnerKey::Agent(AgentKey::from(Uuid::nil())),
        last_owner_id: Uuid::nil(),
        creator_id: AgentKey::from(item.permissions.creator_id.0),
        group: None,
        permissions: Permissions5::empty(),
    }
}

/// Spawn one inline embedded-item box (icon + name) under a line `row`.
fn spawn_embedded_box(
    commands: &mut Commands,
    row: Entity,
    embedded: &sl_notecard::EmbeddedItem,
    source: NotecardSource,
    style: LinkTextStyle,
) {
    let action = resolve_action(&embedded.item, source);
    let item_box = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Px(3.0),
                padding: UiRect::axes(Val::Px(4.0), Val::Px(0.0)),
                ..default()
            },
            BackgroundColor(ITEM_BACKGROUND),
            Button,
            TabIndex(0),
            Pickable::default(),
            EmbeddedItemBox { action },
            ChildOf(row),
        ))
        .id();
    commands.spawn((
        Text::new(embedded_icon(&embedded.item.asset_type).to_owned()),
        UiFont::Sans.at(style.font_size),
        TextColor(style.link_color),
        Pickable::IGNORE,
        ChildOf(item_box),
    ));
    commands.spawn((
        Text::new(embedded.item.name.clone()),
        UiFont::Sans.at(style.font_size),
        TextColor(style.link_color),
        Pickable::IGNORE,
        ChildOf(item_box),
    ));
    commands
        .entity(item_box)
        .observe(on_embedded_press)
        .observe(on_embedded_over)
        .observe(on_embedded_out);
}

// ---------------------------------------------------------------------------
// Observers: hover highlight + click dispatch.
// ---------------------------------------------------------------------------

/// On a primary press, run the item's resolved action. A **copy** is guarded by
/// the reference `ConfirmItemCopy` confirmation — the target is parked until the
/// dialog is answered ([`handle_embedded_copy_confirmations`]) — so a click
/// never silently spawns an inventory item; a profile / texture open is direct.
fn on_embedded_press(
    press: On<Pointer<Press>>,
    boxes: Query<&EmbeddedItemBox>,
    mut pending: ResMut<PendingEmbeddedCopies>,
    mut notifications: MessageWriter<ShowNotification>,
    mut profiles: MessageWriter<OpenAvatarProfile>,
    mut previews: MessageWriter<OpenItemPreview>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let Ok(item_box) = boxes.get(press.entity) else {
        return;
    };
    match &item_box.action {
        EmbeddedAction::Copy(target) => {
            // Park the copy behind the confirmation modal, one per queued dialog.
            pending.queue.push_back(**target);
            notifications.write(ShowNotification::new(CONFIRM_ITEM_COPY_TEMPLATE));
        }
        EmbeddedAction::Profile(agent) => {
            profiles.write(OpenAvatarProfile { agent: *agent });
        }
        EmbeddedAction::Texture(item) => {
            previews.write(OpenItemPreview {
                item: (**item).clone(),
            });
        }
    }
}

/// The copy targets awaiting their `ConfirmItemCopy` answer, oldest first — the
/// modal is answered in order, so each response resolves the front of the queue
/// (the reference parks the item in the notification payload; we park it here).
#[derive(Resource, Debug, Default)]
pub(crate) struct PendingEmbeddedCopies {
    /// The parked copies, front = the dialog raised first.
    queue: VecDeque<CopyTarget>,
}

/// Answer each `ConfirmItemCopy`: on **Copy** issue the parked
/// [`Command::CopyInventoryFromNotecard`]; on cancel / dismiss drop it.
fn handle_embedded_copy_confirmations(
    mut responses: MessageReader<NotificationResponse>,
    mut pending: ResMut<PendingEmbeddedCopies>,
    mut sl_commands: MessageWriter<SlCommand>,
) {
    for response in responses.read() {
        if response.template != CONFIRM_ITEM_COPY_TEMPLATE {
            continue;
        }
        let Some(target) = pending.queue.pop_front() else {
            continue;
        };
        if response.button == Some(CONFIRM_ITEM_COPY_BUTTON) {
            sl_commands.write(SlCommand(Command::CopyInventoryFromNotecard {
                notecard_id: target.notecard,
                object_id: target.holder,
                item_id: target.item,
                folder_id: None,
            }));
        }
    }
}

/// The plugin owning the notecard reader's confirm-to-copy routing (the reader's
/// per-item observers are attached at spawn and need no registration).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct NotecardRenderPlugin;

impl Plugin for NotecardRenderPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PendingEmbeddedCopies>()
            .add_systems(Update, handle_embedded_copy_confirmations);
    }
}

/// Brighten the hovered item box.
fn on_embedded_over(
    over: On<Pointer<Over>>,
    mut colors: Query<&mut BackgroundColor, With<EmbeddedItemBox>>,
) {
    if let Ok(mut color) = colors.get_mut(over.entity) {
        color.0 = ITEM_BACKGROUND_HOVER;
    }
}

/// Restore the item box's resting background on pointer-out.
fn on_embedded_out(
    out: On<Pointer<Out>>,
    mut colors: Query<&mut BackgroundColor, With<EmbeddedItemBox>>,
) {
    if let Ok(mut color) = colors.get_mut(out.entity) {
        color.0 = ITEM_BACKGROUND;
    }
}

#[cfg(test)]
mod tests {
    use super::{EmbeddedAction, resolve_action};
    use crate::edit_notecard::NotecardSource;
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{InventoryKey, Uuid};

    /// A one-item notecard fixture helper: an embedded item of a given type.
    fn item(asset_type: sl_notecard::AssetType, description: &str) -> sl_notecard::InventoryItem {
        sl_notecard::InventoryItem {
            item_id: sl_types::key::Key(Uuid::from_u128(0x42)),
            parent_id: sl_types::key::NULL_KEY,
            permissions: sl_notecard::Permissions {
                creator_id: sl_types::key::Key(Uuid::from_u128(0x99)),
                ..sl_notecard::Permissions::default()
            },
            metadata: None,
            asset_id: sl_types::key::Key(Uuid::from_u128(0x7)),
            asset_id_encoding: sl_notecard::AssetIdEncoding::Plain,
            asset_type,
            inventory_type: sl_notecard::InventoryType::None,
            flags: 0,
            sale_info: sl_notecard::SaleInfo::default(),
            name: "Thing".to_owned(),
            description: description.to_owned(),
            creation_date: 0,
            unknown_fields: Vec::new(),
        }
    }

    /// A landmark (and most types) resolves to a copy naming the notecard + item.
    #[test]
    fn most_types_copy_into_inventory() {
        let notecard = InventoryKey::from(Uuid::from_u128(0x1));
        let source = NotecardSource::Agent { item_id: notecard };
        let action = resolve_action(&item(sl_notecard::AssetType::Landmark, ""), source);
        assert!(
            matches!(&action, EmbeddedAction::Copy(_)),
            "a landmark should copy into inventory"
        );
        if let EmbeddedAction::Copy(target) = &action {
            assert_eq!(target.notecard, notecard);
            assert_eq!(target.holder, None);
            assert_eq!(target.item, InventoryKey::from(Uuid::from_u128(0x42)));
        }
    }

    /// A calling card opens its description's agent profile, falling back to the
    /// creator when the description is not a uuid.
    #[test]
    fn calling_card_opens_a_profile() {
        let source = NotecardSource::Agent {
            item_id: InventoryKey::from(Uuid::from_u128(0x1)),
        };
        // A description holding a uuid names that agent.
        let described = Uuid::from_u128(0xABCD);
        let action = resolve_action(
            &item(sl_notecard::AssetType::CallingCard, &described.to_string()),
            source,
        );
        assert!(
            matches!(&action, EmbeddedAction::Profile(_)),
            "a calling card should open a profile"
        );
        if let EmbeddedAction::Profile(agent) = &action {
            assert_eq!(agent.uuid(), described);
        }
        // A non-uuid description falls back to the creator.
        let action = resolve_action(
            &item(sl_notecard::AssetType::CallingCard, "not a uuid"),
            source,
        );
        assert!(
            matches!(&action, EmbeddedAction::Profile(_)),
            "a calling card should open a profile"
        );
        if let EmbeddedAction::Profile(agent) = &action {
            assert_eq!(agent.uuid(), Uuid::from_u128(0x99));
        }
    }

    /// A texture opens the texture preview carrying the item's asset id.
    #[test]
    fn texture_opens_the_preview() {
        let source = NotecardSource::Agent {
            item_id: InventoryKey::from(Uuid::from_u128(0x1)),
        };
        let action = resolve_action(&item(sl_notecard::AssetType::Texture, ""), source);
        assert!(
            matches!(&action, EmbeddedAction::Texture(_)),
            "a texture should open the preview"
        );
        if let EmbeddedAction::Texture(info) = &action {
            assert_eq!(info.asset_id, Uuid::from_u128(0x7));
        }
    }
}
