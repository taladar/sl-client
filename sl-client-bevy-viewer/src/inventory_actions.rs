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
    RezAttachment, ScriptLanguage, SlCommand, SlEvent, SlIdentity, SlSessionEvent, TransactionId,
    Uuid, VisualParams, Wearable, WearableType,
};
use std::collections::{HashSet, VecDeque};

use crate::avatar_menu::UNIMPLEMENTED;
use crate::conversations::{ConversationKey, OpenConversation};
use crate::inventory::{
    InventoryModel, InventorySelection, InventoryView, RowKey, query_folder_page,
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

/// The target can be shared (given to another avatar via the picker): own
/// inventory, and for an item one the owner may transfer.
pub(crate) const CAN_SHARE: &str = "can-share";

/// The target item's type has an Open preview in this viewer.
pub(crate) const CAN_OPEN: &str = "can-open";

/// The target item's asset id may be copied (full owner permissions — the
/// reference's `canCopyAssetID` gate, sans the creator-override).
pub(crate) const CAN_COPY_UUID: &str = "can-copy-uuid";

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
        MenuItemDef::Command(MenuCommand::new("New Shirt", "new-shirt")),
        MenuItemDef::Command(MenuCommand::new("New Pants", "new-pants")),
        MenuItemDef::Command(MenuCommand::new("New Shoes", "new-shoes")),
        MenuItemDef::Command(MenuCommand::new("New Socks", "new-socks")),
        MenuItemDef::Command(MenuCommand::new("New Jacket", "new-jacket")),
        MenuItemDef::Command(MenuCommand::new("New Skirt", "new-skirt")),
        MenuItemDef::Command(MenuCommand::new("New Gloves", "new-gloves")),
        MenuItemDef::Command(MenuCommand::new("New Undershirt", "new-undershirt")),
        MenuItemDef::Command(MenuCommand::new("New Underpants", "new-underpants")),
        MenuItemDef::Command(MenuCommand::new("New Alpha Mask", "new-alpha")),
        MenuItemDef::Command(MenuCommand::new("New Tattoo", "new-tattoo")),
        MenuItemDef::Command(MenuCommand::new("New Universal", "new-universal")),
        MenuItemDef::Command(MenuCommand::new("New Physics", "new-physics")),
    ],
};

/// The "New Body Parts >" submenu — placeholders, as [`NEW_CLOTHES_MENU`].
static NEW_BODY_PARTS_MENU: MenuDef = MenuDef {
    label: "New Body Parts",
    items: &[
        MenuItemDef::Command(MenuCommand::new("New Shape", "new-shape")),
        MenuItemDef::Command(MenuCommand::new("New Skin", "new-skin")),
        MenuItemDef::Command(MenuCommand::new("New Hair", "new-hair")),
        MenuItemDef::Command(MenuCommand::new("New Eyes", "new-eyes")),
    ],
};

/// The `element` the inventory **+ (create)** menu attributes its picks to.
pub(crate) const INVENTORY_ADD_ELEMENT: &str = "inventory-add";

/// The + menu's Upload submenu — every uploader is a future task
/// (`viewer-image-upload`, `viewer-mesh-*`), kept greyed in reference order.
static UPLOAD_MENU: MenuDef = MenuDef {
    label: "Upload",
    items: &[
        MenuItemDef::Command(
            MenuCommand::new("Image...", "upload-image")
                .accel("Ctrl+U")
                .enabled_when(UNIMPLEMENTED),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Sound...", "upload-sound").enabled_when(UNIMPLEMENTED),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Animation...", "upload-animation").enabled_when(UNIMPLEMENTED),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Model...", "upload-model").enabled_when(UNIMPLEMENTED),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Material...", "upload-material").enabled_when(UNIMPLEMENTED),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Bulk...", "upload-bulk").enabled_when(UNIMPLEMENTED),
        ),
    ],
};

/// The + menu's New Settings submenu — environment-settings creation is a
/// future task, kept greyed in reference order.
static NEW_SETTINGS_MENU: MenuDef = MenuDef {
    label: "New Settings",
    items: &[
        MenuItemDef::Command(MenuCommand::new("New Sky", "new-sky").enabled_when(UNIMPLEMENTED)),
        MenuItemDef::Command(
            MenuCommand::new("New Water", "new-water").enabled_when(UNIMPLEMENTED),
        ),
        MenuItemDef::Command(
            MenuCommand::new("New Day Cycle", "new-daycycle").enabled_when(UNIMPLEMENTED),
        ),
    ],
};

/// The inventory window's **+ (create)** menu — the reference's
/// `menu_inventory_add.xml` in its order: the Upload submenu, the New …
/// creators, the wearable submenus, then Shop. Creation targets the selected
/// folder (or the root); [`handle_inventory_add_actions`] routes the picks.
pub(crate) static INVENTORY_ADD_MENU: MenuDef = MenuDef {
    label: "+",
    items: &[
        MenuItemDef::Submenu(&UPLOAD_MENU),
        MenuItemDef::Separator,
        MenuItemDef::Command(MenuCommand::new("New Folder", "new-folder")),
        MenuItemDef::Command(MenuCommand::new("New Script", "new-script")),
        MenuItemDef::Command(MenuCommand::new("New Notecard", "new-notecard")),
        MenuItemDef::Command(MenuCommand::new("New Gesture", "new-gesture")),
        MenuItemDef::Command(
            MenuCommand::new("New Material", "new-material").enabled_when(UNIMPLEMENTED),
        ),
        MenuItemDef::Submenu(&NEW_CLOTHES_MENU),
        MenuItemDef::Submenu(&NEW_BODY_PARTS_MENU),
        MenuItemDef::Submenu(&NEW_SETTINGS_MENU),
        MenuItemDef::Separator,
        MenuItemDef::Command(MenuCommand::new("Shop...", "shop").enabled_when(UNIMPLEMENTED)),
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
        MenuItemDef::Command(MenuCommand::new("Share", "share").enabled_when(CAN_SHARE)),
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
                .enabled_when(FOLDER_HAS_WEARABLES),
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
        MenuItemDef::Command(MenuCommand::new("Copy", "copy").enabled_when(CAN_COPY)),
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
        MenuItemDef::Command(MenuCommand::new("Share", "share").enabled_when(CAN_SHARE)),
        MenuItemDef::Command(MenuCommand::new("Open", "open").enabled_when(CAN_OPEN)),
        MenuItemDef::Command(MenuCommand::new("Properties", "properties")),
        MenuItemDef::Command(MenuCommand::new("Rename", "rename").enabled_when(CAN_RENAME)),
        MenuItemDef::Command(
            MenuCommand::new("Copy Asset UUID", "copy-asset-uuid").enabled_when(CAN_COPY_UUID),
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

/// The rows the currently-open inventory context menu acts on — one snapshot
/// per selected row (view order), a single entry for a plain right-click.
/// Set on every open; a stale value between opens is harmless because the
/// menu's element is only emitted while a menu is open.
#[derive(Resource, Debug, Default)]
pub(crate) struct InventoryMenuTarget {
    /// The snapshotted rows, empty before any menu has opened.
    pub(crate) targets: Vec<MenuTarget>,
}

/// Whether a clipboard entry pastes as a copy or a move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClipboardMode {
    /// Copy on paste (the source stays).
    Copy,
    /// Move on paste (the source goes).
    Cut,
}

/// The inventory clipboard: the copied / cut rows of one Copy / Cut action
/// (the whole selection). Paste consumes a Cut entry; a Copy entry can be
/// pasted repeatedly, matching the reference.
#[derive(Resource, Debug, Default)]
pub(crate) struct InventoryClipboard {
    /// The held entries, or `None` when the clipboard is empty.
    pub(crate) entry: Option<(ClipboardMode, Vec<MenuTarget>)>,
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
    if mutable && item.permissions.owner.contains(Permissions::TRANSFER) {
        held.push(CAN_SHARE);
    }
    if crate::inventory_properties::previewable(item.inv_type) {
        held.push(CAN_OPEN);
    }
    if item
        .permissions
        .owner
        .contains(Permissions::MODIFY | Permissions::COPY | Permissions::TRANSFER)
    {
        held.push(CAN_COPY_UUID);
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
    // A plain user folder (or an outfit) deep-copies — Library ones included,
    // which is what lets a Library folder be copied out.
    if matches!(folder.folder_type, FolderType::None | FolderType::Outfit) {
        held.push(CAN_COPY);
    }
    held.push(if facts.in_trash {
        IN_TRASH
    } else {
        NOT_IN_TRASH
    });
    let mutable = !facts.in_library;
    if mutable {
        held.push(CAN_CREATE);
        held.push(CAN_SHARE);
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

/// The commands that **replace** the current outfit with a folder's contents
/// (the reference's `wearInventoryCategory` with `append == false`): body
/// parts are kept unless the folder supplies a replacement of that slot,
/// clothing is replaced wholesale by the folder's layers, worn attachments
/// not in the folder are detached and the folder's objects are added
/// alongside. Returns the commands, the attachment ids now worn, and the
/// item ids no longer worn (clothing and attachments).
pub(crate) fn outfit_replace_commands(
    items: &[ItemInfo],
    current: &[Wearable],
    cof_items: &[ItemInfo],
    tracked_attachments: &HashSet<InventoryKey>,
    own_agent: Option<AgentKey>,
) -> (Vec<Command>, Vec<InventoryKey>, Vec<InventoryKey>) {
    let mut commands = Vec::new();
    let mut no_longer_worn = Vec::new();

    // The new legacy wear-set: the folder's wearables, with each body-part
    // slot falling back to what is currently worn there.
    let folder_wearables: Vec<(&ItemInfo, WearableType)> = items
        .iter()
        .filter(|item| item.inv_type == InventoryType::Wearable)
        .map(|item| (item, wearable_type_of(item)))
        .collect();
    let mut set: Vec<Wearable> = Vec::new();
    for (item, slot) in &folder_wearables {
        if set.iter().any(|worn| worn.item_id == item.item_id) {
            continue;
        }
        if slot.is_body_part() && set.iter().any(|worn| worn.wearable_type == *slot) {
            continue;
        }
        set.push(Wearable {
            item_id: item.item_id,
            asset_id: None,
            wearable_type: *slot,
        });
    }
    for worn in current {
        if worn.wearable_type.is_body_part()
            && !set
                .iter()
                .any(|new| new.wearable_type == worn.wearable_type)
        {
            // A body part the folder does not replace stays on — the avatar
            // is never left without one.
            set.push(*worn);
        } else if !set.iter().any(|new| new.item_id == worn.item_id) {
            // Everything else (a clothing layer not re-worn) comes off.
            no_longer_worn.push(worn.item_id);
        }
    }
    commands.push(Command::SetWearing(set));
    commands.push(Command::RequestWearables);

    // Attachments: the currently worn set is the viewer-tracked ids plus the
    // COF's object links.
    let mut worn_attachments: HashSet<InventoryKey> = tracked_attachments.clone();
    for link in cof_items {
        if matches!(
            link.inv_type,
            InventoryType::Object | InventoryType::Attachment
        ) {
            worn_attachments.insert(InventoryKey::from(link.asset_id));
        }
    }
    let folder_attachments: Vec<&ItemInfo> = items
        .iter()
        .filter(|item| {
            matches!(
                item.inv_type,
                InventoryType::Object | InventoryType::Attachment
            )
        })
        .collect();
    let keep: HashSet<InventoryKey> = folder_attachments.iter().map(|item| item.item_id).collect();
    for worn in &worn_attachments {
        if !keep.contains(worn) {
            commands.push(Command::DetachAttachmentIntoInventory { item_id: *worn });
            no_longer_worn.push(*worn);
        }
    }
    let mut now_worn = Vec::new();
    let to_attach: Vec<RezAttachment> = folder_attachments
        .iter()
        .filter(|item| !worn_attachments.contains(&item.item_id))
        .map(|item| {
            now_worn.push(item.item_id);
            RezAttachment {
                item_id: item.item_id,
                owner_id: own_agent.map_or_else(Uuid::nil, |agent| agent.uuid()),
                attachment_point: AttachmentPoint::Default,
                mode: AttachmentMode::Add,
                name: item.name.clone(),
                description: item.description.clone(),
            }
        })
        .collect();
    if !to_attach.is_empty() {
        commands.push(Command::RezAttachments {
            compound_id: TransactionId::from(Uuid::new_v4()),
            detach: DetachOrder::Keep,
            attachments: to_attach,
        });
    }
    (commands, now_worn, no_longer_worn)
}

/// The item ids a legacy Wear of `item` replaces (the same slot's current
/// wearables) — the links the COF must drop when the wear commits.
pub(crate) fn replaced_by_wear(current: &[Wearable], item: &ItemInfo) -> Vec<InventoryKey> {
    if item.inv_type != InventoryType::Wearable {
        return Vec::new();
    }
    let slot = wearable_type_of(item);
    current
        .iter()
        .filter(|worn| worn.wearable_type == slot && worn.item_id != item.item_id)
        .map(|worn| worn.item_id)
        .collect()
}

/// The reference's clothing layer-ordering token for a COF link description:
/// `"@" + (wearable_type * 100 + layer_index)` (`build_order_string`). The
/// same-type sort every COF reader — the SL bake service included — applies
/// to stack clothing layers.
pub(crate) fn cof_order_description(slot: WearableType, index: u32) -> String {
    let token = u32::from(slot.to_code())
        .saturating_mul(100)
        .saturating_add(index);
    format!("@{token}")
}

/// Parse a COF link description's ordering token back into its
/// `(wearable-type code, layer index)`. `None` for a token-less description.
pub(crate) fn parse_order_token(description: &str) -> Option<(u8, u32)> {
    let token: u32 = description.trim().strip_prefix('@')?.parse().ok()?;
    let code = u8::try_from(token / 100).ok()?;
    Some((code, token % 100))
}

/// A COF link paired with the **clothing slot** it orders under: the resolved
/// target's slot, or `None` for body parts, attachments and gestures (which
/// the reference does not order by description). Built by
/// [`cof_links_with_slots`]; a plain-data pair so the ordering arithmetic
/// stays pure and testable.
pub(crate) type CofLink = (ItemInfo, Option<WearableType>);

/// The COF's links, each resolved to its clothing slot: through the model
/// when the link's target is loaded, else through the link's own ordering
/// token; `None` when neither names a clothing layer.
pub(crate) fn cof_links_with_slots(model: &InventoryModel) -> Vec<CofLink> {
    model
        .cof_items()
        .iter()
        .map(|link| {
            let resolved = model
                .find_item(InventoryKey::from(link.asset_id))
                .filter(|target| {
                    target.inv_type == InventoryType::Wearable
                        && target.asset_type == AssetType::Clothing
                })
                .map(wearable_type_of);
            let from_token = parse_order_token(&link.description)
                .map(|(code, _index)| WearableType::from_code(code))
                .filter(|slot| !slot.is_body_part());
            (link.clone(), resolved.or(from_token))
        })
        .collect()
}

/// The dense renumbering updates for the given clothing `slots` over the
/// surviving links: each slot's links sorted by their current token
/// (token-less last, otherwise stable), renumbered `0..`, and every link
/// whose token changed re-written via `UpdateInventoryItem` — the
/// reference's `getWearableOrderingDescUpdates`.
fn renumber_slot_updates(surviving: &[CofLink], slot_codes: &HashSet<u8>) -> Vec<Command> {
    let mut out = Vec::new();
    for &code in slot_codes {
        let slot = WearableType::from_code(code);
        let mut layer_links: Vec<&ItemInfo> = surviving
            .iter()
            .filter(|(_link, link_slot)| *link_slot == Some(slot))
            .map(|(link, _slot)| link)
            .collect();
        layer_links.sort_by_key(|link| {
            parse_order_token(&link.description).map_or(u32::MAX, |(_code, index)| index)
        });
        for (index, link) in layer_links.iter().enumerate() {
            let wanted = cof_order_description(slot, u32::try_from(index).unwrap_or(u32::MAX));
            if link.description != wanted {
                let mut updated = (*link).clone();
                updated.description = wanted;
                out.push(Command::UpdateInventoryItem {
                    item: Box::new(crate::inventory_properties::to_wire_item(&updated)),
                    transaction_id: TransactionId::from(Uuid::nil()),
                });
            }
        }
    }
    out
}

/// The COF-link commands after wearing `item`: drop the `replaced` items'
/// links, renumber the affected clothing slots dense, then link the item
/// into the COF with the slot's next ordering token (skipped when already
/// linked) — the reference's `addCOFItemLink` + `build_order_string`.
/// No-ops on a grid without a located COF.
pub(crate) fn cof_wear_link_commands(
    cof: Option<InventoryFolderKey>,
    cof_links: &[CofLink],
    item: &ItemInfo,
    replaced: &[InventoryKey],
) -> Vec<Command> {
    let Some(cof) = cof else {
        return Vec::new();
    };
    let mut out = cof_remove_link_commands(cof_links, replaced);
    let already_linked = cof_links
        .iter()
        .any(|(link, _slot)| link.asset_id == item.item_id.uuid());
    if !already_linked {
        // A clothing layer's link carries the slot's next dense ordering
        // token (counted over the links that survive the removals above).
        let description =
            if item.inv_type == InventoryType::Wearable && item.asset_type == AssetType::Clothing {
                let slot = wearable_type_of(item);
                let surviving = cof_links
                    .iter()
                    .filter(|(link, link_slot)| {
                        *link_slot == Some(slot)
                            && !replaced.iter().any(|target| link.asset_id == target.uuid())
                    })
                    .count();
                cof_order_description(slot, u32::try_from(surviving).unwrap_or(u32::MAX))
            } else {
                String::new()
            };
        out.push(Command::LinkInventoryItem(NewInventoryLink {
            folder_id: cof,
            linked_id: InventoryItemOrFolderKey::Item(item.item_id),
            // `AT_LINK` (24): an item link.
            link_type: AssetType::Other(24),
            inv_type: item.inv_type,
            name: item.name.clone(),
            description,
        }));
    }
    out
}

/// The COF-link removals for taken-off / detached items: one batch
/// `RemoveInventoryItems` of every COF link whose target is in `removed` —
/// the reference's `removeCOFItemLinks` — followed by a dense renumbering of
/// each affected clothing slot's surviving links.
pub(crate) fn cof_remove_link_commands(
    cof_links: &[CofLink],
    removed: &[InventoryKey],
) -> Vec<Command> {
    let is_removed = |link: &ItemInfo| removed.iter().any(|target| link.asset_id == target.uuid());
    let links: Vec<InventoryKey> = cof_links
        .iter()
        .filter(|(link, _slot)| is_removed(link))
        .map(|(link, _slot)| link.item_id)
        .collect();
    if links.is_empty() {
        return Vec::new();
    }
    let affected: HashSet<u8> = cof_links
        .iter()
        .filter(|(link, _slot)| is_removed(link))
        .filter_map(|(_link, slot)| slot.map(WearableType::to_code))
        .collect();
    let surviving: Vec<CofLink> = cof_links
        .iter()
        .filter(|(link, _slot)| !is_removed(link))
        .cloned()
        .collect();
    let mut out = vec![Command::RemoveInventoryItems(links)];
    out.extend(renumber_slot_updates(&surviving, &affected));
    out
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

/// The conditions common to **every** selected row: the intersection, in
/// the first row's order — the reference's multi-select gating (Delete only
/// when everything is deletable, the type blocks only on a uniform
/// selection). A multi-selection additionally withholds the single-row
/// affordances (rename).
pub(crate) fn intersect_conditions(sets: &[Vec<&'static str>]) -> Vec<&'static str> {
    let Some(first) = sets.first() else {
        return Vec::new();
    };
    let multi = sets.len() > 1;
    first
        .iter()
        .copied()
        .filter(|condition| sets.iter().all(|set| set.contains(condition)))
        .filter(|condition| !(multi && *condition == CAN_RENAME))
        .collect()
}

/// Resolve one row key to its snapshot and condition set. `None` when the
/// key cannot be resolved in the model (e.g. a Recent entry whose folder is
/// not loaded).
fn resolve_row_target(
    key: RowKey,
    model: &InventoryModel,
    clipboard: &InventoryClipboard,
    worn: &WornAttachments,
    gestures: &ActiveGestures,
) -> Option<(MenuTarget, Vec<&'static str>)> {
    let trash = model.folder_by_type(FolderType::Trash);
    let in_trash = |folder: InventoryFolderKey| {
        trash.is_some_and(|trash_key| model.is_within(folder, trash_key))
    };
    let clipboard_has_entry = clipboard.entry.is_some();
    match key {
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
            let conditions = folder_conditions(&info, facts);
            Some((MenuTarget::Folder(info), conditions))
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
            let conditions = item_conditions(&info, facts);
            Some((MenuTarget::Item(info), conditions))
        }
    }
}

/// Resolve a set of row keys (the clicked row, or the whole selection) to
/// snapshots + intersected conditions and open the matching context menu —
/// shared by the tree rows and the gallery tiles. An all-folder selection
/// opens the folder menu; anything else the item menu. Returns `None` when
/// nothing resolved.
#[expect(
    clippy::too_many_arguments,
    reason = "the resolution reads every fact source the conditions draw on: the model, the \
              clipboard, the tracked worn / gesture sets, and the two output channels"
)]
pub(crate) fn open_inventory_context_menu(
    keys: &[RowKey],
    at: Vec2,
    model: &InventoryModel,
    clipboard: &InventoryClipboard,
    worn: &WornAttachments,
    gestures: &ActiveGestures,
    target: &mut InventoryMenuTarget,
    menus: &mut MessageWriter<OpenContextMenu>,
) -> Option<()> {
    let mut snapshots = Vec::new();
    let mut condition_sets = Vec::new();
    for &key in keys {
        if let Some((snapshot, conditions)) =
            resolve_row_target(key, model, clipboard, worn, gestures)
        {
            snapshots.push(snapshot);
            condition_sets.push(conditions);
        }
    }
    if snapshots.is_empty() {
        return None;
    }
    let all_folders = snapshots
        .iter()
        .all(|snapshot| matches!(snapshot, MenuTarget::Folder(_)));
    let menu = if all_folders {
        &INVENTORY_FOLDER_MENU
    } else {
        &INVENTORY_ITEM_MENU
    };
    let conditions = intersect_conditions(&condition_sets);
    target.targets = snapshots;
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
    // The usual list semantics: a right-click on an unselected row selects it;
    // a right-click **inside** the selection keeps it, and the menu acts on
    // the whole selection.
    if !selection.contains(display.key()) {
        selection.select_single(display.key(), index);
    }
    let keys = if selection.count() > 1 {
        selection.keys_in_view_order(view.rows())
    } else {
        vec![display.key()]
    };
    let _opened = open_inventory_context_menu(
        &keys,
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
    target.targets = vec![MenuTarget::Folder(info)];
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

/// Write a COF-link batch and refresh the COF page (a no-op for an empty
/// batch or a grid without a located COF).
fn write_cof_commands(
    model: &InventoryModel,
    batch: Vec<Command>,
    commands: &mut MessageWriter<SlCommand>,
) {
    if batch.is_empty() {
        return;
    }
    for command in batch {
        commands.write(SlCommand(command));
    }
    if let Some(cof) = model.cof_key() {
        query_folder_page(cof, commands);
    }
}

/// Handle a picked inventory context-menu entry.
#[expect(
    clippy::too_many_arguments,
    clippy::type_complexity,
    reason = "a Bevy system's parameters are its injected resources — grouped into tuples by \
              role (the mutated stashes, the message outputs) to fit the SystemParam arity"
)]
fn handle_inventory_menu_actions(
    mut actions: MessageReader<UiAction>,
    target: Res<InventoryMenuTarget>,
    model: Res<InventoryModel>,
    identity: Res<SlIdentity>,
    stashes: (
        ResMut<InventoryClipboard>,
        ResMut<WornAttachments>,
        ResMut<ActiveGestures>,
        ResMut<crate::inventory::InlineRename>,
        ResMut<PendingShare>,
        ResMut<PendingWearableUploads>,
    ),
    library: Option<Res<crate::avatar_assets::AvatarAssetLibrary>>,
    mut system_clipboard: Option<ResMut<bevy::clipboard::Clipboard>>,
    outputs: (
        MessageWriter<crate::inventory::InventoryUiAction>,
        MessageWriter<OpenConversation>,
        MessageWriter<crate::avatar_picker::OpenAvatarPicker>,
        MessageWriter<crate::inventory_properties::OpenItemPreview>,
        MessageWriter<crate::inventory_properties::OpenItemProperties>,
        MessageWriter<SlCommand>,
    ),
) {
    let (
        mut clipboard,
        mut worn,
        mut gestures,
        mut rename,
        mut pending_share,
        mut pending_wearables,
    ) = stashes;
    let (
        mut ui_actions,
        mut conversations,
        mut picker_opens,
        mut previews,
        mut properties,
        mut commands,
    ) = outputs;
    for action in actions.read() {
        if action.element != INVENTORY_MENU_ELEMENT {
            continue;
        }
        let targets = target.targets.clone();
        let Some(menu_target) = targets.first().cloned() else {
            continue;
        };
        let dest = destination_folder(&menu_target);
        match action.action {
            "share" => {
                pending_share.targets.clone_from(&targets);
                picker_opens.write(crate::avatar_picker::OpenAvatarPicker {
                    requester: SHARE_REQUESTER,
                });
            }
            "open" => {
                for target_row in &targets {
                    if let MenuTarget::Item(item) = target_row {
                        previews.write(crate::inventory_properties::OpenItemPreview {
                            item: item.clone(),
                        });
                    }
                }
            }
            "properties" => {
                if let MenuTarget::Item(item) = &menu_target {
                    properties.write(crate::inventory_properties::OpenItemProperties {
                        item: item.clone(),
                    });
                }
            }
            "copy-asset-uuid" => {
                if let MenuTarget::Item(item) = &menu_target
                    && let Some(clipboard) = system_clipboard.as_deref_mut()
                {
                    // A failed clipboard write (headless run) is dropped.
                    let _set = clipboard.set_text(item.asset_id.to_string());
                }
            }
            "rename" => {
                // The tree edits the label in place ([`crate::inventory`]'s
                // inline rename).
                rename.pending = Some(match &menu_target {
                    MenuTarget::Item(item) => RowKey::Item(item.item_id),
                    MenuTarget::Folder(folder) => RowKey::Folder(folder.folder_id),
                });
            }
            "cut" => {
                clipboard.entry = Some((ClipboardMode::Cut, targets.clone()));
            }
            "copy" => {
                clipboard.entry = Some((ClipboardMode::Copy, targets.clone()));
                // A copied folder prefetches its subtree, so the deep copy a
                // later paste plans over sees the contents (the session's
                // fetcher dedupes and serves repeats from its cache).
                for target_row in &targets {
                    if let MenuTarget::Folder(folder) = target_row {
                        for sub in model.subtree_folders(folder.folder_id) {
                            query_folder_page(sub, &mut commands);
                        }
                    }
                }
            }
            "paste" => {
                if let Some((mode, entries)) = clipboard.entry.clone() {
                    for entry in &entries {
                        if let (ClipboardMode::Copy, MenuTarget::Folder(folder)) = (mode, entry) {
                            // A copied folder pastes as a recursive deep copy.
                            for command in deep_copy_commands(
                                &model,
                                folder.folder_id,
                                &folder.name,
                                dest,
                                identity.agent_id,
                            ) {
                                commands.write(SlCommand(command));
                            }
                            commands.write(SlCommand(Command::QueryInventoryFolders));
                            query_folder_page(dest, &mut commands);
                        } else {
                            let (paste, refresh) =
                                paste_commands(mode, entry, dest, identity.agent_id);
                            for command in paste {
                                commands.write(SlCommand(command));
                            }
                            for folder in refresh {
                                query_folder_page(folder, &mut commands);
                            }
                        }
                    }
                    if mode == ClipboardMode::Cut {
                        clipboard.entry = None;
                    }
                }
            }
            "paste-link" => {
                if let Some((_mode, entries)) = clipboard.entry.clone() {
                    for entry in &entries {
                        let link = match entry {
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
            }
            "delete" => {
                if let Some(trash) = model.folder_by_type(FolderType::Trash) {
                    for target_row in &targets {
                        match target_row {
                            MenuTarget::Item(item) => {
                                commands.write(SlCommand(Command::MoveInventoryItem {
                                    item_id: item.item_id,
                                    folder_id: trash,
                                    new_name: String::new(),
                                }));
                                query_folder_page(item.folder_id, &mut commands);
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
                    query_folder_page(trash, &mut commands);
                }
            }
            "purge" => {
                // One batch removal for the whole selection.
                let mut item_ids = Vec::new();
                let mut folder_ids = Vec::new();
                let mut refresh = Vec::new();
                for target_row in &targets {
                    match target_row {
                        MenuTarget::Item(item) => {
                            item_ids.push(item.item_id);
                            refresh.push(item.folder_id);
                        }
                        MenuTarget::Folder(folder) => folder_ids.push(folder.folder_id),
                    }
                }
                if !item_ids.is_empty() && folder_ids.is_empty() {
                    commands.write(SlCommand(Command::RemoveInventoryItems(item_ids)));
                } else if item_ids.is_empty() && !folder_ids.is_empty() {
                    commands.write(SlCommand(Command::RemoveInventoryFolders(folder_ids)));
                    commands.write(SlCommand(Command::QueryInventoryFolders));
                } else if !item_ids.is_empty() || !folder_ids.is_empty() {
                    commands.write(SlCommand(Command::RemoveInventoryObjects {
                        folder_ids,
                        item_ids,
                    }));
                    commands.write(SlCommand(Command::QueryInventoryFolders));
                }
                for folder in refresh {
                    query_folder_page(folder, &mut commands);
                }
            }
            "restore" => {
                // The wire does not record where a trashed row came from; the
                // reference restores to the type's system folder
                // (`LLItemBridge::restoreItem` via `findCategoryUUIDForType`),
                // falling back to the agent root.
                for target_row in &targets {
                    match target_row {
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
            "new-folder" | "new-script" | "new-notecard" | "new-gesture" | "new-shirt"
            | "new-pants" | "new-shoes" | "new-socks" | "new-jacket" | "new-skirt"
            | "new-gloves" | "new-undershirt" | "new-underpants" | "new-alpha" | "new-tattoo"
            | "new-universal" | "new-physics" | "new-shape" | "new-skin" | "new-hair"
            | "new-eyes" => {
                dispatch_create(
                    action.action,
                    dest,
                    identity.agent_id,
                    library.as_ref().map(|library| library.params()),
                    &mut pending_wearables,
                    &mut commands,
                    &mut ui_actions,
                    &mut rename,
                );
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
                let batch: Vec<GestureActivation> = targets
                    .iter()
                    .filter_map(|target_row| match target_row {
                        MenuTarget::Item(item) => Some(GestureActivation {
                            item_id: item.item_id,
                            asset_id: item.asset_id,
                        }),
                        MenuTarget::Folder(_folder) => None,
                    })
                    .collect();
                if !batch.is_empty() {
                    for activation in &batch {
                        gestures.items.insert(activation.item_id);
                    }
                    commands.write(SlCommand(Command::ActivateGestures { gestures: batch }));
                }
            }
            "deactivate-gesture" => {
                let item_ids: Vec<InventoryKey> = targets
                    .iter()
                    .filter_map(|target_row| match target_row {
                        MenuTarget::Item(item) => Some(item.item_id),
                        MenuTarget::Folder(_folder) => None,
                    })
                    .collect();
                if !item_ids.is_empty() {
                    for item_id in &item_ids {
                        gestures.items.remove(item_id);
                    }
                    commands.write(SlCommand(Command::DeactivateGestures { item_ids }));
                }
            }
            "wear-wearable" | "attach" => {
                for target_row in &targets {
                    if let MenuTarget::Item(item) = target_row {
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
                        // Keep the COF authoritative: replace the slot's
                        // links with this item's.
                        let replaced = replaced_by_wear(model.worn_wearables(), item);
                        write_cof_commands(
                            &model,
                            cof_wear_link_commands(
                                model.cof_key(),
                                &cof_links_with_slots(&model),
                                item,
                                &replaced,
                            ),
                            &mut commands,
                        );
                    }
                }
            }
            "add-wearable" | "attach-add" => {
                for target_row in &targets {
                    if let MenuTarget::Item(item) = target_row {
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
                        write_cof_commands(
                            &model,
                            cof_wear_link_commands(
                                model.cof_key(),
                                &cof_links_with_slots(&model),
                                item,
                                &[],
                            ),
                            &mut commands,
                        );
                    }
                }
            }
            "take-off" => {
                // One SetWearing without every taken-off layer.
                let mut set: Vec<Wearable> = model.worn_wearables().to_vec();
                let before = set.len();
                let mut removed = Vec::new();
                for target_row in &targets {
                    if let MenuTarget::Item(item) = target_row {
                        set = take_off_set(&set, item.item_id);
                        removed.push(item.item_id);
                    }
                }
                if set.len() != before {
                    commands.write(SlCommand(Command::SetWearing(set)));
                    commands.write(SlCommand(Command::RequestWearables));
                }
                write_cof_commands(
                    &model,
                    cof_remove_link_commands(&cof_links_with_slots(&model), &removed),
                    &mut commands,
                );
            }
            "detach" => {
                let mut removed = Vec::new();
                for target_row in &targets {
                    if let MenuTarget::Item(item) = target_row {
                        commands.write(SlCommand(Command::DetachAttachmentIntoInventory {
                            item_id: item.item_id,
                        }));
                        worn.items.remove(&item.item_id);
                        removed.push(item.item_id);
                    }
                }
                write_cof_commands(
                    &model,
                    cof_remove_link_commands(&cof_links_with_slots(&model), &removed),
                    &mut commands,
                );
            }
            "replace-outfit" => {
                if let MenuTarget::Folder(folder) = &menu_target {
                    let items: Vec<ItemInfo> = model
                        .subtree_items(folder.folder_id)
                        .into_iter()
                        .cloned()
                        .collect();
                    let (batch, now_worn, no_longer_worn) = outfit_replace_commands(
                        &items,
                        model.worn_wearables(),
                        model.cof_items(),
                        &worn.items,
                        identity.agent_id,
                    );
                    for command in batch {
                        commands.write(SlCommand(command));
                    }
                    worn.items.extend(now_worn.iter().copied());
                    for item_id in &no_longer_worn {
                        worn.items.remove(item_id);
                    }
                    // Rewrite the COF: removed links out, the folder's
                    // outfit items linked in.
                    let mut cof_batch =
                        cof_remove_link_commands(&cof_links_with_slots(&model), &no_longer_worn);
                    for item in items.iter().filter(|item| is_outfit_item(item)) {
                        cof_batch.extend(cof_wear_link_commands(
                            model.cof_key(),
                            &cof_links_with_slots(&model),
                            item,
                            &[],
                        ));
                    }
                    write_cof_commands(&model, cof_batch, &mut commands);
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
                    // COF links: a replaced body part's link drops, every
                    // added outfit item links in.
                    let mut cof_batch = Vec::new();
                    for item in items.iter().filter(|item| is_outfit_item(item)) {
                        let replaced = replaced_by_wear(model.worn_wearables(), item);
                        cof_batch.extend(cof_wear_link_commands(
                            model.cof_key(),
                            &cof_links_with_slots(&model),
                            item,
                            &replaced,
                        ));
                    }
                    write_cof_commands(&model, cof_batch, &mut commands);
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
                    // Every removed item's COF link drops: the detached
                    // attachments plus the taken-off clothing layers.
                    let mut removed: Vec<InventoryKey> = no_longer_worn.clone();
                    for item in &items {
                        if item.inv_type == InventoryType::Wearable
                            && !wearable_type_of(item).is_body_part()
                            && is_worn(item, model.worn_wearables(), model.cof_items(), &worn.items)
                        {
                            removed.push(item.item_id);
                        }
                    }
                    for item_id in no_longer_worn {
                        worn.items.remove(&item_id);
                    }
                    write_cof_commands(
                        &model,
                        cof_remove_link_commands(&cof_links_with_slots(&model), &removed),
                        &mut commands,
                    );
                }
            }
            // The Attach To ▸ / Attach To HUD ▸ submenus: the action names the
            // chosen point's wire id, and the attach **replaces** what is on
            // that point (the reference's bare-point-id semantics; Add-along
            // is the plain "Add" entry on the default point).
            other => {
                if let Some(point) = attach_point_of(other) {
                    // The first selected object replaces the point; any
                    // further selected objects add alongside it there.
                    let mut first = true;
                    for target_row in &targets {
                        if let MenuTarget::Item(item) = target_row {
                            commands.write(SlCommand(Command::RezAttachment(RezAttachment {
                                item_id: item.item_id,
                                owner_id: identity
                                    .agent_id
                                    .map_or_else(Uuid::nil, |agent| agent.uuid()),
                                attachment_point: point,
                                mode: if first {
                                    AttachmentMode::Replace
                                } else {
                                    AttachmentMode::Add
                                },
                                name: item.name.clone(),
                                description: item.description.clone(),
                            })));
                            worn.items.insert(item.item_id);
                            first = false;
                            write_cof_commands(
                                &model,
                                cof_wear_link_commands(
                                    model.cof_key(),
                                    &cof_links_with_slots(&model),
                                    item,
                                    &[],
                                ),
                                &mut commands,
                            );
                        }
                    }
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

// ---------------------------------------------------------------------------
// Folder deep copy (viewer-inventory-folder-deep-copy).
// ---------------------------------------------------------------------------

/// The `AT_LINK` / `AT_LINK_FOLDER` wire codes, for the deep copy's re-link
/// handling.
const LINK_ASSET_CODES: (i32, i32) = (24, 25);

/// The commands of a **recursive folder copy** into `dest` — the reference's
/// `copy_inventory_category`: create the destination folder first, then per
/// folder copy its loaded items (a link is **re-linked** to its target, a
/// no-copy item is **skipped**, everything else `CopyInventoryItem`s), then
/// recurse into the sub-folders. Only **loaded** contents copy — the Copy
/// action prefetches the subtree so a paste normally sees everything.
pub(crate) fn deep_copy_commands(
    model: &InventoryModel,
    source: InventoryFolderKey,
    source_name: &str,
    dest: InventoryFolderKey,
    own_agent: Option<AgentKey>,
) -> Vec<Command> {
    let mut out = Vec::new();
    deep_copy_folder(model, source, source_name, dest, own_agent, &mut out);
    out
}

/// One level of [`deep_copy_commands`]'s walk.
fn deep_copy_folder(
    model: &InventoryModel,
    source: InventoryFolderKey,
    source_name: &str,
    dest: InventoryFolderKey,
    own_agent: Option<AgentKey>,
    out: &mut Vec<Command>,
) {
    // The UDP create lets the viewer pick the id, so the items can be
    // addressed into the new folder in the same command batch.
    let new_id = InventoryFolderKey::from(Uuid::new_v4());
    out.push(Command::CreateInventoryFolder {
        folder_id: new_id,
        parent_id: dest,
        folder_type: FolderType::None,
        name: source_name.to_owned(),
    });
    let from_library = model.is_library(source);
    for item in model.loaded_items_of(source) {
        let (link_code, link_folder_code) = LINK_ASSET_CODES;
        if item.asset_type == AssetType::Other(link_code) {
            // A link re-links to its original target in the copy.
            out.push(Command::LinkInventoryItem(NewInventoryLink {
                folder_id: new_id,
                linked_id: InventoryItemOrFolderKey::Item(InventoryKey::from(item.asset_id)),
                link_type: AssetType::Other(link_code),
                inv_type: item.inv_type,
                name: item.name.clone(),
                description: item.description.clone(),
            }));
            continue;
        }
        if item.asset_type == AssetType::Other(link_folder_code) {
            // A folder link likewise re-links.
            out.push(Command::LinkInventoryItem(NewInventoryLink {
                folder_id: new_id,
                linked_id: InventoryItemOrFolderKey::Folder(InventoryFolderKey::from(
                    item.asset_id,
                )),
                link_type: AssetType::Other(link_folder_code),
                inv_type: InventoryType::Category,
                name: item.name.clone(),
                description: item.description.clone(),
            }));
            continue;
        }
        // The reference skips a no-copy item (move_no_copy_items is only used
        // by the marketplace path).
        if !from_library && !item.permissions.owner.contains(Permissions::COPY) {
            continue;
        }
        let owner = match item.owner {
            sl_client_bevy::OwnerKey::Agent(agent) => agent,
            _other => own_agent.unwrap_or_else(|| AgentKey::from(Uuid::nil())),
        };
        out.push(Command::CopyInventoryItem {
            old_agent_id: owner,
            old_item_id: item.item_id,
            new_folder_id: new_id,
            new_name: String::new(),
        });
    }
    let children: Vec<InventoryFolderKey> = model.child_folders_of(source).to_vec();
    for child in children {
        let child_name = model
            .folder_info(child)
            .map_or_else(String::new, |info| info.name.clone());
        deep_copy_folder(model, child, &child_name, new_id, own_agent, out);
    }
}

// ---------------------------------------------------------------------------
// New-wearable creation (viewer-inventory-new-wearables).
// ---------------------------------------------------------------------------

/// The wearable creations whose upload reply has not arrived yet, oldest
/// first. `NewFileAgentInventory` creates the item server-side but leaves its
/// flags empty, so the reply is followed with a `ChangeInventoryItemFlags`
/// carrying the slot — matched FIFO (the reply carries no correlation id).
#[derive(Resource, Debug, Default)]
pub(crate) struct PendingWearableUploads {
    /// The in-flight creations: the slot to stamp and the folder to refresh.
    queue: VecDeque<(WearableType, InventoryFolderKey)>,
}

/// The wearable slot (and default item name) a create action names.
pub(crate) fn wearable_slot_of(action: &str) -> Option<(WearableType, &'static str)> {
    match action {
        "new-shirt" => Some((WearableType::Shirt, "New Shirt")),
        "new-pants" => Some((WearableType::Pants, "New Pants")),
        "new-shoes" => Some((WearableType::Shoes, "New Shoes")),
        "new-socks" => Some((WearableType::Socks, "New Socks")),
        "new-jacket" => Some((WearableType::Jacket, "New Jacket")),
        "new-skirt" => Some((WearableType::Skirt, "New Skirt")),
        "new-gloves" => Some((WearableType::Gloves, "New Gloves")),
        "new-undershirt" => Some((WearableType::Undershirt, "New Undershirt")),
        "new-underpants" => Some((WearableType::Underpants, "New Underpants")),
        "new-alpha" => Some((WearableType::Alpha, "New Alpha")),
        "new-tattoo" => Some((WearableType::Tattoo, "New Tattoo")),
        "new-universal" => Some((WearableType::Universal, "New Universal")),
        "new-physics" => Some((WearableType::Physics, "New Physics")),
        "new-shape" => Some((WearableType::Shape, "New Shape")),
        "new-skin" => Some((WearableType::Skin, "New Skin")),
        "new-hair" => Some((WearableType::Hair, "New Hair")),
        "new-eyes" => Some((WearableType::Eyes, "New Eyes")),
        _other => None,
    }
}

/// The `avatar_lad.xml` `wearable` attribute value naming a slot's visual
/// params (the filter [`default_wearable_asset`] applies).
const fn wearable_param_group(slot: WearableType) -> &'static str {
    match slot {
        WearableType::Shape => "shape",
        WearableType::Skin => "skin",
        WearableType::Hair => "hair",
        WearableType::Eyes => "eyes",
        WearableType::Shirt => "shirt",
        WearableType::Pants => "pants",
        WearableType::Shoes => "shoes",
        WearableType::Socks => "socks",
        WearableType::Jacket => "jacket",
        WearableType::Gloves => "gloves",
        WearableType::Undershirt => "undershirt",
        WearableType::Underpants => "underpants",
        WearableType::Skirt => "skirt",
        WearableType::Alpha => "alpha",
        WearableType::Tattoo => "tattoo",
        WearableType::Physics => "physics",
        _other => "universal",
    }
}

/// Author the default `.wearable` asset text for a fresh wearable of `slot` —
/// the reference's `LLWearable::exportStream` shape (`LLWearable version 22`,
/// the permissions / sale blocks, `type`, then every visual param of the
/// slot's `avatar_lad` group at its default weight; no layer textures). The
/// param set comes from the loaded avatar definitions; without them
/// (`--viewer-assets` absent) the asset carries no params, which the grid
/// accepts and the reference viewer treats as all-defaults.
pub(crate) fn default_wearable_asset(
    name: &str,
    slot: WearableType,
    own_agent: Option<AgentKey>,
    params: Option<&VisualParams>,
) -> String {
    use std::fmt::Write as _;
    let group = wearable_param_group(slot);
    let defaults: Vec<(i32, f32)> = params.map_or_else(Vec::new, |params| {
        params
            .all()
            .iter()
            .filter(|param| param.wearable.as_deref() == Some(group))
            .map(|param| (param.id, param.default))
            .collect()
    });
    let creator = own_agent.map_or_else(Uuid::nil, |agent| agent.uuid());
    let mut text = String::new();
    let _written = writeln!(text, "LLWearable version 22");
    let _written = writeln!(text, "{name}");
    let _written = writeln!(text);
    let _written = writeln!(text, "\tpermissions 0");
    let _written = writeln!(text, "\t{{");
    let _written = writeln!(text, "\t\tbase_mask\t7fffffff");
    let _written = writeln!(text, "\t\towner_mask\t7fffffff");
    let _written = writeln!(text, "\t\tgroup_mask\t00000000");
    let _written = writeln!(text, "\t\teveryone_mask\t00000000");
    let _written = writeln!(text, "\t\tnext_owner_mask\t0008e000");
    let _written = writeln!(text, "\t\tcreator_id\t{creator}");
    let _written = writeln!(text, "\t\towner_id\t{creator}");
    let _written = writeln!(text, "\t\tlast_owner_id\t{}", Uuid::nil());
    let _written = writeln!(text, "\t\tgroup_id\t{}", Uuid::nil());
    let _written = writeln!(text, "\t}}");
    let _written = writeln!(text, "\tsale_info\t0");
    let _written = writeln!(text, "\t{{");
    let _written = writeln!(text, "\t\tsale_type\tnot");
    let _written = writeln!(text, "\t\tsale_price\t10");
    let _written = writeln!(text, "\t}}");
    let _written = writeln!(text, "type {}", slot.to_code());
    let _written = writeln!(text, "parameters {}", defaults.len());
    for (id, weight) in defaults {
        let _written = writeln!(text, "{id} {weight}");
    }
    let _written = writeln!(text, "textures 0");
    text
}

/// Finish an in-flight wearable creation when its upload reply lands: stamp
/// the fresh item's flags with the wearable slot (the uploader path leaves
/// them empty, which would read as a Shape) and refresh its folder. Matched
/// FIFO against [`PendingWearableUploads`]; an upload failure drops the
/// oldest pending entry.
fn handle_wearable_uploads(
    mut events: MessageReader<SlEvent>,
    mut pending: ResMut<PendingWearableUploads>,
    mut commands: MessageWriter<SlCommand>,
) {
    for event in events.read() {
        match &event.0 {
            SlSessionEvent::AssetUploaded {
                new_inventory_item: Some(item),
                ..
            } => {
                if let Some((slot, folder)) = pending.queue.pop_front() {
                    commands.write(SlCommand(Command::ChangeInventoryItemFlags {
                        item_id: InventoryKey::from(*item),
                        flags: u32::from(slot.to_code()),
                    }));
                    query_folder_page(folder, &mut commands);
                }
            }
            SlSessionEvent::AssetUploadFailed { .. } => {
                let _dropped = pending.queue.pop_front();
            }
            _other => {}
        }
    }
}

/// Issue the create commands for a New Folder / Script / Notecard / Gesture
/// action into `dest` — shared by the folder context menu and the toolbar's
/// **+** menu. Returns whether the action was one of the creators.
#[expect(
    clippy::too_many_arguments,
    reason = "the shared create dispatcher takes every creation input: the action, the \
              destination, the identity, the wearable param source, the pending-upload queue \
              and the three output channels"
)]
fn dispatch_create(
    action: &str,
    dest: InventoryFolderKey,
    own_agent: Option<AgentKey>,
    params: Option<&VisualParams>,
    pending_wearables: &mut PendingWearableUploads,
    commands: &mut MessageWriter<SlCommand>,
    ui_actions: &mut MessageWriter<crate::inventory::InventoryUiAction>,
    rename: &mut crate::inventory::InlineRename,
) -> bool {
    // The wearable creators: author the slot's default asset, upload it (the
    // uploader creates the item), and stamp the flags when the reply lands.
    if let Some((slot, name)) = wearable_slot_of(action) {
        let asset_type = if slot.is_body_part() {
            AssetType::Bodypart
        } else {
            AssetType::Clothing
        };
        let text = default_wearable_asset(name, slot, own_agent, params);
        commands.write(SlCommand(Command::UploadAsset {
            folder_id: dest,
            asset_type,
            inventory_type: InventoryType::Wearable,
            name: name.to_owned(),
            description: String::new(),
            next_owner_mask: NEXT_OWNER_DEFAULT,
            group_mask: 0,
            everyone_mask: 0,
            expected_upload_cost: 0,
            data: text.into_bytes(),
        }));
        pending_wearables.queue.push_back((slot, dest));
        return true;
    }
    match action {
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
            true
        }
        "new-script" => {
            commands.write(SlCommand(Command::CreateScript {
                folder_id: dest,
                name: "New Script".to_owned(),
                description: String::new(),
                next_owner_mask: NEXT_OWNER_DEFAULT,
                language: ScriptLanguage::Lsl,
            }));
            true
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
            true
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
            true
        }
        _other => false,
    }
}

/// Route the toolbar **+** menu's picks: a create action lands in the
/// **selected** folder — the selected folder row itself, a selected item's
/// containing folder — or the agent root when nothing is selected, the
/// reference's behaviour for the add menu.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources: the pick stream, the \
              model / selection, the identity, the wearable param source, the pending queue \
              and the output channels"
)]
fn handle_inventory_add_actions(
    mut actions: MessageReader<UiAction>,
    model: Res<InventoryModel>,
    selection: Res<InventorySelection>,
    identity: Res<SlIdentity>,
    library: Option<Res<crate::avatar_assets::AvatarAssetLibrary>>,
    mut pending_wearables: ResMut<PendingWearableUploads>,
    mut rename: ResMut<crate::inventory::InlineRename>,
    mut ui_actions: MessageWriter<crate::inventory::InventoryUiAction>,
    mut commands: MessageWriter<SlCommand>,
) {
    for action in actions.read() {
        if action.element != INVENTORY_ADD_ELEMENT {
            continue;
        }
        let dest = selection
            .single()
            .and_then(|key| match key {
                RowKey::Folder(folder) => Some(folder),
                RowKey::Item(item) => model.find_item(item).map(|info| info.folder_id),
            })
            .filter(|folder| !model.is_library(*folder))
            .or_else(|| model.agent_root());
        let Some(dest) = dest else {
            continue;
        };
        dispatch_create(
            action.action,
            dest,
            identity.agent_id,
            library.as_ref().map(|lib| lib.params()),
            &mut pending_wearables,
            &mut commands,
            &mut ui_actions,
            &mut rename,
        );
    }
}

/// The pending Share flow: the rows chosen when Share was picked, awaiting
/// the avatar picker's choice.
#[derive(Resource, Debug, Default)]
pub(crate) struct PendingShare {
    /// The snapshotted share sources; empty when no Share is in flight.
    pub(crate) targets: Vec<MenuTarget>,
}

/// The avatar-picker requester tag the Share flow uses.
const SHARE_REQUESTER: &str = "inventory-share";

/// Complete a Share when the avatar picker confirms: give the stashed item /
/// folder to the chosen avatar (the same wire path as drag-to-give).
fn handle_share_picks(
    mut picks: MessageReader<crate::avatar_picker::AvatarPicked>,
    mut pending: ResMut<PendingShare>,
    mut commands: MessageWriter<SlCommand>,
) {
    for pick in picks.read() {
        if pick.requester != SHARE_REQUESTER {
            continue;
        }
        for target in pending.targets.drain(..) {
            if let Some(give) = crate::inventory_drag::give_command(&target, false, pick.agent) {
                commands.write(SlCommand(give));
            }
        }
    }
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
            .init_resource::<PendingShare>()
            .init_resource::<PendingWearableUploads>()
            .add_systems(
                Update,
                (
                    handle_inventory_menu_actions,
                    handle_inventory_add_actions,
                    handle_share_picks,
                    handle_wearable_uploads,
                    seed_worn_from_cof,
                )
                    .chain(),
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

    /// **The + (create) menu's entry table, pinned.** As the context menus.
    #[test]
    fn add_menu_keeps_every_entry() {
        let expected: Vec<(&str, &str)> = vec![
            ("Image...", "upload-image"),
            ("Sound...", "upload-sound"),
            ("Animation...", "upload-animation"),
            ("Model...", "upload-model"),
            ("Material...", "upload-material"),
            ("Bulk...", "upload-bulk"),
            ("New Folder", "new-folder"),
            ("New Script", "new-script"),
            ("New Notecard", "new-notecard"),
            ("New Gesture", "new-gesture"),
            ("New Material", "new-material"),
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
            ("New Sky", "new-sky"),
            ("New Water", "new-water"),
            ("New Day Cycle", "new-daycycle"),
            ("Shop...", "shop"),
        ];
        assert_eq!(
            entries(&super::INVENTORY_ADD_MENU),
            expected,
            "a + (create) menu entry moved — if intended, bless it by editing this table"
        );
    }

    /// Share is enabled for own transferable items and own folders, withheld
    /// in the Library and for no-transfer items.
    #[test]
    fn share_follows_transfer_and_library() {
        use super::CAN_SHARE;
        let transferable = item(
            1,
            InventoryType::Notecard,
            AssetType::Notecard,
            Permissions::TRANSFER.bits(),
        );
        let held = item_conditions(&transferable, ItemMenuFacts::default());
        assert!(held.contains(&CAN_SHARE));

        let no_transfer = item(2, InventoryType::Notecard, AssetType::Notecard, 0);
        let held = item_conditions(&no_transfer, ItemMenuFacts::default());
        assert!(!held.contains(&CAN_SHARE));

        let held = item_conditions(
            &transferable,
            ItemMenuFacts {
                in_library: true,
                ..ItemMenuFacts::default()
            },
        );
        assert!(!held.contains(&CAN_SHARE));

        let user_folder = folder(3, FolderType::None);
        let held = folder_conditions(&user_folder, FolderMenuFacts::default());
        assert!(held.contains(&CAN_SHARE));
        let held = folder_conditions(
            &user_folder,
            FolderMenuFacts {
                in_library: true,
                ..FolderMenuFacts::default()
            },
        );
        assert!(!held.contains(&CAN_SHARE));
    }

    /// A freshly authored default wearable parses back through the shared
    /// `.wearable` decoder with its version, type and (empty) sections
    /// intact — the byte-level contract with the reference importer.
    #[test]
    fn default_wearable_asset_round_trips() {
        use super::default_wearable_asset;
        let text = default_wearable_asset(
            "New Shirt",
            WearableType::Shirt,
            Some(AgentKey::from(Uuid::from_u128(7))),
            None,
        );
        let parsed = sl_client_bevy::WearableAsset::parse(&text).ok();
        assert_eq!(parsed.as_ref().map(|asset| asset.version), Some(22));
        assert_eq!(
            parsed.as_ref().map(|asset| asset.name.as_str()),
            Some("New Shirt")
        );
        assert_eq!(
            parsed.as_ref().map(|asset| asset.wearable_type),
            Some(WearableType::Shirt)
        );
        assert_eq!(parsed.as_ref().map(|asset| asset.params.len()), Some(0));
        assert_eq!(parsed.as_ref().map(|asset| asset.textures.len()), Some(0));
    }

    /// The wearable create actions map every slot, and asset types split
    /// body parts from clothing.
    #[test]
    fn wearable_slots_cover_every_creator() {
        use super::wearable_slot_of;
        assert_eq!(
            wearable_slot_of("new-shirt"),
            Some((WearableType::Shirt, "New Shirt"))
        );
        assert_eq!(
            wearable_slot_of("new-shape"),
            Some((WearableType::Shape, "New Shape"))
        );
        assert_eq!(wearable_slot_of("new-folder"), None);
        // Every creator entry of the two submenus resolves to a slot.
        for action in [
            "new-shirt",
            "new-pants",
            "new-shoes",
            "new-socks",
            "new-jacket",
            "new-skirt",
            "new-gloves",
            "new-undershirt",
            "new-underpants",
            "new-alpha",
            "new-tattoo",
            "new-universal",
            "new-physics",
            "new-shape",
            "new-skin",
            "new-hair",
            "new-eyes",
        ] {
            assert!(wearable_slot_of(action).is_some(), "unmapped: {action}");
        }
    }

    /// Replace Current Outfit keeps unreplaced body parts, swaps clothing
    /// wholesale, detaches worn attachments the folder lacks and adds the
    /// folder's own.
    #[test]
    fn replace_outfit_swaps_safely() {
        use super::outfit_replace_commands;
        // Currently worn: a shape, a shirt, and a tracked attachment.
        let current = vec![
            Wearable {
                item_id: InventoryKey::from(Uuid::from_u128(0x80)),
                asset_id: None,
                wearable_type: WearableType::Shape,
            },
            Wearable {
                item_id: InventoryKey::from(Uuid::from_u128(0x81)),
                asset_id: None,
                wearable_type: WearableType::Shirt,
            },
        ];
        let mut tracked = HashSet::new();
        let old_attachment = InventoryKey::from(Uuid::from_u128(0x82));
        tracked.insert(old_attachment);
        // The folder: new pants (clothing), and one new attachment. No body
        // parts, so the shape must survive.
        let mut pants = item(0x90, InventoryType::Wearable, AssetType::Clothing, 0);
        pants.flags = u32::from(WearableType::Pants.to_code());
        let new_attachment = item(0x91, InventoryType::Object, AssetType::Object, 0);

        let (commands, now_worn, no_longer) = outfit_replace_commands(
            &[pants.clone(), new_attachment.clone()],
            &current,
            &[],
            &tracked,
            Some(AgentKey::from(Uuid::from_u128(1))),
        );
        let set = commands
            .iter()
            .find_map(|command| match command {
                Command::SetWearing(set) => Some(set.clone()),
                _other => None,
            })
            .unwrap_or_default();
        // Shape kept, shirt gone, pants on.
        assert!(
            set.iter()
                .any(|worn| worn.wearable_type == WearableType::Shape)
        );
        assert!(
            !set.iter()
                .any(|worn| worn.item_id == InventoryKey::from(Uuid::from_u128(0x81)))
        );
        assert!(set.iter().any(|worn| worn.item_id == pants.item_id));
        // The old attachment detaches; the new one attaches alongside.
        assert!(commands.iter().any(|command| matches!(
            command,
            Command::DetachAttachmentIntoInventory { item_id } if *item_id == old_attachment
        )));
        assert!(commands.iter().any(|command| matches!(
            command,
            Command::RezAttachments { attachments, .. }
                if attachments.iter().any(|rez| rez.item_id == new_attachment.item_id)
        )));
        assert_eq!(now_worn, vec![new_attachment.item_id]);
        assert!(no_longer.contains(&old_attachment));
        assert!(no_longer.contains(&InventoryKey::from(Uuid::from_u128(0x81))));
    }

    /// COF link maintenance: wearing links the item in (dropping the
    /// replaced slot's links, never double-linking); removal drops exactly
    /// the targets' links.
    #[test]
    fn cof_links_follow_wear_and_removal() {
        use super::{cof_order_description, cof_remove_link_commands, cof_wear_link_commands};
        let cof = InventoryFolderKey::from(Uuid::from_u128(0xC0));
        let mut shirt = item(0x10, InventoryType::Wearable, AssetType::Clothing, 0);
        shirt.flags = u32::from(WearableType::Shirt.to_code());
        // An existing shirt-slot link to another item (the replaced shirt).
        let mut old_link = item(0x11, InventoryType::Wearable, AssetType::Other(24), 0);
        old_link.asset_id = Uuid::from_u128(0x12);
        old_link.description = cof_order_description(WearableType::Shirt, 0);
        let replaced = vec![InventoryKey::from(Uuid::from_u128(0x12))];
        let links = vec![(old_link.clone(), Some(WearableType::Shirt))];

        let batch = cof_wear_link_commands(Some(cof), &links, &shirt, &replaced);
        assert!(batch.iter().any(|command| matches!(
            command,
            Command::RemoveInventoryItems(removed) if removed.contains(&old_link.item_id)
        )));
        // The fresh link carries the slot's dense token: the replaced link is
        // gone, so the new shirt is layer 0.
        assert!(batch.iter().any(|command| matches!(
            command,
            Command::LinkInventoryItem(link)
                if link.folder_id == cof
                    && link.description == cof_order_description(WearableType::Shirt, 0)
        )));
        // Adding alongside (no replacement): the token continues the stack.
        let batch = cof_wear_link_commands(Some(cof), &links, &shirt, &[]);
        assert!(batch.iter().any(|command| matches!(
            command,
            Command::LinkInventoryItem(link)
                if link.description == cof_order_description(WearableType::Shirt, 1)
        )));
        // Already linked: nothing added.
        let mut own_link = item(0x13, InventoryType::Wearable, AssetType::Other(24), 0);
        own_link.asset_id = shirt.item_id.uuid();
        let batch = cof_wear_link_commands(
            Some(cof),
            &[(own_link, Some(WearableType::Shirt))],
            &shirt,
            &[],
        );
        assert!(batch.is_empty());
        // No COF located: a no-op.
        assert!(cof_wear_link_commands(None, &[], &shirt, &[]).is_empty());
        // Removal drops exactly the matching links.
        let batch = cof_remove_link_commands(&links, &[InventoryKey::from(Uuid::from_u128(0x12))]);
        assert!(matches!(
            batch.first(),
            Some(Command::RemoveInventoryItems(removed)) if removed.contains(&old_link.item_id)
        ));
        assert!(cof_remove_link_commands(&links, &[]).is_empty());
    }

    /// The ordering tokens round-trip, and removing a middle layer
    /// renumbers the survivors dense (via link-description updates).
    #[test]
    fn cof_layer_tokens_renumber_dense() {
        use super::{cof_order_description, cof_remove_link_commands, parse_order_token};
        assert_eq!(
            parse_order_token(&cof_order_description(WearableType::Tattoo, 3)),
            Some((WearableType::Tattoo.to_code(), 3))
        );
        assert_eq!(parse_order_token(""), None);
        assert_eq!(parse_order_token("a note"), None);

        // Three tattoo layers 0/1/2; remove the middle one.
        let make_link = |id: u128, target: u128, index: u32| {
            let mut link = item(id, InventoryType::Wearable, AssetType::Other(24), 0);
            link.asset_id = Uuid::from_u128(target);
            link.description = cof_order_description(WearableType::Tattoo, index);
            (link, Some(WearableType::Tattoo))
        };
        let links = vec![
            make_link(0x20, 0x30, 0),
            make_link(0x21, 0x31, 1),
            make_link(0x22, 0x32, 2),
        ];
        let batch = cof_remove_link_commands(&links, &[InventoryKey::from(Uuid::from_u128(0x31))]);
        // One removal batch, then exactly one renumber: layer 2 -> 1. The
        // untouched layer 0 gets no update.
        assert!(matches!(
            batch.first(),
            Some(Command::RemoveInventoryItems(removed))
                if removed == &vec![InventoryKey::from(Uuid::from_u128(0x21))]
        ));
        let updates: Vec<_> = batch
            .iter()
            .filter_map(|command| match command {
                Command::UpdateInventoryItem { item, .. } => {
                    Some((item.item_id, item.description.clone()))
                }
                _other => None,
            })
            .collect();
        assert_eq!(
            updates,
            vec![(
                InventoryKey::from(Uuid::from_u128(0x22)),
                cof_order_description(WearableType::Tattoo, 1)
            )]
        );
    }

    /// Multi-selection conditions are the intersection of every row's set —
    /// Delete survives only if every row is deletable, a type block only on
    /// a uniform selection — and a multi-selection withholds Rename.
    #[test]
    fn multi_select_conditions_intersect() {
        use super::intersect_conditions;
        let landmark = item(1, InventoryType::Landmark, AssetType::Landmark, 0);
        let object = item(2, InventoryType::Object, AssetType::Object, 0);
        let landmark_set = item_conditions(&landmark, ItemMenuFacts::default());
        let object_set = item_conditions(&object, ItemMenuFacts::default());
        let mixed = intersect_conditions(&[landmark_set.clone(), object_set.clone()]);
        // Mixed types: neither type block survives, the shared mutations do.
        assert!(!mixed.contains(&IS_LANDMARK));
        assert!(!mixed.contains(&IS_OBJECT));
        assert!(mixed.contains(&CAN_DELETE));
        assert!(mixed.contains(&CAN_CUT));
        // A multi-selection cannot rename, even though each row could.
        assert!(landmark_set.contains(&CAN_RENAME));
        assert!(!mixed.contains(&CAN_RENAME));
        // A single-row "intersection" keeps everything, rename included.
        let single = intersect_conditions(std::slice::from_ref(&landmark_set));
        assert_eq!(single, landmark_set);
        // A trashed row's absence of CAN_DELETE wins over the other row.
        let trashed = item_conditions(
            &object,
            ItemMenuFacts {
                in_trash: true,
                ..ItemMenuFacts::default()
            },
        );
        let with_trashed = intersect_conditions(&[object_set, trashed]);
        assert!(!with_trashed.contains(&CAN_DELETE));
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
