//! The **inventory context menu** and its actions
//! (`viewer-inventory-context-actions`): the right-click menu on an inventory
//! row — rename, cut / copy / paste / paste-as-link, delete / purge / empty
//! trash, create (folder / script / notecard / gesture), wear / add / take off,
//! attach / detach, landmark teleport, gesture activate / deactivate — plus the
//! rename dialog and the inventory clipboard the menu drives.
//!
//! # One menu per row kind, entries gated like the reference
//!
//! The reference viewer builds every inventory context menu from **one** shared
//! `menu_inventory.xml`; each bridge type (`LL*Bridge::buildContextMenu`) then
//! shows / hides / disables the entries that apply to it. This module mirrors
//! that: one [`INVENTORY_ITEM_MENU`] whose type-specific entries carry
//! `visible_when` conditions (`is-landmark`, `is-object`, …) and one
//! [`INVENTORY_FOLDER_MENU`] likewise — so a landmark shows Teleport, an object
//! shows Wear / Detach, the Trash shows Empty Trash, exactly as the reference
//! decides per bridge. Entries whose feature this viewer does not have yet are
//! declared in their reference order but gated on
//! [`UNIMPLEMENTED`](crate::avatar_menu::UNIMPLEMENTED) — visible, greyed, and
//! one deliberate edit away from going live (the same pattern as the avatar
//! pies).
//!
//! # The Library is read-only
//!
//! `InventoryModel` flags the shared Library subtree; every mutating capability
//! condition (`can-rename`, `can-cut`, `can-delete`, `can-paste`, …) is simply
//! never pushed for a Library row, so the whole mutation surface reads greyed
//! there. Copying **out** of the Library is allowed (that is what it is for).
//!
//! # Dispatch
//!
//! The widget ([`crate::menu`]) emits a [`UiAction`] tagged
//! [`INVENTORY_MENU_ELEMENT`]; [`handle_inventory_menu_actions`] routes it. The
//! acted-on row is snapshotted into [`InventoryMenuTarget`] when the menu opens
//! (action strings are `&'static` and cannot carry a key — the same out-of-band
//! shape as [`crate::avatar_menu::AvatarMenuTarget`]). Mutations go through the
//! session commands, whose cache applies them optimistically; the affected
//! folder pages are re-queried immediately after, so the tree reflects the
//! change without waiting for a server round-trip.
//!
//! Reference (Firestorm, read-only): `menu_inventory.xml` (the entry set),
//! `llinventorybridge.cpp` (`buildContextMenu` per bridge, the per-type
//! show / hide / disable), `llinventoryfunctions.cpp` (the operations).

use bevy::prelude::*;
use sl_client_bevy::{
    AgentKey, AssetKey, AssetType, AttachmentMode, AttachmentPoint, Command, DetachOrder,
    FolderInfo, FolderType, GestureActivation, InventoryFolderKey, InventoryItemOrFolderKey,
    InventoryKey, InventoryType, ItemInfo, NewInventoryItem, NewInventoryLink, Permissions,
    RezAttachment, ScriptLanguage, SlCommand, SlIdentity, TransactionId, Uuid, Wearable,
    WearableType,
};
use std::collections::HashSet;

use crate::avatar_menu::UNIMPLEMENTED;
use crate::conversations::{ConversationKey, OpenConversation};
use crate::inventory::{
    DisplayRow, InventoryModel, InventorySelection, InventoryView, RowKey, query_folder_page,
};
use crate::menu::{MenuCommand, MenuDef, MenuItemDef, OpenContextMenu};
use crate::ui_element::UiAction;
use crate::virtual_list::VirtualRow;

/// The `element` the inventory context menus attribute their [`UiAction`]s to.
pub(crate) const INVENTORY_MENU_ELEMENT: &str = "inventory-menu";

// ---------------------------------------------------------------------------
// The condition vocabulary. Visibility keys select the per-type entries (the
// reference's per-bridge show/hide); capability keys enable the mutations (the
// reference's on_enable). All are computed at open time from the model.
// ---------------------------------------------------------------------------

/// The target is a landmark — shows the Teleport / map block.
pub(crate) const IS_LANDMARK: &str = "is-landmark";

/// The target is a gesture — shows Play / Activate / Deactivate.
pub(crate) const IS_GESTURE: &str = "is-gesture";

/// The target is an object / attachment — shows Wear / Add / Detach.
pub(crate) const IS_OBJECT: &str = "is-object";

/// The target is a body wearable (clothing **or** body part) — shows Wear.
pub(crate) const IS_WEARABLE: &str = "is-wearable";

/// The target is a clothing **layer** (not a body part) — shows Add / Take Off,
/// which a body part cannot do (an avatar always wears exactly one of each
/// part).
pub(crate) const IS_CLOTHING: &str = "is-clothing";

/// The target is a calling card — shows the IM / teleport-offer block.
pub(crate) const IS_CALLING_CARD: &str = "is-calling-card";

/// The target is a sound — shows Play.
pub(crate) const IS_SOUND: &str = "is-sound";

/// The target is an animation — shows Play Inworld / Play Locally.
pub(crate) const IS_ANIMATION: &str = "is-animation";

/// The target is a texture / snapshot — shows Save As.
pub(crate) const IS_TEXTURE: &str = "is-texture";

/// The target is an environment-settings item — shows the Apply entries.
pub(crate) const IS_SETTINGS: &str = "is-settings";

/// The target row sits inside the Trash — Purge / Restore replace Delete.
pub(crate) const IN_TRASH: &str = "in-trash";

/// The target row is not in the Trash — Delete is offered.
pub(crate) const NOT_IN_TRASH: &str = "not-in-trash";

/// The target folder **is** the Trash — offers Empty Trash.
pub(crate) const IS_TRASH_FOLDER: &str = "is-trash-folder";

/// The target folder **is** Lost And Found — offers Empty Lost And Found.
pub(crate) const IS_LOST_FOUND_FOLDER: &str = "is-lost-found-folder";

/// The target can be renamed (own inventory, and for a folder a plain user
/// folder rather than a system one).
pub(crate) const CAN_RENAME: &str = "can-rename";

/// The target can be cut (moved on the next paste).
pub(crate) const CAN_CUT: &str = "can-cut";

/// The target can be copied (the item grants copy, or it is a Library item —
/// copying out of the Library is what the Library is for).
pub(crate) const CAN_COPY: &str = "can-copy";

/// The clipboard holds an entry and the paste destination is writable.
pub(crate) const CAN_PASTE: &str = "can-paste";

/// As [`CAN_PASTE`], for Paste As Link.
pub(crate) const CAN_PASTE_LINK: &str = "can-paste-link";

/// The target can be deleted (moved to the Trash).
pub(crate) const CAN_DELETE: &str = "can-delete";

/// New-item entries (New Folder / Script / Notecard / Gesture) apply — the
/// target folder is writable.
pub(crate) const CAN_CREATE: &str = "can-create";

/// The target wearable / attachment is currently worn — enables Take Off /
/// Detach.
pub(crate) const WORN: &str = "worn";

/// The target wearable / attachment is not currently worn — enables Wear / Add.
pub(crate) const NOT_WORN: &str = "not-worn";

/// The target gesture is active this session — enables Deactivate.
pub(crate) const GESTURE_ACTIVE: &str = "gesture-active";

/// The target gesture is not active — enables Activate.
pub(crate) const GESTURE_INACTIVE: &str = "gesture-inactive";

/// The target folder's (loaded) subtree holds something wearable — a body
/// wearable or an attachable object — enabling Add To Current Outfit.
pub(crate) const FOLDER_HAS_WEARABLES: &str = "folder-has-wearables";

/// The target folder's (loaded) subtree holds something currently worn —
/// enabling Remove From Current Outfit.
pub(crate) const FOLDER_HAS_WORN: &str = "folder-has-worn";

/// The default next-owner permission mask for a freshly created item: modify,
/// copy and transfer (`PERM_ITEM_UNRESTRICTED`), the reference viewer's default
/// for new notecards / scripts / gestures.
const NEXT_OWNER_DEFAULT: u32 = 0x0000_E000;

// ---------------------------------------------------------------------------
// The menus. Reference order (menu_inventory.xml); the marketplace block and
// Firestorm's upload-defaults block are gated to folders we never surface and
// are omitted entirely, like the reference hides them outside those folders.
// ---------------------------------------------------------------------------

/// The "New Clothes >" submenu — every layer is a placeholder until wearable
/// creation lands (creating a wearable needs a default asset to be authored).
static NEW_CLOTHES_MENU: MenuDef = MenuDef {
    label: "New Clothes",
    items: &[
        MenuItemDef::Command(
            MenuCommand::new("New Shirt", "new-shirt").enabled_when(UNIMPLEMENTED),
        ),
        MenuItemDef::Command(
            MenuCommand::new("New Pants", "new-pants").enabled_when(UNIMPLEMENTED),
        ),
        MenuItemDef::Command(
            MenuCommand::new("New Shoes", "new-shoes").enabled_when(UNIMPLEMENTED),
        ),
        MenuItemDef::Command(
            MenuCommand::new("New Socks", "new-socks").enabled_when(UNIMPLEMENTED),
        ),
        MenuItemDef::Command(
            MenuCommand::new("New Jacket", "new-jacket").enabled_when(UNIMPLEMENTED),
        ),
        MenuItemDef::Command(
            MenuCommand::new("New Skirt", "new-skirt").enabled_when(UNIMPLEMENTED),
        ),
        MenuItemDef::Command(
            MenuCommand::new("New Gloves", "new-gloves").enabled_when(UNIMPLEMENTED),
        ),
        MenuItemDef::Command(
            MenuCommand::new("New Undershirt", "new-undershirt").enabled_when(UNIMPLEMENTED),
        ),
        MenuItemDef::Command(
            MenuCommand::new("New Underpants", "new-underpants").enabled_when(UNIMPLEMENTED),
        ),
        MenuItemDef::Command(
            MenuCommand::new("New Alpha Mask", "new-alpha").enabled_when(UNIMPLEMENTED),
        ),
        MenuItemDef::Command(
            MenuCommand::new("New Tattoo", "new-tattoo").enabled_when(UNIMPLEMENTED),
        ),
        MenuItemDef::Command(
            MenuCommand::new("New Universal", "new-universal").enabled_when(UNIMPLEMENTED),
        ),
        MenuItemDef::Command(
            MenuCommand::new("New Physics", "new-physics").enabled_when(UNIMPLEMENTED),
        ),
    ],
};

/// The "New Body Parts >" submenu — placeholders, as [`NEW_CLOTHES_MENU`].
static NEW_BODY_PARTS_MENU: MenuDef = MenuDef {
    label: "New Body Parts",
    items: &[
        MenuItemDef::Command(
            MenuCommand::new("New Shape", "new-shape").enabled_when(UNIMPLEMENTED),
        ),
        MenuItemDef::Command(MenuCommand::new("New Skin", "new-skin").enabled_when(UNIMPLEMENTED)),
        MenuItemDef::Command(MenuCommand::new("New Hair", "new-hair").enabled_when(UNIMPLEMENTED)),
        MenuItemDef::Command(MenuCommand::new("New Eyes", "new-eyes").enabled_when(UNIMPLEMENTED)),
    ],
};

/// The **"Attach To"** submenu: every named body attachment point, in wire-id
/// order (the reference iterates `mAttachmentPoints`, a map keyed by id, and
/// labels each point "Name (id)"; HUD points 31–38 live in the separate
/// [`ATTACH_TO_HUD_MENU`]). Actions carry the wire id (`attach-point-<id>`),
/// which the dispatcher parses back into an [`AttachmentPoint`].
static ATTACH_TO_MENU: MenuDef = MenuDef {
    label: "Attach To",
    items: &[
        MenuItemDef::Command(MenuCommand::new("Chest (1)", "attach-point-1")),
        MenuItemDef::Command(MenuCommand::new("Skull (2)", "attach-point-2")),
        MenuItemDef::Command(MenuCommand::new("Left Shoulder (3)", "attach-point-3")),
        MenuItemDef::Command(MenuCommand::new("Right Shoulder (4)", "attach-point-4")),
        MenuItemDef::Command(MenuCommand::new("Left Hand (5)", "attach-point-5")),
        MenuItemDef::Command(MenuCommand::new("Right Hand (6)", "attach-point-6")),
        MenuItemDef::Command(MenuCommand::new("Left Foot (7)", "attach-point-7")),
        MenuItemDef::Command(MenuCommand::new("Right Foot (8)", "attach-point-8")),
        MenuItemDef::Command(MenuCommand::new("Spine (9)", "attach-point-9")),
        MenuItemDef::Command(MenuCommand::new("Pelvis (10)", "attach-point-10")),
        MenuItemDef::Command(MenuCommand::new("Mouth (11)", "attach-point-11")),
        MenuItemDef::Command(MenuCommand::new("Chin (12)", "attach-point-12")),
        MenuItemDef::Command(MenuCommand::new("Left Ear (13)", "attach-point-13")),
        MenuItemDef::Command(MenuCommand::new("Right Ear (14)", "attach-point-14")),
        MenuItemDef::Command(MenuCommand::new("Left Eyeball (15)", "attach-point-15")),
        MenuItemDef::Command(MenuCommand::new("Right Eyeball (16)", "attach-point-16")),
        MenuItemDef::Command(MenuCommand::new("Nose (17)", "attach-point-17")),
        MenuItemDef::Command(MenuCommand::new("R Upper Arm (18)", "attach-point-18")),
        MenuItemDef::Command(MenuCommand::new("R Forearm (19)", "attach-point-19")),
        MenuItemDef::Command(MenuCommand::new("L Upper Arm (20)", "attach-point-20")),
        MenuItemDef::Command(MenuCommand::new("L Forearm (21)", "attach-point-21")),
        MenuItemDef::Command(MenuCommand::new("Right Hip (22)", "attach-point-22")),
        MenuItemDef::Command(MenuCommand::new("R Upper Leg (23)", "attach-point-23")),
        MenuItemDef::Command(MenuCommand::new("R Lower Leg (24)", "attach-point-24")),
        MenuItemDef::Command(MenuCommand::new("Left Hip (25)", "attach-point-25")),
        MenuItemDef::Command(MenuCommand::new("L Upper Leg (26)", "attach-point-26")),
        MenuItemDef::Command(MenuCommand::new("L Lower Leg (27)", "attach-point-27")),
        MenuItemDef::Command(MenuCommand::new("Stomach (28)", "attach-point-28")),
        MenuItemDef::Command(MenuCommand::new("Left Pec (29)", "attach-point-29")),
        MenuItemDef::Command(MenuCommand::new("Right Pec (30)", "attach-point-30")),
        MenuItemDef::Command(MenuCommand::new("Neck (39)", "attach-point-39")),
        MenuItemDef::Command(MenuCommand::new("Avatar Center (40)", "attach-point-40")),
        MenuItemDef::Command(MenuCommand::new("Left Ring Finger (41)", "attach-point-41")),
        MenuItemDef::Command(MenuCommand::new(
            "Right Ring Finger (42)",
            "attach-point-42",
        )),
        MenuItemDef::Command(MenuCommand::new("Tail Base (43)", "attach-point-43")),
        MenuItemDef::Command(MenuCommand::new("Tail Tip (44)", "attach-point-44")),
        MenuItemDef::Command(MenuCommand::new("Left Wing (45)", "attach-point-45")),
        MenuItemDef::Command(MenuCommand::new("Right Wing (46)", "attach-point-46")),
        MenuItemDef::Command(MenuCommand::new("Jaw (47)", "attach-point-47")),
        MenuItemDef::Command(MenuCommand::new("Alt Left Ear (48)", "attach-point-48")),
        MenuItemDef::Command(MenuCommand::new("Alt Right Ear (49)", "attach-point-49")),
        MenuItemDef::Command(MenuCommand::new("Alt Left Eye (50)", "attach-point-50")),
        MenuItemDef::Command(MenuCommand::new("Alt Right Eye (51)", "attach-point-51")),
        MenuItemDef::Command(MenuCommand::new("Tongue (52)", "attach-point-52")),
        MenuItemDef::Command(MenuCommand::new("Groin (53)", "attach-point-53")),
        MenuItemDef::Command(MenuCommand::new("Left Hind Foot (54)", "attach-point-54")),
        MenuItemDef::Command(MenuCommand::new("Right Hind Foot (55)", "attach-point-55")),
    ],
};

/// The **"Attach To HUD"** submenu: the eight HUD slots (wire ids 31–38). The
/// reference labels HUD points without the id suffix.
static ATTACH_TO_HUD_MENU: MenuDef = MenuDef {
    label: "Attach To HUD",
    items: &[
        MenuItemDef::Command(MenuCommand::new("Center 2", "attach-point-31")),
        MenuItemDef::Command(MenuCommand::new("Top Right", "attach-point-32")),
        MenuItemDef::Command(MenuCommand::new("Top", "attach-point-33")),
        MenuItemDef::Command(MenuCommand::new("Top Left", "attach-point-34")),
        MenuItemDef::Command(MenuCommand::new("Center", "attach-point-35")),
        MenuItemDef::Command(MenuCommand::new("Bottom Left", "attach-point-36")),
        MenuItemDef::Command(MenuCommand::new("Bottom", "attach-point-37")),
        MenuItemDef::Command(MenuCommand::new("Bottom Right", "attach-point-38")),
    ],
};

/// The context menu for a **folder** row. Reference order: the trash /
/// lost-and-found emptiers, the New … creators, the outfit block, then the
/// shared rename / clipboard / delete tail.
pub(crate) static INVENTORY_FOLDER_MENU: MenuDef = MenuDef {
    label: "Folder",
    items: &[
        MenuItemDef::Command(MenuCommand::new("Share", "share").enabled_when(UNIMPLEMENTED)),
        MenuItemDef::Command(
            MenuCommand::new("Empty Trash", "empty-trash").visible_when(IS_TRASH_FOLDER),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Empty Lost And Found", "empty-lost-and-found")
                .visible_when(IS_LOST_FOUND_FOLDER),
        ),
        MenuItemDef::Command(MenuCommand::new("New Folder", "new-folder").enabled_when(CAN_CREATE)),
        MenuItemDef::Command(MenuCommand::new("New Script", "new-script").enabled_when(CAN_CREATE)),
        MenuItemDef::Command(
            MenuCommand::new("New Notecard", "new-notecard").enabled_when(CAN_CREATE),
        ),
        MenuItemDef::Command(
            MenuCommand::new("New Gesture", "new-gesture").enabled_when(CAN_CREATE),
        ),
        MenuItemDef::Submenu(&NEW_CLOTHES_MENU),
        MenuItemDef::Submenu(&NEW_BODY_PARTS_MENU),
        MenuItemDef::Separator,
        MenuItemDef::Command(
            MenuCommand::new("Replace Current Outfit", "replace-outfit")
                .enabled_when(UNIMPLEMENTED),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Add To Current Outfit", "add-to-outfit")
                .enabled_when(FOLDER_HAS_WEARABLES),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Remove From Current Outfit", "remove-from-outfit")
                .enabled_when(FOLDER_HAS_WORN),
        ),
        MenuItemDef::Separator,
        MenuItemDef::Command(MenuCommand::new("Rename", "rename").enabled_when(CAN_RENAME)),
        MenuItemDef::Command(MenuCommand::new("Cut", "cut").enabled_when(CAN_CUT)),
        // A folder deep-copy (recursive item copies into a fresh tree) is not
        // wired yet; the entry keeps its reference place.
        MenuItemDef::Command(MenuCommand::new("Copy", "copy").enabled_when(UNIMPLEMENTED)),
        MenuItemDef::Command(MenuCommand::new("Paste", "paste").enabled_when(CAN_PASTE)),
        MenuItemDef::Command(
            MenuCommand::new("Paste As Link", "paste-link").enabled_when(CAN_PASTE_LINK),
        ),
        MenuItemDef::Separator,
        MenuItemDef::Command(
            MenuCommand::new("Delete", "delete")
                .visible_when(NOT_IN_TRASH)
                .enabled_when(CAN_DELETE),
        ),
        MenuItemDef::Command(MenuCommand::new("Purge Item", "purge").visible_when(IN_TRASH)),
        MenuItemDef::Command(MenuCommand::new("Restore Item", "restore").visible_when(IN_TRASH)),
    ],
};

/// The context menu for an **item** row. Reference order: the open / properties
/// head, the clipboard block, the delete block, then the per-type tail (each
/// entry visible only for its type, the reference's per-bridge selection).
pub(crate) static INVENTORY_ITEM_MENU: MenuDef = MenuDef {
    label: "Item",
    items: &[
        MenuItemDef::Command(MenuCommand::new("Share", "share").enabled_when(UNIMPLEMENTED)),
        MenuItemDef::Command(MenuCommand::new("Open", "open").enabled_when(UNIMPLEMENTED)),
        MenuItemDef::Command(
            MenuCommand::new("Properties", "properties").enabled_when(UNIMPLEMENTED),
        ),
        MenuItemDef::Command(MenuCommand::new("Rename", "rename").enabled_when(CAN_RENAME)),
        MenuItemDef::Command(
            MenuCommand::new("Copy Asset UUID", "copy-asset-uuid").enabled_when(UNIMPLEMENTED),
        ),
        MenuItemDef::Separator,
        MenuItemDef::Command(MenuCommand::new("Copy", "copy").enabled_when(CAN_COPY)),
        MenuItemDef::Command(MenuCommand::new("Cut", "cut").enabled_when(CAN_CUT)),
        MenuItemDef::Command(MenuCommand::new("Paste", "paste").enabled_when(CAN_PASTE)),
        MenuItemDef::Command(
            MenuCommand::new("Paste As Link", "paste-link").enabled_when(CAN_PASTE_LINK),
        ),
        MenuItemDef::Separator,
        MenuItemDef::Command(
            MenuCommand::new("Delete", "delete")
                .visible_when(NOT_IN_TRASH)
                .enabled_when(CAN_DELETE),
        ),
        MenuItemDef::Command(MenuCommand::new("Purge Item", "purge").visible_when(IN_TRASH)),
        MenuItemDef::Command(MenuCommand::new("Restore Item", "restore").visible_when(IN_TRASH)),
        MenuItemDef::Separator,
        // Landmark.
        MenuItemDef::Command(MenuCommand::new("Teleport", "teleport").visible_when(IS_LANDMARK)),
        MenuItemDef::Command(
            MenuCommand::new("About Landmark", "about-landmark")
                .visible_when(IS_LANDMARK)
                .enabled_when(UNIMPLEMENTED),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Show on Map", "show-on-map")
                .visible_when(IS_LANDMARK)
                .enabled_when(UNIMPLEMENTED),
        ),
        // Sound.
        MenuItemDef::Command(
            MenuCommand::new("Play", "play-sound")
                .visible_when(IS_SOUND)
                .enabled_when(UNIMPLEMENTED),
        ),
        // Animation.
        MenuItemDef::Command(
            MenuCommand::new("Play Inworld", "play-inworld")
                .visible_when(IS_ANIMATION)
                .enabled_when(UNIMPLEMENTED),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Play Locally", "play-locally")
                .visible_when(IS_ANIMATION)
                .enabled_when(UNIMPLEMENTED),
        ),
        // Calling card: IM the person it names (the card's creator).
        MenuItemDef::Command(
            MenuCommand::new("Send Instant Message", "send-im").visible_when(IS_CALLING_CARD),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Offer Teleport...", "offer-teleport")
                .visible_when(IS_CALLING_CARD)
                .enabled_when(UNIMPLEMENTED),
        ),
        // Gesture.
        MenuItemDef::Command(
            MenuCommand::new("Play", "play-gesture")
                .visible_when(IS_GESTURE)
                .enabled_when(UNIMPLEMENTED),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Activate", "activate-gesture")
                .visible_when(IS_GESTURE)
                .enabled_when(GESTURE_INACTIVE),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Deactivate", "deactivate-gesture")
                .visible_when(IS_GESTURE)
                .enabled_when(GESTURE_ACTIVE),
        ),
        // Texture.
        MenuItemDef::Command(
            MenuCommand::new("Save As", "save-as")
                .visible_when(IS_TEXTURE)
                .enabled_when(UNIMPLEMENTED),
        ),
        // Wearable (clothing and body parts).
        MenuItemDef::Command(
            MenuCommand::new("Wear", "wear-wearable")
                .visible_when(IS_WEARABLE)
                .enabled_when(NOT_WORN),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Add", "add-wearable")
                .visible_when(IS_CLOTHING)
                .enabled_when(NOT_WORN),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Take Off", "take-off")
                .visible_when(IS_CLOTHING)
                .enabled_when(WORN),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Edit", "edit-wearable")
                .visible_when(IS_WEARABLE)
                .enabled_when(UNIMPLEMENTED),
        ),
        // Object / attachment.
        MenuItemDef::Command(
            MenuCommand::new("Wear", "attach")
                .visible_when(IS_OBJECT)
                .enabled_when(NOT_WORN),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Add", "attach-add")
                .visible_when(IS_OBJECT)
                .enabled_when(NOT_WORN),
        ),
        MenuItemDef::SubmenuWhen(&ATTACH_TO_MENU, IS_OBJECT),
        MenuItemDef::SubmenuWhen(&ATTACH_TO_HUD_MENU, IS_OBJECT),
        MenuItemDef::Command(
            MenuCommand::new("Touch", "touch")
                .visible_when(IS_OBJECT)
                .enabled_when(UNIMPLEMENTED),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Detach From Yourself", "detach")
                .visible_when(IS_OBJECT)
                .enabled_when(WORN),
        ),
        // Settings.
        MenuItemDef::Command(
            MenuCommand::new("Apply Only To Myself", "settings-apply-local")
                .visible_when(IS_SETTINGS)
                .enabled_when(UNIMPLEMENTED),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Apply To Parcel", "settings-apply-parcel")
                .visible_when(IS_SETTINGS)
                .enabled_when(UNIMPLEMENTED),
        ),
    ],
};

// ---------------------------------------------------------------------------
// The target / clipboard / worn-state resources.
// ---------------------------------------------------------------------------

/// What an inventory context action (or drag) acts on: a snapshot of the row's
/// resolved info, taken when the menu opened (or the drag began), so the acted-
/// on data cannot be recycled out from under the action.
#[derive(Debug, Clone)]
pub(crate) enum MenuTarget {
    /// An item row.
    Item(ItemInfo),
    /// A folder row.
    Folder(FolderInfo),
}

/// The row the currently-open inventory context menu acts on. Set on every
/// open; a stale value between opens is harmless because the menu's element is
/// only emitted while a menu is open.
#[derive(Resource, Debug, Default)]
pub(crate) struct InventoryMenuTarget {
    /// The snapshotted row, or `None` before any menu has opened.
    pub(crate) target: Option<MenuTarget>,
}

/// Whether a clipboard entry pastes as a copy or a move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClipboardMode {
    /// Copy on paste (the source stays).
    Copy,
    /// Move on paste (the source goes).
    Cut,
}

/// The inventory clipboard: at most one copied / cut row (the tree has no
/// multi-select yet). Paste consumes a Cut entry; a Copy entry can be pasted
/// repeatedly, matching the reference.
#[derive(Resource, Debug, Default)]
pub(crate) struct InventoryClipboard {
    /// The held entry, or `None` when the clipboard is empty.
    pub(crate) entry: Option<(ClipboardMode, MenuTarget)>,
}

/// The attachments this viewer session knows to be worn, by inventory item id.
///
/// Best-effort, like [`crate::avatar_menu::SelfGroundSit`]: seeded from the
/// Current Outfit Folder's links when the grid maintains one, and tracked
/// through the wear / detach commands this viewer itself sends. An attachment
/// worn by another viewer mid-session is not observed; the worst case is a
/// momentarily wrong Wear / Detach enable.
#[derive(Resource, Debug, Default)]
pub(crate) struct WornAttachments {
    /// The inventory item ids of attachments known worn.
    pub(crate) items: HashSet<InventoryKey>,
}

/// The gestures activated this session, by inventory item id. Viewer-tracked
/// (the wire carries no "active gestures" read-back); a gesture activated in a
/// previous session reads as inactive until toggled.
#[derive(Resource, Debug, Default)]
pub(crate) struct ActiveGestures {
    /// The inventory item ids of gestures activated this session.
    pub(crate) items: HashSet<InventoryKey>,
}

// ---------------------------------------------------------------------------
// Pure condition / wear-set helpers, tested in isolation.
// ---------------------------------------------------------------------------

/// The wearable slot an inventory wearable occupies, from the item's low flag
/// byte (`LLInventoryItemFlags::II_FLAGS_SUBTYPE_MASK`).
pub(crate) fn wearable_type_of(item: &ItemInfo) -> WearableType {
    WearableType::from_code(u8::try_from(item.flags & 0xFF).unwrap_or(0))
}

/// Whether `item` is currently worn: as a legacy wearable (the
/// `AgentWearables` set), or as a Current Outfit Folder link (the modern worn
/// set — a COF link's asset id names the linked item), or — for attachments —
/// tracked by this viewer's own wear / detach commands.
pub(crate) fn is_worn(
    item: &ItemInfo,
    wearables: &[Wearable],
    cof_items: &[ItemInfo],
    tracked_attachments: &HashSet<InventoryKey>,
) -> bool {
    wearables.iter().any(|worn| worn.item_id == item.item_id)
        || tracked_attachments.contains(&item.item_id)
        || cof_items
            .iter()
            .any(|link| link.item_id == item.item_id || link.asset_id == item.item_id.uuid())
}

/// The facts about an item row the condition computation needs beyond the item
/// itself — resolved from the model / clipboard by the opener, plain data so
/// the computation is testable without a Bevy world.
#[expect(
    clippy::struct_excessive_bools,
    reason = "five independent yes/no facts about one row; they are consumed together by one \
              pure function and a state machine would invent couplings that do not exist"
)]
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ItemMenuFacts {
    /// The item sits in the read-only Library tree.
    pub(crate) in_library: bool,
    /// The item sits inside the Trash.
    pub(crate) in_trash: bool,
    /// The clipboard holds an entry (paste has a source).
    pub(crate) clipboard_has_entry: bool,
    /// The item is currently worn (wearable or attachment).
    pub(crate) worn: bool,
    /// The gesture is active this session (meaningful only for gestures).
    pub(crate) gesture_active: bool,
}

/// The conditions that hold for an **item** row's context menu.
pub(crate) fn item_conditions(item: &ItemInfo, facts: ItemMenuFacts) -> Vec<&'static str> {
    let mut held = Vec::new();
    // Type visibility.
    match item.inv_type {
        InventoryType::Landmark => held.push(IS_LANDMARK),
        InventoryType::Gesture => held.push(IS_GESTURE),
        InventoryType::Object | InventoryType::Attachment => held.push(IS_OBJECT),
        InventoryType::Wearable => {
            held.push(IS_WEARABLE);
            if item.asset_type == AssetType::Clothing {
                held.push(IS_CLOTHING);
            }
        }
        InventoryType::CallingCard => held.push(IS_CALLING_CARD),
        InventoryType::Sound => held.push(IS_SOUND),
        InventoryType::Animation => held.push(IS_ANIMATION),
        InventoryType::Texture | InventoryType::Snapshot => held.push(IS_TEXTURE),
        InventoryType::Settings => held.push(IS_SETTINGS),
        _other => {}
    }
    held.push(if facts.in_trash {
        IN_TRASH
    } else {
        NOT_IN_TRASH
    });
    // Capabilities. Everything mutating is withheld for a Library row.
    let mutable = !facts.in_library;
    if mutable {
        held.push(CAN_RENAME);
        held.push(CAN_CUT);
        if !facts.in_trash {
            held.push(CAN_DELETE);
        }
    }
    if facts.in_library || item.permissions.owner.contains(Permissions::COPY) {
        held.push(CAN_COPY);
    }
    if facts.clipboard_has_entry && mutable {
        held.push(CAN_PASTE);
        held.push(CAN_PASTE_LINK);
    }
    held.push(if facts.worn { WORN } else { NOT_WORN });
    if item.inv_type == InventoryType::Gesture {
        held.push(if facts.gesture_active {
            GESTURE_ACTIVE
        } else {
            GESTURE_INACTIVE
        });
    }
    held
}

/// The facts about a folder row the condition computation needs, as
/// [`ItemMenuFacts`].
#[expect(
    clippy::struct_excessive_bools,
    reason = "five independent yes/no facts about one folder; they are consumed together by \
              one pure function and a state machine would invent couplings that do not exist"
)]
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct FolderMenuFacts {
    /// The folder sits in the read-only Library tree.
    pub(crate) in_library: bool,
    /// The folder sits **inside** the Trash (not: is the Trash).
    pub(crate) in_trash: bool,
    /// The clipboard holds an entry.
    pub(crate) clipboard_has_entry: bool,
    /// The folder's loaded subtree holds something wearable.
    pub(crate) has_wearables: bool,
    /// The folder's loaded subtree holds something currently worn.
    pub(crate) has_worn: bool,
}

/// The conditions that hold for a **folder** row's context menu.
pub(crate) fn folder_conditions(folder: &FolderInfo, facts: FolderMenuFacts) -> Vec<&'static str> {
    let mut held = Vec::new();
    match folder.folder_type {
        FolderType::Trash => held.push(IS_TRASH_FOLDER),
        FolderType::LostAndFound => held.push(IS_LOST_FOUND_FOLDER),
        _other => {}
    }
    held.push(if facts.in_trash {
        IN_TRASH
    } else {
        NOT_IN_TRASH
    });
    let mutable = !facts.in_library;
    if mutable {
        held.push(CAN_CREATE);
        // Only a plain user folder may be renamed / cut / deleted; the system
        // folders (Trash, Clothing, the root, …) keep their role.
        if folder.folder_type == FolderType::None {
            held.push(CAN_RENAME);
            held.push(CAN_CUT);
            if !facts.in_trash {
                held.push(CAN_DELETE);
            }
        }
    }
    if facts.clipboard_has_entry && mutable {
        held.push(CAN_PASTE);
        held.push(CAN_PASTE_LINK);
    }
    if facts.has_wearables {
        held.push(FOLDER_HAS_WEARABLES);
    }
    if facts.has_worn {
        held.push(FOLDER_HAS_WORN);
    }
    held
}

/// The system folder a restored item of a given asset type belongs in — the
/// reference's `LLFolderType::assetTypeToFolderType`, used by Restore Item
/// (`LLItemBridge::restoreItem`) because the wire does not record where a
/// trashed item used to live. Asset types without a same-named system folder
/// map to [`FolderType::None`], which the caller resolves to the agent root.
pub(crate) const fn default_folder_type(asset_type: AssetType) -> FolderType {
    match asset_type {
        AssetType::Texture => FolderType::Texture,
        AssetType::Sound => FolderType::Sound,
        AssetType::CallingCard => FolderType::CallingCard,
        AssetType::Landmark => FolderType::Landmark,
        AssetType::Clothing => FolderType::Clothing,
        AssetType::Object => FolderType::Object,
        AssetType::Notecard => FolderType::Notecard,
        AssetType::ScriptText => FolderType::ScriptText,
        AssetType::Bodypart => FolderType::Bodypart,
        AssetType::Animation => FolderType::Animation,
        AssetType::Gesture => FolderType::Gesture,
        AssetType::Mesh => FolderType::Mesh,
        AssetType::Settings => FolderType::Settings,
        AssetType::Material => FolderType::Material,
        _other => FolderType::None,
    }
}

/// Whether an item can be part of an outfit: a body wearable or an attachable
/// object.
pub(crate) const fn is_outfit_item(item: &ItemInfo) -> bool {
    matches!(
        item.inv_type,
        InventoryType::Wearable | InventoryType::Object | InventoryType::Attachment
    )
}

/// The commands that **add** a folder's items to the current outfit: one
/// `AgentIsNowWearing` folding every body wearable in (a body part replaces
/// its slot, a clothing layer stacks), and one compound
/// `RezMultipleAttachmentsFromInv` adding the objects alongside what is worn.
/// Returns the commands and the attachment item ids now known worn.
pub(crate) fn outfit_add_commands(
    items: &[ItemInfo],
    current: &[Wearable],
    own_agent: Option<AgentKey>,
) -> (Vec<Command>, Vec<InventoryKey>) {
    let mut set: Vec<Wearable> = current.to_vec();
    let mut wearables_changed = false;
    let mut attachments = Vec::new();
    let mut tracked = Vec::new();
    for item in items {
        match item.inv_type {
            InventoryType::Wearable => {
                if set.iter().any(|worn| worn.item_id == item.item_id) {
                    continue;
                }
                let slot = wearable_type_of(item);
                if slot.is_body_part() {
                    set.retain(|worn| worn.wearable_type != slot);
                }
                set.push(Wearable {
                    item_id: item.item_id,
                    asset_id: None,
                    wearable_type: slot,
                });
                wearables_changed = true;
            }
            InventoryType::Object | InventoryType::Attachment => {
                attachments.push(RezAttachment {
                    item_id: item.item_id,
                    owner_id: own_agent.map_or_else(Uuid::nil, |agent| agent.uuid()),
                    attachment_point: AttachmentPoint::Default,
                    mode: AttachmentMode::Add,
                    name: item.name.clone(),
                    description: item.description.clone(),
                });
                tracked.push(item.item_id);
            }
            _other => {}
        }
    }
    let mut commands = Vec::new();
    if wearables_changed {
        commands.push(Command::SetWearing(set));
        commands.push(Command::RequestWearables);
    }
    if !attachments.is_empty() {
        commands.push(Command::RezAttachments {
            compound_id: TransactionId::from(Uuid::new_v4()),
            detach: DetachOrder::Keep,
            attachments,
        });
    }
    (commands, tracked)
}

/// The commands that **remove** a folder's worn items from the current outfit:
/// one `AgentIsNowWearing` without its worn clothing layers (body parts stay —
/// an avatar always wears each part), and a detach per worn attachment.
/// Returns the commands and the attachment item ids no longer worn.
pub(crate) fn outfit_remove_commands(
    items: &[ItemInfo],
    current: &[Wearable],
    cof_items: &[ItemInfo],
    tracked_attachments: &HashSet<InventoryKey>,
) -> (Vec<Command>, Vec<InventoryKey>) {
    let mut set: Vec<Wearable> = current.to_vec();
    let mut wearables_changed = false;
    let mut commands = Vec::new();
    let mut untracked = Vec::new();
    for item in items {
        if !is_worn(item, current, cof_items, tracked_attachments) {
            continue;
        }
        match item.inv_type {
            InventoryType::Wearable => {
                if wearable_type_of(item).is_body_part() {
                    continue;
                }
                let before = set.len();
                set.retain(|worn| worn.item_id != item.item_id);
                if set.len() != before {
                    wearables_changed = true;
                }
            }
            InventoryType::Object | InventoryType::Attachment => {
                commands.push(Command::DetachAttachmentIntoInventory {
                    item_id: item.item_id,
                });
                untracked.push(item.item_id);
            }
            _other => {}
        }
    }
    if wearables_changed {
        commands.push(Command::SetWearing(set));
        commands.push(Command::RequestWearables);
    }
    (commands, untracked)
}

/// The wear-set the legacy `AgentIsNowWearing` should carry after wearing
/// `item`: the current set with the same slot **replaced** (`add == false`, the
/// Wear action and any body part) or with the item **added alongside**
/// (`add == true`, the clothing Add action).
pub(crate) fn wear_set(current: &[Wearable], item: &ItemInfo, add: bool) -> Vec<Wearable> {
    let slot = wearable_type_of(item);
    let mut set: Vec<Wearable> = current.to_vec();
    if !add {
        set.retain(|worn| worn.wearable_type != slot);
    }
    set.push(Wearable {
        item_id: item.item_id,
        asset_id: None,
        wearable_type: slot,
    });
    set
}

/// The wear-set after taking `item` off: the current set without it.
pub(crate) fn take_off_set(current: &[Wearable], item: InventoryKey) -> Vec<Wearable> {
    current
        .iter()
        .copied()
        .filter(|worn| worn.item_id != item)
        .collect()
}

/// The commands that wear `item` (an object → attach; a wearable → the legacy
/// wear-set update plus a re-read). `add` keeps what is already worn alongside.
/// Shared by the context menu and a drag-drop onto the own avatar.
pub(crate) fn wear_commands(
    item: &ItemInfo,
    own_agent: Option<AgentKey>,
    current_wearables: &[Wearable],
    add: bool,
) -> Vec<Command> {
    match item.inv_type {
        InventoryType::Object | InventoryType::Attachment => {
            vec![Command::RezAttachment(sl_client_bevy::RezAttachment {
                item_id: item.item_id,
                owner_id: own_agent.map_or_else(Uuid::nil, |agent| agent.uuid()),
                attachment_point: sl_client_bevy::AttachmentPoint::Default,
                mode: if add {
                    sl_client_bevy::AttachmentMode::Add
                } else {
                    sl_client_bevy::AttachmentMode::Replace
                },
                name: item.name.clone(),
                description: item.description.clone(),
            })]
        }
        InventoryType::Wearable => vec![
            Command::SetWearing(wear_set(current_wearables, item, add)),
            Command::RequestWearables,
        ],
        _other => Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Opening: right-click on a row / the list background.
// ---------------------------------------------------------------------------

/// Resolve a row's [`DisplayRow`] to a snapshot + conditions and open its menu.
/// Returns `None` when the row cannot be resolved in the model (e.g. a Recent
/// entry whose folder is not loaded).
#[expect(
    clippy::too_many_arguments,
    reason = "the resolution reads every fact source the conditions draw on: the model, the \
              clipboard, the tracked worn / gesture sets, and the two output channels"
)]
fn open_menu_for_row(
    row: &DisplayRow,
    at: Vec2,
    model: &InventoryModel,
    clipboard: &InventoryClipboard,
    worn: &WornAttachments,
    gestures: &ActiveGestures,
    target: &mut InventoryMenuTarget,
    menus: &mut MessageWriter<OpenContextMenu>,
) -> Option<()> {
    let trash = model.folder_by_type(FolderType::Trash);
    let in_trash = |folder: InventoryFolderKey| {
        trash.is_some_and(|trash_key| model.is_within(folder, trash_key))
    };
    let clipboard_has_entry = clipboard.entry.is_some();
    let (menu, conditions, snapshot) = match row.key() {
        RowKey::Folder(key) => {
            let info = model.folder_info(key)?.clone();
            let subtree = model.subtree_items(key);
            let facts = FolderMenuFacts {
                in_library: model.is_library(key),
                // "In the trash" for Delete-vs-Purge means *below* the Trash;
                // the Trash folder itself is emptied, not purged.
                in_trash: info.parent_id.is_some_and(in_trash),
                clipboard_has_entry,
                has_wearables: subtree.iter().any(|item| is_outfit_item(item)),
                has_worn: subtree.iter().any(|item| {
                    is_worn(item, model.worn_wearables(), model.cof_items(), &worn.items)
                }),
            };
            (
                &INVENTORY_FOLDER_MENU,
                folder_conditions(&info, facts),
                MenuTarget::Folder(info),
            )
        }
        RowKey::Item(key) => {
            let info = model.find_item(key)?.clone();
            let facts = ItemMenuFacts {
                in_library: model.is_library(info.folder_id),
                in_trash: in_trash(info.folder_id),
                clipboard_has_entry,
                worn: is_worn(
                    &info,
                    model.worn_wearables(),
                    model.cof_items(),
                    &worn.items,
                ),
                gesture_active: gestures.items.contains(&info.item_id),
            };
            (
                &INVENTORY_ITEM_MENU,
                item_conditions(&info, facts),
                MenuTarget::Item(info),
            )
        }
    };
    target.target = Some(snapshot);
    menus.write(OpenContextMenu {
        menu,
        at,
        element: INVENTORY_MENU_ELEMENT,
        conditions,
    });
    Some(())
}

/// A right-click on a pooled inventory row: resolve the row it currently
/// presents and open the matching context menu at the pointer.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy observer's parameters are its injected resources: the row pool, the view, \
              the model and the fact sources, plus the target stash and the open channel"
)]
pub(crate) fn on_row_context(
    mut press: On<Pointer<Press>>,
    rows: Query<&VirtualRow>,
    view: Res<InventoryView>,
    model: Res<InventoryModel>,
    clipboard: Res<InventoryClipboard>,
    worn: Res<WornAttachments>,
    gestures: Res<ActiveGestures>,
    mut selection: ResMut<InventorySelection>,
    mut target: ResMut<InventoryMenuTarget>,
    mut menus: MessageWriter<OpenContextMenu>,
) {
    if press.button != PointerButton::Secondary {
        return;
    }
    let Ok(row) = rows.get(press.entity) else {
        return;
    };
    let Some(index) = row.index else {
        return;
    };
    let Some(display) = view.rows().get(index) else {
        return;
    };
    // Consume the press so the viewport's background handler does not also open
    // the root menu underneath this row's.
    press.propagate(false);
    // The usual list semantics: a right-click on an unselected row selects it
    // (a right-click inside the selection keeps the selection).
    if !selection.contains(display.key()) {
        selection.select_single(display.key(), index);
    }
    open_menu_for_row(
        display,
        press.pointer_location.position,
        &model,
        &clipboard,
        &worn,
        &gestures,
        &mut target,
        &mut menus,
    );
}

/// A right-click on the list's empty background: target the agent's root
/// folder ("My Inventory"), the reference's top-level New Folder / Paste menu.
pub(crate) fn on_viewport_context(
    press: On<Pointer<Press>>,
    model: Res<InventoryModel>,
    clipboard: Res<InventoryClipboard>,
    mut target: ResMut<InventoryMenuTarget>,
    mut menus: MessageWriter<OpenContextMenu>,
) {
    if press.button != PointerButton::Secondary {
        return;
    }
    let Some(root) = model.agent_root() else {
        return;
    };
    let Some(info) = model.folder_info(root).cloned() else {
        return;
    };
    let conditions = folder_conditions(
        &info,
        FolderMenuFacts {
            clipboard_has_entry: clipboard.entry.is_some(),
            ..FolderMenuFacts::default()
        },
    );
    target.target = Some(MenuTarget::Folder(info));
    menus.write(OpenContextMenu {
        menu: &INVENTORY_FOLDER_MENU,
        at: press.pointer_location.position,
        element: INVENTORY_MENU_ELEMENT,
        conditions,
    });
}

// ---------------------------------------------------------------------------
// Dispatch: a picked entry → session commands.
// ---------------------------------------------------------------------------

/// The folder a paste / create targets for a given menu target: the folder row
/// itself, or the containing folder of an item row.
const fn destination_folder(target: &MenuTarget) -> InventoryFolderKey {
    match target {
        MenuTarget::Folder(info) => info.folder_id,
        MenuTarget::Item(info) => info.folder_id,
    }
}

/// The commands a **paste** issues for the clipboard entry into `dest`, plus
/// the folders to re-query afterwards. A Copy entry copies (`CopyInventoryItem`
/// — the reply's bulk update refreshes the destination); a Cut entry moves.
/// Pure, so the copy-vs-move and refresh choices are testable.
pub(crate) fn paste_commands(
    mode: ClipboardMode,
    entry: &MenuTarget,
    dest: InventoryFolderKey,
    own_agent: Option<AgentKey>,
) -> (Vec<Command>, Vec<InventoryFolderKey>) {
    match (mode, entry) {
        (ClipboardMode::Copy, MenuTarget::Item(item)) => {
            let owner = match item.owner {
                sl_client_bevy::OwnerKey::Agent(agent) => agent,
                _other => own_agent.unwrap_or_else(|| AgentKey::from(Uuid::nil())),
            };
            (
                vec![Command::CopyInventoryItem {
                    old_agent_id: owner,
                    old_item_id: item.item_id,
                    new_folder_id: dest,
                    new_name: String::new(),
                }],
                // The new item arrives via the bulk-update reply, which the
                // ingest already re-queries; nothing to refresh eagerly.
                Vec::new(),
            )
        }
        (ClipboardMode::Cut, MenuTarget::Item(item)) => (
            vec![Command::MoveInventoryItem {
                item_id: item.item_id,
                folder_id: dest,
                new_name: String::new(),
            }],
            vec![item.folder_id, dest],
        ),
        (ClipboardMode::Cut, MenuTarget::Folder(folder)) => (
            vec![
                Command::MoveInventoryFolder {
                    folder_id: folder.folder_id,
                    parent_id: dest,
                },
                Command::QueryInventoryFolders,
            ],
            Vec::new(),
        ),
        // A folder deep-copy is not wired (its menu entry is a placeholder), so
        // a copied folder can never be in the clipboard.
        (ClipboardMode::Copy, MenuTarget::Folder(_folder)) => (Vec::new(), Vec::new()),
    }
}

/// Handle a picked inventory context-menu entry.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources: the action stream, the \
              target / clipboard / worn / gesture stashes, the model for trash lookups, the \
              identity, and the command / conversation / rename channels"
)]
fn handle_inventory_menu_actions(
    mut actions: MessageReader<UiAction>,
    target: Res<InventoryMenuTarget>,
    model: Res<InventoryModel>,
    identity: Res<SlIdentity>,
    mut clipboard: ResMut<InventoryClipboard>,
    mut worn: ResMut<WornAttachments>,
    mut gestures: ResMut<ActiveGestures>,
    mut rename: ResMut<crate::inventory::InlineRename>,
    mut ui_actions: MessageWriter<crate::inventory::InventoryUiAction>,
    mut conversations: MessageWriter<OpenConversation>,
    mut commands: MessageWriter<SlCommand>,
) {
    for action in actions.read() {
        if action.element != INVENTORY_MENU_ELEMENT {
            continue;
        }
        let Some(menu_target) = target.target.clone() else {
            continue;
        };
        let dest = destination_folder(&menu_target);
        match action.action {
            "rename" => {
                // The tree edits the label in place ([`crate::inventory`]'s
                // inline rename).
                rename.pending = Some(match &menu_target {
                    MenuTarget::Item(item) => RowKey::Item(item.item_id),
                    MenuTarget::Folder(folder) => RowKey::Folder(folder.folder_id),
                });
            }
            "cut" => {
                clipboard.entry = Some((ClipboardMode::Cut, menu_target.clone()));
            }
            "copy" => {
                clipboard.entry = Some((ClipboardMode::Copy, menu_target.clone()));
            }
            "paste" => {
                if let Some((mode, entry)) = clipboard.entry.clone() {
                    let (paste, refresh) = paste_commands(mode, &entry, dest, identity.agent_id);
                    for command in paste {
                        commands.write(SlCommand(command));
                    }
                    for folder in refresh {
                        query_folder_page(folder, &mut commands);
                    }
                    if mode == ClipboardMode::Cut {
                        clipboard.entry = None;
                    }
                }
            }
            "paste-link" => {
                if let Some((_mode, entry)) = clipboard.entry.clone() {
                    let link = match &entry {
                        MenuTarget::Item(item) => NewInventoryLink {
                            folder_id: dest,
                            linked_id: InventoryItemOrFolderKey::Item(item.item_id),
                            // `AT_LINK` (24): an item link.
                            link_type: AssetType::Other(24),
                            inv_type: item.inv_type,
                            name: item.name.clone(),
                            description: String::new(),
                        },
                        MenuTarget::Folder(folder) => NewInventoryLink {
                            folder_id: dest,
                            linked_id: InventoryItemOrFolderKey::Folder(folder.folder_id),
                            // `AT_LINK_FOLDER` (25): a folder link.
                            link_type: AssetType::Other(25),
                            inv_type: InventoryType::Category,
                            name: folder.name.clone(),
                            description: String::new(),
                        },
                    };
                    commands.write(SlCommand(Command::LinkInventoryItem(link)));
                }
            }
            "delete" => {
                if let Some(trash) = model.folder_by_type(FolderType::Trash) {
                    match &menu_target {
                        MenuTarget::Item(item) => {
                            commands.write(SlCommand(Command::MoveInventoryItem {
                                item_id: item.item_id,
                                folder_id: trash,
                                new_name: String::new(),
                            }));
                            query_folder_page(item.folder_id, &mut commands);
                            query_folder_page(trash, &mut commands);
                        }
                        MenuTarget::Folder(folder) => {
                            commands.write(SlCommand(Command::MoveInventoryFolder {
                                folder_id: folder.folder_id,
                                parent_id: trash,
                            }));
                            commands.write(SlCommand(Command::QueryInventoryFolders));
                        }
                    }
                }
            }
            "purge" => match &menu_target {
                MenuTarget::Item(item) => {
                    commands.write(SlCommand(Command::RemoveInventoryItems(vec![item.item_id])));
                    query_folder_page(item.folder_id, &mut commands);
                }
                MenuTarget::Folder(folder) => {
                    commands.write(SlCommand(Command::RemoveInventoryFolders(vec![
                        folder.folder_id,
                    ])));
                    commands.write(SlCommand(Command::QueryInventoryFolders));
                }
            },
            "restore" => {
                // The wire does not record where a trashed row came from; the
                // reference restores to the type's system folder
                // (`LLItemBridge::restoreItem` via `findCategoryUUIDForType`),
                // falling back to the agent root.
                match &menu_target {
                    MenuTarget::Item(item) => {
                        // A snapshot restores to the Photo Album, not Textures
                        // (the reference's `IT_SNAPSHOT` special case).
                        let wanted = if item.inv_type == InventoryType::Snapshot {
                            FolderType::SnapshotCategory
                        } else {
                            default_folder_type(item.asset_type)
                        };
                        let dest = if wanted == FolderType::None {
                            model.agent_root()
                        } else {
                            model.folder_by_type(wanted).or_else(|| model.agent_root())
                        };
                        if let Some(dest) = dest {
                            commands.write(SlCommand(Command::MoveInventoryItem {
                                item_id: item.item_id,
                                folder_id: dest,
                                new_name: String::new(),
                            }));
                            query_folder_page(item.folder_id, &mut commands);
                            query_folder_page(dest, &mut commands);
                        }
                    }
                    MenuTarget::Folder(folder) => {
                        // A folder has no typed home; it restores to the root.
                        if let Some(root) = model.agent_root() {
                            commands.write(SlCommand(Command::MoveInventoryFolder {
                                folder_id: folder.folder_id,
                                parent_id: root,
                            }));
                            commands.write(SlCommand(Command::QueryInventoryFolders));
                        }
                    }
                }
            }
            "empty-trash" => {
                if let Some(trash) = model.folder_by_type(FolderType::Trash) {
                    commands.write(SlCommand(Command::PurgeInventoryDescendents(trash)));
                    commands.write(SlCommand(Command::QueryInventoryFolders));
                    query_folder_page(trash, &mut commands);
                }
            }
            "empty-lost-and-found" => {
                if let Some(lost) = model.folder_by_type(FolderType::LostAndFound) {
                    commands.write(SlCommand(Command::PurgeInventoryDescendents(lost)));
                    commands.write(SlCommand(Command::QueryInventoryFolders));
                    query_folder_page(lost, &mut commands);
                }
            }
            "new-folder" => {
                // The UDP create lets the viewer pick the id, so the fresh
                // folder can be put straight into **inline rename** (the
                // reference's new-folder-starts-editing behaviour) once the
                // refreshed skeleton shows it under its (expanded) parent.
                let folder_id = InventoryFolderKey::from(Uuid::new_v4());
                commands.write(SlCommand(Command::CreateInventoryFolder {
                    folder_id,
                    parent_id: dest,
                    folder_type: FolderType::None,
                    name: "New Folder".to_owned(),
                }));
                commands.write(SlCommand(Command::QueryInventoryFolders));
                ui_actions.write(crate::inventory::InventoryUiAction::ExpandFolder(dest));
                rename.pending = Some(RowKey::Folder(folder_id));
            }
            "new-script" => {
                commands.write(SlCommand(Command::CreateScript {
                    folder_id: dest,
                    name: "New Script".to_owned(),
                    description: String::new(),
                    next_owner_mask: NEXT_OWNER_DEFAULT,
                    language: ScriptLanguage::Lsl,
                }));
            }
            "new-notecard" => {
                commands.write(SlCommand(Command::CreateInventoryItem(NewInventoryItem {
                    folder_id: dest,
                    next_owner_mask: NEXT_OWNER_DEFAULT,
                    asset_type: AssetType::Notecard,
                    inv_type: InventoryType::Notecard,
                    name: "New Note".to_owned(),
                    ..NewInventoryItem::default()
                })));
            }
            "new-gesture" => {
                commands.write(SlCommand(Command::CreateInventoryItem(NewInventoryItem {
                    folder_id: dest,
                    next_owner_mask: NEXT_OWNER_DEFAULT,
                    asset_type: AssetType::Gesture,
                    inv_type: InventoryType::Gesture,
                    name: "New Gesture".to_owned(),
                    ..NewInventoryItem::default()
                })));
            }
            "teleport" => {
                if let MenuTarget::Item(item) = &menu_target {
                    commands.write(SlCommand(Command::TeleportViaLandmark {
                        landmark: Some(AssetKey::from(item.asset_id)),
                    }));
                }
            }
            "send-im" => {
                if let MenuTarget::Item(item) = &menu_target {
                    // A calling card names its person as the item's creator.
                    conversations.write(OpenConversation {
                        key: ConversationKey::Direct(item.creator_id),
                    });
                }
            }
            "activate-gesture" => {
                if let MenuTarget::Item(item) = &menu_target {
                    commands.write(SlCommand(Command::ActivateGestures {
                        gestures: vec![GestureActivation {
                            item_id: item.item_id,
                            asset_id: item.asset_id,
                        }],
                    }));
                    gestures.items.insert(item.item_id);
                }
            }
            "deactivate-gesture" => {
                if let MenuTarget::Item(item) = &menu_target {
                    commands.write(SlCommand(Command::DeactivateGestures {
                        item_ids: vec![item.item_id],
                    }));
                    gestures.items.remove(&item.item_id);
                }
            }
            "wear-wearable" | "attach" => {
                if let MenuTarget::Item(item) = &menu_target {
                    for command in
                        wear_commands(item, identity.agent_id, model.worn_wearables(), false)
                    {
                        commands.write(SlCommand(command));
                    }
                    if matches!(
                        item.inv_type,
                        InventoryType::Object | InventoryType::Attachment
                    ) {
                        worn.items.insert(item.item_id);
                    }
                }
            }
            "add-wearable" | "attach-add" => {
                if let MenuTarget::Item(item) = &menu_target {
                    for command in
                        wear_commands(item, identity.agent_id, model.worn_wearables(), true)
                    {
                        commands.write(SlCommand(command));
                    }
                    if matches!(
                        item.inv_type,
                        InventoryType::Object | InventoryType::Attachment
                    ) {
                        worn.items.insert(item.item_id);
                    }
                }
            }
            "take-off" => {
                if let MenuTarget::Item(item) = &menu_target {
                    commands.write(SlCommand(Command::SetWearing(take_off_set(
                        model.worn_wearables(),
                        item.item_id,
                    ))));
                    commands.write(SlCommand(Command::RequestWearables));
                }
            }
            "detach" => {
                if let MenuTarget::Item(item) = &menu_target {
                    commands.write(SlCommand(Command::DetachAttachmentIntoInventory {
                        item_id: item.item_id,
                    }));
                    worn.items.remove(&item.item_id);
                }
            }
            "add-to-outfit" => {
                if let MenuTarget::Folder(folder) = &menu_target {
                    let items: Vec<ItemInfo> = model
                        .subtree_items(folder.folder_id)
                        .into_iter()
                        .cloned()
                        .collect();
                    let (batch, now_worn) =
                        outfit_add_commands(&items, model.worn_wearables(), identity.agent_id);
                    for command in batch {
                        commands.write(SlCommand(command));
                    }
                    worn.items.extend(now_worn);
                }
            }
            "remove-from-outfit" => {
                if let MenuTarget::Folder(folder) = &menu_target {
                    let items: Vec<ItemInfo> = model
                        .subtree_items(folder.folder_id)
                        .into_iter()
                        .cloned()
                        .collect();
                    let (batch, no_longer_worn) = outfit_remove_commands(
                        &items,
                        model.worn_wearables(),
                        model.cof_items(),
                        &worn.items,
                    );
                    for command in batch {
                        commands.write(SlCommand(command));
                    }
                    for item_id in no_longer_worn {
                        worn.items.remove(&item_id);
                    }
                }
            }
            // The Attach To ▸ / Attach To HUD ▸ submenus: the action names the
            // chosen point's wire id, and the attach **replaces** what is on
            // that point (the reference's bare-point-id semantics; Add-along
            // is the plain "Add" entry on the default point).
            other => {
                if let Some(point) = attach_point_of(other)
                    && let MenuTarget::Item(item) = &menu_target
                {
                    commands.write(SlCommand(Command::RezAttachment(RezAttachment {
                        item_id: item.item_id,
                        owner_id: identity
                            .agent_id
                            .map_or_else(Uuid::nil, |agent| agent.uuid()),
                        attachment_point: point,
                        mode: AttachmentMode::Replace,
                        name: item.name.clone(),
                        description: item.description.clone(),
                    })));
                    worn.items.insert(item.item_id);
                }
                // Every other entry is a disabled placeholder that never emits.
            }
        }
    }
}

/// Parse an `attach-point-<id>` menu action back into its attachment point.
/// `None` for any other action string.
pub(crate) fn attach_point_of(action: &str) -> Option<AttachmentPoint> {
    let code: u8 = action.strip_prefix("attach-point-")?.parse().ok()?;
    Some(AttachmentPoint::from_code(code))
}

/// Seed [`WornAttachments`] from the Current Outfit Folder once its contents
/// load: a COF **link** whose asset id names an object item marks that item
/// worn. Cheap and idempotent — recomputed only when the model changes.
fn seed_worn_from_cof(model: Res<InventoryModel>, mut worn: ResMut<WornAttachments>) {
    if !model.is_changed() {
        return;
    }
    for link in model.cof_items() {
        if matches!(
            link.inv_type,
            InventoryType::Object | InventoryType::Attachment
        ) {
            let target = InventoryKey::from(link.asset_id);
            worn.items.insert(target);
        }
    }
}

/// The plugin wiring the inventory context actions into the viewer.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct InventoryActionsPlugin;

impl Plugin for InventoryActionsPlugin {
    /// Register the target / clipboard / worn stashes, the rename dialog, and
    /// the dispatch systems. The row observers themselves are installed by
    /// [`crate::inventory`]'s row pool, and the menu widget by
    /// [`crate::menu::MenuWidgetPlugin`].
    fn build(&self, app: &mut App) {
        app.init_resource::<InventoryMenuTarget>()
            .init_resource::<InventoryClipboard>()
            .init_resource::<WornAttachments>()
            .init_resource::<ActiveGestures>()
            .add_systems(
                Update,
                (handle_inventory_menu_actions, seed_worn_from_cof).chain(),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CAN_COPY, CAN_CREATE, CAN_CUT, CAN_DELETE, CAN_PASTE, CAN_PASTE_LINK, CAN_RENAME,
        ClipboardMode, FOLDER_HAS_WEARABLES, FOLDER_HAS_WORN, FolderMenuFacts, GESTURE_ACTIVE,
        GESTURE_INACTIVE, IN_TRASH, INVENTORY_FOLDER_MENU, INVENTORY_ITEM_MENU, IS_CLOTHING,
        IS_LANDMARK, IS_OBJECT, IS_TRASH_FOLDER, IS_WEARABLE, ItemMenuFacts, MenuTarget,
        NOT_IN_TRASH, NOT_WORN, WORN, folder_conditions, is_worn, item_conditions,
        outfit_add_commands, outfit_remove_commands, paste_commands, take_off_set, wear_set,
    };
    use crate::menu::{MenuDef, MenuItemDef};
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{
        AgentKey, AssetType, Command, FolderInfo, FolderState, FolderType, InventoryFolderKey,
        InventoryKey, InventoryType, ItemInfo, Permissions, Permissions5, Uuid, Wearable,
        WearableType,
    };
    use std::collections::HashSet;

    /// A minimal item of the given types, owned with the given owner mask.
    fn item(id: u128, inv_type: InventoryType, asset_type: AssetType, owner_mask: u32) -> ItemInfo {
        ItemInfo {
            item_id: InventoryKey::from(Uuid::from_u128(id)),
            folder_id: InventoryFolderKey::from(Uuid::from_u128(0xF0)),
            name: "Thing".to_owned(),
            description: String::new(),
            asset_id: Uuid::from_u128(0xA0),
            asset_type,
            inv_type,
            flags: 0,
            sale: None,
            creation_date: 0,
            owner: sl_client_bevy::OwnerKey::Agent(AgentKey::from(Uuid::from_u128(1))),
            last_owner_id: Uuid::nil(),
            creator_id: AgentKey::from(Uuid::from_u128(1)),
            group: None,
            permissions: Permissions5 {
                base: Permissions::from_bits(owner_mask),
                owner: Permissions::from_bits(owner_mask),
                group: Permissions::empty(),
                everyone: Permissions::empty(),
                next_owner: Permissions::empty(),
            },
        }
    }

    /// A folder info of the given type.
    fn folder(id: u128, folder_type: FolderType) -> FolderInfo {
        FolderInfo {
            folder_id: InventoryFolderKey::from(Uuid::from_u128(id)),
            parent_id: Some(InventoryFolderKey::from(Uuid::from_u128(0x01))),
            name: "Folder".to_owned(),
            folder_type,
            version: 1,
            state: FolderState::Unknown,
        }
    }

    /// Every (label, action) pair of a menu, in order, submenus flattened.
    fn entries(menu: &'static MenuDef) -> Vec<(&'static str, &'static str)> {
        fn walk(menu: &'static MenuDef, out: &mut Vec<(&'static str, &'static str)>) {
            for entry in menu.items {
                match entry {
                    MenuItemDef::Command(command) => out.push((command.label, command.action)),
                    MenuItemDef::Submenu(sub) | MenuItemDef::SubmenuWhen(sub, _) => {
                        walk(sub, out);
                    }
                    MenuItemDef::Separator => {}
                }
            }
        }
        let mut out = Vec::new();
        walk(menu, &mut out);
        out
    }

    /// **The item menu's entry table, pinned.** Reordering or renaming an entry
    /// re-teaches every user's muscle memory; an intended move must edit this
    /// table in the same commit (the pie-menu address-test convention).
    #[test]
    fn item_menu_keeps_every_entry() {
        let expected: Vec<(&str, &str)> = vec![
            ("Share", "share"),
            ("Open", "open"),
            ("Properties", "properties"),
            ("Rename", "rename"),
            ("Copy Asset UUID", "copy-asset-uuid"),
            ("Copy", "copy"),
            ("Cut", "cut"),
            ("Paste", "paste"),
            ("Paste As Link", "paste-link"),
            ("Delete", "delete"),
            ("Purge Item", "purge"),
            ("Restore Item", "restore"),
            ("Teleport", "teleport"),
            ("About Landmark", "about-landmark"),
            ("Show on Map", "show-on-map"),
            ("Play", "play-sound"),
            ("Play Inworld", "play-inworld"),
            ("Play Locally", "play-locally"),
            ("Send Instant Message", "send-im"),
            ("Offer Teleport...", "offer-teleport"),
            ("Play", "play-gesture"),
            ("Activate", "activate-gesture"),
            ("Deactivate", "deactivate-gesture"),
            ("Save As", "save-as"),
            ("Wear", "wear-wearable"),
            ("Add", "add-wearable"),
            ("Take Off", "take-off"),
            ("Edit", "edit-wearable"),
            ("Wear", "attach"),
            ("Add", "attach-add"),
            ("Chest (1)", "attach-point-1"),
            ("Skull (2)", "attach-point-2"),
            ("Left Shoulder (3)", "attach-point-3"),
            ("Right Shoulder (4)", "attach-point-4"),
            ("Left Hand (5)", "attach-point-5"),
            ("Right Hand (6)", "attach-point-6"),
            ("Left Foot (7)", "attach-point-7"),
            ("Right Foot (8)", "attach-point-8"),
            ("Spine (9)", "attach-point-9"),
            ("Pelvis (10)", "attach-point-10"),
            ("Mouth (11)", "attach-point-11"),
            ("Chin (12)", "attach-point-12"),
            ("Left Ear (13)", "attach-point-13"),
            ("Right Ear (14)", "attach-point-14"),
            ("Left Eyeball (15)", "attach-point-15"),
            ("Right Eyeball (16)", "attach-point-16"),
            ("Nose (17)", "attach-point-17"),
            ("R Upper Arm (18)", "attach-point-18"),
            ("R Forearm (19)", "attach-point-19"),
            ("L Upper Arm (20)", "attach-point-20"),
            ("L Forearm (21)", "attach-point-21"),
            ("Right Hip (22)", "attach-point-22"),
            ("R Upper Leg (23)", "attach-point-23"),
            ("R Lower Leg (24)", "attach-point-24"),
            ("Left Hip (25)", "attach-point-25"),
            ("L Upper Leg (26)", "attach-point-26"),
            ("L Lower Leg (27)", "attach-point-27"),
            ("Stomach (28)", "attach-point-28"),
            ("Left Pec (29)", "attach-point-29"),
            ("Right Pec (30)", "attach-point-30"),
            ("Neck (39)", "attach-point-39"),
            ("Avatar Center (40)", "attach-point-40"),
            ("Left Ring Finger (41)", "attach-point-41"),
            ("Right Ring Finger (42)", "attach-point-42"),
            ("Tail Base (43)", "attach-point-43"),
            ("Tail Tip (44)", "attach-point-44"),
            ("Left Wing (45)", "attach-point-45"),
            ("Right Wing (46)", "attach-point-46"),
            ("Jaw (47)", "attach-point-47"),
            ("Alt Left Ear (48)", "attach-point-48"),
            ("Alt Right Ear (49)", "attach-point-49"),
            ("Alt Left Eye (50)", "attach-point-50"),
            ("Alt Right Eye (51)", "attach-point-51"),
            ("Tongue (52)", "attach-point-52"),
            ("Groin (53)", "attach-point-53"),
            ("Left Hind Foot (54)", "attach-point-54"),
            ("Right Hind Foot (55)", "attach-point-55"),
            ("Center 2", "attach-point-31"),
            ("Top Right", "attach-point-32"),
            ("Top", "attach-point-33"),
            ("Top Left", "attach-point-34"),
            ("Center", "attach-point-35"),
            ("Bottom Left", "attach-point-36"),
            ("Bottom", "attach-point-37"),
            ("Bottom Right", "attach-point-38"),
            ("Touch", "touch"),
            ("Detach From Yourself", "detach"),
            ("Apply Only To Myself", "settings-apply-local"),
            ("Apply To Parcel", "settings-apply-parcel"),
        ];
        assert_eq!(
            entries(&INVENTORY_ITEM_MENU),
            expected,
            "an item context-menu entry moved — if intended, bless it by editing this table"
        );
    }

    /// **The folder menu's entry table, pinned.** As above.
    #[test]
    fn folder_menu_keeps_every_entry() {
        let expected: Vec<(&str, &str)> = vec![
            ("Share", "share"),
            ("Empty Trash", "empty-trash"),
            ("Empty Lost And Found", "empty-lost-and-found"),
            ("New Folder", "new-folder"),
            ("New Script", "new-script"),
            ("New Notecard", "new-notecard"),
            ("New Gesture", "new-gesture"),
            ("New Shirt", "new-shirt"),
            ("New Pants", "new-pants"),
            ("New Shoes", "new-shoes"),
            ("New Socks", "new-socks"),
            ("New Jacket", "new-jacket"),
            ("New Skirt", "new-skirt"),
            ("New Gloves", "new-gloves"),
            ("New Undershirt", "new-undershirt"),
            ("New Underpants", "new-underpants"),
            ("New Alpha Mask", "new-alpha"),
            ("New Tattoo", "new-tattoo"),
            ("New Universal", "new-universal"),
            ("New Physics", "new-physics"),
            ("New Shape", "new-shape"),
            ("New Skin", "new-skin"),
            ("New Hair", "new-hair"),
            ("New Eyes", "new-eyes"),
            ("Replace Current Outfit", "replace-outfit"),
            ("Add To Current Outfit", "add-to-outfit"),
            ("Remove From Current Outfit", "remove-from-outfit"),
            ("Rename", "rename"),
            ("Cut", "cut"),
            ("Copy", "copy"),
            ("Paste", "paste"),
            ("Paste As Link", "paste-link"),
            ("Delete", "delete"),
            ("Purge Item", "purge"),
            ("Restore Item", "restore"),
        ];
        assert_eq!(
            entries(&INVENTORY_FOLDER_MENU),
            expected,
            "a folder context-menu entry moved — if intended, bless it by editing this table"
        );
    }

    /// The attach-point action strings parse back to their wire points, and
    /// anything else parses to nothing.
    #[test]
    fn attach_point_actions_round_trip() {
        use super::attach_point_of;
        use sl_client_bevy::AttachmentPoint;
        assert_eq!(
            attach_point_of("attach-point-1"),
            Some(AttachmentPoint::Chest)
        );
        assert_eq!(
            attach_point_of("attach-point-35"),
            Some(AttachmentPoint::HudCenter)
        );
        assert_eq!(
            attach_point_of("attach-point-55"),
            Some(AttachmentPoint::RightHindFoot)
        );
        assert_eq!(attach_point_of("attach"), None);
        assert_eq!(attach_point_of("attach-point-x"), None);
    }

    /// Restore Item's type→system-folder mapping follows the reference's
    /// `assetTypeToFolderType`: same-named folders for the typed assets, and
    /// `FolderType::None` (→ agent root) for the rest.
    #[test]
    fn restore_maps_asset_types_to_their_system_folders() {
        use super::default_folder_type;
        assert_eq!(default_folder_type(AssetType::Texture), FolderType::Texture);
        assert_eq!(
            default_folder_type(AssetType::Landmark),
            FolderType::Landmark
        );
        assert_eq!(
            default_folder_type(AssetType::Clothing),
            FolderType::Clothing
        );
        assert_eq!(
            default_folder_type(AssetType::Bodypart),
            FolderType::Bodypart
        );
        assert_eq!(
            default_folder_type(AssetType::ScriptText),
            FolderType::ScriptText
        );
        // No same-named system folder: the caller falls back to the root.
        assert_eq!(default_folder_type(AssetType::ImageJpeg), FolderType::None);
        assert_eq!(default_folder_type(AssetType::Other(24)), FolderType::None);
    }

    /// A Library item exposes no mutation capabilities, but stays copyable —
    /// the read-only rule the task pins.
    #[test]
    fn library_item_is_read_only_but_copyable() {
        let info = item(1, InventoryType::Notecard, AssetType::Notecard, 0);
        let held = item_conditions(
            &info,
            ItemMenuFacts {
                in_library: true,
                clipboard_has_entry: true,
                ..ItemMenuFacts::default()
            },
        );
        assert!(!held.contains(&CAN_RENAME));
        assert!(!held.contains(&CAN_CUT));
        assert!(!held.contains(&CAN_DELETE));
        assert!(!held.contains(&CAN_PASTE));
        assert!(!held.contains(&CAN_PASTE_LINK));
        assert!(held.contains(&CAN_COPY), "library items must stay copyable");
    }

    /// An own no-copy item may be renamed / cut / deleted but not copied; with
    /// an empty clipboard nothing is pasteable.
    #[test]
    fn own_item_capabilities_follow_permissions_and_clipboard() {
        let info = item(1, InventoryType::Notecard, AssetType::Notecard, 0);
        let held = item_conditions(&info, ItemMenuFacts::default());
        assert!(held.contains(&CAN_RENAME));
        assert!(held.contains(&CAN_CUT));
        assert!(held.contains(&CAN_DELETE));
        assert!(!held.contains(&CAN_COPY), "no copy permission, no Copy");
        assert!(!held.contains(&CAN_PASTE), "empty clipboard, no Paste");

        let copyable = item(
            2,
            InventoryType::Notecard,
            AssetType::Notecard,
            Permissions::COPY.bits(),
        );
        let held = item_conditions(
            &copyable,
            ItemMenuFacts {
                clipboard_has_entry: true,
                ..ItemMenuFacts::default()
            },
        );
        assert!(held.contains(&CAN_COPY));
        assert!(held.contains(&CAN_PASTE));
    }

    /// The trash / worn / type flags land as visibility conditions.
    #[test]
    fn visibility_conditions_follow_type_and_state() {
        let landmark = item(1, InventoryType::Landmark, AssetType::Landmark, 0);
        let held = item_conditions(&landmark, ItemMenuFacts::default());
        assert!(held.contains(&IS_LANDMARK));
        assert!(held.contains(&NOT_IN_TRASH));
        assert!(held.contains(&NOT_WORN));

        let worn_object = item(2, InventoryType::Object, AssetType::Object, 0);
        let held = item_conditions(
            &worn_object,
            ItemMenuFacts {
                in_trash: true,
                worn: true,
                ..ItemMenuFacts::default()
            },
        );
        assert!(held.contains(&IS_OBJECT));
        assert!(held.contains(&IN_TRASH));
        assert!(held.contains(&WORN));
        assert!(
            !held.contains(&CAN_DELETE),
            "a trashed row offers Purge, not Delete"
        );

        let shirt = item(3, InventoryType::Wearable, AssetType::Clothing, 0);
        let held = item_conditions(&shirt, ItemMenuFacts::default());
        assert!(held.contains(&IS_WEARABLE));
        assert!(held.contains(&IS_CLOTHING));

        let shape = item(4, InventoryType::Wearable, AssetType::Bodypart, 0);
        let held = item_conditions(&shape, ItemMenuFacts::default());
        assert!(held.contains(&IS_WEARABLE));
        assert!(
            !held.contains(&IS_CLOTHING),
            "a body part cannot Add / Take Off"
        );

        let gesture = item(5, InventoryType::Gesture, AssetType::Gesture, 0);
        let held = item_conditions(
            &gesture,
            ItemMenuFacts {
                gesture_active: true,
                ..ItemMenuFacts::default()
            },
        );
        assert!(held.contains(&GESTURE_ACTIVE));
        let held = item_conditions(&gesture, ItemMenuFacts::default());
        assert!(held.contains(&GESTURE_INACTIVE));
    }

    /// System folders refuse rename / cut / delete; user folders allow them;
    /// the Trash advertises Empty Trash.
    #[test]
    fn folder_capabilities_protect_system_folders() {
        let trash = folder(1, FolderType::Trash);
        let held = folder_conditions(&trash, FolderMenuFacts::default());
        assert!(held.contains(&IS_TRASH_FOLDER));
        assert!(held.contains(&CAN_CREATE));
        assert!(!held.contains(&CAN_RENAME), "the Trash keeps its role");
        assert!(!held.contains(&CAN_DELETE));

        let user = folder(2, FolderType::None);
        let held = folder_conditions(&user, FolderMenuFacts::default());
        assert!(held.contains(&CAN_RENAME));
        assert!(held.contains(&CAN_CUT));
        assert!(held.contains(&CAN_DELETE));

        let library = folder(3, FolderType::None);
        let held = folder_conditions(
            &library,
            FolderMenuFacts {
                in_library: true,
                clipboard_has_entry: true,
                ..FolderMenuFacts::default()
            },
        );
        assert!(!held.contains(&CAN_CREATE));
        assert!(!held.contains(&CAN_RENAME));
        assert!(!held.contains(&CAN_PASTE));
    }

    /// The outfit-folder entries follow the subtree facts: Add To Current
    /// Outfit needs something wearable (a wearable **or** an attachment) in
    /// the subtree, Remove needs something worn.
    #[test]
    fn outfit_entries_follow_the_subtree_facts() {
        let user = folder(1, FolderType::None);
        let empty = folder_conditions(&user, FolderMenuFacts::default());
        assert!(!empty.contains(&FOLDER_HAS_WEARABLES));
        assert!(!empty.contains(&FOLDER_HAS_WORN));

        let held = folder_conditions(
            &user,
            FolderMenuFacts {
                has_wearables: true,
                has_worn: true,
                ..FolderMenuFacts::default()
            },
        );
        assert!(held.contains(&FOLDER_HAS_WEARABLES));
        assert!(held.contains(&FOLDER_HAS_WORN));
    }

    /// Add To Current Outfit folds the body wearables into one wear-set (a
    /// body part replaces its slot, clothing stacks) and batches the
    /// attachments into one compound add.
    #[test]
    fn outfit_add_batches_wearables_and_attachments() {
        let worn_shape = Wearable {
            item_id: InventoryKey::from(Uuid::from_u128(0x60)),
            asset_id: None,
            wearable_type: WearableType::Shape,
        };
        let mut new_shape = item(0x61, InventoryType::Wearable, AssetType::Bodypart, 0);
        new_shape.flags = u32::from(WearableType::Shape.to_code());
        let mut shirt = item(0x62, InventoryType::Wearable, AssetType::Clothing, 0);
        shirt.flags = u32::from(WearableType::Shirt.to_code());
        let attachment = item(0x63, InventoryType::Object, AssetType::Object, 0);

        let (commands, tracked) = outfit_add_commands(
            &[new_shape.clone(), shirt.clone(), attachment.clone()],
            &[worn_shape],
            Some(AgentKey::from(Uuid::from_u128(1))),
        );
        // One SetWearing carrying the replaced shape and the added shirt.
        let set = commands.iter().find_map(|command| match command {
            Command::SetWearing(set) => Some(set.clone()),
            _other => None,
        });
        let set = set.unwrap_or_default();
        assert_eq!(set.len(), 2, "shape replaced, shirt added");
        assert!(set.iter().any(|worn| worn.item_id == new_shape.item_id));
        assert!(!set.iter().any(|worn| worn.item_id == worn_shape.item_id));
        // One compound attachment add, keeping what is worn.
        assert!(commands.iter().any(|command| matches!(
            command,
            Command::RezAttachments {
                detach: sl_client_bevy::DetachOrder::Keep,
                attachments,
                ..
            } if attachments.len() == 1
        )));
        assert_eq!(tracked, vec![attachment.item_id]);
    }

    /// Remove From Current Outfit strips worn clothing (never a body part) and
    /// detaches worn attachments; unworn items are untouched.
    #[test]
    fn outfit_remove_strips_worn_clothing_and_attachments() {
        let mut shirt = item(0x70, InventoryType::Wearable, AssetType::Clothing, 0);
        shirt.flags = u32::from(WearableType::Shirt.to_code());
        let mut shape = item(0x71, InventoryType::Wearable, AssetType::Bodypart, 0);
        shape.flags = u32::from(WearableType::Shape.to_code());
        let attachment = item(0x72, InventoryType::Object, AssetType::Object, 0);
        let unworn = item(0x73, InventoryType::Object, AssetType::Object, 0);

        let current = vec![
            Wearable {
                item_id: shirt.item_id,
                asset_id: None,
                wearable_type: WearableType::Shirt,
            },
            Wearable {
                item_id: shape.item_id,
                asset_id: None,
                wearable_type: WearableType::Shape,
            },
        ];
        let mut tracked = HashSet::new();
        tracked.insert(attachment.item_id);

        let (commands, untracked) = outfit_remove_commands(
            &[shirt.clone(), shape.clone(), attachment.clone(), unworn],
            &current,
            &[],
            &tracked,
        );
        let set = commands.iter().find_map(|command| match command {
            Command::SetWearing(set) => Some(set.clone()),
            _other => None,
        });
        let set = set.unwrap_or_default();
        assert!(
            !set.iter().any(|worn| worn.item_id == shirt.item_id),
            "the worn shirt comes off"
        );
        assert!(
            set.iter().any(|worn| worn.item_id == shape.item_id),
            "a body part is never removed"
        );
        assert!(commands.iter().any(|command| matches!(
            command,
            Command::DetachAttachmentIntoInventory { item_id } if *item_id == attachment.item_id
        )));
        assert_eq!(untracked, vec![attachment.item_id]);
    }

    /// Paste of a copy issues a `CopyInventoryItem`; paste of a cut issues a
    /// move and refreshes both folders.
    #[test]
    fn paste_maps_copy_to_copy_and_cut_to_move() {
        let source = item(
            1,
            InventoryType::Notecard,
            AssetType::Notecard,
            Permissions::COPY.bits(),
        );
        let dest = InventoryFolderKey::from(Uuid::from_u128(0xD0));

        let (commands, refresh) = paste_commands(
            ClipboardMode::Copy,
            &MenuTarget::Item(source.clone()),
            dest,
            Some(AgentKey::from(Uuid::from_u128(1))),
        );
        assert!(matches!(
            commands.first(),
            Some(Command::CopyInventoryItem { new_folder_id, .. }) if *new_folder_id == dest
        ));
        assert_eq!(refresh, Vec::new());

        let (commands, refresh) = paste_commands(
            ClipboardMode::Cut,
            &MenuTarget::Item(source.clone()),
            dest,
            None,
        );
        assert!(matches!(
            commands.first(),
            Some(Command::MoveInventoryItem { folder_id, .. }) if *folder_id == dest
        ));
        assert_eq!(refresh, vec![source.folder_id, dest]);
    }

    /// The wear-set arithmetic: Wear replaces the slot, Add keeps it, Take Off
    /// removes exactly the item.
    #[test]
    fn wear_sets_replace_add_and_remove() {
        let shirt_a = Wearable {
            item_id: InventoryKey::from(Uuid::from_u128(0x10)),
            asset_id: None,
            wearable_type: WearableType::Shirt,
        };
        let pants = Wearable {
            item_id: InventoryKey::from(Uuid::from_u128(0x11)),
            asset_id: None,
            wearable_type: WearableType::Pants,
        };
        let current = vec![shirt_a, pants];
        // A shirt item: flags low byte 4 = Shirt.
        let mut shirt_b = item(0x20, InventoryType::Wearable, AssetType::Clothing, 0);
        shirt_b.flags = u32::from(WearableType::Shirt.to_code());

        let replaced = wear_set(&current, &shirt_b, false);
        assert_eq!(replaced.len(), 2, "Wear replaces the same slot");
        assert!(replaced.iter().any(|worn| worn.item_id == shirt_b.item_id));
        assert!(!replaced.iter().any(|worn| worn.item_id == shirt_a.item_id));

        let added = wear_set(&current, &shirt_b, true);
        assert_eq!(added.len(), 3, "Add keeps the existing layer");

        let stripped = take_off_set(&replaced, shirt_b.item_id);
        assert!(!stripped.iter().any(|worn| worn.item_id == shirt_b.item_id));
        assert_eq!(stripped.len(), 1);
    }

    /// Worn detection sees the legacy set, the COF links, and the tracked
    /// attachments.
    #[test]
    fn worn_detection_reads_all_three_sources() {
        let object = item(0x30, InventoryType::Object, AssetType::Object, 0);
        let none: HashSet<InventoryKey> = HashSet::new();
        assert!(!is_worn(&object, &[], &[], &none));

        // Tracked by our own wear command.
        let mut tracked = HashSet::new();
        tracked.insert(object.item_id);
        assert!(is_worn(&object, &[], &[], &tracked));

        // A COF link whose asset id names the item.
        let mut link = item(0x31, InventoryType::Object, AssetType::Object, 0);
        link.asset_id = object.item_id.uuid();
        assert!(is_worn(&object, &[], &[link], &none));

        // The legacy wearables set.
        let worn = Wearable {
            item_id: object.item_id,
            asset_id: None,
            wearable_type: WearableType::Shape,
        };
        assert!(is_worn(&object, &[worn], &[], &none));
    }
}
