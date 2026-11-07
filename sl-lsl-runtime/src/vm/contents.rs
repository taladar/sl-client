//! A prim's contents as a script reaches them (`server-lsl-lib-task-inventory`):
//! the items it lists, the objects it rezzes, what it gives an avatar or
//! another prim, the scripts it loads elsewhere and the notecards it reads.
//!
//! The host keeps the contents — the items, their order of arrival, the
//! serial a viewer's Contents tab follows — and lends them to the library as
//! plain [`Item`]s. Everything a grid decides about them is the library's:
//! the order a listing is read in, which calls a missing name is refused by,
//! how a notecard splits into lines, which change a removal raises. What the
//! host is handed back — a [`Rez`], an [`Offer`] — is finished, so it only
//! applies it (`book/src/gridspec/lsl.md` § Task inventory, giving, rezzing
//! and notecards has the measurements).

use sl_types::lsl::{Rotation, Vector};

/// `INVENTORY_ALL`: every type, in a listing.
pub const INVENTORY_ALL: i32 = -1;
/// `INVENTORY_NONE`: what `llGetInventoryType` answers for a name the prim
/// does not hold.
pub const INVENTORY_NONE: i32 = -1;
/// `INVENTORY_TEXTURE`.
pub const INVENTORY_TEXTURE: i32 = 0;
/// `INVENTORY_SOUND`.
pub const INVENTORY_SOUND: i32 = 1;
/// `INVENTORY_LANDMARK`.
pub const INVENTORY_LANDMARK: i32 = 3;
/// `INVENTORY_CLOTHING`.
pub const INVENTORY_CLOTHING: i32 = 5;
/// `INVENTORY_OBJECT`.
pub const INVENTORY_OBJECT: i32 = 6;
/// `INVENTORY_NOTECARD`.
pub const INVENTORY_NOTECARD: i32 = 7;
/// `INVENTORY_SCRIPT`.
pub const INVENTORY_SCRIPT: i32 = 10;
/// `INVENTORY_BODYPART`.
pub const INVENTORY_BODYPART: i32 = 13;
/// `INVENTORY_ANIMATION`.
pub const INVENTORY_ANIMATION: i32 = 20;
/// `INVENTORY_GESTURE`.
pub const INVENTORY_GESTURE: i32 = 21;
/// `INVENTORY_SETTING`.
pub const INVENTORY_SETTING: i32 = 56;
/// `INVENTORY_MATERIAL`, which OpenSim's document does not name.
pub const INVENTORY_MATERIAL: i32 = 57;

/// `PERM_TRANSFER`.
pub const PERM_TRANSFER: i32 = 0x2000;
/// `PERM_MODIFY`.
pub const PERM_MODIFY: i32 = 0x4000;
/// `PERM_COPY`.
pub const PERM_COPY: i32 = 0x8000;
/// `PERM_MOVE`.
pub const PERM_MOVE: i32 = 0x8_0000;
/// What an owner may do with an item of every right: `0x7FFFFFFF`, as both
/// grids report a full-permission item's base, owner and next-owner masks.
pub const PERM_ALL: i32 = 0x7FFF_FFFF;

/// The five permission masks of an item, as `llGetInventoryPermMask` reads
/// them (`MASK_BASE` 0, `MASK_OWNER` 1, `MASK_GROUP` 2, `MASK_EVERYONE` 3,
/// `MASK_NEXT` 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Masks {
    /// What anybody who owns it may ever do with it.
    pub base: i32,
    /// What its owner may do with it.
    pub owner: i32,
    /// What its group may do with it.
    pub group: i32,
    /// What everybody may do with it.
    pub everyone: i32,
    /// What its next owner will be able to do with it.
    pub next: i32,
}

impl Masks {
    /// An item of every right to its owner, nothing to the group or to
    /// everybody, and every right to the next owner: an item the owner made.
    #[must_use]
    pub const fn full() -> Self {
        Self {
            base: PERM_ALL,
            owner: PERM_ALL,
            group: 0,
            everyone: 0,
            next: PERM_ALL,
        }
    }

    /// The mask `which` names (`MASK_BASE` … `MASK_NEXT`), [`None`] for any
    /// other number.
    #[must_use]
    pub const fn get(&self, which: i32) -> Option<i32> {
        match which {
            0 => Some(self.base),
            1 => Some(self.owner),
            2 => Some(self.group),
            3 => Some(self.everyone),
            4 => Some(self.next),
            _ => None,
        }
    }
}

/// One item of a prim's contents, as the library reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// Its name, exactly as the grid stored it.
    pub name: String,
    /// Its `INVENTORY_*` type.
    pub kind: i32,
    /// The key of the item itself (OpenSim's `osGetInventoryItemKey`).
    pub item: String,
    /// The key of its asset; `NULL_KEY` for an item with none.
    pub asset: String,
    /// The key of its creator.
    pub creator: String,
    /// The key of its owner before the prim's owner, or `NULL_KEY`.
    pub last_owner: String,
    /// Its permission masks.
    pub masks: Masks,
    /// Its description.
    pub description: String,
    /// When it came into the prim's owner's hands, in seconds since the
    /// epoch (`llGetInventoryAcquireTime`).
    pub acquired: i64,
}

impl Item {
    /// Whether its owner may modify, copy and transfer it: only then do both
    /// grids hand a script its asset key (`llGetInventoryKey`) and OpenSim
    /// its item key (`osGetInventoryItemKey`).
    #[must_use]
    pub const fn full_permission(&self) -> bool {
        let wanted = PERM_MODIFY | PERM_COPY | PERM_TRANSFER;
        self.masks.owner & wanted == wanted
    }
}

/// A rez of an object out of the caller's prim's contents, as the library
/// hands it to the host: checked, its rotation made unit length, and with
/// what the rezzed object's scripts are to be started with.
#[derive(Debug, Clone, PartialEq)]
pub struct Rez {
    /// The object item's name.
    pub item: String,
    /// Where the object goes, in region coordinates: its root
    /// (`llRezAtRoot`), or the centre of its bounding box (`llRezObject`).
    pub position: Vector,
    /// Whether [`Self::position`] places the root rather than the centre.
    pub at_root: bool,
    /// The velocity given it; the host keeps it only for a physical object.
    pub velocity: Vector,
    /// Its rotation, unit length.
    pub rotation: Rotation,
    /// What its scripts' `on_rez` and `llGetStartParameter` get.
    pub start_parameter: i32,
    /// What its scripts' `llGetStartString` gets (`REZ_PARAM_STRING`).
    pub start_string: String,
    /// Whether the object is temporary (`REZ_FLAG_TEMP`).
    pub temporary: bool,
    /// Whether the object dies at the region's edge (`STATUS_DIE_AT_EDGE`):
    /// OpenSim sets it on every object a script rezzes, Second Life does not.
    pub die_at_edge: bool,
}

/// Why the host rezzed nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RezRefusal {
    /// The item named is not an object.
    NotAnObject,
    /// The object's body is not there to rez from.
    Missing,
}

/// Items offered to an avatar from the caller's prim
/// (`llGiveInventory`, `llGiveInventoryList`): an instant message of dialog
/// `TaskInventoryOffered` under the object owner's id and the object's name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    /// The avatar.
    pub agent: String,
    /// The items, by name, all of them in the caller's prim.
    pub items: Vec<String>,
    /// The folder the items arrive in (`llGiveInventoryList`), or [`None`]
    /// for one item given alone.
    pub folder: Option<String>,
    /// The offer's text, in the flavour's words.
    pub message: String,
    /// Whether the offer is marked offline, as both grids mark a single
    /// item's (`inventory/give`).
    pub offline: bool,
}

/// A script loaded into another prim (`llRemoteLoadScriptPin`), the pin
/// already checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteLoad {
    /// The key of the prim it goes into.
    pub target: String,
    /// The script's name in the caller's prim, which it keeps there,
    /// replacing a script of that name.
    pub script: String,
    /// Whether it runs.
    pub running: bool,
    /// What its `llGetStartParameter` reads.
    pub start_parameter: i32,
}

/// `PARCEL_FLAG_ALLOW_ALL_OBJECT_ENTRY`.
pub const PARCEL_FLAG_ALLOW_ALL_OBJECT_ENTRY: u32 = 0x0800_0000;
/// `PARCEL_FLAG_ALLOW_CREATE_OBJECTS`.
pub const PARCEL_FLAG_ALLOW_CREATE_OBJECTS: u32 = 0x40;
/// `PARCEL_FLAG_ALLOW_SCRIPTS`: scripts of objects nobody of the parcel owns.
pub const PARCEL_FLAG_ALLOW_SCRIPTS: u32 = 0x2;
/// `PARCEL_FLAG_ALLOW_GROUP_SCRIPTS`.
pub const PARCEL_FLAG_ALLOW_GROUP_SCRIPTS: u32 = 0x0200_0000;

/// What a library call does to another script of its own prim, carried out
/// once the slice is over: `llSetScriptState`, `llResetOtherScript`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    /// Set its run flag.
    Run(bool),
    /// Reset it, as its flavour resets another script.
    Reset,
}

/// The lines of the notecard `text` as `flavour`'s grid reads them.
///
/// Second Life splits at every newline, so a final newline starts an empty
/// last line and an empty notecard is one empty line; OpenSim's
/// `ParseNotecardToArray` drops the empty line after a final newline, and an
/// empty notecard has none (`inventory/notecards`).
#[must_use]
pub fn notecard_lines(text: &str, opensim: bool) -> Vec<String> {
    let mut lines: Vec<String> = text.split('\n').map(str::to_owned).collect();
    if opensim && lines.last().is_some_and(String::is_empty) {
        let _last = lines.pop();
    }
    lines
}
