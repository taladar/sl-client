//! The **embedded inventory item** a notecard body carries (part of
//! `viewer-notecard-editor`): the clickable icon-and-name box drawn where the
//! text references an item, and what a click on one does — the reference
//! `llviewertexteditor`'s embedded-item segment.
//!
//! # Where the box is drawn
//!
//! The body itself is one laid-out buffer: the rich-text field
//! ([`sl_viewer_ui_widgets::ui_rich_text`]) reserves a box in the text flow at
//! the byte offset of each item's marker code point and positions the node this
//! module fills there. The marker itself is styled away, so what the resident
//! sees at that point in the sentence is the item — while the character is
//! still in the buffer, so deleting it removes the item and a save reconciles
//! the table against what is left.
//!
//! Before that field existed this module drew the body itself, as a column of
//! wrapping rows of discrete nodes. That reads fine and edits not at all: a
//! caret belongs to one laid-out buffer and cannot walk a row of sibling nodes,
//! which is why an editable notecard used to need a *toggle* to a separate
//! read-only preview to make its items legible.
//!
//! # Clicking an embedded item (reference `openEmbeddedItem`)
//!
//! - a **calling card** opens the named avatar's profile (the reference uses
//!   the card's description-uuid, else its creator);
//! - a **texture / snapshot** opens the texture preview;
//! - a **sound** plays locally — heard by this viewer only, on the UI bus —
//!   and then offers to copy it into inventory;
//! - a **landmark** opens the About Landmark window on it, with its Teleport
//!   button (the reference's place details). It is shown, not copied: the
//!   reference's own `EmbeddedLandmarkCopyToInventory` off, and the copy is one
//!   right-click away;
//! - a **material** opens the material editor on it, without a Save — it is
//!   not an item of the agent's to save onto;
//! - **every other type** copies the embedded item into the agent's inventory
//!   over [`Command::CopyInventoryFromNotecard`], behind the reference
//!   `ConfirmItemCopy` confirmation ("Copy this item to your inventory?") — the
//!   universal "keep this item" action for an object, notecard, wearable, … a
//!   resident dropped into the body.
//!
//! An item dropped into the body **since the notecard was last saved** is not
//! in the stored asset yet, so the grid can neither copy it nor serve it: any
//! click on one asks to save the notecard first (the reference
//! `ConfirmNotecardSave`), and does nothing else.
//!
//! A **right-click** offers the reference's (Catznip's) two-line menu, *Open*
//! and *Copy to Inventory*, so a landmark or a material — whose click opens
//! rather than copies — can still be kept.
//!
//! Reference (Firestorm, read-only): `llviewertexteditor` (the embedded-item
//! segment rendering, `openEmbeddedItem`, `showCopyToInvDialog`),
//! `menu_embedded_item.xml`.

use std::collections::{HashSet, VecDeque};

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::ui_widgets::Button;
use bevy_flair::style::components::ClassList;
use sl_client_bevy::{
    AgentKey, AssetKey, AssetType, Command, GroupKey, InventoryFolderKey, InventoryKey,
    InventoryType, ItemInfo, ObjectKey, OwnerKey, Permissions, Permissions5, SaleInfo, SlCommand,
    Uuid,
};
use sl_viewer_ui_sounds::ui_sounds::PlayAssetSound;
use sl_viewer_ui_widgets::menu::{MenuCommand, MenuDef, MenuItemDef, OpenContextMenu};

use crate::edit_notecard::embedded_icon;
use crate::intents::NotecardSource;
use crate::intents::OpenAvatarProfile;
use crate::inventory::{OpenAboutLandmark, OpenMaterialEditor};
use crate::inventory_properties::OpenItemPreview;
use crate::linkified_text::LinkTextStyle;
use crate::notifications::{NotificationResponse, ShowNotification};
use crate::ui_element::UiAction;
use crate::ui_font::UiFont;

/// The catalogue template for the copy-embedded-item confirmation — the
/// reference `ConfirmItemCopy` alertmodal ("Copy this item to your inventory?").
const CONFIRM_ITEM_COPY_TEMPLATE: &str = "ConfirmItemCopy";

/// The affirmative button's stable functor name on `ConfirmItemCopy`.
const CONFIRM_ITEM_COPY_BUTTON: &str = "OK";

/// The catalogue template asking to save the notecard before an item dropped
/// into it since the last save can be opened or copied — the reference
/// `ConfirmNotecardSave`.
pub(crate) const CONFIRM_NOTECARD_SAVE_TEMPLATE: &str = "ConfirmNotecardSave";

/// The affirmative button's stable functor name on `ConfirmNotecardSave`.
pub(crate) const CONFIRM_NOTECARD_SAVE_BUTTON: &str = "OK";

/// The element an embedded item's context menu attributes its actions to.
const EMBEDDED_MENU_ELEMENT: &str = "notecard-embedded-item";

/// The context menu's *Open* action.
const ACTION_OPEN: &str = "open";

/// The context menu's *Copy to Inventory* action.
const ACTION_COPY: &str = "copy-to-inventory";

/// The right-click menu on an embedded item — the reference's
/// `menu_embedded_item.xml`, whose two lines are always enabled.
static EMBEDDED_MENU: MenuDef = MenuDef {
    label_key: "menu-embedded-item",
    items: &[
        MenuItemDef::Command(MenuCommand::new("menu-embedded-open", ACTION_OPEN)),
        MenuItemDef::Command(MenuCommand::new(
            "menu-embedded-copy-to-inventory",
            ACTION_COPY,
        )),
    ],
};

/// The skin class on an embedded-item box — a clickable object inside notecard
/// prose. Its hover is `:hover` and nothing else; this used to be a
/// `Pointer<Over>` / `Pointer<Out>` observer pair writing two colours by hand.
///
/// Named in `sl_viewer_ui_core::skin`, because `stamp_hover_state` is driven by
/// the list of classes with a `:hover` rule and a copy of the string here would
/// be a class that stamp does not know.
const ITEM_CLASS: &str = sl_viewer_ui_core::skin::INLINE_ITEM_CLASS;

/// The skin class on the item's icon and name — `--inline-item-text`, or its
/// read-only twin when the box also wears [`READ_ONLY_CLASS`]. The link colour
/// the notecard passes is only the unskinned fallback: a cornflower name is
/// legible on a dark field and not on a light one, which is a skin's call.
const ITEM_TEXT_CLASS: &str = "sk-inline-item-text";

/// Stamped on the item box in a notecard that cannot be edited, because the
/// box floats in the rich text's overlay rather than inside the field that
/// carries the field's own `.sk-read-only`.
const READ_ONLY_CLASS: &str = sl_viewer_ui_core::skin::READ_ONLY_CLASS;

// ---------------------------------------------------------------------------
// The embedded-item box.
// ---------------------------------------------------------------------------

/// A rendered inline embedded-item box: what it stands for, and where.
#[derive(Component, Debug, Clone)]
struct EmbeddedItemBox {
    /// The body field the box is drawn in, whose [`UnsavedEmbeddedItems`] says
    /// whether the grid has the item yet.
    body: Entity,
    /// The item's index in the notecard's table.
    index: u32,
    /// What a click on it does.
    action: EmbeddedAction,
    /// Where the notecard lives — what a copy names, and what an opened
    /// landmark or material says it was read out of.
    source: NotecardSource,
    /// The embedded item's own id, which a copy names.
    item: InventoryKey,
}

impl EmbeddedItemBox {
    /// What *Copy to Inventory* copies — every type has one, whatever its
    /// click does.
    const fn copy(&self) -> CopyTarget {
        CopyTarget {
            notecard: self.source.item_id(),
            holder: self.source.object_id(),
            item: self.item,
        }
    }
}

/// The indices of a notecard body's embedded items that were dropped in
/// **since the notecard was last saved** — a component on the body field,
/// kept by the editor.
///
/// Such an item is in the buffer and the local table but not in the stored
/// asset, so a copy out of the notecard (which the grid serves from the stored
/// asset) would fail.
#[derive(Component, Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct UnsavedEmbeddedItems(pub(crate) HashSet<u32>);

/// A copy-embedded-item-into-inventory target (the reference default action).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    /// Copy the item into the agent's inventory (the reference default for an
    /// object, notecard, wearable, animation, gesture, script, …).
    Copy,
    /// Open an avatar's profile — a calling card.
    Profile(AgentKey),
    /// Open the texture preview — a texture / snapshot.
    Texture(Box<ItemInfo>),
    /// Play the sound locally, then offer the copy — a sound.
    Sound(AssetKey),
    /// Open About Landmark on it — a landmark.
    Landmark(Box<ItemInfo>),
    /// Open the material editor on it, without a Save — a material.
    Material(Box<ItemInfo>),
}

/// Resolve the click action for `item`.
fn resolve_action(item: &sl_notecard::InventoryItem) -> EmbeddedAction {
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
            EmbeddedAction::Texture(Box::new(embedded_item_info(
                item,
                AssetType::Texture,
                InventoryType::Texture,
            )))
        }
        A::Sound | A::SoundWav => EmbeddedAction::Sound(AssetKey::from(item.asset_id.0)),
        A::Landmark => EmbeddedAction::Landmark(Box::new(embedded_item_info(
            item,
            AssetType::Landmark,
            InventoryType::Landmark,
        ))),
        A::Material => EmbeddedAction::Material(Box::new(embedded_item_info(
            item,
            AssetType::Material,
            InventoryType::Material,
        ))),
        // Everything else copies into inventory.
        _other => EmbeddedAction::Copy,
    }
}

/// An [`ItemInfo`] for an embedded item, so a surface that opens inventory
/// items (the texture preview, About Landmark, the material editor) can open
/// one read out of a notecard. `asset_type` / `inv_type` are the class the
/// caller dispatched on, which is the class that surface expects.
///
/// Its ids, name, description, creator, owner, permissions and date are the
/// embedded item's own; its folder is nil, because it is in no folder of the
/// agent's.
fn embedded_item_info(
    item: &sl_notecard::InventoryItem,
    asset_type: AssetType,
    inv_type: InventoryType,
) -> ItemInfo {
    let perms = &item.permissions;
    let owner = if perms.group_owned {
        OwnerKey::Group(GroupKey::from(perms.group_id.0))
    } else {
        OwnerKey::Agent(AgentKey::from(perms.owner_id.0))
    };
    ItemInfo {
        item_id: InventoryKey::from(item.item_id.0),
        folder_id: InventoryFolderKey::from(Uuid::nil()),
        name: item.name.clone(),
        description: item.description.clone(),
        asset_id: item.asset_id.0,
        asset_type,
        inv_type,
        flags: item.flags,
        sale: SaleInfo::default(),
        creation_date: i32::try_from(item.creation_date).unwrap_or(0),
        owner,
        last_owner_id: perms.last_owner_id.0,
        creator_id: AgentKey::from(perms.creator_id.0),
        group: (!perms.group_id.0.is_nil()).then(|| GroupKey::from(perms.group_id.0)),
        permissions: Permissions5 {
            base: Permissions::from_bits(perms.base_mask.0),
            owner: Permissions::from_bits(perms.owner_mask.0),
            group: Permissions::from_bits(perms.group_mask.0),
            everyone: Permissions::from_bits(perms.everyone_mask.0),
            next_owner: Permissions::from_bits(perms.next_owner_mask.0),
        },
    }
}

/// Where an embedded item box sits and what it stands for — what
/// [`spawn_embedded_item_box`] needs beyond the node it fills.
#[derive(Debug, Clone, Copy)]
pub(crate) struct EmbeddedItemPlace {
    /// The body field the box is drawn in.
    pub(crate) body: Entity,
    /// The item's index in the notecard's table.
    pub(crate) index: u32,
    /// Where the notecard lives.
    pub(crate) source: NotecardSource,
}

/// Spawn one inline embedded-item box (icon + name) under `parent`, returning
/// it. `read_only` marks the box for a notecard that cannot be edited, whose
/// items the skin draws in their own colour.
///
/// `parent` is the object node the rich-text field positions
/// ([`sl_viewer_ui_widgets::ui_rich_text::spawn_rich_text_object`]): the box is
/// its content, so the node is the size of the box and parley reserves exactly
/// that much room in the flow.
pub(crate) fn spawn_embedded_item_box(
    commands: &mut Commands,
    parent: Entity,
    item: &sl_notecard::InventoryItem,
    place: EmbeddedItemPlace,
    style: LinkTextStyle,
    read_only: bool,
) -> Entity {
    let classes = core::iter::once(ITEM_CLASS).chain(read_only.then_some(READ_ONLY_CLASS));
    let item_box = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Px(3.0),
                padding: UiRect::axes(Val::Px(4.0), Val::Px(0.0)),
                ..default()
            },
            ClassList::new_with_classes(classes),
            Button,
            TabIndex(0),
            Pickable::default(),
            EmbeddedItemBox {
                body: place.body,
                index: place.index,
                action: resolve_action(item),
                source: place.source,
                item: InventoryKey::from(item.item_id.0),
            },
            ChildOf(parent),
        ))
        .id();
    commands.spawn((
        Text::new(embedded_icon(&item.asset_type).to_owned()),
        UiFont::Sans.at(style.font_size),
        TextColor(style.link_color),
        ClassList::new_with_classes([ITEM_TEXT_CLASS]),
        Pickable::IGNORE,
        ChildOf(item_box),
    ));
    commands.spawn((
        Text::new(item.name.clone()),
        UiFont::Sans.at(style.font_size),
        TextColor(style.link_color),
        ClassList::new_with_classes([ITEM_TEXT_CLASS]),
        Pickable::IGNORE,
        ChildOf(item_box),
    ));
    commands.entity(item_box).observe(on_embedded_press);
    item_box
}

// ---------------------------------------------------------------------------
// Dispatch: a click, or a line of the context menu.
// ---------------------------------------------------------------------------

/// What an embedded item's click or menu line is carried out through, bundled
/// as one [`SystemParam`](bevy::ecs::system::SystemParam).
///
/// Every destination is a *viewer* channel, and the item boxes are drawn
/// wherever a notecard body is — including the gallery, which spawns the
/// notecard specimens with none of them. Bevy takes an app down when a system
/// parameter fails validation, so the whole bundle is taken through [`If`]: in
/// an app that cannot route the action a press is **inert** rather than fatal.
/// (Found by a click on the specimen's item in the gallery, 2026-09-13.)
#[derive(bevy::ecs::system::SystemParam)]
struct EmbeddedOutputs<'w> {
    /// The copies parked behind their confirmation.
    pending: ResMut<'w, PendingEmbeddedCopies>,
    /// The notecard saves an unsaved item's click asked for.
    saves: ResMut<'w, PendingNotecardSaveConfirms>,
    /// The confirmations themselves.
    notifications: MessageWriter<'w, ShowNotification>,
    /// A calling card's profile.
    profiles: MessageWriter<'w, OpenAvatarProfile>,
    /// A texture's preview.
    previews: MessageWriter<'w, OpenItemPreview>,
    /// A sound, played locally.
    sounds: MessageWriter<'w, PlayAssetSound>,
    /// A landmark's details.
    landmarks: MessageWriter<'w, OpenAboutLandmark>,
    /// A material's editor.
    materials: MessageWriter<'w, OpenMaterialEditor>,
}

/// What one press or menu line asks of an embedded item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EmbeddedRequest {
    /// Open it the way a click does ([`EmbeddedAction`]).
    Open,
    /// Offer to copy it into inventory, whatever its type.
    Copy,
}

/// Carry out `request` on `item_box` — unless the grid does not have the item
/// yet, in which case ask to save the notecard first and do nothing else.
fn run_embedded_request(
    item_box: &EmbeddedItemBox,
    request: EmbeddedRequest,
    unsaved: Option<&UnsavedEmbeddedItems>,
    out: &mut EmbeddedOutputs,
) {
    if unsaved.is_some_and(|unsaved| unsaved.0.contains(&item_box.index)) {
        out.saves.queue.push_back(item_box.body);
        out.notifications
            .write(ShowNotification::new(CONFIRM_NOTECARD_SAVE_TEMPLATE));
        return;
    }
    let action = match request {
        EmbeddedRequest::Open => &item_box.action,
        EmbeddedRequest::Copy => &EmbeddedAction::Copy,
    };
    let source = item_box.source;
    match action {
        EmbeddedAction::Copy => offer_copy(item_box.copy(), out),
        EmbeddedAction::Profile(agent) => {
            out.profiles.write(OpenAvatarProfile { agent: *agent });
        }
        EmbeddedAction::Texture(item) => {
            out.previews.write(OpenItemPreview {
                item: (**item).clone(),
            });
        }
        EmbeddedAction::Sound(asset) => {
            out.sounds.write(PlayAssetSound { asset: *asset });
            offer_copy(item_box.copy(), out);
        }
        EmbeddedAction::Landmark(item) => {
            out.landmarks.write(OpenAboutLandmark {
                item: (**item).clone(),
                notecard: Some(source),
            });
        }
        EmbeddedAction::Material(item) => {
            out.materials.write(OpenMaterialEditor {
                item: (**item).clone(),
                notecard: Some(source),
            });
        }
    }
}

/// Park a copy behind the `ConfirmItemCopy` confirmation — one per queued
/// dialog, answered by [`handle_embedded_copy_confirmations`] — so a click
/// never silently spawns an inventory item.
fn offer_copy(target: CopyTarget, out: &mut EmbeddedOutputs) {
    out.pending.queue.push_back(target);
    out.notifications
        .write(ShowNotification::new(CONFIRM_ITEM_COPY_TEMPLATE));
}

/// A primary press opens the item ([`EmbeddedAction`]); a secondary press
/// opens the *Open* / *Copy to Inventory* menu on it.
fn on_embedded_press(
    press: On<Pointer<Press>>,
    boxes: Query<&EmbeddedItemBox>,
    unsaved: Query<&UnsavedEmbeddedItems>,
    If(mut out): If<EmbeddedOutputs>,
    If(mut menus): If<MessageWriter<OpenContextMenu>>,
    If(mut target): If<ResMut<EmbeddedMenuTarget>>,
) {
    let Ok(item_box) = boxes.get(press.entity) else {
        return;
    };
    match press.button {
        PointerButton::Primary => run_embedded_request(
            item_box,
            EmbeddedRequest::Open,
            unsaved.get(item_box.body).ok(),
            &mut out,
        ),
        PointerButton::Secondary => {
            target.0 = Some(press.entity);
            menus.write(OpenContextMenu {
                menu: &EMBEDDED_MENU,
                at: press.pointer_location.position,
                element: EMBEDDED_MENU_ELEMENT,
                conditions: Vec::new(),
            });
        }
        PointerButton::Middle => {}
    }
}

/// The item box the embedded-item menu was last opened on.
#[derive(Resource, Debug, Default)]
pub(crate) struct EmbeddedMenuTarget(Option<Entity>);

/// Carry out a picked line of the embedded-item menu on the box it was opened
/// on.
fn handle_embedded_menu_actions(
    mut actions: MessageReader<UiAction>,
    target: Res<EmbeddedMenuTarget>,
    boxes: Query<&EmbeddedItemBox>,
    unsaved: Query<&UnsavedEmbeddedItems>,
    If(mut out): If<EmbeddedOutputs>,
) {
    for action in actions.read() {
        if action.element != EMBEDDED_MENU_ELEMENT {
            continue;
        }
        let request = match action.action {
            ACTION_OPEN => EmbeddedRequest::Open,
            ACTION_COPY => EmbeddedRequest::Copy,
            _other => continue,
        };
        // The box may have gone with its notecard while the menu was up.
        let Some(item_box) = target.0.and_then(|entity| boxes.get(entity).ok()) else {
            continue;
        };
        run_embedded_request(item_box, request, unsaved.get(item_box.body).ok(), &mut out);
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

/// The notecard bodies awaiting their `ConfirmNotecardSave` answer, oldest
/// first; the editor turns an **OK** into a save of the window holding the body.
#[derive(Resource, Debug, Default)]
pub(crate) struct PendingNotecardSaveConfirms {
    /// The parked bodies, front = the dialog raised first.
    pub(crate) queue: VecDeque<Entity>,
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

/// The plugin owning the embedded items' confirm-to-copy routing and their
/// context menu (the per-item observers are attached at spawn and need no
/// registration).
#[derive(Debug, Clone, Copy, Default)]
pub struct NotecardRenderPlugin;

impl Plugin for NotecardRenderPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PendingEmbeddedCopies>()
            .init_resource::<PendingNotecardSaveConfirms>()
            .init_resource::<EmbeddedMenuTarget>()
            .add_systems(
                Update,
                (
                    handle_embedded_copy_confirmations,
                    handle_embedded_menu_actions,
                ),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::{EmbeddedAction, embedded_item_info, resolve_action};
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{
        AgentKey, AssetKey, AssetType, InventoryType, OwnerKey, Permissions, Uuid,
    };

    /// A one-item notecard fixture helper: an embedded item of a given type.
    fn item(asset_type: sl_notecard::AssetType, description: &str) -> sl_notecard::InventoryItem {
        sl_notecard::InventoryItem {
            item_id: sl_types::key::Key(Uuid::from_u128(0x42)),
            parent_id: sl_types::key::NULL_KEY,
            permissions: sl_notecard::Permissions {
                creator_id: sl_types::key::Key(Uuid::from_u128(0x99)),
                owner_id: sl_types::key::Key(Uuid::from_u128(0x98)),
                owner_mask: sl_notecard::PermissionMask(0x0008_e000),
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
            creation_date: 1_700_000_000,
            unknown_fields: Vec::new(),
        }
    }

    /// The types the reference only copies — an object, a notecard, a
    /// wearable, a gesture — resolve to a copy.
    #[test]
    fn copy_only_types_copy_into_inventory() {
        for asset_type in [
            sl_notecard::AssetType::Object,
            sl_notecard::AssetType::Notecard,
            sl_notecard::AssetType::Clothing,
            sl_notecard::AssetType::Gesture,
        ] {
            let action = resolve_action(&item(asset_type.clone(), ""));
            assert!(
                matches!(action, EmbeddedAction::Copy),
                "{asset_type:?} should copy into inventory, got {action:?}"
            );
        }
    }

    /// A calling card opens its description's agent profile, falling back to the
    /// creator when the description is not a uuid.
    #[test]
    fn calling_card_opens_a_profile() {
        // A description holding a uuid names that agent.
        let described = Uuid::from_u128(0xABCD);
        let action = resolve_action(&item(
            sl_notecard::AssetType::CallingCard,
            &described.to_string(),
        ));
        assert!(
            matches!(&action, EmbeddedAction::Profile(agent) if agent.uuid() == described),
            "a calling card should open its described agent's profile, got {action:?}"
        );
        // A non-uuid description falls back to the creator.
        let action = resolve_action(&item(sl_notecard::AssetType::CallingCard, "not a uuid"));
        assert!(
            matches!(&action, EmbeddedAction::Profile(agent) if agent.uuid() == Uuid::from_u128(0x99)),
            "a calling card should fall back to its creator, got {action:?}"
        );
    }

    /// A texture opens the texture preview carrying the item's asset id.
    #[test]
    fn texture_opens_the_preview() {
        let action = resolve_action(&item(sl_notecard::AssetType::Texture, ""));
        assert!(
            matches!(&action, EmbeddedAction::Texture(info) if info.asset_id == Uuid::from_u128(0x7)),
            "a texture should open the preview on its asset, got {action:?}"
        );
    }

    /// A sound plays its own asset (and the copy is offered after it).
    #[test]
    fn sound_plays_its_asset() {
        let action = resolve_action(&item(sl_notecard::AssetType::Sound, ""));
        assert!(
            matches!(action, EmbeddedAction::Sound(asset) if asset == AssetKey::from(Uuid::from_u128(0x7))),
            "a sound should play its asset, got {action:?}"
        );
    }

    /// A landmark opens About Landmark, and a material the material editor,
    /// each on an item of the class that surface expects.
    #[test]
    fn landmark_and_material_open_their_surfaces() {
        let action = resolve_action(&item(sl_notecard::AssetType::Landmark, ""));
        assert!(
            matches!(&action, EmbeddedAction::Landmark(info)
                if info.asset_type == AssetType::Landmark
                    && info.inv_type == InventoryType::Landmark),
            "a landmark should open About Landmark, got {action:?}"
        );
        let action = resolve_action(&item(sl_notecard::AssetType::Material, ""));
        assert!(
            matches!(&action, EmbeddedAction::Material(info)
                if info.asset_type == AssetType::Material),
            "a material should open the material editor, got {action:?}"
        );
    }

    /// The item a surface opens carries the embedded item's own identity,
    /// ownership and permissions, not placeholders — About Landmark shows its
    /// creator and date, and decides editability from its owner.
    #[test]
    fn embedded_item_info_is_the_items_own() {
        let info = embedded_item_info(
            &item(sl_notecard::AssetType::Landmark, "notes"),
            AssetType::Landmark,
            InventoryType::Landmark,
        );
        assert_eq!(info.item_id.uuid(), Uuid::from_u128(0x42));
        assert_eq!(info.asset_id, Uuid::from_u128(0x7));
        assert_eq!(info.description, "notes");
        assert_eq!(info.creator_id, AgentKey::from(Uuid::from_u128(0x99)));
        assert_eq!(
            info.owner,
            OwnerKey::Agent(AgentKey::from(Uuid::from_u128(0x98)))
        );
        assert_eq!(info.permissions.owner, Permissions::from_bits(0x0008_e000));
        assert_eq!(info.creation_date, 1_700_000_000);
        assert_eq!(info.group, None);
    }
}
