//! A census of how each grid hands a viewer its inventory to read: the login
//! skeleton, the fetch capabilities, AIS3, the shared Library, and the folder
//! versions a cache is checked against.
//!
//! Every answer is asked of the grid **raw** — the capability's own request
//! with `reqwest`, the UDP message with [`Command::Send`] — and recorded whole
//! (status, the key set at each level, the shape of an entry), so the fake
//! grid's encoders can be held to the same strings. The legs:
//!
//! 1. **The login skeleton** of each tree: the fields an entry carries, the
//!    root's type and parent, the Library's owner, the UDP blacklist's
//!    inventory names — and, against a crawl of the whole tree, whether the
//!    skeleton names every folder at the version and under the parent the
//!    fetch states.
//! 2. **The capabilities** a seed grants for reading inventory.
//! 3. **The folder fetch** (`FetchInventoryDescendents2`): the answer's keys,
//!    a folder's, a sub-folder's, an item's shape, the descendent count, a
//!    folder nobody has, and the Library asked through the agent's capability.
//!    The crawls behind leg 1 read both trees this way, the Library through
//!    `FetchLibDescendents2` where it is granted, and count the nil-id folders
//!    a listing names (OpenSim's phantom).
//! 4. **The item fetch** (`FetchInventory2` / `FetchLib2`): one item, compared
//!    with what the folder fetch said of it; an item nobody has; a Library item
//!    through the agent's capability.
//! 5. **The UDP fetches** (`FetchInventoryDescendents`, `FetchInventory`), sent
//!    raw with the circuit probe on: which inventory datagrams answer, if any.
//! 6. **AIS3**: a folder, an item, a folder nobody has, the children listing at
//!    depths 0, 1, 2, 3 and 100 — how far each reaches and whether it nests
//!    a level per depth — and the Library's.
//! 7. **Versions**: a folder made over UDP, over `CreateInventoryCategory` and
//!    over AIS3, a UDP rename, move into the Trash and removal, and the AIS3
//!    move and removal, each read back as the step of the parent's (and the
//!    Trash's) version through the folder fetch; then a relogin, and whether
//!    the new skeleton states the versions the last session read.
//!
//! A grid that grants a capability no leg can use records `no capability`.
//! What each live grid answered is stated as a [`Measured`] and written up in
//! `book/src/gridspec/inventory.md` § Fetching; every run is held to it.
//!
//! `1av`, live and fake grids.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::fmt::Write as _;
use std::time::{Duration, Instant};

use reqwest::header::{ACCEPT, CONTENT_TYPE};
use sl_client_tokio::{
    AnyMessage, CircuitProbe, Command, Diagnostic, FolderType, InventoryFolderKey, Reliability,
    Uuid,
};
use sl_wire::messages::{
    CreateInventoryFolder, CreateInventoryFolderAgentDataBlock,
    CreateInventoryFolderFolderDataBlock, FetchInventory, FetchInventoryAgentDataBlock,
    FetchInventoryDescendents, FetchInventoryDescendentsAgentDataBlock,
    FetchInventoryDescendentsInventoryDataBlock, FetchInventoryInventoryDataBlock,
    MoveInventoryFolder, MoveInventoryFolderAgentDataBlock, MoveInventoryFolderInventoryDataBlock,
    MoveInventoryItem, MoveInventoryItemAgentDataBlock, MoveInventoryItemInventoryDataBlock,
    RemoveInventoryFolder, RemoveInventoryFolderAgentDataBlock,
    RemoveInventoryFolderFolderDataBlock, UpdateInventoryFolder,
    UpdateInventoryFolderAgentDataBlock, UpdateInventoryFolderFolderDataBlock,
};
use sl_wire::{Llsd, parse_llsd_xml};

use crate::cases::asset_http::without_ids;
use crate::cases::object_rez_derez::Answer;
use crate::cases::task_inventory::{hold_to_measured, shape_of};
use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::metrics::Metrics;
use crate::registry::{GridTest, TestFuture};
use crate::support::REGION_TIMEOUT;

/// Where the measurements below are written down.
const SOURCE: &str = "book/src/gridspec/inventory.md § Fetching (inventory-fetch, 2026-10-21)";

/// The agent folder fetch.
pub(crate) const FID2: &str = "FetchInventoryDescendents2";

/// The Library folder fetch.
pub(crate) const FLD2: &str = "FetchLibDescendents2";

/// The agent item fetch.
pub(crate) const FI2: &str = "FetchInventory2";

/// The Library item fetch.
const FL2: &str = "FetchLib2";

/// AIS3 over the agent's tree.
pub(crate) const AIS: &str = "InventoryAPIv3";

/// AIS3 over the Library.
pub(crate) const LIBRARY_AIS: &str = "LibraryAPIv3";

/// Every capability the census asks the seed about.
const CAPABILITIES: [&str; 7] = [
    FID2,
    FLD2,
    FI2,
    FL2,
    AIS,
    LIBRARY_AIS,
    "CreateInventoryCategory",
];

/// What a raw answer reads on a grid that grants no capability.
pub(crate) const NO_CAPABILITY: &str = "no capability";

/// How long to poll for the capabilities after region arrival: the seed fetch
/// races the arrival.
pub(crate) const CAP_WAIT: Duration = Duration::from_secs(20);

/// How long one HTTP answer may take.
pub(crate) const HTTP_TIMEOUT: Duration = Duration::from_secs(30);

/// How long a UDP fetch is listened to.
pub(crate) const UDP_WINDOW: Duration = Duration::from_secs(6);

/// How long a mutation may take to show in the folder fetch.
pub(crate) const SETTLE: Duration = Duration::from_secs(12);

/// How often a settling mutation is re-read.
pub(crate) const POLL: Duration = Duration::from_millis(750);

/// How many folders one folder-fetch request names, as the reference batches.
const BATCH: usize = 10;

/// Upper bound on the folders a crawl reads.
const MAX_FOLDERS: usize = 3_000;

/// The AIS3 children depths the census asks for.
const DEPTHS: [i32; 5] = [0, 1, 2, 3, 100];

/// The asset types of an inventory link.
const LINK_TYPES: [i32; 2] = [24, 25];

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
/// under — generated from two runs on each live grid (aditi and the local
/// OpenSim, 2026-10-21), every key that held still between them. Left out as
/// the tree's or the run's rather than the grid's: the tree's height, a
/// folder's thumbnail, the root's embedded items, folders an earlier run left
/// behind, datagram counts and timings.
const MEASURED: &[(&str, Measured<Answer>)] = &[
    ("agent_crawl_nil_folders", both(Answer::Count(0))),
    ("agent_crawl_refused", both(Answer::Count(0))),
    (
        "ais_base_uri",
        each(
            Answer::Text("slcap://InventoryAPIv3"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ais_category_keys",
        each(
            Answer::Text("_links,agent_id,category_id,name,parent_id,type_default,version"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ais_category_shape",
        each(
            Answer::Text(
                "{_links:{parent:{href:uri},self:{href:uri}},agent_id:string,category_id:string,name:string,parent_id:string,type_default:integer,version:integer}",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ais_children_keys",
        each(
            Answer::Text(
                "_base_uri,_embedded,_links,agent_id,category_id,name,parent_id,type_default,version",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ais_children_links",
        each(
            Answer::Text(
                "categories=/category/<root>/categories category=/category/<root> (self) children=/category/<root>/children items=/category/<root>/items links=/category/<root>/links parent=/category/<id> self=/category/<root>/children",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ais_create_listed",
        each(Answer::Flag(true), Answer::Text("no capability")),
    ),
    (
        "ais_create_root_step",
        each(Answer::Count(1), Answer::Text("no capability")),
    ),
    (
        "ais_depth_0_nesting",
        each(
            Answer::Text("each folder under its parent"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ais_depth_0_reach",
        each(
            Answer::Text("the depth plus one levels"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ais_depth_0_status",
        each(Answer::Text("200"), Answer::Text("no capability")),
    ),
    (
        "ais_depth_100_nesting",
        each(
            Answer::Text("each folder under its parent"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ais_depth_100_reach",
        each(
            Answer::Text("the depth plus one levels"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ais_depth_100_status",
        each(Answer::Text("200"), Answer::Text("no capability")),
    ),
    (
        "ais_depth_1_nesting",
        each(
            Answer::Text("each folder under its parent"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ais_depth_1_reach",
        each(
            Answer::Text("the depth plus one levels"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ais_depth_1_status",
        each(Answer::Text("200"), Answer::Text("no capability")),
    ),
    (
        "ais_depth_2_nesting",
        each(
            Answer::Text("each folder under its parent"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ais_depth_2_reach",
        each(
            Answer::Text("the depth plus one levels"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ais_depth_2_status",
        each(Answer::Text("200"), Answer::Text("no capability")),
    ),
    (
        "ais_depth_3_nesting",
        each(
            Answer::Text("each folder under its parent"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ais_depth_3_reach",
        each(
            Answer::Text("the depth plus one levels"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ais_depth_3_status",
        each(Answer::Text("200"), Answer::Text("no capability")),
    ),
    (
        "ais_embedded_link_keys",
        each(Answer::Text("none"), Answer::Text("no capability")),
    ),
    (
        "ais_folder_links",
        each(
            Answer::Text(
                "categories=/category/<root>/categories children=/category/<root>/children items=/category/<root>/items links=/category/<root>/links parent=/category/<id> self=/category/<root>",
            ),
            Answer::Text("none"),
        ),
    ),
    (
        "ais_folder_shape",
        each(
            Answer::Text(
                "{_base_uri:uri,_links:{categories:{href:uri},children:{href:uri},items:{href:uri},links:{href:uri},parent:{href:uri},self:{href:uri}},agent_id:string,category_id:string,name:string,parent_id:string,type_default:integer,version:integer}",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ais_folder_status",
        each(Answer::Text("200"), Answer::Text("no capability")),
    ),
    (
        "ais_item_links",
        each(
            Answer::Text("parent=/category/<id> self=/item/<id>"),
            Answer::Text("none"),
        ),
    ),
    (
        "ais_item_move_answer",
        each(
            Answer::Text(
                "400: error_code,error_description,error_filename,error_function,error_line_number; error_code=0; error_description=Cannot change parent_id.  Use MOVE method. <id> <id>; error_filename=items.py; error_function=Item.mergeUntrusted; error_line_number=732; says Cannot change parent_id.  Use MOVE method. <id> <id>",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ais_item_move_moved",
        each(Answer::Flag(false), Answer::Text("no capability")),
    ),
    (
        "ais_item_shape",
        each(
            Answer::Text(
                "{_base_uri:uri,_links:{parent:{href:uri},self:{href:uri}},agent_id:string,asset_id:string,created_at:integer,desc:string,flags:integer,inv_type:integer,item_id:string,name:string,parent_id:string,permissions:{base_mask:integer,creator_id:string,everyone_mask:integer,group_id:string,group_mask:integer,last_owner_id:string,next_owner_mask:integer,owner_id:string,owner_mask:integer},sale_info:{sale_price:integer,sale_type:integer},type:integer}",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ais_item_status",
        each(Answer::Text("200"), Answer::Text("no capability")),
    ),
    (
        "ais_library_agent_folder",
        each(
            Answer::Text(
                "404: category_id,error_code,error_description,error_filename,error_function,error_line_number; category_id=<id>; error_code=4; error_description=Category not found.; error_filename=category.py; error_function=Category._get; error_line_number=427; says Category not found.",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ais_library_keys",
        each(
            Answer::Text(
                "_base_uri,_embedded,_links,agent_id,category_id,name,parent_id,type_default,version",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ais_library_status",
        each(Answer::Text("200"), Answer::Text("no capability")),
    ),
    (
        "ais_links_shape",
        each(
            Answer::Text(
                "{categories:{href:uri},category:{href:uri,name:string},children:{href:uri},items:{href:uri},links:{href:uri},parent:{href:uri},self:{href:uri}}",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ais_move_answer",
        each(
            Answer::Text(
                "400: error_code,error_description,error_filename,error_function,error_line_number; error_code=0; error_description=Cannot change parent_id.  Use MOVE method.  <id>, <id>; error_filename=categories.py; error_function=Category.mergeUntrusted; error_line_number=306; says Cannot change parent_id.  Use MOVE method.  <id>, <id> ",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ais_move_listed",
        each(Answer::Flag(false), Answer::Text("no capability")),
    ),
    (
        "ais_move_listed_ais",
        each(Answer::Flag(false), Answer::Text("no capability")),
    ),
    (
        "ais_move_method_absolute_answer",
        each(
            Answer::Text(
                "405: allowed_methods,error_code,error_description,error_filename,error_function,error_line_number,method,url; allowed_methods 7; error_code=7; error_description=Method Not Allowed; error_filename=access.py; error_function=Category._not_allowed; error_line_number=61; method=MOVE; url=/category/<id>; allowed_methods=[DELETE,GET,PATCH,POST,PUT,HEAD,OPTIONS]; says Method Not Allowed",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ais_move_method_moved_by",
        each(Answer::Text("neither"), Answer::Text("no capability")),
    ),
    (
        "ais_move_method_relative_answer",
        each(
            Answer::Text(
                "405: allowed_methods,error_code,error_description,error_filename,error_function,error_line_number,method,url; allowed_methods 7; error_code=7; error_description=Method Not Allowed; error_filename=access.py; error_function=Category._not_allowed; error_line_number=61; method=MOVE; url=/category/<id>; allowed_methods=[DELETE,GET,PATCH,POST,PUT,HEAD,OPTIONS]; says Method Not Allowed",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ais_move_root_step",
        each(Answer::Count(0), Answer::Text("no capability")),
    ),
    (
        "ais_move_root_step_ais",
        each(Answer::Count(0), Answer::Text("no capability")),
    ),
    (
        "ais_move_trash_step",
        each(Answer::Count(0), Answer::Text("no capability")),
    ),
    (
        "ais_move_trash_step_ais",
        each(Answer::Count(0), Answer::Text("no capability")),
    ),
    (
        "ais_remove_gone",
        each(Answer::Flag(true), Answer::Text("no capability")),
    ),
    (
        "ais_remove_listed_ais",
        each(Answer::Flag(false), Answer::Text("no capability")),
    ),
    (
        "ais_remove_root_step_ais",
        each(Answer::Count(1), Answer::Text("no capability")),
    ),
    (
        "ais_remove_trash_step",
        each(Answer::Count(0), Answer::Text("no capability")),
    ),
    (
        "ais_remove_trash_step_ais",
        each(Answer::Count(0), Answer::Text("no capability")),
    ),
    (
        "ais_unknown_folder",
        each(
            Answer::Text(
                "404: category_id,error_code,error_description,error_filename,error_function,error_line_number; category_id=<id>; error_code=4; error_description=Category not found.; error_filename=category.py; error_function=Category._get; error_line_number=427; says Category not found.",
            ),
            Answer::Text("no capability"),
        ),
    ),
    ("cap_CreateInventoryCategory", both(Answer::Flag(true))),
    ("cap_FetchInventory2", both(Answer::Flag(true))),
    ("cap_FetchInventoryDescendents2", both(Answer::Flag(true))),
    (
        "cap_FetchLib2",
        each(Answer::Flag(true), Answer::Flag(false)),
    ),
    (
        "cap_FetchLibDescendents2",
        each(Answer::Flag(true), Answer::Flag(false)),
    ),
    (
        "cap_InventoryAPIv3",
        each(Answer::Flag(true), Answer::Flag(false)),
    ),
    (
        "cap_LibraryAPIv3",
        each(Answer::Flag(true), Answer::Flag(false)),
    ),
    ("cap_create_answered", both(Answer::Flag(true))),
    ("cap_create_listed", both(Answer::Flag(true))),
    ("cap_create_root_step", both(Answer::Count(1))),
    (
        "fi2_library_item",
        both(Answer::Text("200: agent_id,items; items 0")),
    ),
    ("fi2_status", both(Answer::Text("200"))),
    ("fi2_top_keys", both(Answer::Text("agent_id,items"))),
    (
        "fi2_unknown_item",
        both(Answer::Text("200: agent_id,items; items 0")),
    ),
    ("fi2_vs_folder_fetch", both(Answer::Text("identical"))),
    (
        "fid2_category_keys",
        each(
            Answer::Text("agent_id,category_id,name,parent_id,type_default,version"),
            Answer::Text("category_id,name,parent_id,type_default,version"),
        ),
    ),
    (
        "fid2_category_shape",
        each(
            Answer::Text(
                "{agent_id:string,category_id:string,name:string,parent_id:string,type_default:integer,version:integer}",
            ),
            Answer::Text(
                "{category_id:uuid,name:string,parent_id:uuid,type_default:integer,version:integer}",
            ),
        ),
    ),
    (
        "fid2_descendents",
        both(Answer::Text("sub-folders plus items")),
    ),
    (
        "fid2_folder_shape",
        each(
            Answer::Text(
                "{agent_id:string,categories:array,descendents:integer,folder_id:uuid,items:array,owner_id:uuid,version:integer}",
            ),
            Answer::Text(
                "{agent_id:uuid,categories:array,descendents:integer,folder_id:uuid,items:array,owner_id:uuid,version:integer}",
            ),
        ),
    ),
    (
        "fid2_item_shape",
        each(
            Answer::Text(
                "{agent_id:string,asset_id:string,created_at:integer,desc:string,flags:integer,inv_type:integer,item_id:string,metadata:{},name:string,parent_id:string,permissions:{base_mask:integer,creator_id:string,everyone_mask:integer,group_id:string,group_mask:integer,last_owner_id:string,next_owner_mask:integer,owner_id:string,owner_mask:integer},sale_info:{sale_price:integer,sale_type:integer},type:integer}",
            ),
            Answer::Text(
                "{asset_id:uuid,created_at:integer,desc:string,flags:integer,inv_type:integer,item_id:uuid,name:string,parent_id:uuid,permissions:{base_mask:integer,creator_id:uuid,everyone_mask:integer,group_id:uuid,group_mask:integer,is_owner_group:boolean,next_owner_mask:integer,owner_id:uuid,owner_mask:integer},sale_info:{sale_price:integer,sale_type:integer},type:integer}",
            ),
        ),
    ),
    (
        "fid2_library_folder",
        both(Answer::Text("200: folders; folders 1")),
    ),
    (
        "fid2_library_folder_listed",
        each(
            Answer::Text("an entry at version 0"),
            Answer::Text("the Library's root listed"),
        ),
    ),
    ("fid2_owner", both(Answer::Text("the agent"))),
    ("fid2_status", both(Answer::Text("200"))),
    ("fid2_top_keys", both(Answer::Text("folders"))),
    (
        "fid2_unknown_folder",
        both(Answer::Text("200: folders; folders 1")),
    ),
    (
        "fid2_unknown_folder_entry",
        each(
            Answer::Text(
                "{agent_id:string,categories:array,descendents:integer,folder_id:uuid,items:array,owner_id:uuid,version:integer}",
            ),
            Answer::Text(
                "{agent_id:uuid,categories:array,descendents:integer,folder_id:uuid,items:array,owner_id:uuid,version:integer}",
            ),
        ),
    ),
    (
        "fid2_unknown_folder_values",
        both(Answer::Text(
            "version 0, descendents 0, 0 sub-folders, 0 items, agent_id the agent, owner_id the agent",
        )),
    ),
    (
        "fl2_status",
        each(Answer::Text("200"), Answer::Text("no capability")),
    ),
    (
        "fl2_vs_folder_fetch",
        each(Answer::Text("identical"), Answer::Text("no capability")),
    ),
    ("item_copy_made", both(Answer::Flag(true))),
    (
        "library_crawl_cap",
        each(
            Answer::Text("FetchLibDescendents2"),
            Answer::Text("FetchInventoryDescendents2"),
        ),
    ),
    (
        "library_crawl_category_keys",
        each(
            Answer::Text("agent_id,category_id,name,parent_id,type_default,version"),
            Answer::Text("category_id,name,parent_id,type_default,version"),
        ),
    ),
    (
        "library_crawl_folder_shape",
        each(
            Answer::Text(
                "{agent_id:string,categories:array,descendents:integer,folder_id:uuid,items:array,owner_id:uuid,version:integer}",
            ),
            Answer::Text(
                "{agent_id:uuid,categories:array,descendents:integer,folder_id:uuid,items:array,owner_id:uuid,version:integer}",
            ),
        ),
    ),
    ("library_crawl_nil_folders", both(Answer::Count(0))),
    ("library_crawl_refused", both(Answer::Count(0))),
    ("library_crawl_status", both(Answer::Text("200"))),
    ("library_owner", both(Answer::Text("another agent"))),
    (
        "library_skeleton_fields",
        both(Answer::Text(
            "folder_id,name,parent_id,type_default,version",
        )),
    ),
    ("library_skeleton_present", both(Answer::Flag(true))),
    ("library_skeleton_root_parent", both(Answer::Text("nil"))),
    ("library_skeleton_root_type", both(Answer::Count(8))),
    (
        "library_skeleton_versions_positive",
        both(Answer::Flag(true)),
    ),
    (
        "library_skeleton_vs_crawl_folders",
        both(Answer::Text("the same folders")),
    ),
    (
        "library_skeleton_vs_crawl_parents",
        both(Answer::Text("every parent equal")),
    ),
    (
        "library_skeleton_vs_crawl_versions",
        both(Answer::Text("every version equal")),
    ),
    ("relogin_fetch_root_step", both(Answer::Count(0))),
    ("relogin_skeleton_root_step", both(Answer::Count(0))),
    ("relogin_skeleton_trash_step", both(Answer::Count(0))),
    (
        "skeleton_fields",
        both(Answer::Text(
            "folder_id,name,parent_id,type_default,version",
        )),
    ),
    ("skeleton_present", both(Answer::Flag(true))),
    ("skeleton_root_parent", both(Answer::Text("nil"))),
    ("skeleton_root_type", both(Answer::Count(8))),
    ("skeleton_root_vs_fetch", both(Answer::Count(0))),
    ("skeleton_versions_positive", both(Answer::Flag(true))),
    (
        "skeleton_vs_crawl_folders",
        both(Answer::Text("the same folders")),
    ),
    (
        "skeleton_vs_crawl_parents",
        both(Answer::Text("every parent equal")),
    ),
    (
        "skeleton_vs_crawl_versions",
        both(Answer::Text("every version equal")),
    ),
    ("udp_blacklist_inventory", both(Answer::Text("none"))),
    ("udp_create_listed", both(Answer::Flag(true))),
    ("udp_create_root_step", both(Answer::Count(1))),
    (
        "udp_descendents",
        each(
            Answer::Text("nothing"),
            Answer::Text("InventoryDescendents"),
        ),
    ),
    (
        "udp_item",
        each(Answer::Text("nothing"), Answer::Text("FetchInventoryReply")),
    ),
    ("udp_item_move_moved", both(Answer::Flag(true))),
    (
        "udp_library_descendents",
        each(
            Answer::Text("nothing"),
            Answer::Text("InventoryDescendents"),
        ),
    ),
    ("udp_move_listed", both(Answer::Flag(true))),
    (
        "udp_move_listed_ais",
        each(Answer::Flag(true), Answer::Text("no capability")),
    ),
    ("udp_move_root_step", both(Answer::Count(1))),
    (
        "udp_move_root_step_ais",
        each(Answer::Count(1), Answer::Text("no capability")),
    ),
    ("udp_move_trash_step", both(Answer::Count(1))),
    (
        "udp_move_trash_step_ais",
        each(Answer::Count(1), Answer::Text("no capability")),
    ),
    ("udp_remove_gone", both(Answer::Flag(true))),
    (
        "udp_remove_listed_ais",
        each(Answer::Flag(false), Answer::Text("no capability")),
    ),
    (
        "udp_remove_root_step_ais",
        each(Answer::Count(0), Answer::Text("no capability")),
    ),
    (
        "udp_remove_trash_step",
        each(Answer::Count(1), Answer::Count(0)),
    ),
    (
        "udp_remove_trash_step_ais",
        each(Answer::Count(1), Answer::Text("no capability")),
    ),
    // Second Life stated 0 on six runs and -1 on the last two: not a
    // relation to hold.
    (
        "udp_rename_folder_step",
        each(Answer::Any, Answer::Count(0)),
    ),
    ("udp_rename_listed", both(Answer::Flag(true))),
    ("udp_rename_root_step", both(Answer::Count(1))),
    ("udp_subject", both(Answer::Text("the UDP-made folder"))),
];

/// The metrics a fake flavour is not held to yet, by prefix, with the
/// flavour (`None` for both) and why — each a gap a named task closes, or
/// the one deliberate deviation (`sl_fake_grid::LegacyUdpInventory`).
const FAKE_EXEMPT: &[(&str, Option<sl_fake_grid::ImitatedGrid>, &str)] = &[
    (
        "relogin_",
        None,
        "the fake grid seeds each login's inventory afresh, so versions fall back \
         (server-fake-grid-inventory-persists-across-logins)",
    ),
    (
        "item_copy_made",
        None,
        "the fake grid does not serve a UDP CopyInventoryItem \
         (server-fake-grid-inventory-udp-mutations)",
    ),
    (
        "ais_item_move_",
        None,
        "needs the copy the fake grid does not make (server-fake-grid-inventory-udp-mutations)",
    ),
    (
        "udp_item_move_moved",
        None,
        "needs the copy the fake grid does not make (server-fake-grid-inventory-udp-mutations)",
    ),
    (
        "library_crawl_category_keys",
        None,
        "the fixture Library has no sub-folder to state keys for",
    ),
    (
        "udp_descendents",
        Some(sl_fake_grid::ImitatedGrid::SecondLife),
        "the Second Life flavour refuses the deprecated fetch out loud (LegacyUdpInventory)",
    ),
    (
        "udp_library_descendents",
        Some(sl_fake_grid::ImitatedGrid::SecondLife),
        "the Second Life flavour refuses the deprecated fetch out loud (LegacyUdpInventory)",
    ),
];

/// Why `grid` is not held to the metric `key`, if it is a fake flavour
/// [`FAKE_EXEMPT`] names for it.
fn fake_exemption(grid: Grid, key: &str) -> Option<&'static str> {
    if !grid.is_fake() {
        return None;
    }
    FAKE_EXEMPT
        .iter()
        .find(|(prefix, flavour, _why)| {
            key.starts_with(prefix) && flavour.is_none_or(|flavour| flavour == grid.behaves_like())
        })
        .map(|(_prefix, _flavour, why)| *why)
}

/// The census of inventory fetching.
#[derive(Debug)]
pub struct InventoryFetch;

impl GridTest for InventoryFetch {
    fn name(&self) -> &'static str {
        "inventory-fetch"
    }

    fn description(&self) -> &'static str {
        "How each grid hands out inventory to read: the login skeleton, the fetch \
         capabilities, AIS3, the Library, UDP, and the folder versions"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi, Grid::FakeSl, Grid::FakeOpensim]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let mut metrics = Metrics::new();
            let outcome = census(ctx.primary(), &mut metrics).await;
            let grid = ctx.grid();
            ctx.metrics().merge(metrics.clone());
            let held: Vec<(&str, Measured<Answer>)> = MEASURED
                .iter()
                .filter(|(key, _measured)| fake_exemption(grid, key).is_none())
                .copied()
                .collect();
            outcome.and_then(|()| hold_to_measured(grid, &metrics, &held, SOURCE))
        })
    }
}

/// One raw HTTP answer.
#[derive(Debug, Clone)]
pub(crate) struct Raw {
    /// The status, or why there is none.
    pub(crate) status: Result<u16, String>,
    /// The body, where it was LLSD.
    pub(crate) body: Option<Llsd>,
}

impl Raw {
    /// The answer of a grid that grants no capability.
    pub(crate) fn ungranted() -> Self {
        Self {
            status: Err(NO_CAPABILITY.to_owned()),
            body: None,
        }
    }

    /// The status as a metric: the code, or the reason there is none.
    pub(crate) fn status_metric(&self) -> String {
        match &self.status {
            Ok(status) => status.to_string(),
            Err(reason) => reason.clone(),
        }
    }

    /// The keys of the body's top-level map, or what stands in for them.
    pub(crate) fn top_keys(&self) -> String {
        match (&self.status, &self.body) {
            (Err(reason), _) => reason.clone(),
            (Ok(_), Some(body)) => keys_of(body),
            (Ok(_), None) => "no LLSD".to_owned(),
        }
    }

    /// One line for the whole answer: status, top keys and, where the body
    /// names a refused entry, that entry's keys.
    pub(crate) fn summary(&self, refused: &str) -> String {
        let Ok(status) = &self.status else {
            return self.status_metric();
        };
        let mut line = format!("{status}: {}", self.top_keys());
        if let Some(map) = self.body.as_ref().and_then(Llsd::as_map) {
            let arrays: BTreeMap<&str, usize> = map
                .iter()
                .filter_map(|(key, value)| Some((key.as_str(), value.as_array()?.len())))
                .collect();
            for (key, len) in arrays {
                let _written = write!(line, "; {key} {len}");
            }
            // An error states its fields: every scalar, ids scrubbed.
            if *status >= 400 {
                let scalars: BTreeMap<&str, String> = map
                    .iter()
                    .filter(|(_key, value)| !matches!(value, Llsd::Map(_) | Llsd::Array(_)))
                    .map(|(key, value)| {
                        let text = match value {
                            Llsd::String(text) | Llsd::Uri(text) | Llsd::Date(text) => text.clone(),
                            Llsd::Integer(number) => number.to_string(),
                            Llsd::Boolean(flag) => flag.to_string(),
                            Llsd::Uuid(id) => id.to_string(),
                            other => shape_of(other),
                        };
                        (key.as_str(), without_ids(text.trim()))
                    })
                    .collect();
                for (key, value) in scalars {
                    let _written = write!(line, "; {key}={value}");
                }
                // A short list of words (Second Life's `allowed_methods`) is
                // stated whole, in order.
                for (key, value) in map.iter().collect::<BTreeMap<_, _>>() {
                    if let Llsd::Array(entries) = value
                        && entries.len() <= 10
                        && let Some(words) = entries
                            .iter()
                            .map(Llsd::as_str)
                            .collect::<Option<Vec<&str>>>()
                    {
                        let _written = write!(line, "; {key}=[{}]", words.join(","));
                    }
                }
            }
        }
        if let Some(entry) = self
            .body
            .as_ref()
            .and_then(|body| body.get(refused))
            .and_then(Llsd::as_array)
            .and_then(<[Llsd]>::first)
        {
            let _written = write!(line, "; {refused} entry {}", keys_of(entry));
            if let Some(error) = entry.get("error").and_then(Llsd::as_str) {
                let _written = write!(line, " ({})", without_ids(error));
            }
        }
        if let Some(message) = self
            .body
            .as_ref()
            .and_then(|body| {
                body.get("error_description")
                    .or_else(|| body.get("message"))
            })
            .and_then(Llsd::as_str)
        {
            let _written = write!(line, "; says {}", without_ids(message));
        }
        line
    }
}

/// What a crawl of one tree found.
#[derive(Debug, Default)]
struct Crawl {
    /// Every folder read: its version, parent and depth below the root.
    folders: BTreeMap<Uuid, Folder>,
    /// Every item read, by id, as the folder fetch stated it.
    items: BTreeMap<Uuid, Llsd>,
    /// How many listings named a sub-folder with the nil id.
    nil_folders: usize,
    /// How many folders the fetch refused.
    refused: usize,
    /// The first listing's answer, for its shape.
    first: Option<Raw>,
    /// The union of the keys a sub-folder entry carried.
    category_keys: BTreeSet<String>,
    /// The capability the crawl read through.
    cap: &'static str,
}

/// One folder of a crawl.
#[derive(Debug, Clone, Copy)]
struct Folder {
    /// The folder's version, as its own listing states it.
    version: Option<i32>,
    /// The parent, as the listing that named it states it.
    parent: Option<Uuid>,
    /// How far below the root it sits.
    depth: usize,
}

/// Everything the legs share.
pub(crate) struct Census<'a> {
    /// The avatar's session.
    pub(crate) a: &'a mut Session,
    /// The agent's id.
    pub(crate) agent: Uuid,
    /// The session id, for a raw message.
    pub(crate) session_id: Uuid,
    /// The HTTP client.
    pub(crate) http: reqwest::Client,
    /// A tag naming this run's folders.
    pub(crate) tag: String,
}

impl Census<'_> {
    /// POSTs `body` to the capability `name`, if granted.
    pub(crate) async fn post(&self, name: &str, body: &Llsd) -> Raw {
        let Some(url) = self.a.cap(name) else {
            return Raw::ungranted();
        };
        let request = self
            .http
            .post(url)
            .header(ACCEPT, "application/llsd+xml")
            .header(CONTENT_TYPE, "application/llsd+xml")
            .body(body.to_llsd_xml());
        answer(request).await
    }

    /// GETs `suffix` under the capability `name`, if granted.
    pub(crate) async fn get(&self, name: &str, suffix: &str) -> Raw {
        let Some(url) = self.a.cap(name) else {
            return Raw::ungranted();
        };
        let request = self
            .http
            .get(format!("{url}{suffix}"))
            .header(ACCEPT, "application/llsd+xml");
        answer(request).await
    }

    /// Sends `method` with an LLSD `body` to `suffix` under the capability
    /// `name`, if granted.
    pub(crate) async fn send_llsd(
        &self,
        name: &str,
        method: reqwest::Method,
        suffix: &str,
        body: &str,
    ) -> Raw {
        let Some(url) = self.a.cap(name) else {
            return Raw::ungranted();
        };
        let request = self
            .http
            .request(method, format!("{url}{suffix}"))
            .header(ACCEPT, "application/llsd+xml")
            .header(CONTENT_TYPE, "application/llsd+xml")
            .body(body.to_owned());
        answer(request).await
    }

    /// An AIS3 `MOVE` of `suffix` to `destination` (the `Destination`
    /// header), if AIS3 is granted.
    pub(crate) async fn move_method(&self, suffix: &str, destination: &str) -> Raw {
        let Some(url) = self.a.cap(AIS) else {
            return Raw::ungranted();
        };
        let Ok(method) = reqwest::Method::from_bytes(b"MOVE") else {
            return Raw {
                status: Err("no MOVE method".to_owned()),
                body: None,
            };
        };
        let request = self
            .http
            .request(method, format!("{url}{suffix}"))
            .header(ACCEPT, "application/llsd+xml")
            .header("Destination", destination);
        answer(request).await
    }

    /// The folder fetch of `folders`, all owned by `owner`, through `cap`.
    pub(crate) async fn fetch_folders(&self, cap: &str, owner: Uuid, folders: &[Uuid]) -> Raw {
        let entries = folders
            .iter()
            .map(|folder| {
                map([
                    ("folder_id", Llsd::Uuid(*folder)),
                    ("owner_id", Llsd::Uuid(owner)),
                    ("fetch_folders", Llsd::Boolean(true)),
                    ("fetch_items", Llsd::Boolean(true)),
                    ("sort_order", Llsd::Integer(0)),
                ])
            })
            .collect();
        self.post(cap, &map([("folders", Llsd::Array(entries))]))
            .await
    }

    /// The item fetch of `item`, owned by `owner`, through `cap`.
    pub(crate) async fn fetch_item(&self, cap: &str, owner: Uuid, item: Uuid) -> Raw {
        let body = map([
            ("agent_id", Llsd::Uuid(self.agent)),
            (
                "items",
                Llsd::Array(vec![map([
                    ("owner_id", Llsd::Uuid(owner)),
                    ("item_id", Llsd::Uuid(item)),
                ])]),
            ),
        ]);
        self.post(cap, &body).await
    }

    /// The versions of `folders` through the agent's folder fetch.
    pub(crate) async fn versions(&self, folders: &[Uuid]) -> HashMap<Uuid, i32> {
        let raw = self.fetch_folders(FID2, self.agent, folders).await;
        listed_folders(raw.body.as_ref())
            .filter_map(|entry| {
                Some((
                    uuid_of(entry.get("folder_id")?)?,
                    entry.get("version")?.as_i32()?,
                ))
            })
            .collect()
    }

    /// The names of the sub-folders the agent fetch lists under `folder`,
    /// with their ids.
    pub(crate) async fn children(&self, folder: Uuid) -> Vec<(String, Uuid)> {
        let raw = self.fetch_folders(FID2, self.agent, &[folder]).await;
        listed_folders(raw.body.as_ref())
            .flat_map(|entry| {
                entry
                    .get("categories")
                    .and_then(Llsd::as_array)
                    .unwrap_or_default()
            })
            .filter_map(|category| {
                Some((
                    category.get("name")?.as_str()?.to_owned(),
                    category_id(category)?,
                ))
            })
            .collect()
    }

    /// The names and ids of the sub-folders AIS3 lists under `folder`, or
    /// `None` where AIS3 is not granted or gave no listing.
    pub(crate) async fn ais_children(&self, folder: Uuid) -> Option<Vec<(String, Uuid)>> {
        self.a.cap(AIS)?;
        let raw = self
            .get(AIS, &format!("/category/{folder}/children?depth=0"))
            .await;
        let categories = raw
            .body
            .as_ref()?
            .get("_embedded")?
            .get("categories")?
            .as_map()?;
        Some(
            categories
                .values()
                .filter_map(|category| {
                    Some((
                        category.get("name")?.as_str()?.to_owned(),
                        category.get("category_id").and_then(uuid_of)?,
                    ))
                })
                .collect(),
        )
    }

    /// The versions of `folders` as AIS3 states them, one `GET` each; empty
    /// where AIS3 is not granted.
    pub(crate) async fn ais_versions(&self, folders: &[Uuid]) -> HashMap<Uuid, i32> {
        let mut versions = HashMap::new();
        for folder in folders {
            let raw = self.get(AIS, &format!("/category/{folder}")).await;
            if let Some(version) = raw
                .body
                .as_ref()
                .and_then(|body| body.get("version"))
                .and_then(Llsd::as_i32)
            {
                let _version = versions.insert(*folder, version);
            }
        }
        versions
    }

    /// The sub-folders under `folder` by `view`: AIS3, or the folder fetch.
    pub(crate) async fn listing(&self, folder: Uuid, view: View) -> Vec<(String, Uuid)> {
        match view {
            View::Ais => self.ais_children(folder).await.unwrap_or_default(),
            View::Fetch => self.children(folder).await,
        }
    }

    /// Polls `folder`'s folder-fetch listing until `wanted` says it is
    /// right, or [`SETTLE`] passes; whether it came right.
    pub(crate) async fn settle(
        &self,
        folder: Uuid,
        wanted: impl Fn(&[(String, Uuid)]) -> bool + Send + Sync,
    ) -> bool {
        self.settle_by(folder, View::Fetch, wanted).await
    }

    /// [`Self::settle`] by `view`.
    pub(crate) async fn settle_by(
        &self,
        folder: Uuid,
        view: View,
        wanted: impl Fn(&[(String, Uuid)]) -> bool + Send + Sync,
    ) -> bool {
        let started = Instant::now();
        loop {
            if wanted(&self.listing(folder, view).await) {
                return true;
            }
            if started.elapsed() > SETTLE {
                return false;
            }
            tokio::time::sleep(POLL).await;
        }
    }

    /// Lets the session take in what arrives for `window`.
    pub(crate) async fn idle(&mut self, window: Duration) {
        let _idle = self.a.wait_for(window, |_event| None::<()>).await;
    }

    /// A folder name of this run's.
    pub(crate) fn name(&self, word: &str) -> String {
        format!("conf-inv-{}-{word}", self.tag)
    }

    /// Sends a raw UDP message.
    pub(crate) async fn send_raw(&self, message: AnyMessage) -> Result<(), TestFailure> {
        self.a
            .send(Command::Send {
                message: Box::new(message),
                reliability: Reliability::Reliable,
            })
            .await
    }

    /// Sends `message` raw and names the inventory datagrams that arrived in
    /// [`UDP_WINDOW`], or `nothing`, with how many arrived: how a listing is
    /// split into packets is the tree's, not the grid's.
    pub(crate) async fn udp_answer(
        &mut self,
        message: AnyMessage,
    ) -> Result<(String, usize), TestFailure> {
        let before = self.a.diagnostics().len();
        self.send_raw(message).await?;
        self.idle(UDP_WINDOW).await;
        let mut seen: BTreeMap<&'static str, usize> = BTreeMap::new();
        for diagnostic in self.a.diagnostics().iter().skip(before) {
            if let Diagnostic::Datagram {
                name: Some(name),
                child: false,
                ..
            } = diagnostic
                && (name.contains("Inventory")
                    || *name == "FeatureDisabled"
                    || *name == "AlertMessage")
            {
                let count = seen.entry(name).or_default();
                *count = count.saturating_add(1);
            }
        }
        if seen.is_empty() {
            return Ok(("nothing".to_owned(), 0));
        }
        let total = seen.values().copied().fold(0_usize, usize::saturating_add);
        Ok((seen.keys().copied().collect::<Vec<_>>().join(", "), total))
    }

    /// Crawls the tree under `root`, owned by `owner`, through `cap`.
    async fn crawl(&self, cap: &'static str, owner: Uuid, root: Uuid) -> Crawl {
        let mut crawl = Crawl {
            cap,
            ..Crawl::default()
        };
        let _root = crawl.folders.insert(
            root,
            Folder {
                version: None,
                parent: None,
                depth: 0,
            },
        );
        let mut queue: VecDeque<Uuid> = VecDeque::from([root]);
        while !queue.is_empty() && crawl.folders.len() <= MAX_FOLDERS {
            let batch: Vec<Uuid> = std::iter::from_fn(|| queue.pop_front())
                .take(BATCH)
                .collect();
            let raw = self.fetch_folders(cap, owner, &batch).await;
            if crawl.first.is_none() {
                crawl.first = Some(raw.clone());
            }
            crawl.refused = crawl.refused.saturating_add(
                raw.body
                    .as_ref()
                    .and_then(|body| body.get("bad_folders"))
                    .and_then(Llsd::as_array)
                    .map_or(0, <[Llsd]>::len),
            );
            for entry in listed_folders(raw.body.as_ref()) {
                let Some(id) = entry.get("folder_id").and_then(uuid_of) else {
                    continue;
                };
                let depth = crawl.folders.get(&id).map_or(0, |folder| folder.depth);
                if let Some(folder) = crawl.folders.get_mut(&id) {
                    folder.version = entry.get("version").and_then(Llsd::as_i32);
                }
                for category in entry
                    .get("categories")
                    .and_then(Llsd::as_array)
                    .unwrap_or_default()
                {
                    if let Some(keys) = category.as_map() {
                        crawl.category_keys.extend(keys.keys().cloned());
                    }
                    let Some(child) = category_id(category) else {
                        continue;
                    };
                    if child.is_nil() {
                        crawl.nil_folders = crawl.nil_folders.saturating_add(1);
                        continue;
                    }
                    if let std::collections::btree_map::Entry::Vacant(slot) =
                        crawl.folders.entry(child)
                    {
                        let _new = slot.insert(Folder {
                            version: None,
                            parent: category.get("parent_id").and_then(uuid_of),
                            depth: depth.saturating_add(1),
                        });
                        queue.push_back(child);
                    }
                }
                for item in entry
                    .get("items")
                    .and_then(Llsd::as_array)
                    .unwrap_or_default()
                {
                    if let Some(id) = item.get("item_id").and_then(uuid_of) {
                        let _item = crawl.items.insert(id, item.clone());
                    }
                }
            }
            if raw.status != Ok(200) {
                break;
            }
        }
        crawl
    }
}

/// Which listing a leg reads a folder through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum View {
    /// `FetchInventoryDescendents2`.
    Fetch,
    /// AIS3's children listing.
    Ais,
}

/// Reads one HTTP answer.
pub(crate) async fn answer(request: reqwest::RequestBuilder) -> Raw {
    match request.send().await {
        Ok(response) => {
            let status = response.status().as_u16();
            let text = response.text().await.unwrap_or_default();
            let body = Some(text.as_str())
                .filter(|text| text.contains("<llsd"))
                .and_then(|text| parse_llsd_xml(text).ok());
            Raw {
                status: Ok(status),
                body,
            }
        }
        Err(error) => Raw {
            status: Err(format!("no answer: {}", without_ids(&error.to_string()))),
            body: None,
        },
    }
}

/// An LLSD map of `entries`.
pub(crate) fn map<const N: usize>(entries: [(&str, Llsd); N]) -> Llsd {
    Llsd::Map(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
    )
}

/// The sorted keys of an LLSD map, `,`-joined; `not a map` for anything else.
pub(crate) fn keys_of(value: &Llsd) -> String {
    value.as_map().map_or_else(
        || "not a map".to_owned(),
        |map| {
            let keys: BTreeSet<&str> = map.keys().map(String::as_str).collect();
            if keys.is_empty() {
                "no keys".to_owned()
            } else {
                keys.into_iter().collect::<Vec<_>>().join(",")
            }
        },
    )
}

/// A UUID an LLSD value states, as a UUID or a string.
pub(crate) fn uuid_of(value: &Llsd) -> Option<Uuid> {
    match value {
        Llsd::Uuid(id) => Some(*id),
        Llsd::String(text) => Uuid::parse_str(text.trim()).ok(),
        _other => None,
    }
}

/// The id of a folder-fetch sub-folder entry, under either name a grid uses.
pub(crate) fn category_id(category: &Llsd) -> Option<Uuid> {
    category
        .get("category_id")
        .or_else(|| category.get("folder_id"))
        .and_then(uuid_of)
}

/// The folder entries of a folder-fetch answer.
pub(crate) fn listed_folders(body: Option<&Llsd>) -> impl Iterator<Item = &Llsd> {
    body.and_then(|body| body.get("folders"))
        .and_then(Llsd::as_array)
        .unwrap_or_default()
        .iter()
}

/// The keys of a map whose values differ between `a` and `b`, or that only
/// one of them has, sorted; `not maps` when either is not one.
fn differing_keys(a: &Llsd, b: &Llsd) -> String {
    let (Some(a), Some(b)) = (a.as_map(), b.as_map()) else {
        return "not maps".to_owned();
    };
    let keys: BTreeSet<&String> = a.keys().chain(b.keys()).collect();
    let differing: Vec<&str> = keys
        .into_iter()
        .filter(|key| a.get(*key) != b.get(*key))
        .map(String::as_str)
        .collect();
    if differing.is_empty() {
        "identical".to_owned()
    } else {
        format!("differs in {}", differing.join(","))
    }
}

/// The shape of an LLSD value as [`shape_of`] states it, but with every
/// array named `array` rather than counted: how long a listing is belongs to
/// the tree, not the grid.
pub(crate) fn flat_shape(value: &Llsd) -> String {
    match value {
        Llsd::Map(map) => {
            let entries: BTreeMap<&String, &Llsd> = map.iter().collect();
            let entries: Vec<String> = entries
                .into_iter()
                .map(|(key, value)| format!("{key}:{}", flat_shape(value)))
                .collect();
            format!("{{{}}}", entries.join(","))
        }
        Llsd::Array(_items) => "array".to_owned(),
        scalar => shape_of(scalar),
    }
}

/// The values of a folder-fetch entry that say what it is, in words that
/// name no id: its version and descendent count, how many sub-folders and
/// items it lists, and whom `agent_id` and `owner_id` name.
fn entry_values(entry: &Llsd, agent: Uuid) -> String {
    let number = |key: &str| {
        entry
            .get(key)
            .and_then(Llsd::as_i32)
            .map_or_else(|| "none".to_owned(), |value| value.to_string())
    };
    let length = |key: &str| {
        entry
            .get(key)
            .and_then(Llsd::as_array)
            .map_or(0, <[Llsd]>::len)
    };
    let who = |key: &str| match entry.get(key).and_then(uuid_of) {
        None => "none",
        Some(id) if id == agent => "the agent",
        Some(id) if id.is_nil() => "nil",
        Some(_id) => "another id",
    };
    format!(
        "version {}, descendents {}, {} sub-folders, {} items, agent_id {}, owner_id {}",
        number("version"),
        number("descendents"),
        length("categories"),
        length("items"),
        who("agent_id"),
        who("owner_id"),
    )
}

/// A count as a metric.
pub(crate) fn count(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

/// A version step as a metric: how far `after` is from `before`, or
/// `-1000` where either was not read.
pub(crate) fn step(before: Option<i32>, after: Option<i32>) -> i64 {
    match (before, after) {
        (Some(before), Some(after)) => i64::from(after).saturating_sub(i64::from(before)),
        _ => -1000,
    }
}

/// Runs every leg, recording into `metrics`.
async fn census(session: &mut Session, metrics: &mut Metrics) -> Result<(), TestFailure> {
    session.wait_for_region(REGION_TIMEOUT).await?;
    let started = Instant::now();
    while !CAPABILITIES.iter().all(|name| session.cap(name).is_some())
        && started.elapsed() < CAP_WAIT
    {
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    for name in CAPABILITIES {
        metrics.set(&format!("cap_{name}"), session.cap(name).is_some());
    }
    let agent = session
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?
        .uuid();
    let session_id = session
        .session_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no session id".to_owned()))?;
    let login = session
        .login_success()
        .cloned()
        .ok_or_else(|| TestFailure::Assertion("no login answer kept".to_owned()))?;
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
    let mut census = Census {
        a: session,
        agent,
        session_id,
        http,
        tag,
    };
    census
        .a
        .send(Command::ProbeCircuits(CircuitProbe::Observe))
        .await?;
    let outcome = legs(&mut census, &login, metrics).await;
    census
        .a
        .send(Command::ProbeCircuits(CircuitProbe::Off))
        .await?;
    outcome
}

/// Every leg, in order.
async fn legs(
    census: &mut Census<'_>,
    login: &sl_client_tokio::LoginSuccess,
    metrics: &mut Metrics,
) -> Result<(), TestFailure> {
    let root = login
        .inventory_root
        .ok_or_else(|| TestFailure::Assertion("login named no inventory root".to_owned()))?
        .uuid();
    let trash = login
        .inventory_skeleton
        .iter()
        .find(|folder| FolderType::from_code(folder.type_default) == FolderType::Trash)
        .map(|folder| folder.folder_id.uuid())
        .ok_or_else(|| TestFailure::Assertion("the skeleton named no Trash".to_owned()))?;

    // --- 1/3. The skeleton against a crawl of each tree. ---
    let agent_crawl = census.crawl(FID2, census.agent, root).await;
    skeleton(
        metrics,
        "skeleton",
        &login.inventory_skeleton,
        &login.inventory_skeleton_fields,
        root,
        &agent_crawl,
    );
    metrics.set(
        "udp_blacklist_inventory",
        Some(
            login
                .udp_blacklist
                .iter()
                .filter(|name| name.contains("Inventory"))
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(","),
        )
        .filter(|names| !names.is_empty())
        .unwrap_or_else(|| "none".to_owned()),
    );
    folder_fetch(census, metrics, root, &agent_crawl).await;

    let library = login.library_root.map(|root| root.uuid());
    let owner = login.library_owner.map(|owner| owner.uuid());
    metrics.set(
        "library_owner",
        match owner {
            None => "none",
            Some(owner) if owner == census.agent => "the agent",
            Some(_owner) => "another agent",
        },
    );
    let library_crawl = match (library, owner) {
        (Some(library), Some(owner)) => {
            let cap = if census.a.cap(FLD2).is_some() {
                FLD2
            } else {
                FID2
            };
            Some(census.crawl(cap, owner, library).await)
        }
        _ => None,
    };
    match (&library_crawl, library) {
        (Some(crawl), Some(library)) => {
            skeleton(
                metrics,
                "library_skeleton",
                &login.library_skeleton,
                &login.library_skeleton_fields,
                library,
                crawl,
            );
            metrics.set("library_crawl_cap", crawl.cap);
            metrics.set(
                "library_crawl_status",
                crawl
                    .first
                    .as_ref()
                    .map_or_else(|| "nothing asked".to_owned(), Raw::status_metric),
            );
            metrics.set("library_crawl_nil_folders", count(crawl.nil_folders));
            metrics.set("library_crawl_refused", count(crawl.refused));
            metrics.set(
                "library_crawl_folder_shape",
                crawl
                    .first
                    .as_ref()
                    .and_then(|raw| listed_folders(raw.body.as_ref()).next())
                    .map_or_else(|| "none".to_owned(), flat_shape),
            );
            metrics.set("library_crawl_category_keys", joined(&crawl.category_keys));
        }
        _ => {
            for key in [
                "library_crawl_cap",
                "library_crawl_status",
                "library_crawl_folder_shape",
                "library_crawl_category_keys",
            ] {
                metrics.set(key, "no library");
            }
        }
    }
    // The Library asked through the agent's own capability.
    let (cross, listed) = match (library, owner) {
        (Some(library), Some(owner)) => {
            let raw = census.fetch_folders(FID2, owner, &[library]).await;
            let entry = listed_folders(raw.body.as_ref()).next();
            // Listed when the entry states the version the Library's own
            // crawl read for its root; an unknown folder's entry says 0.
            let read = library_crawl
                .as_ref()
                .and_then(|crawl| crawl.folders.get(&library))
                .and_then(|folder| folder.version);
            let stated = entry
                .and_then(|entry| entry.get("version"))
                .and_then(Llsd::as_i32);
            let listed = match (stated, read) {
                (Some(stated), Some(read)) if stated == read => {
                    "the Library's root listed".to_owned()
                }
                (Some(stated), _) => format!("an entry at version {stated}"),
                (None, _) => "no entry".to_owned(),
            };
            (raw.summary("bad_folders"), listed)
        }
        _ => ("no library".to_owned(), "no library".to_owned()),
    };
    metrics.set("fid2_library_folder", cross);
    metrics.set("fid2_library_folder_listed", listed);

    // --- 4. The item fetch. ---
    item_fetch(census, metrics, &agent_crawl, library_crawl.as_ref(), owner).await;

    // --- 5. UDP. ---
    udp(census, metrics, root, library, owner, &agent_crawl).await?;

    // --- 6. AIS3. ---
    ais(census, metrics, root, library, &agent_crawl).await;

    // --- 7. Versions. ---
    let subject = subject_item(&agent_crawl).map(|(id, _item)| id);
    versions(census, metrics, root, trash, subject).await
}

/// The `,`-joined members of a set, or `none`.
fn joined(set: &BTreeSet<String>) -> String {
    if set.is_empty() {
        "none".to_owned()
    } else {
        set.iter().map(String::as_str).collect::<Vec<_>>().join(",")
    }
}

/// Records, under `word`, what a login skeleton states and how it compares
/// with a crawl of its tree.
fn skeleton(
    metrics: &mut Metrics,
    word: &str,
    skeleton: &[sl_wire::SkeletonFolder],
    fields: &[String],
    root: Uuid,
    crawl: &Crawl,
) {
    metrics.set(&format!("{word}_present"), !skeleton.is_empty());
    metrics.set(
        &format!("{word}_fields"),
        if fields.is_empty() {
            "none".to_owned()
        } else {
            fields.join(",")
        },
    );
    let root_entry = skeleton
        .iter()
        .find(|folder| folder.folder_id.uuid() == root);
    metrics.set(
        &format!("{word}_root_type"),
        root_entry.map_or(-1000, |folder| i64::from(folder.type_default)),
    );
    metrics.set(
        &format!("{word}_root_parent"),
        root_entry.map_or("no root entry", |folder| {
            if folder.parent_id.uuid().is_nil() {
                "nil"
            } else {
                "another folder"
            }
        }),
    );
    let named: BTreeMap<Uuid, &sl_wire::SkeletonFolder> = skeleton
        .iter()
        .map(|folder| (folder.folder_id.uuid(), folder))
        .collect();
    let only_skeleton = named
        .keys()
        .filter(|id| !crawl.folders.contains_key(id))
        .count();
    let only_crawl = crawl
        .folders
        .keys()
        .filter(|id| !named.contains_key(id))
        .count();
    metrics.set(
        &format!("{word}_vs_crawl_folders"),
        if only_skeleton == 0 && only_crawl == 0 {
            "the same folders".to_owned()
        } else {
            format!("{only_skeleton} only in the skeleton, {only_crawl} only in the crawl")
        },
    );
    let mut version_differs = 0_usize;
    let mut unversioned = 0_usize;
    let mut parent_differs = 0_usize;
    for (id, folder) in &crawl.folders {
        let Some(entry) = named.get(id) else {
            continue;
        };
        match folder.version {
            Some(version) if version != entry.version => {
                version_differs = version_differs.saturating_add(1);
            }
            Some(_version) => {}
            None => unversioned = unversioned.saturating_add(1),
        }
        if folder
            .parent
            .is_some_and(|parent| parent != entry.parent_id.uuid())
        {
            parent_differs = parent_differs.saturating_add(1);
        }
    }
    metrics.set(
        &format!("{word}_vs_crawl_versions"),
        match (version_differs, unversioned) {
            (0, 0) => "every version equal".to_owned(),
            (differs, 0) => format!("{differs} differ"),
            (differs, unread) => format!("{differs} differ, {unread} unread"),
        },
    );
    metrics.set(
        &format!("{word}_vs_crawl_parents"),
        if parent_differs == 0 {
            "every parent equal".to_owned()
        } else {
            format!("{parent_differs} differ")
        },
    );
    metrics.set(
        &format!("{word}_versions_positive"),
        skeleton.iter().all(|folder| folder.version > 0),
    );
}

/// Leg 3: the agent folder fetch's shape.
async fn folder_fetch(census: &Census<'_>, metrics: &mut Metrics, root: Uuid, crawl: &Crawl) {
    let first = crawl.first.clone().unwrap_or_else(Raw::ungranted);
    metrics.set("fid2_status", first.status_metric());
    metrics.set("fid2_top_keys", first.top_keys());
    let root_entry = listed_folders(first.body.as_ref())
        .find(|entry| entry.get("folder_id").and_then(uuid_of) == Some(root));
    metrics.set(
        "fid2_folder_shape",
        root_entry.map_or_else(|| "no root entry".to_owned(), flat_shape),
    );
    metrics.set(
        "fid2_category_shape",
        root_entry
            .and_then(|entry| entry.get("categories"))
            .and_then(Llsd::as_array)
            .and_then(<[Llsd]>::first)
            .map_or_else(|| "no sub-folder".to_owned(), flat_shape),
    );
    metrics.set("fid2_category_keys", joined(&crawl.category_keys));
    metrics.set(
        "fid2_descendents",
        root_entry.map_or_else(
            || "no root entry".to_owned(),
            |entry| {
                let listed = ["categories", "items"]
                    .iter()
                    .map(|key| {
                        entry
                            .get(key)
                            .and_then(Llsd::as_array)
                            .map_or(0, <[Llsd]>::len)
                    })
                    .fold(0_usize, usize::saturating_add);
                match entry.get("descendents").and_then(Llsd::as_i32) {
                    None => "no count".to_owned(),
                    Some(stated) if usize::try_from(stated).ok() == Some(listed) => {
                        "sub-folders plus items".to_owned()
                    }
                    Some(stated) => format!("{stated} against {listed} listed"),
                }
            },
        ),
    );
    metrics.set(
        "fid2_owner",
        match root_entry.map(|entry| entry.get("owner_id").and_then(uuid_of)) {
            None => "no root entry",
            Some(None) => "not stated",
            Some(Some(owner)) if owner == census.agent => "the agent",
            Some(Some(_owner)) => "another agent",
        },
    );
    metrics.set("agent_crawl_nil_folders", count(crawl.nil_folders));
    metrics.set(
        "agent_crawl_height",
        count(
            crawl
                .folders
                .values()
                .map(|folder| folder.depth)
                .max()
                .unwrap_or(0),
        ),
    );
    metrics.set("agent_crawl_refused", count(crawl.refused));
    metrics.set(
        "fid2_item_shape",
        subject_item(crawl).map_or_else(|| "no item".to_owned(), |(_id, item)| shape_of(item)),
    );
    let unknown = census
        .fetch_folders(FID2, census.agent, &[Uuid::new_v4()])
        .await;
    metrics.set("fid2_unknown_folder", unknown.summary("bad_folders"));
    metrics.set(
        "fid2_unknown_folder_entry",
        listed_folders(unknown.body.as_ref())
            .next()
            .map_or_else(|| "none".to_owned(), flat_shape),
    );
    metrics.set(
        "fid2_unknown_folder_values",
        listed_folders(unknown.body.as_ref()).next().map_or_else(
            || "none".to_owned(),
            |entry| entry_values(entry, census.agent),
        ),
    );
}

/// The item the item legs fetch: the lowest-id non-link item of the crawl,
/// so a rerun picks the same one.
fn subject_item(crawl: &Crawl) -> Option<(Uuid, &Llsd)> {
    crawl
        .items
        .iter()
        .find(|(_id, item)| {
            item.get("type")
                .and_then(Llsd::as_i32)
                .is_none_or(|kind| !LINK_TYPES.contains(&kind))
        })
        .map(|(id, item)| (*id, item))
}

/// The first item of a fetch answer's `items`.
fn first_item(raw: &Raw) -> Option<&Llsd> {
    raw.body
        .as_ref()
        .and_then(|body| body.get("items"))
        .and_then(Llsd::as_array)
        .and_then(<[Llsd]>::first)
}

/// Leg 4: the item fetches.
async fn item_fetch(
    census: &Census<'_>,
    metrics: &mut Metrics,
    crawl: &Crawl,
    library: Option<&Crawl>,
    owner: Option<Uuid>,
) {
    match subject_item(crawl) {
        Some((id, listed)) => {
            let raw = census.fetch_item(FI2, census.agent, id).await;
            metrics.set("fi2_status", raw.status_metric());
            metrics.set("fi2_top_keys", raw.top_keys());
            metrics.set(
                "fi2_vs_folder_fetch",
                first_item(&raw).map_or_else(
                    || "no item".to_owned(),
                    |fetched| differing_keys(fetched, listed),
                ),
            );
        }
        None => {
            for key in ["fi2_status", "fi2_top_keys", "fi2_vs_folder_fetch"] {
                metrics.set(key, "no item");
            }
        }
    }
    let unknown = census.fetch_item(FI2, census.agent, Uuid::new_v4()).await;
    metrics.set("fi2_unknown_item", unknown.summary("bad_items"));
    match (library.and_then(subject_item), owner) {
        (Some((id, listed)), Some(owner)) => {
            let raw = census.fetch_item(FL2, owner, id).await;
            metrics.set("fl2_status", raw.status_metric());
            metrics.set(
                "fl2_vs_folder_fetch",
                first_item(&raw).map_or_else(
                    || raw.summary("bad_items"),
                    |fetched| differing_keys(fetched, listed),
                ),
            );
            let cross = census.fetch_item(FI2, owner, id).await;
            metrics.set(
                "fi2_library_item",
                first_item(&cross).map_or_else(
                    || cross.summary("bad_items"),
                    |fetched| {
                        format!(
                            "{}: {}",
                            cross.status_metric(),
                            differing_keys(fetched, listed)
                        )
                    },
                ),
            );
        }
        _ => {
            for key in ["fl2_status", "fl2_vs_folder_fetch", "fi2_library_item"] {
                metrics.set(key, "no library item");
            }
        }
    }
}

/// Leg 5: the UDP fetches, sent raw.
async fn udp(
    census: &mut Census<'_>,
    metrics: &mut Metrics,
    root: Uuid,
    library: Option<Uuid>,
    owner: Option<Uuid>,
    crawl: &Crawl,
) -> Result<(), TestFailure> {
    let descendents = |folder: Uuid, owner: Uuid, agent: Uuid, session: Uuid| {
        AnyMessage::FetchInventoryDescendents(FetchInventoryDescendents {
            agent_data: FetchInventoryDescendentsAgentDataBlock {
                agent_id: agent,
                session_id: session,
            },
            inventory_data: FetchInventoryDescendentsInventoryDataBlock {
                folder_id: folder,
                owner_id: owner,
                sort_order: 0,
                fetch_folders: true,
                fetch_items: true,
            },
        })
    };
    let (agent, session) = (census.agent, census.session_id);
    let (answer, datagrams) = census
        .udp_answer(descendents(root, agent, agent, session))
        .await?;
    metrics.set("udp_descendents", answer);
    metrics.set("udp_descendents_datagrams", count(datagrams));
    let answer = match (library, owner) {
        (Some(library), Some(owner)) => {
            census
                .udp_answer(descendents(library, owner, agent, session))
                .await?
        }
        _ => ("no library".to_owned(), 0),
    };
    metrics.set("udp_library_descendents", answer.0);
    metrics.set("udp_library_descendents_datagrams", count(answer.1));
    let answer = match subject_item(crawl) {
        Some((item, _listed)) => {
            census
                .udp_answer(AnyMessage::FetchInventory(FetchInventory {
                    agent_data: FetchInventoryAgentDataBlock {
                        agent_id: agent,
                        session_id: session,
                    },
                    inventory_data: vec![FetchInventoryInventoryDataBlock {
                        owner_id: agent,
                        item_id: item,
                    }],
                }))
                .await?
                .0
        }
        None => "no item".to_owned(),
    };
    metrics.set("udp_item", answer);
    Ok(())
}

/// What an AIS3 children listing reached: every category it embeds, by id,
/// with the level it sits at, and how many sit beside a folder that is not
/// their parent.
#[derive(Debug, Default)]
struct Reach {
    /// Each embedded category's level below the listed folder.
    levels: BTreeMap<Uuid, usize>,
    /// How many categories are embedded under a folder that is not their
    /// parent — a flat listing.
    misplaced: usize,
    /// The union of the keys a category carried.
    category_keys: BTreeSet<String>,
    /// The union of the keys an embedded item carried.
    item_keys: BTreeSet<String>,
    /// The union of the keys an embedded link carried.
    link_keys: BTreeSet<String>,
}

/// Walks an AIS3 listing's `_embedded` maps from `body`, a folder at `level`.
fn walk(body: &Llsd, folder: Option<Uuid>, level: usize, reach: &mut Reach) {
    let Some(embedded) = body.get("_embedded") else {
        return;
    };
    if let Some(categories) = embedded.get("categories").and_then(Llsd::as_map) {
        for category in categories.values() {
            if let Some(keys) = category.as_map() {
                reach.category_keys.extend(keys.keys().cloned());
            }
            let id = category.get("category_id").and_then(uuid_of);
            let parent = category.get("parent_id").and_then(uuid_of);
            if folder.is_some() && parent != folder {
                reach.misplaced = reach.misplaced.saturating_add(1);
            }
            if let Some(id) = id {
                let _level = reach.levels.insert(id, level.saturating_add(1));
            }
            walk(category, id, level.saturating_add(1), reach);
        }
    }
    for (kind, keys) in [
        ("items", &mut reach.item_keys),
        ("links", &mut reach.link_keys),
    ] {
        if let Some(entries) = embedded.get(kind).and_then(Llsd::as_map) {
            for entry in entries.values() {
                if let Some(map) = entry.as_map() {
                    keys.extend(map.keys().cloned());
                }
            }
        }
    }
}

/// The `_links` of an AIS3 answer, each `name=href` (and the `name` a link
/// carries), with the root's id written `<root>` and every other id `<id>`;
/// `none` where the answer has none.
fn links_of(body: Option<&Llsd>, root: Uuid) -> String {
    let Some(links) = body
        .and_then(|body| body.get("_links"))
        .and_then(Llsd::as_map)
    else {
        return "none".to_owned();
    };
    let links: BTreeMap<&String, &Llsd> = links.iter().collect();
    links
        .into_iter()
        .map(|(name, link)| {
            let text = |key: &str| {
                link.get(key)
                    .and_then(Llsd::as_str)
                    .map(|text| without_ids(&text.replace(&root.to_string(), "<root>")))
            };
            match (text("href"), text("name")) {
                (Some(href), Some(title)) => format!("{name}={href} ({title})"),
                (Some(href), None) => format!("{name}={href}"),
                _ => format!("{name}={}", flat_shape(link)),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The [`flat_shape`] of the lowest-id entry of `kind` a listing embeds at
/// its top level, without its own `_embedded` and `thumbnail` (a folder's
/// own, not the grid's shape); `none` where it embeds none.
fn embedded_shape(body: &Llsd, kind: &str) -> String {
    let entries: Option<BTreeMap<&String, &Llsd>> = body
        .get("_embedded")
        .and_then(|embedded| embedded.get(kind))
        .and_then(Llsd::as_map)
        .map(|map| map.iter().collect());
    let Some(entry) = entries.and_then(|entries| entries.into_values().next().cloned()) else {
        return "none".to_owned();
    };
    let mut entry = entry;
    if let Llsd::Map(map) = &mut entry {
        let _embedded = map.remove("_embedded");
        let _thumbnail = map.remove("thumbnail");
    }
    flat_shape(&entry)
}

/// How far a listing asked at `depth` reached, against the crawl, in words
/// that do not turn on how deep the tree is: a listing that holds every
/// folder down to `depth + 1` levels and none below reads "the depth plus one
/// levels" whether or not the tree goes deeper.
fn reach_metric(reach: &Reach, depth: i32, crawl: &Crawl) -> String {
    let asked = usize::try_from(depth).unwrap_or(0).saturating_add(1);
    let deepest = reach.levels.values().copied().max().unwrap_or(0);
    let missing = crawl
        .folders
        .iter()
        .filter(|(_id, folder)| folder.depth > 0 && folder.depth <= asked)
        .filter(|(id, _folder)| !reach.levels.contains_key(id))
        .count();
    match (deepest > asked, missing) {
        (false, 0) => "the depth plus one levels".to_owned(),
        (true, 0) => format!("{deepest} levels, deeper than asked"),
        (_, missing) => format!("{missing} folders missing within the depth"),
    }
}

/// Leg 6: AIS3.
async fn ais(
    census: &Census<'_>,
    metrics: &mut Metrics,
    root: Uuid,
    library: Option<Uuid>,
    crawl: &Crawl,
) {
    let folder = census.get(AIS, &format!("/category/{root}")).await;
    metrics.set("ais_folder_status", folder.status_metric());
    metrics.set(
        "ais_folder_shape",
        folder
            .body
            .as_ref()
            .map_or_else(|| folder.top_keys(), flat_shape),
    );
    metrics.set("ais_folder_links", links_of(folder.body.as_ref(), root));
    match subject_item(crawl) {
        Some((id, _listed)) => {
            let item = census.get(AIS, &format!("/item/{id}")).await;
            metrics.set("ais_item_status", item.status_metric());
            metrics.set(
                "ais_item_shape",
                item.body
                    .as_ref()
                    .map_or_else(|| item.top_keys(), flat_shape),
            );
            metrics.set("ais_item_links", links_of(item.body.as_ref(), root));
        }
        None => {
            metrics.set("ais_item_status", "no item");
            metrics.set("ais_item_shape", "no item");
            metrics.set("ais_item_links", "no item");
        }
    }
    let unknown = census
        .get(
            AIS,
            &format!("/category/{}/children?depth=0", Uuid::new_v4()),
        )
        .await;
    metrics.set("ais_unknown_folder", unknown.summary("errors"));
    for depth in DEPTHS {
        let raw = census
            .get(AIS, &format!("/category/{root}/children?depth={depth}"))
            .await;
        metrics.set(&format!("ais_depth_{depth}_status"), raw.status_metric());
        let Some(body) = raw.body.as_ref() else {
            for part in ["reach", "nesting"] {
                metrics.set(&format!("ais_depth_{depth}_{part}"), raw.top_keys());
            }
            if depth == 0 {
                for key in [
                    "ais_children_keys",
                    "ais_links_shape",
                    "ais_children_links",
                    "ais_base_uri",
                    "ais_category_thumbnail",
                    "ais_category_keys",
                    "ais_category_shape",
                    "ais_embedded_item_shape",
                    "ais_embedded_item_keys",
                    "ais_embedded_link_keys",
                ] {
                    metrics.set(key, raw.top_keys());
                }
            }
            continue;
        };
        let mut reach = Reach::default();
        walk(body, Some(root), 0, &mut reach);
        metrics.set(
            &format!("ais_depth_{depth}_reach"),
            reach_metric(&reach, depth, crawl),
        );
        metrics.set(
            &format!("ais_depth_{depth}_nesting"),
            if reach.misplaced == 0 {
                "each folder under its parent".to_owned()
            } else {
                format!("{} folders beside their parent", reach.misplaced)
            },
        );
        if depth == 0 {
            metrics.set("ais_children_keys", keys_of(body));
            metrics.set(
                "ais_links_shape",
                body.get("_links")
                    .map_or_else(|| "none".to_owned(), shape_of),
            );
            metrics.set("ais_children_links", links_of(Some(body), root));
            metrics.set(
                "ais_base_uri",
                body.get("_base_uri")
                    .and_then(Llsd::as_str)
                    .map_or_else(|| "none".to_owned(), without_ids),
            );
            // A thumbnail is the folder's own, not the grid's shape.
            metrics.set(
                "ais_category_thumbnail",
                reach.category_keys.remove("thumbnail"),
            );
            metrics.set("ais_category_keys", joined(&reach.category_keys));
            metrics.set("ais_category_shape", embedded_shape(body, "categories"));
            metrics.set("ais_embedded_item_shape", embedded_shape(body, "items"));
            metrics.set("ais_embedded_item_keys", joined(&reach.item_keys));
            metrics.set("ais_embedded_link_keys", joined(&reach.link_keys));
        }
    }
    match library {
        Some(library) => {
            let raw = census
                .get(
                    LIBRARY_AIS,
                    &format!("/category/{library}/children?depth=1"),
                )
                .await;
            metrics.set("ais_library_status", raw.status_metric());
            metrics.set("ais_library_keys", raw.top_keys());
            let cross = census
                .get(LIBRARY_AIS, &format!("/category/{root}/children?depth=0"))
                .await;
            metrics.set("ais_library_agent_folder", cross.summary("errors"));
        }
        None => {
            for key in [
                "ais_library_status",
                "ais_library_keys",
                "ais_library_agent_folder",
            ] {
                metrics.set(key, "no library");
            }
        }
    }
}

/// The folders the version legs made, by how they were made.
#[derive(Debug, Default)]
pub(crate) struct Made {
    /// Every folder this run made, so the clean-up can find it.
    pub(crate) folders: Vec<Uuid>,
}

/// Leg 7: what each change does to the versions, and the relogin.
async fn versions(
    census: &mut Census<'_>,
    metrics: &mut Metrics,
    root: Uuid,
    trash: Uuid,
    item: Option<Uuid>,
) -> Result<(), TestFailure> {
    let ais_road = census.a.cap(AIS).is_some();
    let mut made = Made::default();
    // Folders an earlier run left behind go with this run's clean-up.
    let view = if ais_road { View::Ais } else { View::Fetch };
    for folder in [root, trash] {
        made.folders.extend(
            census
                .listing(folder, view)
                .await
                .into_iter()
                .filter(|(name, _id)| name.starts_with("conf-inv-"))
                .map(|(_name, id)| id),
        );
    }
    metrics.set("leftover_folders", count(made.folders.len()));
    let read = |versions: &HashMap<Uuid, i32>, folder: Uuid| versions.get(&folder).copied();

    // The skeleton's versions against the fetch at the start.
    let at_start = census.versions(&[root, trash]).await;
    let login = census.a.login_success().cloned();
    let skeleton_version = |login: Option<&sl_client_tokio::LoginSuccess>, folder: Uuid| {
        login.and_then(|login| {
            login
                .inventory_skeleton
                .iter()
                .find(|entry| entry.folder_id.uuid() == folder)
                .map(|entry| entry.version)
        })
    };
    metrics.set(
        "skeleton_root_vs_fetch",
        step(
            skeleton_version(login.as_ref(), root),
            read(&at_start, root),
        ),
    );

    // A folder over UDP.
    let udp_name = census.name("udp");
    let udp_id = Uuid::new_v4();
    // Raw: the client's own command takes AIS3 where it is granted.
    census
        .send_raw(AnyMessage::CreateInventoryFolder(CreateInventoryFolder {
            agent_data: CreateInventoryFolderAgentDataBlock {
                agent_id: census.agent,
                session_id: census.session_id,
            },
            folder_data: CreateInventoryFolderFolderDataBlock {
                folder_id: udp_id,
                parent_id: root,
                r#type: FolderType::None.to_code(),
                name: format!("{udp_name}\0").into_bytes(),
            },
        }))
        .await?;
    let listed = census
        .settle(root, |children| {
            children.iter().any(|(name, _id)| *name == udp_name)
        })
        .await;
    let after = census.versions(&[root]).await;
    metrics.set("udp_create_listed", listed);
    metrics.set(
        "udp_create_root_step",
        step(read(&at_start, root), read(&after, root)),
    );
    if listed {
        made.folders.push(udp_id);
    }
    let mut last_root = read(&after, root);

    // A folder over the capability.
    let cap_name = census.name("cap");
    // Raw: the client's own command takes AIS3 where it is granted.
    let answered = census
        .send_llsd(
            "CreateInventoryCategory",
            reqwest::Method::POST,
            "",
            &sl_wire::build_create_inventory_category_request(
                InventoryFolderKey::from(Uuid::new_v4()),
                InventoryFolderKey::from(root),
                -1,
                &cap_name,
            ),
        )
        .await;
    let cap_id = answered
        .body
        .as_ref()
        .and_then(|body| body.get("folder_id"))
        .and_then(uuid_of);
    let listed = census
        .settle(root, |children| {
            children.iter().any(|(name, _id)| *name == cap_name)
        })
        .await;
    let after = census.versions(&[root]).await;
    metrics.set("cap_create_answered", cap_id.is_some());
    metrics.set("cap_create_listed", listed);
    metrics.set("cap_create_root_step", step(last_root, read(&after, root)));
    let cap_id = match cap_id {
        Some(id) => Some(id),
        None => census
            .children(root)
            .await
            .into_iter()
            .find(|(name, _id)| *name == cap_name)
            .map(|(_name, id)| id),
    };
    made.folders.extend(cap_id);
    last_root = read(&after, root);

    // A folder over AIS3.
    let mut ais_id = None;
    if ais_road {
        let ais_name = census.name("ais");
        census
            .a
            .send(Command::Ais3CreateFolder {
                parent_id: InventoryFolderKey::from(root),
                folder_type: -1,
                name: ais_name.clone(),
            })
            .await?;
        let listed = census
            .settle(root, |children| {
                children.iter().any(|(name, _id)| *name == ais_name)
            })
            .await;
        let after = census.versions(&[root]).await;
        metrics.set("ais_create_listed", listed);
        metrics.set("ais_create_root_step", step(last_root, read(&after, root)));
        ais_id = census
            .children(root)
            .await
            .into_iter()
            .find(|(name, _id)| *name == ais_name)
            .map(|(_name, id)| id);
        made.folders.extend(ais_id);
        last_root = read(&after, root);
    } else {
        metrics.set("ais_create_listed", NO_CAPABILITY);
        metrics.set("ais_create_root_step", NO_CAPABILITY);
    }

    // An item moved between two of the run's folders: over AIS3 the way our
    // client did, then over UDP the way the reference does.
    item_move(
        census,
        metrics,
        item,
        cap_id,
        made.folders.contains(&udp_id).then_some(udp_id),
    )
    .await?;

    // The UDP legs act on the UDP-made folder, or the capability's where UDP
    // made none.
    let subject = if made.folders.contains(&udp_id) {
        metrics.set("udp_subject", "the UDP-made folder");
        Some(udp_id)
    } else {
        metrics.set(
            "udp_subject",
            if cap_id.is_some() {
                "the capability-made folder"
            } else {
                "no folder"
            },
        );
        cap_id
    };
    if let Some(subject) = subject {
        let before = census.versions(&[subject]).await;
        let renamed = census.name("renamed");
        // Raw: the client's own command takes AIS3 where it is granted.
        census
            .send_raw(AnyMessage::UpdateInventoryFolder(UpdateInventoryFolder {
                agent_data: UpdateInventoryFolderAgentDataBlock {
                    agent_id: census.agent,
                    session_id: census.session_id,
                },
                folder_data: vec![UpdateInventoryFolderFolderDataBlock {
                    folder_id: subject,
                    parent_id: root,
                    r#type: FolderType::None.to_code(),
                    name: format!("{renamed}\0").into_bytes(),
                }],
            }))
            .await?;
        let listed = census
            .settle(root, |children| {
                children.iter().any(|(name, _id)| *name == renamed)
            })
            .await;
        let after = census.versions(&[root, subject]).await;
        metrics.set("udp_rename_listed", listed);
        metrics.set("udp_rename_root_step", step(last_root, read(&after, root)));
        metrics.set(
            "udp_rename_folder_step",
            step(read(&before, subject), read(&after, subject)),
        );
        last_root = read(&after, root);

        let trash_before = census.versions(&[trash]).await;
        let ais_before = census.ais_versions(&[root, trash]).await;
        // Raw, as the reference sends a move (`updateParentOnServer`): our
        // own command takes AIS3 where it is granted.
        census
            .send_raw(AnyMessage::MoveInventoryFolder(MoveInventoryFolder {
                agent_data: MoveInventoryFolderAgentDataBlock {
                    agent_id: census.agent,
                    session_id: census.session_id,
                    stamp: false,
                },
                inventory_data: vec![MoveInventoryFolderInventoryDataBlock {
                    folder_id: subject,
                    parent_id: trash,
                }],
            }))
            .await?;
        let moved = census
            .settle(trash, |children| {
                children.iter().any(|(_name, id)| *id == subject)
            })
            .await;
        let after = census.versions(&[root, trash]).await;
        metrics.set("udp_move_listed", moved);
        metrics.set("udp_move_root_step", step(last_root, read(&after, root)));
        metrics.set(
            "udp_move_trash_step",
            step(read(&trash_before, trash), read(&after, trash)),
        );
        let moved_ais = ais_view(
            census,
            metrics,
            "udp_move",
            trash,
            subject,
            true,
            &ais_before,
        )
        .await;
        let moved = moved || moved_ais;

        if moved {
            let trash_before = read(&after, trash);
            let ais_moved = census.ais_versions(&[root, trash]).await;
            census
                .send_raw(AnyMessage::RemoveInventoryFolder(RemoveInventoryFolder {
                    agent_data: RemoveInventoryFolderAgentDataBlock {
                        agent_id: census.agent,
                        session_id: census.session_id,
                    },
                    folder_data: vec![RemoveInventoryFolderFolderDataBlock { folder_id: subject }],
                }))
                .await?;
            let gone = census
                .settle(trash, |children| {
                    children.iter().all(|(_name, id)| *id != subject)
                })
                .await;
            let after = census.versions(&[trash]).await;
            metrics.set("udp_remove_gone", gone);
            metrics.set(
                "udp_remove_trash_step",
                step(trash_before, read(&after, trash)),
            );
            let gone_ais = ais_view(
                census,
                metrics,
                "udp_remove",
                trash,
                subject,
                false,
                &ais_moved,
            )
            .await;
            let gone = gone || gone_ais;
            if gone {
                made.folders.retain(|folder| *folder != subject);
            }
        } else {
            for key in [
                "udp_remove_gone",
                "udp_remove_trash_step",
                "udp_remove_listed_ais",
                "udp_remove_root_step_ais",
                "udp_remove_trash_step_ais",
            ] {
                metrics.set(key, "not in the Trash");
            }
        }
    } else {
        for key in [
            "udp_rename_listed",
            "udp_rename_root_step",
            "udp_rename_folder_step",
            "udp_move_listed",
            "udp_move_root_step",
            "udp_move_trash_step",
            "udp_remove_gone",
            "udp_remove_trash_step",
            "udp_move_listed_ais",
            "udp_move_root_step_ais",
            "udp_move_trash_step_ais",
            "udp_remove_listed_ais",
            "udp_remove_root_step_ais",
            "udp_remove_trash_step_ais",
        ] {
            metrics.set(key, "no folder");
        }
    }

    // The AIS3 move and removal.
    match ais_id {
        Some(ais_id) if ais_road => {
            let before = census.versions(&[root, trash]).await;
            let ais_before = census.ais_versions(&[root, trash]).await;
            // Raw, so the answer is the grid's: the body our
            // `Command::Ais3MoveFolder` sends.
            let patched = census
                .send_llsd(
                    AIS,
                    reqwest::Method::PATCH,
                    &format!("/category/{ais_id}"),
                    &sl_wire::build_ais_move_body(InventoryFolderKey::from(trash)),
                )
                .await;
            metrics.set("ais_move_answer", patched.summary("errors"));
            // What the refusal names instead: `MOVE` with a `Destination`,
            // first as an absolute URL, then as a path under the capability.
            let mut moved_by = "neither";
            for (word, destination) in [
                (
                    "absolute",
                    census
                        .a
                        .cap(AIS)
                        .map(|cap| format!("{cap}/category/{trash}")),
                ),
                ("relative", Some(format!("/category/{trash}"))),
            ] {
                let answer = census
                    .move_method(
                        &format!("/category/{ais_id}"),
                        destination.as_deref().unwrap_or(""),
                    )
                    .await;
                metrics.set(
                    &format!("ais_move_method_{word}_answer"),
                    answer.summary("errors"),
                );
                if census
                    .settle_by(trash, View::Ais, |children| {
                        children.iter().any(|(_name, id)| *id == ais_id)
                    })
                    .await
                {
                    moved_by = word;
                    break;
                }
            }
            metrics.set("ais_move_method_moved_by", moved_by);
            if moved_by == "absolute" {
                metrics.set("ais_move_method_relative_answer", "not asked");
            }
            let moved = census
                .settle(trash, |children| {
                    children.iter().any(|(_name, id)| *id == ais_id)
                })
                .await;
            let after = census.versions(&[root, trash]).await;
            metrics.set("ais_move_listed", moved);
            metrics.set(
                "ais_move_root_step",
                step(read(&before, root), read(&after, root)),
            );
            metrics.set(
                "ais_move_trash_step",
                step(read(&before, trash), read(&after, trash)),
            );
            let _moved_ais = ais_view(
                census,
                metrics,
                "ais_move",
                trash,
                ais_id,
                true,
                &ais_before,
            )
            .await;
            let ais_moved = census.ais_versions(&[root, trash]).await;
            census
                .a
                .send(Command::Ais3RemoveFolder(InventoryFolderKey::from(ais_id)))
                .await?;
            let gone = census
                .settle(trash, |children| {
                    children.iter().all(|(_name, id)| *id != ais_id)
                })
                .await;
            let removed = census.versions(&[trash]).await;
            metrics.set("ais_remove_gone", gone);
            metrics.set(
                "ais_remove_trash_step",
                step(read(&after, trash), read(&removed, trash)),
            );
            let gone_ais = ais_view(
                census,
                metrics,
                "ais_remove",
                trash,
                ais_id,
                false,
                &ais_moved,
            )
            .await;
            let gone = gone || gone_ais;
            if gone {
                made.folders.retain(|folder| *folder != ais_id);
            }
        }
        _ => {
            for key in [
                "ais_move_answer",
                "ais_move_method_absolute_answer",
                "ais_move_method_relative_answer",
                "ais_move_method_moved_by",
                "ais_move_listed",
                "ais_move_root_step",
                "ais_move_trash_step",
                "ais_remove_gone",
                "ais_remove_trash_step",
                "ais_move_listed_ais",
                "ais_move_root_step_ais",
                "ais_move_trash_step_ais",
                "ais_remove_listed_ais",
                "ais_remove_root_step_ais",
                "ais_remove_trash_step_ais",
            ] {
                metrics.set(key, NO_CAPABILITY);
            }
        }
    }

    clean_up(census, &made, root, trash, ais_road).await?;

    // The relogin: does the next skeleton state the versions this session
    // read last?
    let last = census.versions(&[root, trash]).await;
    census.idle(Duration::from_secs(2)).await;
    census.a.relogin().await?;
    census.a.wait_for_region(REGION_TIMEOUT).await?;
    let login = census.a.login_success().cloned();
    metrics.set(
        "relogin_skeleton_root_step",
        step(read(&last, root), skeleton_version(login.as_ref(), root)),
    );
    metrics.set(
        "relogin_skeleton_trash_step",
        step(read(&last, trash), skeleton_version(login.as_ref(), trash)),
    );
    let again = census.versions(&[root, trash]).await;
    metrics.set(
        "relogin_fetch_root_step",
        step(read(&last, root), read(&again, root)),
    );
    Ok(())
}

/// The ids of the items the agent folder fetch lists in `folder`.
pub(crate) async fn items_in(census: &Census<'_>, folder: Uuid) -> Vec<Uuid> {
    let raw = census.fetch_folders(FID2, census.agent, &[folder]).await;
    listed_folders(raw.body.as_ref())
        .flat_map(|entry| {
            entry
                .get("items")
                .and_then(Llsd::as_array)
                .unwrap_or_default()
        })
        .filter_map(|item| item.get("item_id").and_then(uuid_of))
        .collect()
}

/// The item legs: a copy of `item` filed in the folder `from`, moved to
/// `to` with the AIS3 `PATCH` our client sent (`ais_item_move_answer`,
/// `ais_item_move_moved`), then with a raw UDP `MoveInventoryItem`
/// (`udp_item_move_moved`, back into `from`). Both folders are the run's, so
/// the clean-up takes the copy with them.
async fn item_move(
    census: &Census<'_>,
    metrics: &mut Metrics,
    item: Option<Uuid>,
    from: Option<Uuid>,
    to: Option<Uuid>,
) -> Result<(), TestFailure> {
    let keys = [
        "item_copy_made",
        "ais_item_move_answer",
        "ais_item_move_moved",
        "udp_item_move_moved",
    ];
    let (Some(item), Some(from), Some(to)) = (item, from, to) else {
        for key in keys {
            metrics.set(key, "no item or folders");
        }
        return Ok(());
    };
    let name = census.name("item");
    census
        .a
        .send(Command::CopyInventoryItem {
            old_agent_id: sl_client_tokio::AgentKey::from(census.agent),
            old_item_id: sl_client_tokio::InventoryKey::from(item),
            new_folder_id: InventoryFolderKey::from(from),
            new_name: name.clone(),
        })
        .await?;
    let started = Instant::now();
    let mut copy = None;
    while copy.is_none() && started.elapsed() < SETTLE {
        let listed = items_in(census, from).await;
        copy = listed.first().copied();
        if copy.is_none() {
            tokio::time::sleep(POLL).await;
        }
    }
    metrics.set("item_copy_made", copy.is_some());
    let Some(copy) = copy else {
        for key in &keys[1..] {
            metrics.set(key, "no copy");
        }
        return Ok(());
    };
    let patched = census
        .send_llsd(
            AIS,
            reqwest::Method::PATCH,
            &format!("/item/{copy}"),
            &sl_wire::build_ais_move_body(InventoryFolderKey::from(to)),
        )
        .await;
    metrics.set("ais_item_move_answer", patched.summary("errors"));
    let moved = if census.a.cap(AIS).is_some() {
        let started = Instant::now();
        let mut moved = false;
        while !moved && started.elapsed() < SETTLE {
            moved = items_in(census, to).await.contains(&copy);
            if !moved {
                tokio::time::sleep(POLL).await;
            }
        }
        metrics.set("ais_item_move_moved", moved);
        moved
    } else {
        metrics.set("ais_item_move_moved", NO_CAPABILITY);
        false
    };
    let (target, source) = if moved { (from, to) } else { (to, from) };
    census
        .send_raw(AnyMessage::MoveInventoryItem(MoveInventoryItem {
            agent_data: MoveInventoryItemAgentDataBlock {
                agent_id: census.agent,
                session_id: census.session_id,
                stamp: false,
            },
            inventory_data: vec![MoveInventoryItemInventoryDataBlock {
                item_id: copy,
                folder_id: target,
                new_name: Vec::new(),
            }],
        }))
        .await?;
    let started = Instant::now();
    let mut moved = false;
    while !moved && started.elapsed() < SETTLE {
        moved = items_in(census, target).await.contains(&copy)
            && !items_in(census, source).await.contains(&copy);
        if !moved {
            tokio::time::sleep(POLL).await;
        }
    }
    metrics.set("udp_item_move_moved", moved);
    Ok(())
}

/// Records, under `word`, what AIS3 says after a move into (`into`) or a
/// removal from the Trash of `folder`: whether it is listed there
/// (`_listed_ais`) and the steps of the root's and the Trash's versions
/// against `before` (`_root_step_ais`, `_trash_step_ais`). Where AIS3 is not
/// granted every key records that. Returns whether AIS3 shows the change.
async fn ais_view(
    census: &Census<'_>,
    metrics: &mut Metrics,
    word: &str,
    trash: Uuid,
    folder: Uuid,
    into: bool,
    before: &HashMap<Uuid, i32>,
) -> bool {
    let keys =
        ["listed_ais", "root_step_ais", "trash_step_ais"].map(|part| format!("{word}_{part}"));
    if census.a.cap(AIS).is_none() {
        for key in &keys {
            metrics.set(key, NO_CAPABILITY);
        }
        return false;
    }
    let shown = census
        .settle_by(trash, View::Ais, |children| {
            children.iter().any(|(_name, id)| *id == folder) == into
        })
        .await;
    let root = census
        .a
        .login_success()
        .and_then(|login| login.inventory_root)
        .map(|root| root.uuid());
    let after = census
        .ais_versions(&root.into_iter().chain([trash]).collect::<Vec<_>>())
        .await;
    let [listed, root_step, trash_step] = keys;
    metrics.set(&listed, shown == into);
    metrics.set(
        &root_step,
        root.map_or(-1000, |root| {
            step(before.get(&root).copied(), after.get(&root).copied())
        }),
    );
    metrics.set(
        &trash_step,
        step(before.get(&trash).copied(), after.get(&trash).copied()),
    );
    shown
}

/// Puts the run's folders that are left into the Trash and removes them, by
/// the grid's own road: AIS3 where it is granted, UDP otherwise.
pub(crate) async fn clean_up(
    census: &Census<'_>,
    made: &Made,
    root: Uuid,
    trash: Uuid,
    ais_road: bool,
) -> Result<(), TestFailure> {
    if made.folders.is_empty() {
        return Ok(());
    }
    let view = if ais_road { View::Ais } else { View::Fetch };
    let under_root: Vec<Uuid> = census
        .listing(root, view)
        .await
        .into_iter()
        .map(|(_name, id)| id)
        .filter(|id| made.folders.contains(id))
        .collect();
    for folder in under_root {
        let key = InventoryFolderKey::from(folder);
        // UDP on both grids: Second Life's AIS3 cannot move a folder.
        census
            .a
            .send(Command::MoveInventoryFolder {
                folder_id: key,
                parent_id: InventoryFolderKey::from(trash),
            })
            .await?;
    }
    let _moved = census
        .settle_by(trash, view, |children| {
            made.folders
                .iter()
                .all(|folder| children.iter().any(|(_name, id)| id == folder))
        })
        .await;
    for folder in &made.folders {
        let key = InventoryFolderKey::from(*folder);
        let command = if ais_road {
            Command::Ais3RemoveFolder(key)
        } else {
            Command::RemoveInventoryFolders(vec![key])
        };
        census.a.send(command).await?;
    }
    let _gone = census
        .settle_by(trash, view, |children| {
            children
                .iter()
                .all(|(_name, id)| !made.folders.contains(id))
        })
        .await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Crawl, Folder, Reach, differing_keys, map, reach_metric, step, walk};
    use pretty_assertions::assert_eq;
    use sl_client_tokio::Uuid;
    use sl_wire::Llsd;

    /// A step is the difference, and an unread side is named.
    #[test]
    fn a_step_is_the_difference() {
        assert_eq!(step(Some(3), Some(5)), 2);
        assert_eq!(step(None, Some(5)), -1000);
    }

    /// Two maps are compared key by key.
    #[test]
    fn differing_keys_are_named() {
        let a = map([("a", Llsd::Integer(1)), ("b", Llsd::Integer(2))]);
        let b = map([("a", Llsd::Integer(1)), ("c", Llsd::Integer(2))]);
        assert_eq!(differing_keys(&a, &a), "identical");
        assert_eq!(differing_keys(&a, &b), "differs in b,c");
    }

    /// A nested listing reaches a level per embedded map, and a crawl of the
    /// same two folders is "the whole tree".
    #[test]
    fn a_nested_listing_is_walked() {
        let root = Uuid::from_u128(1);
        let child = Uuid::from_u128(2);
        let grandchild = Uuid::from_u128(3);
        let embedded = |categories: Vec<(Uuid, Llsd)>| {
            map([(
                "_embedded",
                map([(
                    "categories",
                    Llsd::Map(
                        categories
                            .into_iter()
                            .map(|(id, body)| (id.to_string(), body))
                            .collect(),
                    ),
                )]),
            )])
        };
        let grandchild_body = map([
            ("category_id", Llsd::Uuid(grandchild)),
            ("parent_id", Llsd::Uuid(child)),
        ]);
        let mut child_body = embedded(vec![(grandchild, grandchild_body)]);
        if let Llsd::Map(fields) = &mut child_body {
            let _id = fields.insert("category_id".to_owned(), Llsd::Uuid(child));
            let _parent = fields.insert("parent_id".to_owned(), Llsd::Uuid(root));
        }
        let body = embedded(vec![(child, child_body)]);
        let mut reach = Reach::default();
        walk(&body, Some(root), 0, &mut reach);
        assert_eq!(reach.levels.get(&grandchild), Some(&2));
        assert_eq!(reach.misplaced, 0);
        let mut crawl = Crawl::default();
        for (id, depth) in [(root, 0), (child, 1), (grandchild, 2)] {
            let _folder = crawl.folders.insert(
                id,
                Folder {
                    version: None,
                    parent: None,
                    depth,
                },
            );
        }
        assert_eq!(reach_metric(&reach, 1, &crawl), "the depth plus one levels");
        assert_eq!(
            reach_metric(&reach, 0, &crawl),
            "2 levels, deeper than asked"
        );
        let _deeper = crawl.folders.insert(
            Uuid::from_u128(4),
            Folder {
                version: None,
                parent: None,
                depth: 3,
            },
        );
        assert_eq!(reach_metric(&reach, 1, &crawl), "the depth plus one levels");
        assert_eq!(
            reach_metric(&reach, 2, &crawl),
            "1 folders missing within the depth"
        );
    }
}
