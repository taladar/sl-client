//! A census of how each grid takes a change to a viewer's inventory: every
//! item and folder operation a viewer sends, what the grid pushes back after
//! each, which removals it confines to the Trash, and what Second Life's AIS3
//! answers to the same changes.
//!
//! Each operation is sent **raw** where the client routes it per grid — the UDP
//! message with [`Command::Send`], the AIS3 request with `reqwest` — so the
//! answer is the grid's to that road and not the client's choice of road.
//! Every leg is read back through the folder fetch (and the item fetch), and
//! records:
//!
//! - **what arrived** in a window after it (`_push`): every inventory
//!   message, by name and road — `(UDP)` where its datagrams arrived (the
//!   circuit probe names them), `(event queue)` where the client heard it with
//!   none, `(event queue, unhandled)` for an event the client does not read —
//!   and, in `_push_detail`, what each said, with every id named by its role in
//!   the run (`F`, `the notecard`, `the Trash`, …);
//! - **what changed**: whether the item or folder is listed where the
//!   operation put it, or gone, and the step of each affected folder's version.
//!
//! The legs:
//!
//! 1. **Items over UDP**, in a run folder `F`: `CreateInventoryItem` (a
//!    notecard), `CopyInventoryItem`, `UpdateInventoryItem` (a rename and a new
//!    description), `ChangeInventoryItemFlags`, `MoveInventoryItem` into a
//!    second run folder `G`, `LinkInventoryItem` into `G` and into the Current
//!    Outfit Folder.
//! 2. **Removals and the Trash**: `RemoveInventoryItem` of an item outside the
//!    Trash and of one inside it; `RemoveInventoryFolder` of a folder outside
//!    the Trash; `PurgeInventoryDescendents` of a folder outside the Trash and of
//!    one inside it; `RemoveInventoryObjects` (a folder and an item) outside the
//!    Trash and inside it.
//! 3. **AIS3** (Second Life only): a folder made with `POST`, renamed with
//!    `PATCH`; an item updated with `PATCH`; a link made with `POST` and a
//!    folder's links replaced with `PUT .../links`; an item deleted, a folder
//!    emptied and a folder deleted, each outside the Trash; a Library folder
//!    copied into the run's with `COPY` — each answer recorded whole, with what
//!    its `_created_…` / `_removed_…` / `_updated_category_versions` keys name.
//! 4. **A no-copy give** (two avatars): B makes a notecard whose next owner may
//!    not copy it and gives it to A; A gives it back. Where A's item is after
//!    the offer and after B accepts, and what A was pushed in each window.
//!
//! A grid that grants no AIS3 records `no capability` for its legs. What each
//! live grid answered is stated as a [`Measured`] and written up in
//! `book/src/gridspec/inventory.md` § Changing inventory; every run is held to
//! it.
//!
//! `2av`, live and fake grids.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;
use std::time::{Duration, Instant};

use sl_client_tokio::{
    AgentKey, AnyMessage, AssetType, AssetUpdateLocation, CircuitProbe, Command, Diagnostic, Event,
    FolderType, ImDialog, InventoryCallbackId, InventoryFolderKey, InventoryItem, InventoryKey,
    InventoryType, NewInventoryItem, TransactionId, UpdatableAssetType, Uuid, WearableType,
};
use sl_wire::Llsd;
use sl_wire::messages::{
    LinkInventoryItem, LinkInventoryItemAgentDataBlock, LinkInventoryItemInventoryBlockBlock,
    MoveInventoryItem, MoveInventoryItemAgentDataBlock, MoveInventoryItemInventoryDataBlock,
    PurgeInventoryDescendents, PurgeInventoryDescendentsAgentDataBlock,
    PurgeInventoryDescendentsInventoryDataBlock, RemoveInventoryFolder,
    RemoveInventoryFolderAgentDataBlock, RemoveInventoryFolderFolderDataBlock, RemoveInventoryItem,
    RemoveInventoryItemAgentDataBlock, RemoveInventoryItemInventoryDataBlock,
    RemoveInventoryObjects, RemoveInventoryObjectsAgentDataBlock,
    RemoveInventoryObjectsFolderDataBlock, RemoveInventoryObjectsItemDataBlock,
};

use crate::cases::asset_http::without_ids;
use crate::cases::inventory_fetch::{
    AIS, CAP_WAIT, Census, FI2, FID2, HTTP_TIMEOUT, LIBRARY_AIS, Made, NO_CAPABILITY, POLL, Raw,
    SETTLE, View, answer, clean_up, keys_of, listed_folders, step, uuid_of,
};
use crate::cases::object_rez_derez::Answer;
use crate::cases::task_inventory::hold_to_measured;
use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::metrics::Metrics;
use crate::registry::{GridTest, TestFuture};
use crate::support::REGION_TIMEOUT;

/// Where the measurements below are written down.
const SOURCE: &str =
    "book/src/gridspec/inventory.md § Changing inventory (inventory-mutations, 2026-10-22)";

/// How long what a grid pushes after an operation is listened for.
const PUSH_WINDOW: Duration = Duration::from_secs(5);

/// How long what an earlier step caused is let arrive before an operation.
const DRAIN_WINDOW: Duration = Duration::from_secs(2);

/// How long late pushes are listened for after the read-back.
const LATE_WINDOW: Duration = Duration::from_secs(1);

/// How long an inventory offer may take to reach the other avatar.
const OFFER_TIMEOUT: Duration = Duration::from_secs(30);

/// How long a notecard body's save may take.
const SAVE_TIMEOUT: Duration = Duration::from_secs(60);

/// The owner-copy permission bit (`PERM_COPY`).
const PERM_COPY: i64 = 0x8000;

/// The next-owner mask of the no-copy notecard: move, modify, transfer.
const NEXT_OWNER_NO_COPY: u32 = 0x0008_6000;

/// The next-owner mask of every other notecard: move, copy, modify, transfer.
const NEXT_OWNER_FULL: u32 = 0x0008_e000;

/// The flags the flags leg sets (the reference's landmark "visited" bit, the
/// only value it ever sends).
const FLAGS_SET: u32 = 1;

/// The asset type of an item link (`AT_LINK`).
const AT_LINK: i8 = 24;

/// The asset type of a notecard (`AT_NOTECARD`), no link type.
const AT_NOTECARD: i8 = 7;

/// The inventory type of a notecard (`IT_NOTECARD`).
const IT_NOTECARD: i8 = 7;

/// The most items a Library folder the `COPY` leg copies may hold.
const COPY_MAX_ITEMS: usize = 6;

/// An answer both grids give.
const fn both(answer: Answer) -> Measured<Answer> {
    Measured {
        second_life: answer,
        opensim: answer,
        source: SOURCE,
    }
}

/// An answer the grids differ in.
const fn each(second_life: Answer, opensim: Answer) -> Measured<Answer> {
    Measured {
        second_life,
        opensim,
        source: SOURCE,
    }
}

/// What each grid was measured to answer, by the metric the census records it
/// under — generated from the last two runs on each live grid (aditi and the
/// local OpenSim, 2026-10-22), every key of which held still between them.
/// Second Life's `control_` and `nocopy_` keys are `Any`: the primary test
/// account's notecard gifts are filed in a folder Second Life no longer has
/// (`inventory-offers`, `book/src/gridspec/inventory.md` § Giving), which is
/// the account's, not the grid's.
const MEASURED: &[(&str, Measured<Answer>)] = &[
    (
        "ais_copy_library_answer",
        each(
            Answer::Text(
                "201: _base_uri,_created_categories,_created_items,_embedded,_links,_moved_items,_removed_items,_top_category_id,_updated_category_versions,agent_id,category_id,name,parent_id,type_default,version; _created_categories 1; _created_items 2; _moved_items 0; _removed_items 0",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ais_copy_library_effects",
        each(
            Answer::Text(
                "_created_categories [the copied folder]; _created_items [new,new]; _embedded {categories: 0 entries, items: 2 entries, links: 0 entries}; _moved_items []; _removed_items []; _top_category_id string; _updated_category_versions {F +1}",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ais_copy_library_push",
        each(Answer::Text("nothing"), Answer::Text("no capability")),
    ),
    (
        "ais_copy_library_push_detail",
        each(Answer::Text("nothing"), Answer::Text("no capability")),
    ),
    (
        "ais_copy_library_result",
        each(
            Answer::Text("1 folders in F, 2 of 2 items, F's own items unchanged"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ais_create_answer",
        each(
            Answer::Text(
                "201: _base_uri,_created_categories,_created_items,_embedded,_links,_updated_category_versions,agent_id,category_id,name,parent_id,type_default,version; _created_categories 1; _created_items 0",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ais_create_effects",
        each(
            Answer::Text(
                "_created_categories [the AIS3 folder]; _created_items []; _embedded {categories: 1 entries, items: 0 entries, links: 0 entries}; _updated_category_versions {the root +1}",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ais_create_own_id_answer",
        each(
            Answer::Text(
                "201: _base_uri,_created_categories,_created_items,_embedded,_links,_updated_category_versions,agent_id,category_id,name,parent_id,type_default,version; _created_categories 1; _created_items 0",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ais_create_own_id_effects",
        each(
            Answer::Text(
                "_created_categories [the AIS3 own-id folder]; _created_items []; _embedded {categories: 1 entries, items: 0 entries, links: 0 entries}; _updated_category_versions {the root +1}",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ais_create_own_id_push",
        each(Answer::Text("nothing"), Answer::Text("no capability")),
    ),
    (
        "ais_create_own_id_push_detail",
        each(Answer::Text("nothing"), Answer::Text("no capability")),
    ),
    (
        "ais_create_own_id_result",
        each(
            Answer::Text("listed under another id"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ais_create_push",
        each(Answer::Text("nothing"), Answer::Text("no capability")),
    ),
    (
        "ais_create_push_detail",
        each(Answer::Text("nothing"), Answer::Text("no capability")),
    ),
    (
        "ais_create_result",
        each(Answer::Text("listed"), Answer::Text("no capability")),
    ),
    (
        "ais_delete_folder_answer",
        each(
            Answer::Text(
                "200: _active_gestures_removed,_attachments_removed,_broken_links_removed,_categories_removed,_category_items_removed,_total_items_removed,_updated_category_versions,_wearables_removed; _active_gestures_removed 0; _broken_links_removed 0; _categories_removed 1; _category_items_removed 0",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ais_delete_folder_effects",
        each(
            Answer::Text(
                "_active_gestures_removed []; _attachments_removed {}; _broken_links_removed []; _categories_removed [the AIS3 folder]; _category_items_removed []; _total_items_removed integer; _updated_category_versions {the root +1}; _wearables_removed {}",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ais_delete_folder_push",
        each(Answer::Text("nothing"), Answer::Text("no capability")),
    ),
    (
        "ais_delete_folder_push_detail",
        each(Answer::Text("nothing"), Answer::Text("no capability")),
    ),
    (
        "ais_delete_folder_result",
        each(Answer::Text("gone"), Answer::Text("no capability")),
    ),
    (
        "ais_delete_item_answer",
        each(
            Answer::Text(
                "200: _active_gestures_removed,_attachments_removed,_broken_links_removed,_category_items_removed,_updated_category_versions,_wearables_removed; _active_gestures_removed 0; _broken_links_removed 0; _category_items_removed 1",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ais_delete_item_effects",
        each(
            Answer::Text(
                "_active_gestures_removed []; _attachments_removed {}; _broken_links_removed []; _category_items_removed [the AIS3-deleted item]; _updated_category_versions {the AIS3 folder +1}; _wearables_removed {}",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ais_delete_item_push",
        each(Answer::Text("nothing"), Answer::Text("no capability")),
    ),
    (
        "ais_delete_item_push_detail",
        each(Answer::Text("nothing"), Answer::Text("no capability")),
    ),
    (
        "ais_delete_item_result",
        each(Answer::Text("gone"), Answer::Text("no capability")),
    ),
    (
        "ais_link_answer",
        each(
            Answer::Text(
                "201: _base_uri,_created_items,_embedded,_links,_updated_category_versions,agent_id,category_id,name,parent_id,type_default,version; _created_items 1",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ais_link_effects",
        each(
            Answer::Text(
                "_created_items [the AIS3 link]; _embedded {links: 1 entries}; _updated_category_versions {the AIS3 folder +1}",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ais_link_push",
        each(Answer::Text("nothing"), Answer::Text("no capability")),
    ),
    (
        "ais_link_push_detail",
        each(Answer::Text("nothing"), Answer::Text("no capability")),
    ),
    (
        "ais_link_result",
        each(Answer::Text("listed"), Answer::Text("no capability")),
    ),
    (
        "ais_purge_answer",
        each(
            Answer::Text(
                "200: _active_gestures_removed,_attachments_removed,_broken_links_removed,_categories_removed,_category_items_removed,_total_items_removed,_updated_category_versions,_wearables_removed; _active_gestures_removed 0; _broken_links_removed 0; _categories_removed 0; _category_items_removed 2",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ais_purge_effects",
        each(
            Answer::Text(
                "_active_gestures_removed []; _attachments_removed {}; _broken_links_removed []; _categories_removed []; _category_items_removed [a slammed link,the AIS3-purged item]; _total_items_removed integer; _updated_category_versions {the AIS3 folder +1}; _wearables_removed {}",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ais_purge_push",
        each(Answer::Text("nothing"), Answer::Text("no capability")),
    ),
    (
        "ais_purge_push_detail",
        each(Answer::Text("nothing"), Answer::Text("no capability")),
    ),
    (
        "ais_purge_result",
        each(Answer::Text("emptied"), Answer::Text("no capability")),
    ),
    (
        "ais_rename_answer",
        each(
            Answer::Text(
                "200: _base_uri,_links,_updated_categories,_updated_category_versions,agent_id,category_id,name,parent_id,type_default,version; _updated_categories 1",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ais_rename_effects",
        each(
            Answer::Text(
                "_updated_categories [the AIS3 folder]; _updated_category_versions {the root +1}",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ais_rename_push",
        each(Answer::Text("nothing"), Answer::Text("no capability")),
    ),
    (
        "ais_rename_push_detail",
        each(Answer::Text("nothing"), Answer::Text("no capability")),
    ),
    (
        "ais_rename_result",
        each(Answer::Text("renamed"), Answer::Text("no capability")),
    ),
    (
        "ais_slam_answer",
        each(
            Answer::Text(
                "201: _base_uri,_category_items_removed,_created_items,_embedded,_linked_ids,_links,_updated_category_versions,_updated_items,agent_id,category_id,name,parent_id,type_default,version; _category_items_removed 0; _created_items 0; _updated_items 1",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ais_slam_effects",
        each(
            Answer::Text(
                "_category_items_removed []; _created_items []; _embedded {links: 1 entries}; _linked_ids {the notecard none}; _updated_category_versions {the AIS3 folder +1}; _updated_items [a slammed link]",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ais_slam_push",
        each(Answer::Text("nothing"), Answer::Text("no capability")),
    ),
    (
        "ais_slam_push_detail",
        each(Answer::Text("nothing"), Answer::Text("no capability")),
    ),
    (
        "ais_slam_result",
        each(
            Answer::Text("2 items: a slammed link, another"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ais_slam_duplicate_answer",
        each(
            Answer::Text(
                "400: error_code,error_description,error_filename,error_function,error_line_number; error_code=0; error_description=Duplicate links or invalid link type.; error_filename=categories.py; error_function=CategoryAPI._get_links; error_line_number=<n>; says Duplicate links or invalid link type.",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ais_slam_duplicate_effects",
        each(Answer::Text("no _ keys"), Answer::Text("no capability")),
    ),
    (
        "ais_slam_duplicate_push",
        each(Answer::Text("nothing"), Answer::Text("no capability")),
    ),
    (
        "ais_slam_duplicate_push_detail",
        each(Answer::Text("nothing"), Answer::Text("no capability")),
    ),
    (
        "ais_slam_duplicate_result",
        each(
            Answer::Text("2 items: a slammed link, another; version kept"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ais_slam_map_answer",
        each(
            Answer::Text(
                "400: error_code,error_description,error_filename,error_function,error_line_number; error_code=1; error_description=List of links required.; error_filename=category.py; error_function=Category.put; error_line_number=686; says List of links required.",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ais_slam_map_effects",
        each(Answer::Text("no _ keys"), Answer::Text("no capability")),
    ),
    (
        "ais_slam_map_push",
        each(Answer::Text("nothing"), Answer::Text("no capability")),
    ),
    (
        "ais_slam_map_push_detail",
        each(Answer::Text("nothing"), Answer::Text("no capability")),
    ),
    (
        "ais_slam_map_result",
        each(
            Answer::Text("2 items: a slammed link, another; version kept"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ais_slam_not_link_answer",
        each(
            Answer::Text(
                "400: error_code,error_description,error_filename,error_function,error_line_number; error_code=0; error_description=Duplicate links or invalid link type.; error_filename=categories.py; error_function=CategoryAPI._get_links; error_line_number=<n>; says Duplicate links or invalid link type.",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ais_slam_not_link_effects",
        each(Answer::Text("no _ keys"), Answer::Text("no capability")),
    ),
    (
        "ais_slam_not_link_push",
        each(Answer::Text("nothing"), Answer::Text("no capability")),
    ),
    (
        "ais_slam_not_link_push_detail",
        each(Answer::Text("nothing"), Answer::Text("no capability")),
    ),
    (
        "ais_slam_not_link_result",
        each(
            Answer::Text("2 items: a slammed link, another; version kept"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ais_update_item_answer",
        each(
            Answer::Text(
                "200: _base_uri,_links,_updated_category_versions,_updated_items,agent_id,asset_id,created_at,desc,flags,inv_type,item_id,name,parent_id,permissions,sale_info,type; _updated_items 1",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ais_update_item_effects",
        each(
            Answer::Text("_updated_category_versions {F +1}; _updated_items [the notecard]"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ais_update_item_push",
        each(Answer::Text("nothing"), Answer::Text("no capability")),
    ),
    (
        "ais_update_item_push_detail",
        each(Answer::Text("nothing"), Answer::Text("no capability")),
    ),
    (
        "ais_update_item_result",
        each(
            Answer::Text("renamed and described"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ais_update_whole_answer",
        each(
            Answer::Text(
                "200: _base_uri,_links,_updated_category_versions,_updated_items,agent_id,asset_id,created_at,desc,flags,inv_type,item_id,name,parent_id,permissions,sale_info,type; _updated_items 1",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ais_update_whole_effects",
        each(
            Answer::Text("_updated_category_versions {F +1}; _updated_items [the notecard]"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ais_update_whole_push",
        each(Answer::Text("nothing"), Answer::Text("no capability")),
    ),
    (
        "ais_update_whole_push_detail",
        each(Answer::Text("nothing"), Answer::Text("no capability")),
    ),
    (
        "ais_update_whole_result",
        each(
            Answer::Text("name set, description set, next-owner mask set"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "cof_link_folder_step",
        each(Answer::Count(0), Answer::Count(1)),
    ),
    (
        "cof_link_listed",
        each(Answer::Flag(false), Answer::Flag(true)),
    ),
    (
        "cof_link_push",
        each(
            Answer::Text("AlertMessage (UDP)"),
            Answer::Text("UpdateCreateInventoryItem (UDP)"),
        ),
    ),
    (
        "cof_link_push_detail",
        each(
            Answer::Text("alert: Cannot create requested inventory."),
            Answer::Text("UpdateCreateInventoryItem: the COF link in the COF"),
        ),
    ),
    (
        "cof_link_shape",
        each(
            Answer::Text("no link"),
            Answer::Text("type 24, names the notecard"),
        ),
    ),
    (
        "control_filed_in",
        each(Answer::Any, Answer::Text("Notecards")),
    ),
    (
        "control_receive_push",
        each(Answer::Any, Answer::Text("nothing")),
    ),
    (
        "control_receive_push_detail",
        each(Answer::Any, Answer::Text("nothing")),
    ),
    ("control_received", each(Answer::Any, Answer::Flag(true))),
    ("copy_f_step", both(Answer::Count(1))),
    ("copy_listed", both(Answer::Flag(true))),
    (
        "copy_push",
        each(
            Answer::Text("BulkUpdateInventory (event queue)"),
            Answer::Text("UpdateCreateInventoryItem (UDP)"),
        ),
    ),
    (
        "copy_push_detail",
        each(
            Answer::Text("BulkUpdateInventory: folders [] items [the copy]"),
            Answer::Text("UpdateCreateInventoryItem: the copy in F, with a callback id"),
        ),
    ),
    ("create_answered", both(Answer::Flag(true))),
    (
        "create_asset",
        each(Answer::Text("nil"), Answer::Text("an asset")),
    ),
    (
        "create_base_mask",
        each(Answer::Text("0x7fffffff"), Answer::Text("0x0009e000")),
    ),
    ("create_f_step", both(Answer::Count(1))),
    (
        "create_owner_mask",
        each(Answer::Text("0x7fffffff"), Answer::Text("0x0009e000")),
    ),
    (
        "create_push",
        both(Answer::Text("UpdateCreateInventoryItem (UDP)")),
    ),
    (
        "create_push_detail",
        both(Answer::Text(
            "UpdateCreateInventoryItem: the notecard in F, with a callback id",
        )),
    ),
    ("flags_before", both(Answer::Text("0x00000000"))),
    ("flags_f_step", each(Answer::Count(1), Answer::Count(0))),
    ("flags_push", both(Answer::Text("nothing"))),
    ("flags_push_detail", both(Answer::Text("nothing"))),
    ("flags_read_back", both(Answer::Text("0x00000000"))),
    ("link_folder_step", each(Answer::Count(0), Answer::Count(1))),
    ("link_listed", each(Answer::Flag(false), Answer::Flag(true))),
    (
        "link_push",
        each(
            Answer::Text("AlertMessage (UDP)"),
            Answer::Text("UpdateCreateInventoryItem (UDP)"),
        ),
    ),
    (
        "link_push_detail",
        each(
            Answer::Text("alert: Cannot create requested inventory."),
            Answer::Text("UpdateCreateInventoryItem: the link in G"),
        ),
    ),
    (
        "link_shape",
        each(
            Answer::Text("no link"),
            Answer::Text("type 24, names the notecard"),
        ),
    ),
    ("move_item_at", both(Answer::Text("G"))),
    ("move_item_f_step", both(Answer::Count(1))),
    ("move_item_g_step", both(Answer::Count(1))),
    (
        "move_item_push",
        both(Answer::Text("BulkUpdateInventory (event queue)")),
    ),
    (
        "move_item_push_detail",
        both(Answer::Text(
            "BulkUpdateInventory: folders [] items [the copy]",
        )),
    ),
    (
        "nocopy_accept_push",
        each(Answer::Any, Answer::Text("ImprovedInstantMessage (UDP)")),
    ),
    (
        "nocopy_accept_push_detail",
        each(
            Answer::Any,
            Answer::Text("ImprovedInstantMessage: InventoryAccepted"),
        ),
    ),
    (
        "nocopy_after_accept",
        each(Answer::Any, Answer::Text("gone")),
    ),
    ("nocopy_at_offer", each(Answer::Any, Answer::Text("gone"))),
    (
        "nocopy_filed_in",
        each(Answer::Any, Answer::Text("Notecards")),
    ),
    (
        "nocopy_offer_push",
        each(Answer::Any, Answer::Text("nothing")),
    ),
    (
        "nocopy_offer_push_detail",
        each(Answer::Any, Answer::Text("nothing")),
    ),
    (
        "nocopy_owner_can_copy",
        each(Answer::Any, Answer::Text("false")),
    ),
    (
        "nocopy_receive_push",
        each(Answer::Any, Answer::Text("nothing")),
    ),
    (
        "nocopy_receive_push_detail",
        each(Answer::Any, Answer::Text("nothing")),
    ),
    ("nocopy_received", each(Answer::Any, Answer::Flag(true))),
    ("nocopy_returned", each(Answer::Any, Answer::Flag(true))),
    ("purge_outside_folder_at", both(Answer::Text("the root"))),
    ("purge_outside_folder_step", both(Answer::Count(0))),
    ("purge_outside_items_left", both(Answer::Text("all"))),
    ("purge_outside_push", both(Answer::Text("nothing"))),
    ("purge_outside_push_detail", both(Answer::Text("nothing"))),
    ("purge_trashed_folder_at", both(Answer::Text("the Trash"))),
    ("purge_trashed_folder_step", both(Answer::Count(1))),
    ("purge_trashed_items_left", both(Answer::Text("none"))),
    ("purge_trashed_push", both(Answer::Text("nothing"))),
    ("purge_trashed_push_detail", both(Answer::Text("nothing"))),
    ("remove_folder_outside_at", both(Answer::Text("the root"))),
    ("remove_folder_outside_push", both(Answer::Text("nothing"))),
    (
        "remove_folder_outside_push_detail",
        both(Answer::Text("nothing")),
    ),
    ("remove_folder_outside_root_step", both(Answer::Count(0))),
    ("remove_folder_outside_trash_step", both(Answer::Count(0))),
    ("remove_item_outside_at", both(Answer::Text("gone"))),
    ("remove_item_outside_holder_step", both(Answer::Count(1))),
    ("remove_item_outside_push", both(Answer::Text("nothing"))),
    (
        "remove_item_outside_push_detail",
        both(Answer::Text("nothing")),
    ),
    ("remove_item_outside_trash_step", both(Answer::Count(0))),
    ("remove_item_trashed_at", both(Answer::Text("gone"))),
    ("remove_item_trashed_holder_step", both(Answer::Count(0))),
    ("remove_item_trashed_push", both(Answer::Text("nothing"))),
    (
        "remove_item_trashed_push_detail",
        both(Answer::Text("nothing")),
    ),
    ("remove_item_trashed_trash_step", both(Answer::Count(1))),
    (
        "remove_objects_outside_folder_at",
        both(Answer::Text("the root")),
    ),
    ("remove_objects_outside_item_at", both(Answer::Text("gone"))),
    ("remove_objects_outside_push", both(Answer::Text("nothing"))),
    (
        "remove_objects_outside_push_detail",
        both(Answer::Text("nothing")),
    ),
    (
        "remove_objects_trashed_folder_at",
        both(Answer::Text("gone")),
    ),
    ("remove_objects_trashed_item_at", both(Answer::Text("gone"))),
    ("remove_objects_trashed_push", both(Answer::Text("nothing"))),
    (
        "remove_objects_trashed_push_detail",
        both(Answer::Text("nothing")),
    ),
    ("reverse_received", both(Answer::Text("filed"))),
    ("save_answer", both(Answer::Text("saved"))),
    ("update_described", both(Answer::Flag(true))),
    ("update_f_step", both(Answer::Count(1))),
    ("update_push", both(Answer::Text("nothing"))),
    ("update_push_detail", both(Answer::Text("nothing"))),
    ("update_renamed", both(Answer::Flag(true))),
];

/// The census of inventory mutations.
#[derive(Debug)]
pub struct InventoryMutations;

impl GridTest for InventoryMutations {
    fn name(&self) -> &'static str {
        "inventory-mutations"
    }

    fn description(&self) -> &'static str {
        "How each grid takes an inventory change: every item and folder operation over UDP \
         and AIS3, what it pushes after each, the Trash gating, and a no-copy give"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi, Grid::FakeSl, Grid::FakeOpensim]
    }

    fn accounts(&self) -> u8 {
        2
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();
            let mut metrics = Metrics::new();
            let outcome = match ctx.primary_and_secondary() {
                Some((a, b)) => census(a, b, &mut metrics).await,
                None => Err(TestFailure::Assertion(
                    "two-account census ran without a secondary".to_owned(),
                )),
            };
            ctx.metrics().merge(metrics.clone());
            outcome.and_then(|()| hold_to_measured(grid, &metrics, MEASURED, SOURCE))
        })
    }
}

/// The run's names for the ids its metrics mention.
#[derive(Debug, Default, Clone)]
struct Roles {
    /// Every id the run has named.
    names: BTreeMap<Uuid, String>,
}

impl Roles {
    /// Names `id` as `role`.
    fn name(&mut self, id: Uuid, role: &str) {
        let _previous = self.names.insert(id, role.to_owned());
    }

    /// The role of `id`: its name, `nil`, or `another` for an id the run
    /// never named.
    fn of(&self, id: Uuid) -> String {
        if id.is_nil() {
            return "nil".to_owned();
        }
        self.names
            .get(&id)
            .cloned()
            .unwrap_or_else(|| "another".to_owned())
    }

    /// The roles of `ids`, sorted and `,`-joined.
    fn list(&self, ids: impl IntoIterator<Item = Uuid>) -> String {
        let mut roles: Vec<String> = ids.into_iter().map(|id| self.of(id)).collect();
        roles.sort_unstable();
        roles.join(",")
    }
}

/// What one window of listening saw.
#[derive(Debug, Default)]
struct Heard {
    /// The events, in arrival order.
    events: Vec<Event>,
    /// The inventory datagrams that arrived on the root circuit, by name.
    datagrams: BTreeSet<&'static str>,
    /// The event-queue events the client does not read.
    unhandled: BTreeSet<String>,
}

impl Heard {
    /// Adds what `other` saw.
    fn extend(&mut self, other: Self) {
        self.events.extend(other.events);
        self.datagrams.extend(other.datagrams);
        self.unhandled.extend(other.unhandled);
    }

    /// The messages that arrived, by name and road; `nothing` for none.
    fn push(&self) -> String {
        let mut seen: BTreeSet<String> = self
            .datagrams
            .iter()
            .map(|name| format!("{name} (UDP)"))
            .collect();
        for event in &self.events {
            if let Some(name) = message_of(event)
                && !self.datagrams.contains(name)
            {
                let _new = seen.insert(format!("{name} (event queue)"));
            }
        }
        seen.extend(
            self.unhandled
                .iter()
                .map(|name| format!("{name} (event queue, unhandled)")),
        );
        if seen.is_empty() {
            "nothing".to_owned()
        } else {
            seen.into_iter().collect::<Vec<_>>().join(", ")
        }
    }

    /// What each inventory message said, ids named by `roles`, sorted and
    /// `; `-joined; `nothing` for none.
    fn detail(&self, roles: &Roles) -> String {
        let mut lines: Vec<String> = self
            .events
            .iter()
            .filter_map(|event| describe(event, roles))
            .collect();
        lines.sort_unstable();
        lines.dedup();
        if lines.is_empty() {
            "nothing".to_owned()
        } else {
            lines.join("; ")
        }
    }

    /// The items the window's creation and bulk-update messages named, with
    /// their names.
    fn items(&self) -> Vec<InventoryItem> {
        self.events
            .iter()
            .flat_map(|event| match event {
                Event::InventoryItemCreated { item, .. } => vec![item.clone()],
                Event::InventoryBulkUpdate { items, .. } => items.clone(),
                _ => Vec::new(),
            })
            .collect()
    }
}

/// The message an inventory event was read from, for the events a push
/// consists of.
const fn message_of(event: &Event) -> Option<&'static str> {
    match event {
        Event::InventoryItemCreated { .. } => Some("UpdateCreateInventoryItem"),
        Event::InventoryBulkUpdate { .. } => Some("BulkUpdateInventory"),
        Event::InventoryItemsRemoved { .. } => Some("RemoveInventoryItem"),
        Event::InventoryFoldersRemoved { .. } => Some("RemoveInventoryFolder"),
        Event::InventoryObjectsRemoved { .. } => Some("RemoveInventoryObjects"),
        Event::InventoryItemsMoved { .. } => Some("MoveInventoryItem"),
        Event::AlertMessage { .. } => Some("AlertMessage"),
        Event::AgentAlertMessage { .. } => Some("AgentAlertMessage"),
        Event::InstantMessageReceived(_) => Some("ImprovedInstantMessage"),
        _ => None,
    }
}

/// What an inventory event said, ids named by `roles`.
fn describe(event: &Event, roles: &Roles) -> Option<String> {
    let folders = |folders: &[sl_client_tokio::InventoryFolder]| {
        roles.list(folders.iter().map(|folder| folder.folder_id.uuid()))
    };
    let items = |items: &[InventoryItem]| roles.list(items.iter().map(|item| item.item_id.uuid()));
    Some(match event {
        Event::InventoryItemCreated {
            sim_approved,
            callback_id,
            item,
            ..
        } => format!(
            "UpdateCreateInventoryItem: {} in {}{}{}",
            roles.of(item.item_id.uuid()),
            roles.of(item.folder_id.uuid()),
            if *sim_approved { "" } else { ", not approved" },
            if callback_id.is_some() {
                ", with a callback id"
            } else {
                ""
            },
        ),
        Event::InventoryBulkUpdate {
            folders: bulk_folders,
            items: bulk_items,
            item_callbacks,
            ..
        } => format!(
            "BulkUpdateInventory: folders [{}] items [{}]{}",
            folders(bulk_folders),
            items(bulk_items),
            if item_callbacks.is_empty() {
                ""
            } else {
                ", with callback ids"
            },
        ),
        Event::InventoryItemsRemoved { items: removed } => format!(
            "RemoveInventoryItem: [{}]",
            roles.list(removed.iter().map(|item| item.uuid()))
        ),
        Event::InventoryFoldersRemoved { folders: removed } => format!(
            "RemoveInventoryFolder: [{}]",
            roles.list(removed.iter().map(|folder| folder.uuid()))
        ),
        Event::InventoryObjectsRemoved {
            folders: removed_folders,
            items: removed_items,
        } => format!(
            "RemoveInventoryObjects: folders [{}] items [{}]",
            roles.list(removed_folders.iter().map(|folder| folder.uuid())),
            roles.list(removed_items.iter().map(|item| item.uuid())),
        ),
        Event::InventoryItemsMoved { moves, .. } => format!(
            "MoveInventoryItem: [{}]",
            moves
                .iter()
                .map(|moved| format!(
                    "{} to {}",
                    roles.of(moved.item.uuid()),
                    roles.of(moved.folder.uuid())
                ))
                .collect::<Vec<_>>()
                .join(",")
        ),
        Event::AlertMessage { message, .. } | Event::AgentAlertMessage { message, .. } => {
            format!("alert: {}", without_ids(message.trim()))
        }
        Event::InstantMessageReceived(im) => format!("ImprovedInstantMessage: {:?}", im.dialog),
        _ => return None,
    })
}

/// One item as the item fetch states it.
#[derive(Debug, Clone)]
struct ItemState {
    /// The folder it is in.
    parent: Option<Uuid>,
    /// Its name.
    name: String,
    /// Its description.
    description: String,
    /// Its flags.
    flags: Option<i64>,
    /// Its base and owner permission masks.
    masks: (Option<i64>, Option<i64>),
    /// Its next-owner permission mask.
    next_mask: Option<i64>,
    /// Its asset id, where it names one.
    asset: Option<Uuid>,
    /// Its asset type.
    kind: Option<i64>,
    /// What a link names as its target.
    linked: Option<Uuid>,
}

/// An integer an LLSD value states, as an integer or a string of one.
fn int_of(value: &Llsd) -> Option<i64> {
    match value {
        Llsd::Integer(number) => Some(i64::from(*number)),
        Llsd::String(text) => text.trim().parse().ok(),
        Llsd::Binary(bytes) => Some(
            bytes
                .iter()
                .fold(0_i64, |acc, byte| acc.wrapping_shl(8) | i64::from(*byte)),
        ),
        _other => None,
    }
}

/// An item entry as an [`ItemState`].
fn item_state(item: &Llsd) -> ItemState {
    let permissions = item.get("permissions");
    let mask = |key: &str| permissions.and_then(|map| map.get(key)).and_then(int_of);
    ItemState {
        parent: item.get("parent_id").and_then(uuid_of),
        name: item
            .get("name")
            .and_then(Llsd::as_str)
            .unwrap_or_default()
            .to_owned(),
        description: item
            .get("desc")
            .and_then(Llsd::as_str)
            .unwrap_or_default()
            .to_owned(),
        flags: item.get("flags").and_then(int_of),
        masks: (mask("base_mask"), mask("owner_mask")),
        next_mask: mask("next_owner_mask"),
        asset: item.get("asset_id").and_then(uuid_of),
        kind: item.get("type").and_then(int_of),
        linked: item
            .get("linked_id")
            .or_else(|| item.get("asset_id"))
            .and_then(uuid_of),
    }
}

/// A permission mask as a metric.
fn mask_text(mask: Option<i64>) -> String {
    mask.map_or_else(|| "none".to_owned(), |mask| format!("{mask:#010x}"))
}

/// Everything the legs share beyond the fetch census's [`Census`].
struct Run<'a, 'b> {
    /// Avatar A's census handle.
    census: Census<'a>,
    /// Avatar B's session.
    b: &'b mut Session,
    /// The roles of the run's ids.
    roles: Roles,
    /// Avatar A's inventory root.
    root: Uuid,
    /// Avatar A's Trash.
    trash: Uuid,
    /// The folders the run made, for the clean-up.
    made: Made,
    /// The run's notecard as the grid made it, renamed by the UDP update leg.
    note: Option<InventoryItem>,
}

impl Run<'_, '_> {
    /// The index of the next diagnostic A records.
    fn now(&self) -> usize {
        self.census.a.diagnostics().len()
    }

    /// Lets what an earlier step made A arrive and be dropped, then marks
    /// where the next operation's window begins: a push a setup step caused
    /// would otherwise be read as the next operation's.
    async fn begin(&mut self) -> usize {
        let _drained = self
            .census
            .a
            .wait_for(DRAIN_WINDOW, |_event| None::<()>)
            .await;
        self.now()
    }

    /// Listens to A for `window`, then reads what the circuit probe saw since
    /// `mark`.
    async fn listen(&mut self, mark: usize, window: Duration) -> Heard {
        listen_on(self.census.a, mark, window).await
    }

    /// Sends a raw UDP message from A.
    async fn send_raw(&self, message: AnyMessage) -> Result<(), TestFailure> {
        self.census.send_raw(message).await
    }

    /// Records the pushes `heard` under `leg`.
    fn record_push(&self, metrics: &mut Metrics, leg: &str, heard: &Heard) {
        metrics.set(&format!("{leg}_push"), heard.push());
        metrics.set(&format!("{leg}_push_detail"), heard.detail(&self.roles));
    }

    /// The items the folder fetch lists in `folder`, by name.
    async fn named_items(&self, folder: Uuid) -> Vec<(String, Uuid)> {
        let raw = self
            .census
            .fetch_folders(FID2, self.census.agent, &[folder])
            .await;
        listed_folders(raw.body.as_ref())
            .flat_map(|entry| {
                entry
                    .get("items")
                    .and_then(Llsd::as_array)
                    .unwrap_or_default()
            })
            .filter_map(|item| {
                Some((
                    item.get("name")?.as_str()?.to_owned(),
                    item.get("item_id").and_then(uuid_of)?,
                ))
            })
            .collect()
    }

    /// Polls `folder` until it lists an item named `name`; its id.
    async fn item_named(&self, folder: Uuid, name: &str) -> Option<Uuid> {
        let started = Instant::now();
        loop {
            if let Some((_name, id)) = self
                .named_items(folder)
                .await
                .into_iter()
                .find(|(listed, _id)| listed == name)
            {
                return Some(id);
            }
            if started.elapsed() > SETTLE {
                return None;
            }
            tokio::time::sleep(POLL).await;
        }
    }

    /// The item fetch's statement of `item`, or `None` where it states none.
    async fn item(&self, item: Uuid) -> Option<ItemState> {
        let raw = self.census.fetch_item(FI2, self.census.agent, item).await;
        raw.body
            .as_ref()
            .and_then(|body| body.get("items"))
            .and_then(Llsd::as_array)
            .and_then(<[Llsd]>::first)
            .map(item_state)
    }

    /// Where `item` is: the role of its folder, or `gone`.
    async fn item_whereabouts(&self, item: Uuid) -> String {
        match self.item(item).await {
            None => "gone".to_owned(),
            Some(state) => state
                .parent
                .map_or_else(|| "no parent".to_owned(), |parent| self.roles.of(parent)),
        }
    }

    /// Polls until `item`'s whereabouts are no longer `from`, or [`SETTLE`]
    /// passes; where it is then.
    async fn item_settle(&self, item: Uuid, from: &str) -> String {
        let started = Instant::now();
        loop {
            let now = self.item_whereabouts(item).await;
            if now != from || started.elapsed() > SETTLE {
                return now;
            }
            tokio::time::sleep(POLL).await;
        }
    }

    /// Where `folder` is: `the Trash` or `the root` where it is listed under
    /// one, `gone` where the fetch answers it with the empty entry of a folder
    /// nobody has, `elsewhere` otherwise.
    async fn folder_whereabouts(&self, folder: Uuid) -> String {
        for (parent, word) in [(self.trash, "the Trash"), (self.root, "the root")] {
            if self
                .census
                .children(parent)
                .await
                .iter()
                .any(|(_name, id)| *id == folder)
            {
                return word.to_owned();
            }
        }
        let versions = self.census.versions(&[folder]).await;
        match versions.get(&folder) {
            Some(0) | None => "gone".to_owned(),
            Some(_version) => "elsewhere".to_owned(),
        }
    }

    /// Polls until `folder`'s whereabouts are no longer `from`, or
    /// [`SETTLE`] passes; where it is then.
    async fn folder_settle(&self, folder: Uuid, from: &str) -> String {
        let started = Instant::now();
        loop {
            let now = self.folder_whereabouts(folder).await;
            if now != from || started.elapsed() > SETTLE {
                return now;
            }
            tokio::time::sleep(POLL).await;
        }
    }

    /// How many items the folder fetch lists in `folder`.
    async fn item_count(&self, folder: Uuid) -> usize {
        self.named_items(folder).await.len()
    }

    /// Makes a run folder named `word` under `parent` over UDP, and names it
    /// `role`; its id, where the grid listed it.
    async fn make_folder(
        &mut self,
        parent: Uuid,
        word: &str,
        role: &str,
    ) -> Result<Option<Uuid>, TestFailure> {
        let id = Uuid::new_v4();
        let name = self.census.name(word);
        self.census
            .a
            .send(Command::CreateInventoryFolder {
                folder_id: InventoryFolderKey::from(id),
                parent_id: InventoryFolderKey::from(parent),
                folder_type: FolderType::None,
                name: name.clone(),
            })
            .await?;
        let listed = self
            .census
            .settle(parent, |children| {
                children.iter().any(|(listed, _id)| *listed == name)
            })
            .await;
        if !listed {
            return Ok(None);
        }
        // The id the grid gave it: Second Life's AIS3, which the client's
        // create takes there, mints its own.
        let Some(id) = self
            .census
            .children(parent)
            .await
            .into_iter()
            .find(|(listed, _id)| *listed == name)
            .map(|(_name, id)| id)
        else {
            return Ok(None);
        };
        self.roles.name(id, role);
        self.made.folders.push(id);
        Ok(Some(id))
    }

    /// Makes a notecard named `word` in `folder` with `CreateInventoryItem`,
    /// named `role`; its id and the window's pushes.
    async fn make_notecard(
        &mut self,
        folder: Uuid,
        word: &str,
        role: &str,
    ) -> Result<(Option<InventoryItem>, Heard), TestFailure> {
        let name = self.census.name(word);
        let mark = self.begin().await;
        self.census
            .a
            .send(Command::CreateInventoryItem(NewInventoryItem {
                folder_id: InventoryFolderKey::from(folder),
                transaction_id: Uuid::nil(),
                next_owner_mask: NEXT_OWNER_FULL,
                asset_type: AssetType::Notecard,
                inv_type: InventoryType::Notecard,
                wearable_type: WearableType::Shape,
                name: name.clone(),
                description: "sl-conformance inventory-mutations".to_owned(),
            }))
            .await?;
        let mut heard = self.listen(mark, PUSH_WINDOW).await;
        // The answer has taken longer than the push window on Second Life.
        let started = Instant::now();
        while !heard.items().iter().any(|item| item.name == name)
            && started.elapsed() < HTTP_TIMEOUT
        {
            heard.extend(self.listen(self.now(), LATE_WINDOW).await);
        }
        let item = heard.items().into_iter().find(|item| item.name == name);
        if let Some(item) = &item {
            self.roles.name(item.item_id.uuid(), role);
        }
        Ok((item, heard))
    }

    /// A raw `CopyInventoryItem`-made copy of `item` named `word` in `folder`,
    /// named `role`; its id and the window's pushes.
    async fn copy(
        &mut self,
        item: Uuid,
        folder: Uuid,
        word: &str,
        role: &str,
    ) -> Result<(Option<Uuid>, Heard), TestFailure> {
        let name = self.census.name(word);
        let mark = self.begin().await;
        self.census
            .a
            .send(Command::CopyInventoryItem {
                old_agent_id: AgentKey::from(self.census.agent),
                old_item_id: InventoryKey::from(item),
                new_folder_id: InventoryFolderKey::from(folder),
                new_name: name.clone(),
            })
            .await?;
        let mut heard = self.listen(mark, PUSH_WINDOW).await;
        let id = self.item_named(folder, &name).await;
        heard.extend(self.listen(self.now(), LATE_WINDOW).await);
        if let Some(id) = id {
            self.roles.name(id, role);
        }
        Ok((id, heard))
    }

    /// A raw `MoveInventoryItem` of `item` into `folder`.
    async fn move_item(&self, item: Uuid, folder: Uuid) -> Result<(), TestFailure> {
        self.send_raw(AnyMessage::MoveInventoryItem(MoveInventoryItem {
            agent_data: MoveInventoryItemAgentDataBlock {
                agent_id: self.census.agent,
                session_id: self.census.session_id,
                stamp: false,
            },
            inventory_data: vec![MoveInventoryItemInventoryDataBlock {
                item_id: item,
                folder_id: folder,
                new_name: Vec::new(),
            }],
        }))
        .await
    }

    /// A raw `RemoveInventoryItem` of `item`.
    async fn remove_item(&self, item: Uuid) -> Result<(), TestFailure> {
        self.send_raw(AnyMessage::RemoveInventoryItem(RemoveInventoryItem {
            agent_data: RemoveInventoryItemAgentDataBlock {
                agent_id: self.census.agent,
                session_id: self.census.session_id,
            },
            inventory_data: vec![RemoveInventoryItemInventoryDataBlock { item_id: item }],
        }))
        .await
    }

    /// A raw `LinkInventoryItem` to the notecard `target` in `folder`, named
    /// `name`.
    async fn link(&self, target: Uuid, folder: Uuid, name: &str) -> Result<(), TestFailure> {
        self.send_raw(AnyMessage::LinkInventoryItem(LinkInventoryItem {
            agent_data: LinkInventoryItemAgentDataBlock {
                agent_id: self.census.agent,
                session_id: self.census.session_id,
            },
            inventory_block: LinkInventoryItemInventoryBlockBlock {
                callback_id: 0,
                folder_id: folder,
                transaction_id: Uuid::new_v4(),
                old_item_id: target,
                r#type: AT_LINK,
                inv_type: IT_NOTECARD,
                name: with_nul(name),
                description: with_nul(""),
            },
        }))
        .await
    }

    /// The versions of `folders` through the folder fetch.
    async fn versions(&self, folders: &[Uuid]) -> HashMap<Uuid, i32> {
        self.census.versions(folders).await
    }
}

/// Saves a short body into the notecard `item` through the client's
/// `UpdateInventoryAsset`; `saved`, or why not.
pub(crate) async fn save_text(session: &mut Session, item: Uuid) -> Result<String, TestFailure> {
    let text = "sl-conformance inventory-mutations\n";
    let body = format!(
        "Linden text version 2\n{{\nLLEmbeddedItems version 1\n{{\ncount 0\n}}\nText length {}\n{}}}\n",
        text.len(),
        text,
    );
    session
        .send(Command::UpdateInventoryAsset {
            location: AssetUpdateLocation::AgentInventory {
                item_id: InventoryKey::from(item),
            },
            asset_type: UpdatableAssetType::Notecard,
            data: body.into_bytes(),
        })
        .await?;
    let outcome = session
        .wait_for(SAVE_TIMEOUT, |event| match event {
            Event::AssetUploaded { new_asset, .. } => Some(if new_asset.is_nil() {
                "saved a nil asset".to_owned()
            } else {
                "saved".to_owned()
            }),
            Event::AssetUploadFailed { reason } => Some(format!("failed: {}", without_ids(reason))),
            _ => None,
        })
        .await;
    match outcome {
        Ok(answer) => Ok(answer),
        Err(TestFailure::Timeout(_)) => Ok("no answer".to_owned()),
        Err(other) => Err(other),
    }
}

/// `text` as a NUL-terminated wire string, as the reference packs one.
fn with_nul(text: &str) -> Vec<u8> {
    let mut bytes = text.as_bytes().to_vec();
    bytes.push(0);
    bytes
}

/// Listens to `session` for `window`, then reads what the circuit probe saw
/// since `mark`.
async fn listen_on(session: &mut Session, mark: usize, window: Duration) -> Heard {
    let mut events = Vec::new();
    let _idle = session
        .wait_for(window, |event| {
            if message_of(event).is_some() {
                events.push(event.clone());
            }
            None::<()>
        })
        .await;
    let mut heard = Heard {
        events,
        ..Heard::default()
    };
    for diagnostic in session.diagnostics().iter().skip(mark) {
        match diagnostic {
            Diagnostic::Datagram {
                name: Some(name),
                child: false,
                ..
            } if name.contains("Inventory")
                || matches!(
                    *name,
                    "AlertMessage"
                        | "AgentAlertMessage"
                        | "FeatureDisabled"
                        | "ImprovedInstantMessage"
                ) =>
            {
                let _new = heard.datagrams.insert(name);
            }
            Diagnostic::UnknownCapsEvent { message } => {
                let _new = heard.unhandled.insert(message.clone());
            }
            _other => {}
        }
    }
    heard
}

/// The value of a version step as a metric.
fn read(versions: &HashMap<Uuid, i32>, folder: Uuid) -> Option<i32> {
    versions.get(&folder).copied()
}

/// Runs every leg, recording into `metrics`.
async fn census(
    a: &mut Session,
    b: &mut Session,
    metrics: &mut Metrics,
) -> Result<(), TestFailure> {
    a.wait_for_region(REGION_TIMEOUT).await?;
    b.wait_for_region(REGION_TIMEOUT).await?;
    let started = Instant::now();
    while [FID2, FI2].iter().any(|name| a.cap(name).is_none()) && started.elapsed() < CAP_WAIT {
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    let agent = a
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?
        .uuid();
    let session_id = a
        .session_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no session id".to_owned()))?;
    let login = a
        .login_success()
        .cloned()
        .ok_or_else(|| TestFailure::Assertion("no login answer kept".to_owned()))?;
    let root = login
        .inventory_root
        .ok_or_else(|| TestFailure::Assertion("login named no inventory root".to_owned()))?
        .uuid();
    let system = |kind: FolderType| {
        login
            .inventory_skeleton
            .iter()
            .find(|folder| FolderType::from_code(folder.type_default) == kind)
            .map(|folder| folder.folder_id.uuid())
    };
    let trash = system(FolderType::Trash)
        .ok_or_else(|| TestFailure::Assertion("the skeleton named no Trash".to_owned()))?;
    let notecards = system(FolderType::Notecard)
        .ok_or_else(|| TestFailure::Assertion("the skeleton named no Notecards".to_owned()))?;
    let cof = system(FolderType::CurrentOutfit);
    let http = reqwest::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .pool_max_idle_per_host(0)
        .build()
        .map_err(|error| TestFailure::State(format!("building an HTTP client failed: {error}")))?;
    let tag: String = Uuid::new_v4()
        .simple()
        .to_string()
        .chars()
        .take(6)
        .collect();
    let mut roles = Roles::default();
    roles.name(root, "the root");
    roles.name(trash, "the Trash");
    roles.name(notecards, "Notecards");
    if let Some(cof) = cof {
        roles.name(cof, "the COF");
    }
    let mut run = Run {
        census: Census {
            a,
            agent,
            session_id,
            http,
            tag,
        },
        b,
        roles,
        root,
        trash,
        made: Made::default(),
        note: None,
    };
    run.census
        .a
        .send(Command::ProbeCircuits(CircuitProbe::Observe))
        .await?;
    let outcome = legs(&mut run, notecards, cof, &login, metrics).await;
    run.census
        .a
        .send(Command::ProbeCircuits(CircuitProbe::Off))
        .await?;
    outcome
}

/// Every leg, in order, then the clean-up.
async fn legs(
    run: &mut Run<'_, '_>,
    notecards: Uuid,
    cof: Option<Uuid>,
    login: &sl_client_tokio::LoginSuccess,
    metrics: &mut Metrics,
) -> Result<(), TestFailure> {
    let ais_road = run.census.a.cap(AIS).is_some();
    // Folders and notecards an earlier run left behind go with this run's
    // clean-up.
    let view = if ais_road { View::Ais } else { View::Fetch };
    for folder in [run.root, run.trash] {
        let leftovers: Vec<Uuid> = run
            .census
            .listing(folder, view)
            .await
            .into_iter()
            .filter(|(name, _id)| name.starts_with("conf-inv-"))
            .map(|(_name, id)| id)
            .collect();
        run.made.folders.extend(leftovers);
    }
    let stray: Vec<InventoryKey> = run
        .named_items(notecards)
        .await
        .into_iter()
        .filter(|(name, _id)| name.starts_with("conf-inv-"))
        .map(|(_name, id)| InventoryKey::from(id))
        .collect();
    if !stray.is_empty() {
        run.census
            .a
            .send(Command::RemoveInventoryItems(stray))
            .await?;
    }

    let outcome = async {
        items(run, metrics, cof).await?;
        removals(run, metrics).await?;
        ais(run, metrics, login).await?;
        give(run, metrics, notecards).await
    }
    .await;
    let cleaned = clean_up(&run.census, &run.made, run.root, run.trash, ais_road).await;
    outcome.and(cleaned)
}

/// Leg 1: the item operations over UDP, in run folders `F` and `G`.
async fn items(
    run: &mut Run<'_, '_>,
    metrics: &mut Metrics,
    cof: Option<Uuid>,
) -> Result<(), TestFailure> {
    let root = run.root;
    let folder_f = run.make_folder(root, "f", "F").await?;
    let folder_g = run.make_folder(root, "g", "G").await?;
    let (Some(folder_f), Some(folder_g)) = (folder_f, folder_g) else {
        return Err(TestFailure::Assertion(
            "the grid listed neither run folder made over UDP".to_owned(),
        ));
    };

    // CreateInventoryItem.
    let before = run.versions(&[folder_f]).await;
    let (note, heard) = run.make_notecard(folder_f, "note", "the notecard").await?;
    run.record_push(metrics, "create", &heard);
    let after = run.versions(&[folder_f]).await;
    metrics.set("create_answered", note.is_some());
    metrics.set(
        "create_f_step",
        step(read(&before, folder_f), read(&after, folder_f)),
    );
    let Some(note) = note else {
        return Err(TestFailure::Assertion(
            "the grid made no notecard for CreateInventoryItem".to_owned(),
        ));
    };
    let note_id = note.item_id.uuid();
    let state = run.item(note_id).await;
    metrics.set(
        "create_base_mask",
        mask_text(state.as_ref().and_then(|state| state.masks.0)),
    );
    metrics.set(
        "create_owner_mask",
        mask_text(state.as_ref().and_then(|state| state.masks.1)),
    );
    metrics.set(
        "create_asset",
        match state.as_ref().and_then(|state| state.asset) {
            None => "none",
            Some(asset) if asset.is_nil() => "nil",
            Some(_asset) => "an asset",
        },
    );

    // UpdateInventoryItem over UDP: a rename and a new description.
    let before = run.versions(&[folder_f]).await;
    let renamed = run.census.name("renamed");
    let mut updated = note.clone();
    updated.name.clone_from(&renamed);
    "changed".clone_into(&mut updated.description);
    run.note = Some(updated.clone());
    let mark = run.begin().await;
    // Raw, as the reference packs it: the client's own command takes AIS3
    // where the region grants it.
    let message = sl_client_tokio::update_inventory_item_message(
        AgentKey::from(run.census.agent),
        run.census.session_id,
        &updated,
        Uuid::nil(),
        InventoryCallbackId::new(0),
    )
    .map_err(|error| TestFailure::State(format!("encoding UpdateInventoryItem failed: {error}")))?;
    run.send_raw(message).await?;
    let mut heard = run.listen(mark, PUSH_WINDOW).await;
    let started = Instant::now();
    let mut state = run.item(note_id).await;
    while state.as_ref().is_none_or(|state| state.name != renamed) && started.elapsed() < SETTLE {
        tokio::time::sleep(POLL).await;
        state = run.item(note_id).await;
    }
    heard.extend(run.listen(run.now(), LATE_WINDOW).await);
    run.record_push(metrics, "update", &heard);
    let after = run.versions(&[folder_f]).await;
    metrics.set(
        "update_renamed",
        state.as_ref().is_some_and(|state| state.name == renamed),
    );
    metrics.set(
        "update_described",
        state
            .as_ref()
            .is_some_and(|state| state.description == "changed"),
    );
    metrics.set(
        "update_f_step",
        step(read(&before, folder_f), read(&after, folder_f)),
    );

    // ChangeInventoryItemFlags.
    let before = run.versions(&[folder_f]).await;
    let flags_before = run.item(note_id).await.and_then(|state| state.flags);
    let mark = run.begin().await;
    run.census
        .a
        .send(Command::ChangeInventoryItemFlags {
            item_id: InventoryKey::from(note_id),
            flags: FLAGS_SET,
        })
        .await?;
    let mut heard = run.listen(mark, PUSH_WINDOW).await;
    let started = Instant::now();
    let mut flags = run.item(note_id).await.and_then(|state| state.flags);
    while flags != Some(i64::from(FLAGS_SET)) && started.elapsed() < SETTLE {
        tokio::time::sleep(POLL).await;
        flags = run.item(note_id).await.and_then(|state| state.flags);
    }
    heard.extend(run.listen(run.now(), LATE_WINDOW).await);
    run.record_push(metrics, "flags", &heard);
    let after = run.versions(&[folder_f]).await;
    metrics.set("flags_before", mask_text(flags_before));
    metrics.set("flags_read_back", mask_text(flags));
    metrics.set(
        "flags_f_step",
        step(read(&before, folder_f), read(&after, folder_f)),
    );

    // A body for the notecard, so it names an asset: Second Life makes a
    // notecard with none, and neither links to nor gives such an item.
    let saved = save_text(run.census.a, note_id).await?;
    metrics.set("save_answer", saved.clone());

    // CopyInventoryItem.
    let before = run.versions(&[folder_f]).await;
    let (copy, heard) = run.copy(note_id, folder_f, "copy", "the copy").await?;
    run.record_push(metrics, "copy", &heard);
    let after = run.versions(&[folder_f]).await;
    metrics.set("copy_listed", copy.is_some());
    metrics.set(
        "copy_f_step",
        step(read(&before, folder_f), read(&after, folder_f)),
    );

    // MoveInventoryItem: the copy into G.
    if let Some(copy) = copy {
        let before = run.versions(&[folder_f, folder_g]).await;
        let mark = run.begin().await;
        run.move_item(copy, folder_g).await?;
        let mut heard = run.listen(mark, PUSH_WINDOW).await;
        let at = run.item_settle(copy, "F").await;
        heard.extend(run.listen(run.now(), LATE_WINDOW).await);
        run.record_push(metrics, "move_item", &heard);
        let after = run.versions(&[folder_f, folder_g]).await;
        metrics.set("move_item_at", at);
        metrics.set(
            "move_item_f_step",
            step(read(&before, folder_f), read(&after, folder_f)),
        );
        metrics.set(
            "move_item_g_step",
            step(read(&before, folder_g), read(&after, folder_g)),
        );
    } else {
        for key in [
            "move_item_push",
            "move_item_push_detail",
            "move_item_at",
            "move_item_f_step",
            "move_item_g_step",
        ] {
            metrics.set(key, "no copy");
        }
    }

    // LinkInventoryItem over UDP, into G and into the COF.
    for (leg, folder) in [("link", Some(folder_g)), ("cof_link", cof)] {
        let Some(folder) = folder else {
            for part in ["push", "push_detail", "listed", "folder_step", "shape"] {
                metrics.set(&format!("{leg}_{part}"), "no folder");
            }
            continue;
        };
        let name = run.census.name(leg);
        let before = run.versions(&[folder]).await;
        let mark = run.begin().await;
        run.link(note_id, folder, &name).await?;
        let mut heard = run.listen(mark, PUSH_WINDOW).await;
        let link = run.item_named(folder, &name).await;
        heard.extend(run.listen(run.now(), LATE_WINDOW).await);
        let role = if leg == "link" {
            "the link"
        } else {
            "the COF link"
        };
        if let Some(link) = link {
            run.roles.name(link, role);
        }
        run.record_push(metrics, leg, &heard);
        let after = run.versions(&[folder]).await;
        metrics.set(&format!("{leg}_listed"), link.is_some());
        metrics.set(
            &format!("{leg}_folder_step"),
            step(read(&before, folder), read(&after, folder)),
        );
        let shape = match link {
            Some(link) => match run.item(link).await {
                Some(state) => format!(
                    "type {}, names {}",
                    state
                        .kind
                        .map_or_else(|| "none".to_owned(), |kind| kind.to_string()),
                    state
                        .linked
                        .map_or_else(|| "nothing".to_owned(), |id| run.roles.of(id))
                ),
                None => "not fetched".to_owned(),
            },
            None => "no link".to_owned(),
        };
        metrics.set(&format!("{leg}_shape"), shape);
        // A link left in the COF is removed again at once (UDP, which every
        // grid takes for a removal; the removal legs measure that).
        if leg == "cof_link"
            && let Some(link) = link
        {
            run.remove_item(link).await?;
            let _gone = run.item_settle(link, "the COF").await;
        }
    }
    Ok(())
}

/// Leg 2: the removals, outside the Trash and inside it.
async fn removals(run: &mut Run<'_, '_>, metrics: &mut Metrics) -> Result<(), TestFailure> {
    let root = run.root;
    let trash = run.trash;
    let Some(source) = run
        .roles
        .names
        .iter()
        .find(|(_id, role)| *role == "the notecard")
        .map(|(id, _role)| *id)
    else {
        return Err(TestFailure::Assertion(
            "no notecard to copy for the removals".to_owned(),
        ));
    };

    // RemoveInventoryItem outside the Trash, then inside it.
    let holder = run.make_folder(root, "r", "R").await?;
    for (leg, in_trash) in [
        ("remove_item_outside", false),
        ("remove_item_trashed", true),
    ] {
        let Some(holder) = holder else {
            for part in ["push", "push_detail", "at", "holder_step", "trash_step"] {
                metrics.set(&format!("{leg}_{part}"), "no folder");
            }
            continue;
        };
        let role = format!("the {leg} item");
        let (item, _heard) = run.copy(source, holder, leg, &role).await?;
        let Some(item) = item else {
            for part in ["push", "push_detail", "at", "holder_step", "trash_step"] {
                metrics.set(&format!("{leg}_{part}"), "no copy");
            }
            continue;
        };
        if in_trash {
            run.move_item(item, trash).await?;
            let _moved = run.item_settle(item, "R").await;
        }
        let from = run.item_whereabouts(item).await;
        let before = run.versions(&[holder, trash]).await;
        let mark = run.begin().await;
        run.remove_item(item).await?;
        let mut heard = run.listen(mark, PUSH_WINDOW).await;
        let at = run.item_settle(item, &from).await;
        heard.extend(run.listen(run.now(), LATE_WINDOW).await);
        run.record_push(metrics, leg, &heard);
        let after = run.versions(&[holder, trash]).await;
        metrics.set(&format!("{leg}_at"), at);
        metrics.set(
            &format!("{leg}_holder_step"),
            step(read(&before, holder), read(&after, holder)),
        );
        metrics.set(
            &format!("{leg}_trash_step"),
            step(read(&before, trash), read(&after, trash)),
        );
    }

    // RemoveInventoryFolder outside the Trash.
    let leg = "remove_folder_outside";
    match run.make_folder(root, "h", "H").await? {
        Some(folder) => {
            let before = run.versions(&[root, trash]).await;
            let mark = run.begin().await;
            run.send_raw(AnyMessage::RemoveInventoryFolder(RemoveInventoryFolder {
                agent_data: RemoveInventoryFolderAgentDataBlock {
                    agent_id: run.census.agent,
                    session_id: run.census.session_id,
                },
                folder_data: vec![RemoveInventoryFolderFolderDataBlock { folder_id: folder }],
            }))
            .await?;
            let mut heard = run.listen(mark, PUSH_WINDOW).await;
            let at = run.folder_settle(folder, "the root").await;
            heard.extend(run.listen(run.now(), LATE_WINDOW).await);
            run.record_push(metrics, leg, &heard);
            let after = run.versions(&[root, trash]).await;
            metrics.set(&format!("{leg}_at"), at);
            metrics.set(
                &format!("{leg}_root_step"),
                step(read(&before, root), read(&after, root)),
            );
            metrics.set(
                &format!("{leg}_trash_step"),
                step(read(&before, trash), read(&after, trash)),
            );
        }
        None => {
            for part in ["push", "push_detail", "at", "root_step", "trash_step"] {
                metrics.set(&format!("{leg}_{part}"), "no folder");
            }
        }
    }

    // PurgeInventoryDescendents outside the Trash, then inside it.
    let purged = run.make_folder(root, "p", "P").await?;
    for (leg, in_trash) in [("purge_outside", false), ("purge_trashed", true)] {
        let Some(folder) = purged else {
            for part in [
                "push",
                "push_detail",
                "items_left",
                "folder_at",
                "folder_step",
            ] {
                metrics.set(&format!("{leg}_{part}"), "no folder");
            }
            continue;
        };
        let role = format!("the {leg} item");
        let (_item, _heard) = run.copy(source, folder, leg, &role).await?;
        if in_trash && run.folder_whereabouts(folder).await != "the Trash" {
            run.census
                .a
                .send(Command::MoveInventoryFolder {
                    folder_id: InventoryFolderKey::from(folder),
                    parent_id: InventoryFolderKey::from(trash),
                })
                .await?;
            let _moved = run.folder_settle(folder, "the root").await;
        }
        let before = run.versions(&[folder]).await;
        let held = run.item_count(folder).await;
        let mark = run.begin().await;
        run.send_raw(AnyMessage::PurgeInventoryDescendents(
            PurgeInventoryDescendents {
                agent_data: PurgeInventoryDescendentsAgentDataBlock {
                    agent_id: run.census.agent,
                    session_id: run.census.session_id,
                },
                inventory_data: PurgeInventoryDescendentsInventoryDataBlock { folder_id: folder },
            },
        ))
        .await?;
        let mut heard = run.listen(mark, PUSH_WINDOW).await;
        let started = Instant::now();
        let mut left = run.item_count(folder).await;
        while left == held && started.elapsed() < SETTLE {
            tokio::time::sleep(POLL).await;
            left = run.item_count(folder).await;
        }
        heard.extend(run.listen(run.now(), LATE_WINDOW).await);
        run.record_push(metrics, leg, &heard);
        let after = run.versions(&[folder]).await;
        metrics.set(
            &format!("{leg}_items_left"),
            if left == 0 {
                "none".to_owned()
            } else if left == held {
                "all".to_owned()
            } else {
                "some".to_owned()
            },
        );
        metrics.set(
            &format!("{leg}_folder_at"),
            run.folder_whereabouts(folder).await,
        );
        metrics.set(
            &format!("{leg}_folder_step"),
            step(read(&before, folder), read(&after, folder)),
        );
    }

    // RemoveInventoryObjects, a folder and an item, outside the Trash and
    // inside it.
    for (leg, in_trash) in [
        ("remove_objects_outside", false),
        ("remove_objects_trashed", true),
    ] {
        let parts = ["push", "push_detail", "folder_at", "item_at"];
        let folder = run
            .make_folder(root, leg, &format!("the {leg} folder"))
            .await?;
        let holder = run
            .make_folder(root, &format!("{leg}-holder"), &format!("the {leg} holder"))
            .await?;
        let (Some(folder), Some(holder)) = (folder, holder) else {
            for part in parts {
                metrics.set(&format!("{leg}_{part}"), "no folder");
            }
            continue;
        };
        let (item, _heard) = run
            .copy(
                source,
                holder,
                &format!("{leg}-item"),
                &format!("the {leg} item"),
            )
            .await?;
        let Some(item) = item else {
            for part in parts {
                metrics.set(&format!("{leg}_{part}"), "no copy");
            }
            continue;
        };
        if in_trash {
            run.census
                .a
                .send(Command::MoveInventoryFolder {
                    folder_id: InventoryFolderKey::from(folder),
                    parent_id: InventoryFolderKey::from(trash),
                })
                .await?;
            run.move_item(item, trash).await?;
            let _moved = run.folder_settle(folder, "the root").await;
            let _moved = run.item_settle(item, &format!("the {leg} holder")).await;
        }
        let folder_from = run.folder_whereabouts(folder).await;
        let item_from = run.item_whereabouts(item).await;
        let mark = run.begin().await;
        run.send_raw(AnyMessage::RemoveInventoryObjects(RemoveInventoryObjects {
            agent_data: RemoveInventoryObjectsAgentDataBlock {
                agent_id: run.census.agent,
                session_id: run.census.session_id,
            },
            folder_data: vec![RemoveInventoryObjectsFolderDataBlock { folder_id: folder }],
            item_data: vec![RemoveInventoryObjectsItemDataBlock { item_id: item }],
        }))
        .await?;
        let mut heard = run.listen(mark, PUSH_WINDOW).await;
        let folder_at = run.folder_settle(folder, &folder_from).await;
        let item_at = run.item_settle(item, &item_from).await;
        heard.extend(run.listen(run.now(), LATE_WINDOW).await);
        run.record_push(metrics, leg, &heard);
        metrics.set(&format!("{leg}_folder_at"), folder_at);
        metrics.set(&format!("{leg}_item_at"), item_at);
    }
    Ok(())
}

/// What an AIS3 answer's own keys say — every top-level key that begins with
/// `_` but the links and the base URI — with each id named by its role and
/// each version stated as its step from `before`; and the keys of what it
/// embeds.
fn ais_effects(raw: &Raw, roles: &Roles, before: &HashMap<Uuid, i32>) -> String {
    let Some(map) = raw.body.as_ref().and_then(Llsd::as_map) else {
        return raw.status_metric();
    };
    let mut parts = Vec::new();
    let sorted: BTreeMap<&String, &Llsd> = map.iter().collect();
    for (key, value) in sorted {
        if !key.starts_with('_') || matches!(key.as_str(), "_base_uri" | "_links") {
            continue;
        }
        let text = match value {
            Llsd::Array(ids) => {
                // An id the run never named is one the operation made.
                let mut named: Vec<String> = ids
                    .iter()
                    .filter_map(uuid_of)
                    .map(|id| {
                        if roles.names.contains_key(&id) {
                            roles.of(id)
                        } else {
                            "new".to_owned()
                        }
                    })
                    .collect();
                named.sort_unstable();
                format!("[{}]", named.join(","))
            }
            Llsd::Map(entries) if key == "_embedded" => {
                let kinds: BTreeMap<&String, String> = entries
                    .iter()
                    .map(|(kind, inner)| {
                        (
                            kind,
                            inner.as_map().map_or_else(
                                || keys_of(inner),
                                |inner| format!("{} entries", inner.len()),
                            ),
                        )
                    })
                    .collect();
                format!(
                    "{{{}}}",
                    kinds
                        .iter()
                        .map(|(kind, what)| format!("{kind}: {what}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
            Llsd::Map(entries) => {
                let mut stated: Vec<String> = entries
                    .iter()
                    .map(|(id, version)| {
                        let id = Uuid::parse_str(id).ok();
                        let role = id.map_or_else(|| "not an id".to_owned(), |id| roles.of(id));
                        let version = version.as_i32();
                        let change = match id.and_then(|id| before.get(&id).copied()) {
                            Some(was) => format!("{:+}", step(Some(was), version)),
                            None => version.map_or_else(|| "none".to_owned(), |v| format!("={v}")),
                        };
                        format!("{role} {change}")
                    })
                    .collect();
                stated.sort_unstable();
                format!("{{{}}}", stated.join(", "))
            }
            other => crate::cases::task_inventory::shape_of(other),
        };
        parts.push(format!("{key} {text}"));
    }
    if parts.is_empty() {
        "no _ keys".to_owned()
    } else {
        parts.join("; ")
    }
}

/// Leg 3: AIS3's answers to the same changes (Second Life only).
async fn ais(
    run: &mut Run<'_, '_>,
    metrics: &mut Metrics,
    login: &sl_client_tokio::LoginSuccess,
) -> Result<(), TestFailure> {
    let legs = [
        "ais_create",
        "ais_rename",
        "ais_update_item",
        "ais_update_whole",
        "ais_create_own_id",
        "ais_link",
        "ais_slam",
        "ais_slam_duplicate",
        "ais_slam_not_link",
        "ais_slam_map",
        "ais_delete_item",
        "ais_purge",
        "ais_delete_folder",
        "ais_copy_library",
    ];
    if run.census.a.cap(AIS).is_none() {
        for leg in legs {
            for part in ["answer", "effects", "push", "push_detail", "result"] {
                metrics.set(&format!("{leg}_{part}"), NO_CAPABILITY);
            }
        }
        return Ok(());
    }
    let root = run.root;
    let trash = run.trash;
    let note = run
        .roles
        .names
        .iter()
        .find(|(_id, role)| *role == "the notecard")
        .map(|(id, _role)| *id);
    let Some(note) = note else {
        return Err(TestFailure::Assertion(
            "no notecard for the AIS3 legs".to_owned(),
        ));
    };

    // POST /category/<root>: a folder.
    let name = run.census.name("ais-made");
    let before = run.versions(&[root]).await;
    let mark = run.begin().await;
    let raw = run
        .census
        .send_llsd(
            AIS,
            reqwest::Method::POST,
            &sl_wire::ais_create_category_url(InventoryFolderKey::from(root), Uuid::new_v4()),
            &sl_wire::build_ais_create_category_body(InventoryFolderKey::from(root), -1, &name),
        )
        .await;
    let mut heard = run.listen(mark, PUSH_WINDOW).await;
    let made = run
        .census
        .children(root)
        .await
        .into_iter()
        .find(|(listed, _id)| *listed == name)
        .map(|(_name, id)| id);
    if let Some(made) = made {
        run.roles.name(made, "the AIS3 folder");
        run.made.folders.push(made);
    }
    heard.extend(run.listen(run.now(), LATE_WINDOW).await);
    ais_record(run, metrics, "ais_create", &raw, &before, &heard);
    metrics.set(
        "ais_create_result",
        if made.is_some() {
            "listed"
        } else {
            "not listed"
        },
    );

    // PATCH /category/<id>: a rename.
    if let Some(made) = made {
        let renamed = run.census.name("ais-renamed");
        let before = run.versions(&[root, made]).await;
        let mark = run.begin().await;
        let raw = run
            .census
            .send_llsd(
                AIS,
                reqwest::Method::PATCH,
                &sl_wire::ais_category_url(InventoryFolderKey::from(made)),
                &sl_wire::build_ais_rename_category_body(&renamed),
            )
            .await;
        let mut heard = run.listen(mark, PUSH_WINDOW).await;
        let listed = run
            .census
            .settle(root, |children| {
                children.iter().any(|(name, _id)| *name == renamed)
            })
            .await;
        heard.extend(run.listen(run.now(), LATE_WINDOW).await);
        ais_record(run, metrics, "ais_rename", &raw, &before, &heard);
        metrics.set(
            "ais_rename_result",
            if listed { "renamed" } else { "not renamed" },
        );
    } else {
        for part in ["answer", "effects", "push", "push_detail", "result"] {
            metrics.set(&format!("ais_rename_{part}"), "no folder");
        }
    }

    // PATCH /item/<id>: a new name and description.
    let renamed = run.census.name("ais-item");
    let folder_f = run
        .roles
        .names
        .iter()
        .find(|(_id, role)| *role == "F")
        .map(|(id, _r)| *id);
    let before = run
        .versions(&folder_f.into_iter().collect::<Vec<_>>())
        .await;
    let mark = run.begin().await;
    let raw = run
        .census
        .send_llsd(
            AIS,
            reqwest::Method::PATCH,
            &sl_wire::ais_item_url(InventoryKey::from(note)),
            &sl_wire::build_ais_update_item_body(&renamed, "changed by AIS3"),
        )
        .await;
    let mut heard = run.listen(mark, PUSH_WINDOW).await;
    let started = Instant::now();
    let mut state = run.item(note).await;
    while state.as_ref().is_none_or(|state| state.name != renamed) && started.elapsed() < SETTLE {
        tokio::time::sleep(POLL).await;
        state = run.item(note).await;
    }
    heard.extend(run.listen(run.now(), LATE_WINDOW).await);
    ais_record(run, metrics, "ais_update_item", &raw, &before, &heard);
    metrics.set(
        "ais_update_item_result",
        match state {
            Some(state) if state.name == renamed && state.description == "changed by AIS3" => {
                "renamed and described"
            }
            Some(state) if state.name == renamed => "renamed",
            Some(_state) => "unchanged",
            None => "not fetched",
        },
    );

    // PATCH /item/<id> with the whole item, as the reference's
    // `update_inventory_item` sends one and our client now does: a new name,
    // description and next-owner mask.
    match run.note.clone() {
        Some(mut whole) => {
            let renamed = run.census.name("ais-whole");
            whole.name.clone_from(&renamed);
            "changed whole by AIS3".clone_into(&mut whole.description);
            whole.permissions.next_owner =
                sl_client_tokio::Permissions::from_bits(NEXT_OWNER_NO_COPY);
            let body =
                sl_client_tokio::ais_item_update_body(&whole, TransactionId::from(Uuid::nil()))
                    .map_err(|error| {
                        TestFailure::State(format!("encoding the AIS3 body failed: {error}"))
                    })?;
            let before = run
                .versions(&folder_f.into_iter().collect::<Vec<_>>())
                .await;
            let mark = run.begin().await;
            let raw = run
                .census
                .send_llsd(
                    AIS,
                    reqwest::Method::PATCH,
                    &sl_wire::ais_item_url(InventoryKey::from(note)),
                    &body,
                )
                .await;
            let mut heard = run.listen(mark, PUSH_WINDOW).await;
            let started = Instant::now();
            let mut state = run.item(note).await;
            while state.as_ref().is_none_or(|state| state.name != renamed)
                && started.elapsed() < SETTLE
            {
                tokio::time::sleep(POLL).await;
                state = run.item(note).await;
            }
            heard.extend(run.listen(run.now(), LATE_WINDOW).await);
            ais_record(run, metrics, "ais_update_whole", &raw, &before, &heard);
            metrics.set(
                "ais_update_whole_result",
                state.map_or_else(
                    || "not fetched".to_owned(),
                    |state| {
                        format!(
                            "name {}, description {}, next-owner mask {}",
                            if state.name == renamed {
                                "set"
                            } else {
                                "unchanged"
                            },
                            if state.description == "changed whole by AIS3" {
                                "set"
                            } else {
                                "unchanged"
                            },
                            if state.next_mask == Some(i64::from(NEXT_OWNER_NO_COPY)) {
                                "set".to_owned()
                            } else {
                                mask_text(state.next_mask)
                            },
                        )
                    },
                ),
            );
        }
        None => {
            for part in ["answer", "effects", "push", "push_detail", "result"] {
                metrics.set(&format!("ais_update_whole_{part}"), "no item");
            }
        }
    }

    // POST /category/<root> naming the new folder's id: does AIS3 keep an id
    // the client chose, as the UDP create does?
    let asked = Uuid::new_v4();
    let name = run.census.name("ais-own-id");
    let body = sl_wire::build_ais_create_category_body(InventoryFolderKey::from(root), -1, &name)
        .replace(
            "<key>category_id</key><uuid />",
            &format!("<key>category_id</key><uuid>{asked}</uuid>"),
        );
    let before = run.versions(&[root]).await;
    let mark = run.begin().await;
    let raw = run
        .census
        .send_llsd(
            AIS,
            reqwest::Method::POST,
            &sl_wire::ais_create_category_url(InventoryFolderKey::from(root), Uuid::new_v4()),
            &body,
        )
        .await;
    let mut heard = run.listen(mark, PUSH_WINDOW).await;
    let listed = run
        .census
        .children(root)
        .await
        .into_iter()
        .find(|(listed, _id)| *listed == name)
        .map(|(_name, id)| id);
    run.roles.name(asked, "the asked id");
    if let Some(listed) = listed {
        if listed != asked {
            run.roles.name(listed, "the AIS3 own-id folder");
        }
        run.made.folders.push(listed);
    }
    heard.extend(run.listen(run.now(), LATE_WINDOW).await);
    ais_record(run, metrics, "ais_create_own_id", &raw, &before, &heard);
    metrics.set(
        "ais_create_own_id_result",
        match listed {
            Some(listed) if listed == asked => "listed under the asked id",
            Some(_listed) => "listed under another id",
            None => "not listed",
        },
    );

    // POST /category/<made>: a link; then PUT /category/<made>/links.
    match made {
        Some(made) => {
            let link_name = run.census.name("ais-link");
            let before = run.versions(&[made]).await;
            let mark = run.begin().await;
            let raw = run
                .census
                .send_llsd(
                    AIS,
                    reqwest::Method::POST,
                    &sl_wire::ais_create_category_url(
                        InventoryFolderKey::from(made),
                        Uuid::new_v4(),
                    ),
                    &sl_wire::build_ais_create_link_body(
                        note,
                        i32::from(AT_LINK),
                        i32::from(IT_NOTECARD),
                        &link_name,
                        "",
                    ),
                )
                .await;
            let mut heard = run.listen(mark, PUSH_WINDOW).await;
            let link = run.item_named(made, &link_name).await;
            if let Some(link) = link {
                run.roles.name(link, "the AIS3 link");
            }
            heard.extend(run.listen(run.now(), LATE_WINDOW).await);
            ais_record(run, metrics, "ais_link", &raw, &before, &heard);
            metrics.set(
                "ais_link_result",
                if link.is_some() {
                    "listed"
                } else {
                    "not listed"
                },
            );

            // The slam: the folder's links replaced by one to the notecard.
            // One link: two to one item are refused as duplicates.
            let slam_names = [run.census.name("slam")];
            // A bare array of links, as the reference's
            // `slamCategoryLinks` hands `AISAPI::SlamFolder` one.
            let mut body = String::from("<llsd><array>");
            for name in &slam_names {
                let _written = write!(
                    body,
                    "<map><key>name</key><string>{name}</string><key>desc</key><string />\
                     <key>linked_id</key><uuid>{note}</uuid><key>type</key>\
                     <integer>{AT_LINK}</integer></map>"
                );
            }
            body.push_str("</array></llsd>");
            let before = run.versions(&[made]).await;
            let mark = run.begin().await;
            let raw = run
                .census
                .send_llsd(
                    AIS,
                    reqwest::Method::PUT,
                    &format!("/category/{made}/links?tid={}", Uuid::new_v4()),
                    &body,
                )
                .await;
            let mut heard = run.listen(mark, PUSH_WINDOW).await;
            let started = Instant::now();
            let mut listed = run.named_items(made).await;
            while !slam_names
                .iter()
                .all(|name| listed.iter().any(|(listed, _id)| listed == name))
                && started.elapsed() < SETTLE
            {
                tokio::time::sleep(POLL).await;
                listed = run.named_items(made).await;
            }
            for (name, id) in &listed {
                if slam_names.contains(name) {
                    run.roles.name(*id, "a slammed link");
                }
            }
            heard.extend(run.listen(run.now(), LATE_WINDOW).await);
            ais_record(run, metrics, "ais_slam", &raw, &before, &heard);
            let names: BTreeSet<String> = listed
                .iter()
                .map(|(name, _id)| {
                    if slam_names.contains(name) {
                        "a slammed link".to_owned()
                    } else if *name == link_name {
                        "the AIS3 link".to_owned()
                    } else {
                        "another".to_owned()
                    }
                })
                .collect();
            metrics.set(
                "ais_slam_result",
                format!(
                    "{} items: {}",
                    listed.len(),
                    names.into_iter().collect::<Vec<_>>().join(", ")
                ),
            );

            // The slams the reference never sends: two links to one item,
            // a link whose type is no link type, and the links wrapped in a
            // map rather than the bare array.
            let entry = |name: &str, kind: i8| {
                format!(
                    "<map><key>name</key><string>{name}</string><key>desc</key><string />\
                     <key>linked_id</key><uuid>{note}</uuid><key>type</key>\
                     <integer>{kind}</integer></map>"
                )
            };
            let twice = format!(
                "<llsd><array>{}{}</array></llsd>",
                entry(&run.census.name("slam-twice"), AT_LINK),
                entry(&run.census.name("slam-twice"), AT_LINK),
            );
            let untyped = format!(
                "<llsd><array>{}</array></llsd>",
                entry(&run.census.name("slam-untyped"), AT_NOTECARD),
            );
            let wrapped = format!(
                "<llsd><map><key>links</key><array>{}</array></map></llsd>",
                entry(&run.census.name("slam-wrapped"), AT_LINK),
            );
            for (leg, body) in [
                ("ais_slam_duplicate", twice),
                ("ais_slam_not_link", untyped),
                ("ais_slam_map", wrapped),
            ] {
                ais_slam_refusal(run, metrics, leg, made, &body, &slam_names, &link_name).await;
            }
        }
        None => {
            for leg in [
                "ais_link",
                "ais_slam",
                "ais_slam_duplicate",
                "ais_slam_not_link",
                "ais_slam_map",
            ] {
                for part in ["answer", "effects", "push", "push_detail", "result"] {
                    metrics.set(&format!("{leg}_{part}"), "no folder");
                }
            }
        }
    }

    // DELETE /item/<id> outside the Trash, on a copy in the AIS3 folder or F.
    let holder = made.or(folder_f);
    match holder {
        Some(holder) => {
            let (copy, _heard) = run
                .copy(note, holder, "ais-deleted", "the AIS3-deleted item")
                .await?;
            match copy {
                Some(copy) => {
                    let before = run.versions(&[holder, trash]).await;
                    let from = run.item_whereabouts(copy).await;
                    let mark = run.begin().await;
                    let raw = run
                        .census
                        .send_llsd(
                            AIS,
                            reqwest::Method::DELETE,
                            &sl_wire::ais_item_url(InventoryKey::from(copy)),
                            "",
                        )
                        .await;
                    let mut heard = run.listen(mark, PUSH_WINDOW).await;
                    let at = run.item_settle(copy, &from).await;
                    heard.extend(run.listen(run.now(), LATE_WINDOW).await);
                    ais_record(run, metrics, "ais_delete_item", &raw, &before, &heard);
                    metrics.set("ais_delete_item_result", at);
                }
                None => {
                    for part in ["answer", "effects", "push", "push_detail", "result"] {
                        metrics.set(&format!("ais_delete_item_{part}"), "no copy");
                    }
                }
            }
        }
        None => {
            for part in ["answer", "effects", "push", "push_detail", "result"] {
                metrics.set(&format!("ais_delete_item_{part}"), "no folder");
            }
        }
    }

    // DELETE /category/<id>/children outside the Trash: the AIS3 folder's
    // links and a copy.
    match made {
        Some(made) => {
            let (_copy, _heard) = run
                .copy(note, made, "ais-purged", "the AIS3-purged item")
                .await?;
            let held = run.item_count(made).await;
            let before = run.versions(&[made, trash]).await;
            let mark = run.begin().await;
            let raw = run
                .census
                .send_llsd(
                    AIS,
                    reqwest::Method::DELETE,
                    &sl_wire::ais_category_children_url(InventoryFolderKey::from(made)),
                    "",
                )
                .await;
            let mut heard = run.listen(mark, PUSH_WINDOW).await;
            let started = Instant::now();
            let mut left = run.item_count(made).await;
            while left == held && started.elapsed() < SETTLE {
                tokio::time::sleep(POLL).await;
                left = run.item_count(made).await;
            }
            heard.extend(run.listen(run.now(), LATE_WINDOW).await);
            ais_record(run, metrics, "ais_purge", &raw, &before, &heard);
            metrics.set(
                "ais_purge_result",
                if left == 0 {
                    "emptied".to_owned()
                } else if left == held {
                    "unchanged".to_owned()
                } else {
                    format!("{left} of {held} left")
                },
            );
        }
        None => {
            for part in ["answer", "effects", "push", "push_detail", "result"] {
                metrics.set(&format!("ais_purge_{part}"), "no folder");
            }
        }
    }

    // COPY of a small Library folder into F, as the reference copies one.
    ais_copy_library(run, metrics, login, folder_f).await;

    // DELETE /category/<id> outside the Trash: the AIS3 folder.
    match made {
        Some(made) => {
            let before = run.versions(&[root, trash]).await;
            let mark = run.begin().await;
            let raw = run
                .census
                .send_llsd(
                    AIS,
                    reqwest::Method::DELETE,
                    &sl_wire::ais_category_url(InventoryFolderKey::from(made)),
                    "",
                )
                .await;
            let mut heard = run.listen(mark, PUSH_WINDOW).await;
            let at = run.folder_settle(made, "the root").await;
            heard.extend(run.listen(run.now(), LATE_WINDOW).await);
            ais_record(run, metrics, "ais_delete_folder", &raw, &before, &heard);
            metrics.set("ais_delete_folder_result", at.clone());
            if at == "gone" {
                run.made.folders.retain(|folder| *folder != made);
            }
        }
        None => {
            for part in ["answer", "effects", "push", "push_detail", "result"] {
                metrics.set(&format!("ais_delete_folder_{part}"), "no folder");
            }
        }
    }
    Ok(())
}

/// Records an AIS3 leg's answer, its effects and what was pushed.
fn ais_record(
    run: &Run<'_, '_>,
    metrics: &mut Metrics,
    leg: &str,
    raw: &Raw,
    before: &HashMap<Uuid, i32>,
    heard: &Heard,
) {
    metrics.set(&format!("{leg}_answer"), raw.summary("errors"));
    metrics.set(
        &format!("{leg}_effects"),
        ais_effects(raw, &run.roles, before),
    );
    run.record_push(metrics, leg, heard);
}

/// A slam the reference never sends, put to the AIS3 folder after the real
/// one: its answer, effects and pushes, and what the folder then lists.
async fn ais_slam_refusal(
    run: &mut Run<'_, '_>,
    metrics: &mut Metrics,
    leg: &str,
    made: Uuid,
    body: &str,
    slam_names: &[String],
    link_name: &str,
) {
    let before = run.versions(&[made]).await;
    let mark = run.begin().await;
    let raw = run
        .census
        .send_llsd(
            AIS,
            reqwest::Method::PUT,
            &format!("/category/{made}/links?tid={}", Uuid::new_v4()),
            body,
        )
        .await;
    let mut heard = run.listen(mark, PUSH_WINDOW).await;
    let listed = run.named_items(made).await;
    heard.extend(run.listen(run.now(), LATE_WINDOW).await);
    ais_record(run, metrics, leg, &raw, &before, &heard);
    let names: BTreeSet<String> = listed
        .iter()
        .map(|(name, _id)| {
            if slam_names.contains(name) {
                "a slammed link".to_owned()
            } else if name == link_name {
                "the AIS3 link".to_owned()
            } else if name.contains("slam-") {
                "this leg's link".to_owned()
            } else {
                "another".to_owned()
            }
        })
        .collect();
    let after = run.versions(&[made]).await;
    let stepped = if after.get(&made) == before.get(&made) {
        "version kept"
    } else {
        "version stepped"
    };
    metrics.set(
        &format!("{leg}_result"),
        format!(
            "{} items: {}; {stepped}",
            listed.len(),
            names.into_iter().collect::<Vec<_>>().join(", ")
        ),
    );
}

/// The `COPY` leg: the smallest Library folder holding items and no
/// sub-folders, copied into `F` with `LibraryAPIv3`'s `COPY` and a
/// `Destination` naming `F`, as the reference's `CopyLibraryCategory` does.
async fn ais_copy_library(
    run: &mut Run<'_, '_>,
    metrics: &mut Metrics,
    login: &sl_client_tokio::LoginSuccess,
    folder_f: Option<Uuid>,
) {
    let leg = "ais_copy_library";
    let set_all = |metrics: &mut Metrics, why: &str| {
        for part in ["answer", "effects", "push", "push_detail", "result"] {
            metrics.set(&format!("{leg}_{part}"), why);
        }
    };
    let (Some(library), Some(folder_f)) = (login.library_root.map(|root| root.uuid()), folder_f)
    else {
        set_all(metrics, "no library or folder");
        return;
    };
    if run.census.a.cap(LIBRARY_AIS).is_none() {
        set_all(metrics, NO_CAPABILITY);
        return;
    }
    // The candidates: every Library folder a depth-1 listing of the root
    // opens, with how many items and sub-folders it holds.
    let listing = run
        .census
        .get(
            LIBRARY_AIS,
            &sl_wire::ais_category_children_fetch_url(InventoryFolderKey::from(library), 1),
        )
        .await;
    let mut candidates: Vec<(usize, Uuid)> = listing
        .body
        .as_ref()
        .and_then(|body| body.get("_embedded"))
        .and_then(|embedded| embedded.get("categories"))
        .and_then(Llsd::as_map)
        .map(|categories| {
            categories
                .values()
                .filter_map(|category| {
                    let id = category.get("category_id").and_then(uuid_of)?;
                    let embedded = category.get("_embedded")?;
                    let count = |kind: &str| {
                        embedded
                            .get(kind)
                            .and_then(Llsd::as_map)
                            .map_or(0, |map| map.len())
                    };
                    let items = count("items").saturating_add(count("links"));
                    (count("categories") == 0 && (1..=COPY_MAX_ITEMS).contains(&items))
                        .then_some((items, id))
                })
                .collect()
        })
        .unwrap_or_default();
    candidates.sort_unstable();
    let Some((items, source)) = candidates.first().copied() else {
        set_all(metrics, "no small Library folder");
        return;
    };
    run.roles.name(source, "the Library folder");
    let Some(url) = run.census.a.cap(LIBRARY_AIS) else {
        set_all(metrics, NO_CAPABILITY);
        return;
    };
    let Ok(method) = reqwest::Method::from_bytes(b"COPY") else {
        set_all(metrics, "no COPY method");
        return;
    };
    let before = run.versions(&[folder_f]).await;
    let held = run.item_count(folder_f).await;
    let mark = run.begin().await;
    let request = run
        .census
        .http
        .request(
            method,
            format!("{url}/category/{source}?tid={},depth=0", Uuid::new_v4()),
        )
        .header(reqwest::header::ACCEPT, "application/llsd+xml")
        .header("Destination", folder_f.to_string());
    let raw = answer(request).await;
    let mut heard = run.listen(mark, PUSH_WINDOW).await;
    let started = Instant::now();
    let mut copied: Vec<Uuid> = run
        .census
        .children(folder_f)
        .await
        .into_iter()
        .map(|(_name, id)| id)
        .collect();
    while copied.is_empty() && started.elapsed() < SETTLE {
        tokio::time::sleep(POLL).await;
        copied = run
            .census
            .children(folder_f)
            .await
            .into_iter()
            .map(|(_name, id)| id)
            .collect();
    }
    for id in &copied {
        run.roles.name(*id, "the copied folder");
    }
    heard.extend(run.listen(run.now(), LATE_WINDOW).await);
    ais_record(run, metrics, leg, &raw, &before, &heard);
    let copied_items = match copied.first() {
        Some(copy) => run.item_count(*copy).await,
        None => 0,
    };
    metrics.set(
        &format!("{leg}_result"),
        format!(
            "{} folders in F, {} of {items} items, F's own items {}",
            copied.len(),
            copied_items,
            if run.item_count(folder_f).await == held {
                "unchanged"
            } else {
                "changed"
            },
        ),
    );
}

/// One gift from B to A: B makes a notecard named `word` whose next owner
/// gets `next_owner_mask` and gives it; A accepts it into its Notecards. Where
/// A's copy is filed and what A was pushed are recorded under `word`; A's copy,
/// if one was filed.
async fn gift(
    run: &mut Run<'_, '_>,
    metrics: &mut Metrics,
    word: &str,
    next_owner_mask: u32,
    notecards: Uuid,
) -> Result<Option<Uuid>, TestFailure> {
    let a_id = AgentKey::from(run.census.agent);
    let b_id = run
        .b
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("B's login reported no agent id".to_owned()))?;
    let b_notecards = b_notecards(run)?;
    let name = run.census.name(word);
    run.b
        .send(Command::CreateInventoryItem(NewInventoryItem {
            folder_id: b_notecards,
            transaction_id: Uuid::nil(),
            next_owner_mask,
            asset_type: AssetType::Notecard,
            inv_type: InventoryType::Notecard,
            wearable_type: WearableType::Shape,
            name: name.clone(),
            description: "sl-conformance inventory-mutations".to_owned(),
        }))
        .await?;
    let wanted = name.clone();
    let original = run
        .b
        .wait_for(HTTP_TIMEOUT, |event| match event {
            Event::InventoryItemCreated { item, .. } if item.name == wanted => Some(item.item_id),
            _ => None,
        })
        .await?;
    run.b
        .send(Command::GiveInventory {
            to_agent_id: a_id,
            item_id: original,
            asset_type: AssetType::Notecard,
            item_name: name.clone(),
            transaction_id: TransactionId::from(Uuid::new_v4()),
        })
        .await?;
    let wanted = name.clone();
    let offer = run
        .census
        .a
        .wait_for(OFFER_TIMEOUT, |event| match event {
            Event::InstantMessageReceived(im)
                if im.from_agent_id == b_id
                    && im.dialog == ImDialog::InventoryOffered
                    && im.message == wanted =>
            {
                im.inventory_offer()
            }
            _ => None,
        })
        .await;
    let received = match offer {
        Ok(offer) => {
            let mark = run.begin().await;
            run.census
                .a
                .send(Command::AcceptInventoryOffer {
                    offer,
                    folder_id: InventoryFolderKey::from(notecards),
                })
                .await?;
            let mut heard = run.listen(mark, PUSH_WINDOW).await;
            // Where the accepted copy is filed, for as long as the offer
            // itself was waited for.
            let started = Instant::now();
            let mut received = None;
            while received.is_none() && started.elapsed() < OFFER_TIMEOUT {
                for folder in [notecards, run.root] {
                    if let Some((_name, id)) = run
                        .named_items(folder)
                        .await
                        .into_iter()
                        .find(|(listed, _id)| *listed == name)
                    {
                        received = Some(id);
                        metrics.set(&format!("{word}_filed_in"), run.roles.of(folder));
                    }
                }
                if received.is_none() {
                    heard.extend(run.listen(run.now(), POLL).await);
                }
            }
            if received.is_none() {
                metrics.set(&format!("{word}_filed_in"), "nowhere");
            }
            if let Some(received) = received {
                run.roles
                    .name(received, &format!("the received {word} item"));
            }
            heard.extend(run.listen(run.now(), LATE_WINDOW).await);
            run.record_push(metrics, &format!("{word}_receive"), &heard);
            received
        }
        Err(TestFailure::Timeout(_)) => {
            for part in ["filed_in", "receive_push", "receive_push_detail"] {
                metrics.set(&format!("{word}_{part}"), "no offer");
            }
            None
        }
        Err(other) => return Err(other),
    };
    metrics.set(&format!("{word}_received"), received.is_some());
    Ok(received)
}

/// B's Notecards folder, from its login skeleton.
fn b_notecards(run: &Run<'_, '_>) -> Result<InventoryFolderKey, TestFailure> {
    run.b
        .login_success()
        .and_then(|login| {
            login
                .inventory_skeleton
                .iter()
                .find(|folder| FolderType::from_code(folder.type_default) == FolderType::Notecard)
        })
        .map(|folder| folder.folder_id)
        .ok_or_else(|| TestFailure::Assertion("B's skeleton named no Notecards".to_owned()))
}

/// Leg 4: gifts from B to A — a control shaped as `give-inventory`'s, then
/// one whose next owner may not copy it — and the no-copy one given back.
async fn give(
    run: &mut Run<'_, '_>,
    metrics: &mut Metrics,
    notecards: Uuid,
) -> Result<(), TestFailure> {
    let b_id = run
        .b
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("B's login reported no agent id".to_owned()))?;
    let b_notecards = b_notecards(run)?;
    // The other way first, as `give-inventory` gives: A to B.
    let reverse = run.census.name("reverse");
    run.census
        .a
        .send(Command::CreateInventoryItem(NewInventoryItem {
            folder_id: InventoryFolderKey::from(notecards),
            transaction_id: Uuid::nil(),
            next_owner_mask: NEXT_OWNER_FULL,
            asset_type: AssetType::Notecard,
            inv_type: InventoryType::Notecard,
            wearable_type: WearableType::Shape,
            name: reverse.clone(),
            description: "sl-conformance inventory-mutations".to_owned(),
        }))
        .await?;
    let wanted = reverse.clone();
    let made = run
        .census
        .a
        .wait_for(HTTP_TIMEOUT, |event| match event {
            Event::InventoryItemCreated { item, .. } if item.name == wanted => Some(item.item_id),
            _ => None,
        })
        .await;
    let reverse_received = match made {
        Ok(item) => {
            run.roles.name(item.uuid(), "the received reverse item");
            run.census
                .a
                .send(Command::GiveInventory {
                    to_agent_id: b_id,
                    item_id: item,
                    asset_type: AssetType::Notecard,
                    item_name: reverse.clone(),
                    transaction_id: TransactionId::from(Uuid::new_v4()),
                })
                .await?;
            let wanted = reverse.clone();
            let a_uuid = run.census.agent;
            let offer = run
                .b
                .wait_for(OFFER_TIMEOUT, |event| match event {
                    Event::InstantMessageReceived(im)
                        if im.from_agent_id.uuid() == a_uuid
                            && im.dialog == ImDialog::InventoryOffered
                            && im.message == wanted =>
                    {
                        im.inventory_offer()
                    }
                    _ => None,
                })
                .await;
            match offer {
                Ok(offer) => {
                    run.b
                        .send(Command::AcceptInventoryOffer {
                            offer,
                            folder_id: b_notecards,
                        })
                        .await?;
                    let started = Instant::now();
                    let mut held = false;
                    while !held && started.elapsed() < OFFER_TIMEOUT {
                        let _drained = run.b.wait_for(POLL, |_event| None::<()>).await;
                        held = b_holds(run, b_notecards, &reverse).await >= 1;
                    }
                    if held { "filed" } else { "not filed" }
                }
                Err(TestFailure::Timeout(_)) => "no offer",
                Err(other) => return Err(other),
            }
        }
        Err(TestFailure::Timeout(_)) => "not made",
        Err(other) => return Err(other),
    };
    metrics.set("reverse_received", reverse_received);
    clean_up_give(run, &reverse, b_notecards).await?;
    let control = gift(run, metrics, "control", NEXT_OWNER_FULL, notecards).await;
    let cleaned = clean_up_give(run, &run.census.name("control"), b_notecards).await;
    control?;
    cleaned?;
    let name = run.census.name("nocopy");
    let received = gift(run, metrics, "nocopy", NEXT_OWNER_NO_COPY, notecards).await?;
    let parts = [
        "owner_can_copy",
        "offer_push",
        "offer_push_detail",
        "at_offer",
        "accept_push",
        "accept_push_detail",
        "after_accept",
        "returned",
    ];
    let Some(received) = received else {
        for part in parts {
            metrics.set(&format!("nocopy_{part}"), "not received");
        }
        return clean_up_give(run, &name, b_notecards).await;
    };
    let state = run.item(received).await;
    metrics.set(
        "nocopy_owner_can_copy",
        state.and_then(|state| state.masks.1).map_or_else(
            || "no mask".to_owned(),
            |mask| (mask & PERM_COPY != 0).to_string(),
        ),
    );

    // A gives it back.
    let mark = run.begin().await;
    run.census
        .a
        .send(Command::GiveInventory {
            to_agent_id: b_id,
            item_id: InventoryKey::from(received),
            asset_type: AssetType::Notecard,
            item_name: name.clone(),
            transaction_id: TransactionId::from(Uuid::new_v4()),
        })
        .await?;
    let heard = run.listen(mark, PUSH_WINDOW).await;
    let at_offer = run.item_whereabouts(received).await;
    run.record_push(metrics, "nocopy_offer", &heard);
    metrics.set("nocopy_at_offer", at_offer.clone());
    let wanted = name.clone();
    let a_uuid = run.census.agent;
    let offer = run
        .b
        .wait_for(OFFER_TIMEOUT, |event| match event {
            Event::InstantMessageReceived(im)
                if im.from_agent_id.uuid() == a_uuid
                    && im.dialog == ImDialog::InventoryOffered
                    && im.message == wanted =>
            {
                im.inventory_offer()
            }
            _ => None,
        })
        .await;
    match offer {
        Ok(offer) => {
            let mark = run.begin().await;
            run.b
                .send(Command::AcceptInventoryOffer {
                    offer,
                    folder_id: b_notecards,
                })
                .await?;
            let mut heard = run.listen(mark, PUSH_WINDOW).await;
            let after = run.item_settle(received, &at_offer).await;
            heard.extend(run.listen(run.now(), LATE_WINDOW).await);
            run.record_push(metrics, "nocopy_accept", &heard);
            metrics.set("nocopy_after_accept", after);
            let started = Instant::now();
            let mut returned = false;
            while !returned && started.elapsed() < SETTLE {
                let _drained = run.b.wait_for(POLL, |_event| None::<()>).await;
                returned = b_holds(run, b_notecards, &name).await >= 2;
            }
            metrics.set("nocopy_returned", returned);
        }
        Err(TestFailure::Timeout(_)) => {
            for part in [
                "accept_push",
                "accept_push_detail",
                "after_accept",
                "returned",
            ] {
                metrics.set(&format!("nocopy_{part}"), "no offer");
            }
        }
        Err(other) => return Err(other),
    }
    clean_up_give(run, &name, b_notecards).await
}

/// How many items named `name` B's folder fetch lists in `folder`.
async fn b_holds(run: &Run<'_, '_>, folder: InventoryFolderKey, name: &str) -> usize {
    let Some(url) = run.b.cap(FID2) else {
        return 0;
    };
    let Some(owner) = run.b.agent_id() else {
        return 0;
    };
    let body = crate::cases::inventory_fetch::map([(
        "folders",
        Llsd::Array(vec![crate::cases::inventory_fetch::map([
            ("folder_id", Llsd::Uuid(folder.uuid())),
            ("owner_id", Llsd::Uuid(owner.uuid())),
            ("fetch_folders", Llsd::Boolean(false)),
            ("fetch_items", Llsd::Boolean(true)),
            ("sort_order", Llsd::Integer(0)),
        ])]),
    )]);
    let request = run
        .census
        .http
        .post(url)
        .header(reqwest::header::ACCEPT, "application/llsd+xml")
        .header(reqwest::header::CONTENT_TYPE, "application/llsd+xml")
        .body(body.to_llsd_xml());
    let raw = answer(request).await;
    b_items(&raw, name).len()
}

/// The ids of the items named `name` a folder-fetch answer lists.
fn b_items(raw: &Raw, name: &str) -> Vec<Uuid> {
    listed_folders(raw.body.as_ref())
        .flat_map(|entry| {
            entry
                .get("items")
                .and_then(Llsd::as_array)
                .unwrap_or_default()
        })
        .filter(|item| item.get("name").and_then(Llsd::as_str) == Some(name))
        .filter_map(|item| item.get("item_id").and_then(uuid_of))
        .collect()
}

/// Removes every item named `name` from B's Notecards and A's inventory.
async fn clean_up_give(
    run: &mut Run<'_, '_>,
    name: &str,
    b_notecards: InventoryFolderKey,
) -> Result<(), TestFailure> {
    let Some(url) = run.b.cap(FID2) else {
        return Ok(());
    };
    let Some(owner) = run.b.agent_id() else {
        return Ok(());
    };
    let body = crate::cases::inventory_fetch::map([(
        "folders",
        Llsd::Array(vec![crate::cases::inventory_fetch::map([
            ("folder_id", Llsd::Uuid(b_notecards.uuid())),
            ("owner_id", Llsd::Uuid(owner.uuid())),
            ("fetch_folders", Llsd::Boolean(false)),
            ("fetch_items", Llsd::Boolean(true)),
            ("sort_order", Llsd::Integer(0)),
        ])]),
    )]);
    let request = run
        .census
        .http
        .post(url)
        .header(reqwest::header::ACCEPT, "application/llsd+xml")
        .header(reqwest::header::CONTENT_TYPE, "application/llsd+xml")
        .body(body.to_llsd_xml());
    let raw = answer(request).await;
    let theirs: Vec<InventoryKey> = b_items(&raw, name)
        .into_iter()
        .map(InventoryKey::from)
        .collect();
    if !theirs.is_empty() {
        run.b.send(Command::RemoveInventoryItems(theirs)).await?;
    }
    let ours: Vec<InventoryKey> = run
        .roles
        .names
        .iter()
        .filter(|(_id, role)| role.starts_with("the received "))
        .map(|(id, _role)| InventoryKey::from(*id))
        .collect();
    if !ours.is_empty() {
        run.census
            .a
            .send(Command::RemoveInventoryItems(ours))
            .await?;
    }
    let _drained = run
        .b
        .wait_for(Duration::from_secs(2), |_event| None::<()>)
        .await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Heard, Roles, ais_effects, mask_text};
    use crate::cases::inventory_fetch::{Raw, map};
    use pretty_assertions::assert_eq;
    use sl_client_tokio::{Event, InventoryKey, Uuid};
    use sl_wire::Llsd;
    use std::collections::HashMap;

    /// A push names each message once, by the road it came by.
    #[test]
    fn a_push_names_the_road() {
        let mut heard = Heard::default();
        let _new = heard.datagrams.insert("UpdateCreateInventoryItem");
        heard.events.push(Event::InventoryItemsRemoved {
            items: vec![InventoryKey::from(Uuid::from_u128(1))],
        });
        let _new = heard.unhandled.insert("Odd".to_owned());
        assert_eq!(
            heard.push(),
            "Odd (event queue, unhandled), RemoveInventoryItem (event queue), \
             UpdateCreateInventoryItem (UDP)"
        );
        let mut roles = Roles::default();
        roles.name(Uuid::from_u128(1), "the copy");
        assert_eq!(heard.detail(&roles), "RemoveInventoryItem: [the copy]");
        assert_eq!(Heard::default().push(), "nothing");
    }

    /// An AIS3 answer's own keys are stated by role, a version as its step.
    #[test]
    fn ais_effects_name_roles_and_steps() {
        let parent = Uuid::from_u128(7);
        let made = Uuid::from_u128(8);
        let mut roles = Roles::default();
        roles.name(parent, "the root");
        let body = map([
            ("_base_uri", Llsd::String("x".to_owned())),
            ("_created_categories", Llsd::Array(vec![Llsd::Uuid(made)])),
            (
                "_updated_category_versions",
                Llsd::Map(
                    [(parent.to_string(), Llsd::Integer(5))]
                        .into_iter()
                        .collect(),
                ),
            ),
            ("name", Llsd::String("n".to_owned())),
        ]);
        let raw = Raw {
            status: Ok(201),
            body: Some(body),
        };
        let before: HashMap<Uuid, i32> = [(parent, 4)].into_iter().collect();
        assert_eq!(
            ais_effects(&raw, &roles, &before),
            "_created_categories [new]; _updated_category_versions {the root +1}"
        );
    }

    /// A mask is stated in hex, an absent one by name.
    #[test]
    fn a_mask_is_hex() {
        assert_eq!(mask_text(Some(0x0009_e000)), "0x0009e000");
        assert_eq!(mask_text(None), "none");
    }
}
