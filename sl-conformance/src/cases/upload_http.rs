//! A census of the upload capabilities as HTTP: what each grid's
//! `NewFileAgentInventory`, `MeshUploadFlag`, `InventoryThumbnailUpload` and
//! `SendPostcard` answer a request with, what an upload costs, and what the
//! grid says about it afterwards — rather than whether one upload through the
//! client succeeds (`asset-upload` holds that).
//!
//! An upload is two requests. The first posts LLSD metadata to the capability
//! and is answered with an `uploader` URL; the second posts the asset's bytes
//! there and is answered with the new asset and the new item. The case sends
//! both itself, with a client that follows no redirect, and asks one question
//! a request:
//!
//! 1. **The price**: the texture's class asked for with the right
//!    `expected_upload_cost`, with none, with too little, too much, far more
//!    than the balance, a negative one and one that is text — and, where the
//!    grid hands out an uploader anyway, what the upload is then charged.
//! 2. **The request**: no folder, the nil folder, a folder nobody has, no
//!    name, classes that disagree, a body that is no LLSD, a `GET`.
//! 3. **The classes**: every class a viewer holds in inventory asked for, and
//!    uploaded where the grid takes it.
//! 4. **The bytes**: a texture, a sound and an animation that are what they
//!    say, and ones that are not — nothing, noise, another format, a size or
//!    a length the reference viewer would not send.
//! 5. **A mesh**: what the first request is answered with — the price and the
//!    cost data a viewer shows before the user confirms — for a model, for
//!    one with no physics shape, for noise and for nothing; and the upload.
//! 6. **Somebody who cannot pay**: the second avatar gives the first all but
//!    three of its L$, asks for an upload, and is given them back.
//! 7. **`MeshUploadFlag`**, **`InventoryThumbnailUpload`** (an item, a folder,
//!    ids nobody has, images of sizes the reference would and would not send)
//!    and **`SendPostcard`** (the first request only, unless the fixtures name
//!    an address to send one postcard to).
//!
//! After every upload the case listens for what the grid sends unasked — a
//! balance with a transaction, an alert, an item announced — and then asks
//! for the balance, so each leg records what it was charged.
//!
//! What each live grid answered is stated as a [`Measured`] and written up in
//! `book/src/gridspec/assets.md` § Uploads; every run is held to it. The
//! items and the folder the case makes are removed, also when a leg fails.
//!
//! `2av`, `[both]`, offline on both fake flavours. **On Second Life a run
//! spends L$** — about L$ 100 of the first avatar's.

use std::collections::HashMap;
use std::num::NonZeroU32;
use std::time::Duration;

use sl_client_tokio::{
    AssetType, Command, Event, FolderType, InventoryFolderKey, InventoryKey, LindenAmount,
    MeshUploadStatus, MoneyTransactionType, Throttle, ThumbnailTarget, UploadClass, Uuid,
    expected_upload_cost,
};
use sl_wire::{Llsd, parse_llsd_xml};

use crate::cases::asset_http::{without_ids, without_tags};
use crate::cases::asset_upload::{current_balance, destination_folder};
use crate::cases::object_rez_derez::{Answer, FAKE_QUIET, QUIET};
use crate::cases::task_inventory::{NO_CAPABILITY, hold_to_measured};
use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::metrics::Metrics;
use crate::registry::{GridTest, TestFuture};
use crate::support::{LONG_TIMEOUT, REGION_TIMEOUT, REPLY_TIMEOUT};

/// Where the measurements below are written down.
const SOURCE: &str = "book/src/gridspec/assets.md § Uploads (upload-http, 2026-10-13)";

/// What a leg records when the request before it brought no uploader.
const NO_UPLOADER: &str = "no uploader to post to";

/// What a leg records on a grid that answers no balance request.
const NO_BALANCE: &str = "no balance answered";

/// The L$ the second avatar keeps while it cannot pay.
const PAUPER_KEEPS: i64 = 3;

/// The next-owner mask a viewer sends for a fresh upload (move, modify, copy,
/// transfer).
const NEXT_OWNER: i32 = 0x0008_e000;

/// The JPEG a postcard carries: sixteen pixels square, one colour.
const POSTCARD_JPEG: &[u8] = include_bytes!("upload_http_postcard.jpg");

/// What each grid was measured to answer, by the metric the census records it
/// under. Every run is held to all of it at the end, so a run that disagrees
/// says everything it disagrees about at once.
const MEASURED: &[(&str, Measured<Answer>)] = &[
    ("balance_answered", both(Answer::Flag(true))),
    ("command_mesh_flag", both(Answer::Text("answered"))),
    (
        "command_thumbnail",
        each(
            Answer::Text("uploaded"),
            Answer::Text("failed: InventoryThumbnailUpload capability not available"),
        ),
    ),
    (
        "mesh_flag_get",
        each(
            Answer::Text("200: mesh_upload_status, the agent's name record"),
            Answer::Text("200: mesh_upload_status, nothing else"),
        ),
    ),
    (
        "mesh_flag_post",
        each(
            Answer::Text("200: no mesh_upload_status, the agent's name record"),
            Answer::Text("404: no body"),
        ),
    ),
    ("pauper_can_pay", both(Answer::Text("no"))),
    (
        "postcard_get",
        each(
            Answer::Text("501: text/html \"message Not implemented error\""),
            Answer::Text("no capability"),
        ),
    ),
    (
        "postcard_step1",
        each(
            Answer::Text("200: {state=\"upload\",uploader=<url>}"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "postcard_step1_empty",
        each(
            Answer::Text("200: {state=\"upload\",uploader=<url>}"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "postcard_step1_no_address",
        each(
            Answer::Text("200: {state=\"upload\",uploader=<url>}"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "postcard_step1_not_an_address",
        each(
            Answer::Text("200: {state=\"upload\",uploader=<url>}"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "price_list",
        each(
            Answer::Text("the benefits package"),
            Answer::Text("EconomyData"),
        ),
    ),
    (
        "step1_after_object",
        each(
            Answer::Text(
                "200: {balance=an amount,state=\"upload\",upload_price=10,uploader=<url>}",
            ),
            Answer::Text(
                "200: {error={identifier=nil,message=\"Uploader busy processing previous request\"},state=\"error\",upload_price=0,uploader=\"\"}",
            ),
        ),
    ),
    (
        "step1_animation",
        each(
            Answer::Text(
                "200: {balance=an amount,state=\"upload\",upload_price=10,uploader=<url>}",
            ),
            Answer::Text("200: {state=\"upload\",upload_price=0,uploader=<url>}"),
        ),
    ),
    (
        "step1_class_bodypart",
        each(
            Answer::Text(
                "200: {balance=an amount,state=\"upload\",upload_price=10,uploader=<url>}",
            ),
            Answer::Text("200: {state=\"upload\",upload_price=0,uploader=<url>}"),
        ),
    ),
    (
        "step1_class_callingcard",
        each(
            Answer::Text(
                "500: {error={identifier=\"Upload_ServerError\",message=\"Unable to upload asset.\"},message=\"Unable to upload asset.\",state=\"failure\"}",
            ),
            Answer::Text("200: {state=\"upload\",upload_price=0,uploader=<url>}"),
        ),
    ),
    (
        "step1_class_clothing",
        each(
            Answer::Text(
                "200: {balance=an amount,state=\"upload\",upload_price=10,uploader=<url>}",
            ),
            Answer::Text("200: {state=\"upload\",upload_price=0,uploader=<url>}"),
        ),
    ),
    (
        "step1_class_gesture",
        each(
            Answer::Text(
                "200: {error={identifier=\"Upload_InvalidAssetType\",message=\"Invalid asset type: gesture\"},message=\"Invalid asset type: gesture\",state=\"failure\"}",
            ),
            Answer::Text("200: {state=\"upload\",upload_price=0,uploader=<url>}"),
        ),
    ),
    (
        "step1_class_landmark",
        each(
            Answer::Text(
                "500: {error={identifier=\"Upload_ServerError\",message=\"Unable to upload asset.\"},message=\"Unable to upload asset.\",state=\"failure\"}",
            ),
            Answer::Text("200: {state=\"upload\",upload_price=0,uploader=<url>}"),
        ),
    ),
    (
        "step1_class_material",
        each(
            Answer::Text(
                "200: {error={identifier=\"Upload_InvalidAssetType\",message=\"Invalid asset type: material\"},message=\"Invalid asset type: material\",state=\"failure\"}",
            ),
            Answer::Text("200: {state=\"upload\",upload_price=0,uploader=<url>}"),
        ),
    ),
    (
        "step1_class_mesh_asset",
        each(
            Answer::Text(
                "200: {error={BadValueTypes=[],MissingParameters=[\"instance_list\",\"mesh_list\"],error=\"INVALID_MESH_CONTENTS\",identifier=\"Upload_InvalidAsset\",message=\"Invalid parameters.\"},message=\"Invalid parameters.\",state=\"failure\"}",
            ),
            Answer::Text(
                "200: {error={identifier=nil,message=\"missing model information.\"},state=\"error\",upload_price=0,uploader=\"\"}",
            ),
        ),
    ),
    (
        "step1_class_notecard",
        each(
            Answer::Text(
                "200: {error={identifier=\"Upload_InvalidAssetType\",message=\"Invalid asset type: notecard\"},message=\"Invalid asset type: notecard\",state=\"failure\"}",
            ),
            Answer::Text("200: {state=\"upload\",upload_price=0,uploader=<url>}"),
        ),
    ),
    (
        "step1_class_object",
        each(
            Answer::Text(
                "500: {error={identifier=\"Upload_ServerError\",message=\"Unable to upload asset.\"},message=\"Unable to upload asset.\",state=\"failure\"}",
            ),
            Answer::Text("200: {state=\"upload\",upload_price=0,uploader=<url>}"),
        ),
    ),
    (
        "step1_class_script",
        each(
            Answer::Text(
                "200: {error={identifier=\"Upload_InvalidAssetType\",message=\"Invalid asset type: lsltext\"},message=\"Invalid asset type: lsltext\",state=\"failure\"}",
            ),
            Answer::Text("200: {state=\"upload\",upload_price=0,uploader=<url>}"),
        ),
    ),
    (
        "step1_class_settings",
        each(
            Answer::Text(
                "200: {error={identifier=\"Upload_InvalidAssetType\",message=\"Invalid asset type: settings\"},message=\"Invalid asset type: settings\",state=\"failure\"}",
            ),
            Answer::Text("200: {state=\"upload\",upload_price=0,uploader=<url>}"),
        ),
    ),
    (
        "step1_class_snapshot",
        each(
            Answer::Text(
                "200: {balance=an amount,state=\"upload\",upload_price=10,uploader=<url>}",
            ),
            Answer::Text("200: {state=\"upload\",upload_price=0,uploader=<url>}"),
        ),
    ),
    (
        "step1_class_unknown",
        each(
            Answer::Text(
                "500: {error={identifier=\"Upload_ServerError\",message=\"Unable to upload asset.\"},message=\"Unable to upload asset.\",state=\"failure\"}",
            ),
            Answer::Text("200: {state=\"upload\",upload_price=0,uploader=<url>}"),
        ),
    ),
    (
        "step1_classes_disagree",
        each(
            Answer::Text(
                "200: {error={identifier=\"Upload_InvalidInventoryType\",message=\"invalid inventory type: sound must be in: ['texture', 'snapshot']\",valid_inventory_types=[\"texture\",\"snapshot\"]},message=\"invalid inventory type: sound must be in: ['texture', 'snapshot']\",state=\"failure\"}",
            ),
            Answer::Text("200: {state=\"upload\",upload_price=0,uploader=<url>}"),
        ),
    ),
    (
        "step1_cost_huge",
        each(
            Answer::Text(
                "200: {balance=an amount,state=\"upload\",upload_price=10,uploader=<url>}",
            ),
            Answer::Text("200: {state=\"upload\",upload_price=0,uploader=<url>}"),
        ),
    ),
    (
        "step1_cost_negative",
        each(
            Answer::Text(
                "200: {balance=an amount,state=\"upload\",upload_price=10,uploader=<url>}",
            ),
            Answer::Text("200: {state=\"upload\",upload_price=0,uploader=<url>}"),
        ),
    ),
    (
        "step1_cost_none",
        each(
            Answer::Text(
                "200: {balance=an amount,state=\"upload\",upload_price=10,uploader=<url>}",
            ),
            Answer::Text("200: {state=\"upload\",upload_price=0,uploader=<url>}"),
        ),
    ),
    (
        "step1_cost_one",
        each(
            Answer::Text(
                "200: {balance=an amount,state=\"upload\",upload_price=10,uploader=<url>}",
            ),
            Answer::Text("200: {state=\"upload\",upload_price=0,uploader=<url>}"),
        ),
    ),
    (
        "step1_cost_over",
        each(
            Answer::Text(
                "200: {balance=an amount,state=\"upload\",upload_price=10,uploader=<url>}",
            ),
            Answer::Text("200: {state=\"upload\",upload_price=0,uploader=<url>}"),
        ),
    ),
    (
        "step1_cost_right",
        each(
            Answer::Text(
                "200: {balance=an amount,state=\"upload\",upload_price=10,uploader=<url>}",
            ),
            Answer::Text("200: {state=\"upload\",upload_price=0,uploader=<url>}"),
        ),
    ),
    (
        "step1_cost_text",
        each(
            Answer::Text(
                "200: {error={identifier=\"Upload_BadParameterType\",message=\"Parameter 'expected_upload_cost' is <type 'str'>, expected <type 'int'>.\"},message=\"Parameter 'expected_upload_cost' is <type 'str'>, expected <type 'int'>.\",state=\"failure\"}",
            ),
            Answer::Text("200: {state=\"upload\",upload_price=0,uploader=<url>}"),
        ),
    ),
    (
        "step1_cost_zero",
        each(
            Answer::Text(
                "200: {balance=an amount,state=\"upload\",upload_price=10,uploader=<url>}",
            ),
            Answer::Text("200: {state=\"upload\",upload_price=0,uploader=<url>}"),
        ),
    ),
    (
        "step1_empty_map",
        each(
            Answer::Text(
                "200: {error={identifier=\"Upload_MissingParameter\",message=\"Missing parameter 'folder_id'\"},message=\"Missing parameter 'folder_id'\",state=\"failure\"}",
            ),
            Answer::Text("200: {state=\"upload\",upload_price=0,uploader=<url>}"),
        ),
    ),
    (
        "step1_get",
        each(
            Answer::Text("501: text/html \"message Not implemented error\""),
            Answer::Text("404: text/html \"404 Page not found\""),
        ),
    ),
    (
        "step1_mesh",
        each(
            Answer::Text(
                "200: {balance=an amount,data={coalesce=false,model_physics_cost={decomposition=0.000,decomposition_discounted_vertices=0,decomposition_hulls=0,hull=0.360,hull_discounted_vertices=4,mesh=0.000,mesh_triangles=0},model_streaming_cost=0.050,num_instances=1,num_meshes=1,num_rigged=0,num_textures=0,physics_cost=0.360,resource_cost=0.500,simulation_cost=0.500,streaming_cost=1.000,streaming_params={high_lod=280,low_lod=280,lowest_lod=280,medium_lod=280},upload_price_breakdown={mesh_instance=1,mesh_physics=0,mesh_streaming=0,model=10,texture=0}},state=\"upload\",upload_price=11,uploader=<url>}",
            ),
            Answer::Text(
                "200: {data={model_streaming_cost=0.064,physics_cost=0.360,resource_cost=0.500,simulation_cost=0.500,upload_price_breakdown={mesh_instance=0,mesh_physics=0,mesh_streaming=0,model=0,texture=0}},state=\"upload\",upload_price=0,uploader=<url>}",
            ),
        ),
    ),
    (
        "step1_mesh_no_instances",
        each(
            Answer::Text(
                "200: {error={error=\"INVALID_MESH_CONTENTS\",identifier=\"Upload_InvalidAsset\",message=\"The model contains no instances.\"},message=\"The model contains no instances.\",state=\"failure\"}",
            ),
            Answer::Text(
                "200: {error={identifier=nil,message=\"missing model information.\"},state=\"error\",upload_price=0,uploader=\"\"}",
            ),
        ),
    ),
    (
        "step1_mesh_no_meshes",
        each(
            Answer::Text(
                "200: {error={error=\"INVALID_MESH_CONTENTS\",identifier=\"Upload_InvalidAsset\",message=\"The model contains no meshes.\"},message=\"The model contains no meshes.\",state=\"failure\"}",
            ),
            Answer::Text(
                "200: {error={identifier=nil,message=\"missing model information.\"},state=\"error\",upload_price=0,uploader=\"\"}",
            ),
        ),
    ),
    (
        "step1_mesh_no_physics",
        each(
            Answer::Text(
                "200: {error={BlockName=\"physics_convex\",Context=\"headerHasBlock\",ErrorCode=7,MeshIndex=0,identifier=\"Upload_InvalidAsset\",message=\"MAV_BLOCK_MISSING\"},message=\"MAV_BLOCK_MISSING\",state=\"failure\"}",
            ),
            Answer::Text(
                "200: {error={identifier=nil,message=\"Missing physics_convex block\"},state=\"error\",upload_price=0,uploader=\"\"}",
            ),
        ),
    ),
    (
        "step1_mesh_no_resources",
        each(
            Answer::Text(
                "200: {error={BadValueTypes=[],MissingParameters=[\"instance_list\",\"mesh_list\"],error=\"INVALID_MESH_CONTENTS\",identifier=\"Upload_InvalidAsset\",message=\"Invalid parameters.\"},message=\"Invalid parameters.\",state=\"failure\"}",
            ),
            Answer::Text(
                "200: {error={identifier=nil,message=\"missing model information.\"},state=\"error\",upload_price=0,uploader=\"\"}",
            ),
        ),
    ),
    (
        "step1_mesh_noise",
        each(
            Answer::Text(
                "200: {error={error=\"INVALID_MESH_CONTENTS\",identifier=\"Upload_InvalidAsset\",message=\"Mesh 0: can't parse header: invalid binary token at byte 0: 0\"},message=\"Mesh 0: can't parse header: invalid binary token at byte 0: 0\",state=\"failure\"}",
            ),
            Answer::Text(
                "200: {error={identifier=nil,message=\"Invalid model data\"},state=\"error\",upload_price=0,uploader=\"\"}",
            ),
        ),
    ),
    (
        "step1_nil_folder",
        each(
            Answer::Text(
                "200: {balance=an amount,state=\"upload\",upload_price=10,uploader=<url>}",
            ),
            Answer::Text("200: {state=\"upload\",upload_price=0,uploader=<url>}"),
        ),
    ),
    (
        "step1_no_asset_type",
        each(
            Answer::Text(
                "200: {error={identifier=\"Upload_MissingParameter\",message=\"Missing parameter 'asset_type'\"},message=\"Missing parameter 'asset_type'\",state=\"failure\"}",
            ),
            Answer::Text("200: {state=\"upload\",upload_price=0,uploader=<url>}"),
        ),
    ),
    (
        "step1_no_body",
        each(
            Answer::Text(
                "200: {error={identifier=\"Upload_BadParameterType\",message=\"Parameter 'body' is <type 'bool'>, expected <type 'dict'>.\"},message=\"Parameter 'body' is <type 'bool'>, expected <type 'dict'>.\",state=\"failure\"}",
            ),
            Answer::Text("500: no body"),
        ),
    ),
    (
        "step1_no_folder",
        each(
            Answer::Text(
                "200: {error={identifier=\"Upload_MissingParameter\",message=\"Missing parameter 'folder_id'\"},message=\"Missing parameter 'folder_id'\",state=\"failure\"}",
            ),
            Answer::Text("200: {state=\"upload\",upload_price=0,uploader=<url>}"),
        ),
    ),
    (
        "step1_no_inventory_type",
        each(
            Answer::Text(
                "200: {error={identifier=\"Upload_MissingParameter\",message=\"Missing parameter 'inventory_type'\"},message=\"Missing parameter 'inventory_type'\",state=\"failure\"}",
            ),
            Answer::Text("200: {state=\"upload\",upload_price=0,uploader=<url>}"),
        ),
    ),
    (
        "step1_no_name",
        each(
            Answer::Text(
                "200: {error={identifier=\"Upload_MissingParameter\",message=\"Missing parameter 'name'\"},message=\"Missing parameter 'name'\",state=\"failure\"}",
            ),
            Answer::Text("200: {state=\"upload\",upload_price=0,uploader=<url>}"),
        ),
    ),
    (
        "step1_not_llsd",
        each(
            Answer::Text(
                "200: {error={identifier=\"Upload_InvalidRequestBody\",message=\"Invalid request bodyxxx\"},message=\"Invalid request bodyxxx\",state=\"failure\"}",
            ),
            Answer::Text("500: no body"),
        ),
    ),
    (
        "step1_pauper",
        each(
            Answer::Text(
                "200: {error={balance=an amount,identifier=\"Upload_InsufficientLindenDollarBalance\",message=\"Insufficient L$ balance to upload\",upload_price=10},message=\"Insufficient L$ balance to upload\",state=\"failure\"}",
            ),
            Answer::Text("200: {state=\"upload\",upload_price=0,uploader=<url>}"),
        ),
    ),
    (
        "step1_pauper_mesh",
        each(
            Answer::Text(
                "200: {error={balance=an amount,identifier=\"Upload_InsufficientLindenDollarBalance\",message=\"Insufficient L$ balance to upload\",upload_price=11},message=\"Insufficient L$ balance to upload\",state=\"failure\"}",
            ),
            Answer::Text(
                "200: {data={model_streaming_cost=0.064,physics_cost=0.360,resource_cost=0.500,simulation_cost=0.500,upload_price_breakdown={mesh_instance=0,mesh_physics=0,mesh_streaming=0,model=0,texture=0}},state=\"upload\",upload_price=0,uploader=<url>}",
            ),
        ),
    ),
    (
        "step1_sound",
        each(
            Answer::Text(
                "200: {balance=an amount,state=\"upload\",upload_price=10,uploader=<url>}",
            ),
            Answer::Text("200: {state=\"upload\",upload_price=0,uploader=<url>}"),
        ),
    ),
    (
        "step1_unknown_folder",
        each(
            Answer::Text(
                "200: {balance=an amount,state=\"upload\",upload_price=10,uploader=<url>}",
            ),
            Answer::Text("200: {state=\"upload\",upload_price=0,uploader=<url>}"),
        ),
    ),
    (
        "thumbnail_get",
        each(
            Answer::Text("501: text/html \"message Not implemented error\""),
            Answer::Text("no capability"),
        ),
    ),
    (
        "thumbnail_item_read_back",
        each(
            Answer::Text("200: the uploaded asset"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "thumbnail_step1_both_ids",
        each(
            Answer::Text("200: {state=\"upload\",uploader=<url>}"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "thumbnail_step1_folder",
        each(
            Answer::Text("200: {state=\"upload\",uploader=<url>}"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "thumbnail_step1_item",
        each(
            Answer::Text("200: {state=\"upload\",uploader=<url>}"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "thumbnail_step1_nil_item",
        each(
            Answer::Text("200: {state=\"upload\",uploader=<url>}"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "thumbnail_step1_no_id",
        each(
            Answer::Text("200: {state=\"upload\",uploader=<url>}"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "thumbnail_step1_task",
        each(
            Answer::Text("200: {state=\"upload\",uploader=<url>}"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "thumbnail_step1_unknown_item",
        each(
            Answer::Text("200: {state=\"upload\",uploader=<url>}"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "thumbnail_upload_folder",
        each(
            Answer::Text("200: {new_asset=<id>,state=\"complete\"}"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "thumbnail_upload_item",
        each(
            Answer::Text("200: {new_asset=<id>,state=\"complete\"}"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "thumbnail_upload_item_charged",
        each(Answer::Count(0), Answer::Text("no capability")),
    ),
    (
        "thumbnail_upload_item_told",
        each(Answer::Text("nothing"), Answer::Text("no capability")),
    ),
    (
        "thumbnail_upload_large",
        each(
            Answer::Text("200: {message=\"Invalid width: Value too large.\",state=\"failure\"}"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "thumbnail_upload_no_id",
        each(
            Answer::Text("500: text/html a stack trace"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "thumbnail_upload_noise",
        each(
            Answer::Text("200: {message=\"LLKDUMessageError::flush()\",state=\"failure\"}"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "thumbnail_upload_nothing",
        each(
            Answer::Text("200: {message=\"File is empty FILE: <file>\",state=\"failure\"}"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "thumbnail_upload_oblong",
        each(
            Answer::Text("200: {new_asset=<id>,state=\"complete\"}"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "thumbnail_upload_small",
        each(
            Answer::Text("200: {new_asset=<id>,state=\"complete\"}"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "thumbnail_upload_task",
        each(
            Answer::Text("200: {new_asset=<id>,state=\"complete\"}"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "thumbnail_upload_unknown_item",
        each(
            Answer::Text("500: text/html a stack trace"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "upload_after_get",
        each(
            Answer::Text("404: no media type \"cap not found: '<id>'\""),
            Answer::Text(
                "200: {inventory_item_flags=0,new_asset=\"<id>\",new_everyone_mask=0,new_group_mask=0,new_inventory_item=<id>,new_next_owner_mask=581632,state=\"complete\"}",
            ),
        ),
    ),
    ("upload_after_get_charged", both(Answer::Count(0))),
    ("upload_after_get_told", both(Answer::Text("nothing"))),
    (
        "upload_animation",
        each(
            Answer::Text(
                "200: {balance=undef,inventory_flags=0,new_asset=<id>,new_base_mask=2147483647,new_everyone_mask=0,new_group_mask=0,new_inventory_item=<id>,new_next_owner_mask=581632,new_owner_mask=2147483647,state=\"complete\",transaction_id=<id>,upload_price=10}",
            ),
            Answer::Text(
                "200: {inventory_item_flags=0,new_asset=\"<id>\",new_everyone_mask=0,new_group_mask=0,new_inventory_item=<id>,new_next_owner_mask=581632,state=\"complete\"}",
            ),
        ),
    ),
    (
        "upload_animation_charged",
        each(Answer::Count(10), Answer::Count(0)),
    ),
    (
        "upload_animation_cut_short",
        each(
            Answer::Text(
                "200: {error={identifier=\"Upload_InvalidAsset\",message=\"Bad .anim file conf-<n>-animation_cut_short: error reading <file>: unpack_from requires a buffer of at least 4 bytes\",success=false},message=\"Bad .anim file conf-<n>-animation_cut_short: error reading <file>: unpack_from requires a buffer of at least 4 bytes\",state=\"failure\"}",
            ),
            Answer::Text(
                "200: {inventory_item_flags=0,new_asset=\"<id>\",new_everyone_mask=0,new_group_mask=0,new_inventory_item=<id>,new_next_owner_mask=581632,state=\"complete\"}",
            ),
        ),
    ),
    ("upload_animation_cut_short_charged", both(Answer::Count(0))),
    (
        "upload_animation_cut_short_told",
        both(Answer::Text("nothing")),
    ),
    (
        "upload_animation_long",
        each(
            Answer::Text(
                "200: {balance=undef,inventory_flags=0,new_asset=<id>,new_base_mask=2147483647,new_everyone_mask=0,new_group_mask=0,new_inventory_item=<id>,new_next_owner_mask=581632,new_owner_mask=2147483647,state=\"complete\",transaction_id=<id>,upload_price=10}",
            ),
            Answer::Text(
                "200: {inventory_item_flags=0,new_asset=\"<id>\",new_everyone_mask=0,new_group_mask=0,new_inventory_item=<id>,new_next_owner_mask=581632,state=\"complete\"}",
            ),
        ),
    ),
    (
        "upload_animation_long_charged",
        each(Answer::Count(10), Answer::Count(0)),
    ),
    (
        "upload_animation_long_told",
        each(
            Answer::Text("a balance (success, no transaction, \"\")"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "upload_animation_made",
        both(Answer::Text("asset type 20, inventory type 19, flags 0")),
    ),
    (
        "upload_animation_noise",
        each(
            Answer::Text(
                "200: {error={identifier=\"Upload_InvalidAsset\",message=\"Bad .anim file conf-<n>-animation_noise: Bad combination of version, sub_version: <n> <n>\",success=false},message=\"Bad .anim file conf-<n>-animation_noise: Bad combination of version, sub_version: <n> <n>\",state=\"failure\"}",
            ),
            Answer::Text(
                "200: {inventory_item_flags=0,new_asset=\"<id>\",new_everyone_mask=0,new_group_mask=0,new_inventory_item=<id>,new_next_owner_mask=581632,state=\"complete\"}",
            ),
        ),
    ),
    ("upload_animation_noise_charged", both(Answer::Count(0))),
    ("upload_animation_noise_told", both(Answer::Text("nothing"))),
    (
        "upload_animation_nothing",
        each(
            Answer::Text(
                "200: {error={identifier=\"Upload_InvalidAsset\",message=\"Bad .anim file conf-<n>-animation_nothing: error reading <file>: unpack_from requires a buffer of at least 12 bytes\",success=false},message=\"Bad .anim file conf-<n>-animation_nothing: error reading <file>: unpack_from requires a buffer of at least 12 bytes\",state=\"failure\"}",
            ),
            Answer::Text(
                "200: {inventory_item_flags=0,new_asset=\"<id>\",new_everyone_mask=0,new_group_mask=0,new_inventory_item=<id>,new_next_owner_mask=581632,state=\"complete\"}",
            ),
        ),
    ),
    ("upload_animation_nothing_charged", both(Answer::Count(0))),
    (
        "upload_animation_nothing_told",
        both(Answer::Text("nothing")),
    ),
    (
        "upload_animation_told",
        each(
            Answer::Text("a balance (success, no transaction, \"\")"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "upload_class_bodypart",
        each(
            Answer::Text(
                "200: {error={expected_upload_price=0,identifier=\"Upload_UploadPriceDiffers\",message=\"The server expects a different upload fee\",upload_price=10},message=\"The server expects a different upload fee\",state=\"failure\"}",
            ),
            Answer::Text(
                "200: {inventory_item_flags=0,new_asset=\"<id>\",new_everyone_mask=0,new_group_mask=0,new_inventory_item=<id>,new_next_owner_mask=581632,state=\"complete\"}",
            ),
        ),
    ),
    ("upload_class_bodypart_charged", both(Answer::Count(0))),
    (
        "upload_class_bodypart_made",
        each(
            Answer::Text("no item named"),
            Answer::Text("asset type 13, inventory type 18, flags 0"),
        ),
    ),
    ("upload_class_bodypart_told", both(Answer::Text("nothing"))),
    ("upload_class_callingcard", both(Answer::Text("not asked"))),
    (
        "upload_class_clothing",
        each(
            Answer::Text(
                "200: {error={expected_upload_price=0,identifier=\"Upload_UploadPriceDiffers\",message=\"The server expects a different upload fee\",upload_price=10},message=\"The server expects a different upload fee\",state=\"failure\"}",
            ),
            Answer::Text(
                "200: {inventory_item_flags=0,new_asset=\"<id>\",new_everyone_mask=0,new_group_mask=0,new_inventory_item=<id>,new_next_owner_mask=581632,state=\"complete\"}",
            ),
        ),
    ),
    ("upload_class_clothing_charged", both(Answer::Count(0))),
    (
        "upload_class_clothing_made",
        each(
            Answer::Text("no item named"),
            Answer::Text("asset type 5, inventory type 18, flags 0"),
        ),
    ),
    ("upload_class_clothing_told", both(Answer::Text("nothing"))),
    (
        "upload_class_gesture",
        each(
            Answer::Text("no uploader to post to"),
            Answer::Text(
                "200: {inventory_item_flags=0,new_asset=\"<id>\",new_everyone_mask=0,new_group_mask=0,new_inventory_item=<id>,new_next_owner_mask=581632,state=\"complete\"}",
            ),
        ),
    ),
    (
        "upload_class_gesture_charged",
        each(Answer::Text("no uploader to post to"), Answer::Count(0)),
    ),
    (
        "upload_class_gesture_made",
        each(
            Answer::Text("no item named"),
            Answer::Text("asset type 0, inventory type 0, flags 0"),
        ),
    ),
    (
        "upload_class_gesture_told",
        each(
            Answer::Text("no uploader to post to"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "upload_class_landmark",
        each(
            Answer::Text("no uploader to post to"),
            Answer::Text(
                "200: {inventory_item_flags=0,new_asset=\"<id>\",new_everyone_mask=0,new_group_mask=0,new_inventory_item=<id>,new_next_owner_mask=581632,state=\"complete\"}",
            ),
        ),
    ),
    (
        "upload_class_landmark_charged",
        each(Answer::Text("no uploader to post to"), Answer::Count(0)),
    ),
    (
        "upload_class_landmark_made",
        each(
            Answer::Text("no item named"),
            Answer::Text("asset type 0, inventory type 0, flags 0"),
        ),
    ),
    (
        "upload_class_landmark_told",
        each(
            Answer::Text("no uploader to post to"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "upload_class_material",
        each(
            Answer::Text("no uploader to post to"),
            Answer::Text(
                "200: {inventory_item_flags=0,new_asset=\"<id>\",new_everyone_mask=0,new_group_mask=0,new_inventory_item=<id>,new_next_owner_mask=581632,state=\"complete\"}",
            ),
        ),
    ),
    (
        "upload_class_material_charged",
        each(Answer::Text("no uploader to post to"), Answer::Count(0)),
    ),
    (
        "upload_class_material_made",
        each(
            Answer::Text("no item named"),
            Answer::Text("asset type 0, inventory type 0, flags 0"),
        ),
    ),
    (
        "upload_class_material_told",
        each(
            Answer::Text("no uploader to post to"),
            Answer::Text("nothing"),
        ),
    ),
    ("upload_class_mesh_asset", both(Answer::Text("not asked"))),
    (
        "upload_class_notecard",
        each(
            Answer::Text("no uploader to post to"),
            Answer::Text(
                "200: {inventory_item_flags=0,new_asset=\"<id>\",new_everyone_mask=0,new_group_mask=0,new_inventory_item=<id>,new_next_owner_mask=581632,state=\"complete\"}",
            ),
        ),
    ),
    (
        "upload_class_notecard_charged",
        each(Answer::Text("no uploader to post to"), Answer::Count(0)),
    ),
    (
        "upload_class_notecard_made",
        each(
            Answer::Text("no item named"),
            Answer::Text("asset type 0, inventory type 0, flags 0"),
        ),
    ),
    (
        "upload_class_notecard_told",
        each(
            Answer::Text("no uploader to post to"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "upload_class_object",
        each(
            Answer::Text("no uploader to post to"),
            Answer::Text(
                "200: {inventory_item_flags=0,new_asset=\"<id>\",new_everyone_mask=0,new_group_mask=0,new_inventory_item=<id>,new_next_owner_mask=581632,state=\"complete\"}",
            ),
        ),
    ),
    (
        "upload_class_object_charged",
        each(Answer::Text("no uploader to post to"), Answer::Count(0)),
    ),
    (
        "upload_class_object_made",
        each(
            Answer::Text("no item named"),
            Answer::Text("200: no such item"),
        ),
    ),
    (
        "upload_class_object_told",
        each(
            Answer::Text("no uploader to post to"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "upload_class_script",
        each(
            Answer::Text("no uploader to post to"),
            Answer::Text(
                "200: {inventory_item_flags=0,new_asset=\"<id>\",new_everyone_mask=0,new_group_mask=0,new_inventory_item=<id>,new_next_owner_mask=581632,state=\"complete\"}",
            ),
        ),
    ),
    (
        "upload_class_script_charged",
        each(Answer::Text("no uploader to post to"), Answer::Count(0)),
    ),
    (
        "upload_class_script_made",
        each(
            Answer::Text("no item named"),
            Answer::Text("asset type 0, inventory type 0, flags 0"),
        ),
    ),
    (
        "upload_class_script_told",
        each(
            Answer::Text("no uploader to post to"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "upload_class_settings",
        each(
            Answer::Text("no uploader to post to"),
            Answer::Text(
                "200: {inventory_item_flags=0,new_asset=\"<id>\",new_everyone_mask=0,new_group_mask=0,new_inventory_item=<id>,new_next_owner_mask=581632,state=\"complete\"}",
            ),
        ),
    ),
    (
        "upload_class_settings_charged",
        each(Answer::Text("no uploader to post to"), Answer::Count(0)),
    ),
    (
        "upload_class_settings_made",
        each(
            Answer::Text("no item named"),
            Answer::Text("asset type 0, inventory type 0, flags 0"),
        ),
    ),
    (
        "upload_class_settings_told",
        each(
            Answer::Text("no uploader to post to"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "upload_class_snapshot",
        each(
            Answer::Text(
                "200: {error={expected_upload_price=0,identifier=\"Upload_UploadPriceDiffers\",message=\"The server expects a different upload fee\",upload_price=10},message=\"The server expects a different upload fee\",state=\"failure\"}",
            ),
            Answer::Text(
                "200: {inventory_item_flags=0,new_asset=\"<id>\",new_everyone_mask=0,new_group_mask=0,new_inventory_item=<id>,new_next_owner_mask=581632,state=\"complete\"}",
            ),
        ),
    ),
    ("upload_class_snapshot_charged", both(Answer::Count(0))),
    (
        "upload_class_snapshot_made",
        each(
            Answer::Text("no item named"),
            Answer::Text("asset type 0, inventory type 15, flags 0"),
        ),
    ),
    ("upload_class_snapshot_told", both(Answer::Text("nothing"))),
    ("upload_class_unknown", both(Answer::Text("not asked"))),
    (
        "upload_cost_huge",
        each(
            Answer::Text(
                "200: {error={balance=an amount,identifier=\"Upload_InsufficientLindenDollarBalance\",message=\"Insufficient L$ balance to upload\",upload_price=1000000000},message=\"Insufficient L$ balance to upload\",state=\"failure\"}",
            ),
            Answer::Text(
                "200: {inventory_item_flags=0,new_asset=\"<id>\",new_everyone_mask=0,new_group_mask=0,new_inventory_item=<id>,new_next_owner_mask=581632,state=\"complete\"}",
            ),
        ),
    ),
    ("upload_cost_huge_charged", both(Answer::Count(0))),
    ("upload_cost_huge_told", both(Answer::Text("nothing"))),
    (
        "upload_cost_negative",
        each(
            Answer::Text(
                "200: {error={expected_upload_price=-1,identifier=\"Upload_UploadPriceDiffers\",message=\"The server expects a different upload fee\",upload_price=10},message=\"The server expects a different upload fee\",state=\"failure\"}",
            ),
            Answer::Text(
                "200: {inventory_item_flags=0,new_asset=\"<id>\",new_everyone_mask=0,new_group_mask=0,new_inventory_item=<id>,new_next_owner_mask=581632,state=\"complete\"}",
            ),
        ),
    ),
    ("upload_cost_negative_charged", both(Answer::Count(0))),
    ("upload_cost_negative_told", both(Answer::Text("nothing"))),
    (
        "upload_cost_none",
        each(
            Answer::Text(
                "200: {balance=undef,inventory_flags=0,new_asset=<id>,new_base_mask=2147483647,new_everyone_mask=0,new_group_mask=0,new_inventory_item=<id>,new_next_owner_mask=581632,new_owner_mask=2147483647,state=\"complete\",transaction_id=<id>,upload_price=10}",
            ),
            Answer::Text(
                "200: {inventory_item_flags=0,new_asset=\"<id>\",new_everyone_mask=0,new_group_mask=0,new_inventory_item=<id>,new_next_owner_mask=581632,state=\"complete\"}",
            ),
        ),
    ),
    (
        "upload_cost_none_charged",
        each(Answer::Count(10), Answer::Count(0)),
    ),
    (
        "upload_cost_none_told",
        each(
            Answer::Text("a balance (success, no transaction, \"\")"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "upload_cost_one",
        each(
            Answer::Text(
                "200: {error={expected_upload_price=1,identifier=\"Upload_UploadPriceDiffers\",message=\"The server expects a different upload fee\",upload_price=10},message=\"The server expects a different upload fee\",state=\"failure\"}",
            ),
            Answer::Text(
                "200: {inventory_item_flags=0,new_asset=\"<id>\",new_everyone_mask=0,new_group_mask=0,new_inventory_item=<id>,new_next_owner_mask=581632,state=\"complete\"}",
            ),
        ),
    ),
    ("upload_cost_one_charged", both(Answer::Count(0))),
    ("upload_cost_one_told", both(Answer::Text("nothing"))),
    (
        "upload_cost_over",
        each(
            Answer::Text(
                "200: {error={expected_upload_price=11,identifier=\"Upload_UploadPriceDiffers\",message=\"The server expects a different upload fee\",upload_price=10},message=\"The server expects a different upload fee\",state=\"failure\"}",
            ),
            Answer::Text(
                "200: {inventory_item_flags=0,new_asset=\"<id>\",new_everyone_mask=0,new_group_mask=0,new_inventory_item=<id>,new_next_owner_mask=581632,state=\"complete\"}",
            ),
        ),
    ),
    ("upload_cost_over_charged", both(Answer::Count(0))),
    ("upload_cost_over_told", both(Answer::Text("nothing"))),
    (
        "upload_cost_right",
        each(
            Answer::Text(
                "200: {balance=undef,inventory_flags=0,new_asset=<id>,new_base_mask=2147483647,new_everyone_mask=0,new_group_mask=0,new_inventory_item=<id>,new_next_owner_mask=581632,new_owner_mask=2147483647,state=\"complete\",transaction_id=<id>,upload_price=10}",
            ),
            Answer::Text(
                "200: {inventory_item_flags=0,new_asset=\"<id>\",new_everyone_mask=0,new_group_mask=0,new_inventory_item=<id>,new_next_owner_mask=581632,state=\"complete\"}",
            ),
        ),
    ),
    (
        "upload_cost_right_charged",
        each(Answer::Count(10), Answer::Count(0)),
    ),
    (
        "upload_cost_right_told",
        each(
            Answer::Text("a balance (success, no transaction, \"\")"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "upload_cost_text",
        each(
            Answer::Text("no uploader to post to"),
            Answer::Text(
                "200: {inventory_item_flags=0,new_asset=\"<id>\",new_everyone_mask=0,new_group_mask=0,new_inventory_item=<id>,new_next_owner_mask=581632,state=\"complete\"}",
            ),
        ),
    ),
    (
        "upload_cost_text_charged",
        each(Answer::Text("no uploader to post to"), Answer::Count(0)),
    ),
    (
        "upload_cost_text_told",
        each(
            Answer::Text("no uploader to post to"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "upload_cost_zero",
        each(
            Answer::Text(
                "200: {error={expected_upload_price=0,identifier=\"Upload_UploadPriceDiffers\",message=\"The server expects a different upload fee\",upload_price=10},message=\"The server expects a different upload fee\",state=\"failure\"}",
            ),
            Answer::Text(
                "200: {inventory_item_flags=0,new_asset=\"<id>\",new_everyone_mask=0,new_group_mask=0,new_inventory_item=<id>,new_next_owner_mask=581632,state=\"complete\"}",
            ),
        ),
    ),
    ("upload_cost_zero_charged", both(Answer::Count(0))),
    ("upload_cost_zero_told", both(Answer::Text("nothing"))),
    (
        "upload_pauper",
        each(
            Answer::Text("no uploader to post to"),
            Answer::Text(
                "200: {inventory_item_flags=0,new_asset=\"<id>\",new_everyone_mask=0,new_group_mask=0,new_inventory_item=<id>,new_next_owner_mask=581632,state=\"complete\"}",
            ),
        ),
    ),
    (
        "upload_pauper_charged",
        each(Answer::Text("no uploader to post to"), Answer::Count(0)),
    ),
    (
        "upload_pauper_told",
        each(
            Answer::Text("no uploader to post to"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "upload_sound",
        each(
            Answer::Text(
                "200: {balance=undef,inventory_flags=0,new_asset=<id>,new_base_mask=2147483647,new_everyone_mask=0,new_group_mask=0,new_inventory_item=<id>,new_next_owner_mask=581632,new_owner_mask=2147483647,state=\"complete\",transaction_id=<id>,upload_price=10}",
            ),
            Answer::Text(
                "200: {inventory_item_flags=0,new_asset=\"<id>\",new_everyone_mask=0,new_group_mask=0,new_inventory_item=<id>,new_next_owner_mask=581632,state=\"complete\"}",
            ),
        ),
    ),
    (
        "upload_sound_22k",
        each(
            Answer::Text(
                "200: {balance=undef,inventory_flags=0,new_asset=<id>,new_base_mask=2147483647,new_everyone_mask=0,new_group_mask=0,new_inventory_item=<id>,new_next_owner_mask=581632,new_owner_mask=2147483647,state=\"complete\",transaction_id=<id>,upload_price=10}",
            ),
            Answer::Text(
                "200: {inventory_item_flags=0,new_asset=\"<id>\",new_everyone_mask=0,new_group_mask=0,new_inventory_item=<id>,new_next_owner_mask=581632,state=\"complete\"}",
            ),
        ),
    ),
    (
        "upload_sound_22k_charged",
        each(Answer::Count(10), Answer::Count(0)),
    ),
    (
        "upload_sound_22k_told",
        each(
            Answer::Text("a balance (success, no transaction, \"\")"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "upload_sound_charged",
        each(Answer::Count(10), Answer::Count(0)),
    ),
    (
        "upload_sound_long",
        each(
            Answer::Text(
                "200: {error={identifier=\"Upload_InvalidAsset\",message=\"Sample too long.\",success=false},message=\"Sample too long.\",state=\"failure\"}",
            ),
            Answer::Text(
                "200: {inventory_item_flags=0,new_asset=\"<id>\",new_everyone_mask=0,new_group_mask=0,new_inventory_item=<id>,new_next_owner_mask=581632,state=\"complete\"}",
            ),
        ),
    ),
    ("upload_sound_long_charged", both(Answer::Count(0))),
    ("upload_sound_long_told", both(Answer::Text("nothing"))),
    (
        "upload_sound_made",
        both(Answer::Text("asset type 1, inventory type 1, flags 0")),
    ),
    (
        "upload_sound_noise",
        each(
            Answer::Text(
                "200: {error={identifier=\"Upload_InvalidAsset\",message=\"Internal error from verifier.\",success=false},message=\"Internal error from verifier.\",state=\"failure\"}",
            ),
            Answer::Text(
                "200: {inventory_item_flags=0,new_asset=\"<id>\",new_everyone_mask=0,new_group_mask=0,new_inventory_item=<id>,new_next_owner_mask=581632,state=\"complete\"}",
            ),
        ),
    ),
    ("upload_sound_noise_charged", both(Answer::Count(0))),
    ("upload_sound_noise_told", both(Answer::Text("nothing"))),
    (
        "upload_sound_nothing",
        each(
            Answer::Text(
                "200: {error={identifier=\"Upload_InvalidAsset\",message=\"Internal error from verifier.\",success=false},message=\"Internal error from verifier.\",state=\"failure\"}",
            ),
            Answer::Text(
                "200: {inventory_item_flags=0,new_asset=\"<id>\",new_everyone_mask=0,new_group_mask=0,new_inventory_item=<id>,new_next_owner_mask=581632,state=\"complete\"}",
            ),
        ),
    ),
    ("upload_sound_nothing_charged", both(Answer::Count(0))),
    ("upload_sound_nothing_told", both(Answer::Text("nothing"))),
    (
        "upload_sound_stereo",
        each(
            Answer::Text(
                "200: {balance=undef,inventory_flags=0,new_asset=<id>,new_base_mask=2147483647,new_everyone_mask=0,new_group_mask=0,new_inventory_item=<id>,new_next_owner_mask=581632,new_owner_mask=2147483647,state=\"complete\",transaction_id=<id>,upload_price=10}",
            ),
            Answer::Text(
                "200: {inventory_item_flags=0,new_asset=\"<id>\",new_everyone_mask=0,new_group_mask=0,new_inventory_item=<id>,new_next_owner_mask=581632,state=\"complete\"}",
            ),
        ),
    ),
    (
        "upload_sound_stereo_charged",
        each(Answer::Count(10), Answer::Count(0)),
    ),
    (
        "upload_sound_stereo_told",
        each(
            Answer::Text("a balance (success, no transaction, \"\")"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "upload_sound_told",
        each(
            Answer::Text("a balance (success, no transaction, \"\")"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "upload_sound_wav",
        each(
            Answer::Text(
                "200: {error={identifier=\"Upload_InvalidAsset\",message=\"Internal error from verifier.\",success=false},message=\"Internal error from verifier.\",state=\"failure\"}",
            ),
            Answer::Text(
                "200: {inventory_item_flags=0,new_asset=\"<id>\",new_everyone_mask=0,new_group_mask=0,new_inventory_item=<id>,new_next_owner_mask=581632,state=\"complete\"}",
            ),
        ),
    ),
    ("upload_sound_wav_charged", both(Answer::Count(0))),
    ("upload_sound_wav_told", both(Answer::Text("nothing"))),
    (
        "upload_texture",
        each(
            Answer::Text(
                "200: {balance=undef,inventory_flags=0,new_asset=<id>,new_base_mask=2147483647,new_everyone_mask=0,new_group_mask=0,new_inventory_item=<id>,new_next_owner_mask=581632,new_owner_mask=2147483647,state=\"complete\",transaction_id=<id>,upload_price=10}",
            ),
            Answer::Text(
                "200: {inventory_item_flags=0,new_asset=\"<id>\",new_everyone_mask=0,new_group_mask=0,new_inventory_item=<id>,new_next_owner_mask=581632,state=\"complete\"}",
            ),
        ),
    ),
    (
        "upload_texture_charged",
        each(Answer::Count(10), Answer::Count(0)),
    ),
    (
        "upload_texture_large_flat_cost",
        each(
            Answer::Text(
                "200: {error={expected_upload_price=10,identifier=\"Upload_UploadPriceDiffers\",message=\"The server expects a different upload fee\",upload_price=50},message=\"The server expects a different upload fee\",state=\"failure\"}",
            ),
            Answer::Text(
                "200: {inventory_item_flags=0,new_asset=\"<id>\",new_everyone_mask=0,new_group_mask=0,new_inventory_item=<id>,new_next_owner_mask=581632,state=\"complete\"}",
            ),
        ),
    ),
    (
        "upload_texture_large_flat_cost_charged",
        both(Answer::Count(0)),
    ),
    (
        "upload_texture_large_flat_cost_told",
        both(Answer::Text("nothing")),
    ),
    (
        "upload_texture_made",
        both(Answer::Text("asset type 0, inventory type 0, flags 0")),
    ),
    (
        "upload_texture_nil_folder",
        each(
            Answer::Text(
                "200: {balance=undef,inventory_flags=0,new_asset=<id>,new_base_mask=2147483647,new_everyone_mask=0,new_group_mask=0,new_inventory_item=<id>,new_next_owner_mask=581632,new_owner_mask=2147483647,state=\"complete\",transaction_id=<id>,upload_price=10}",
            ),
            Answer::Text(
                "200: {inventory_item_flags=0,new_asset=\"<id>\",new_everyone_mask=0,new_group_mask=0,new_inventory_item=<id>,new_next_owner_mask=581632,state=\"complete\"}",
            ),
        ),
    ),
    (
        "upload_texture_nil_folder_charged",
        each(Answer::Count(10), Answer::Count(0)),
    ),
    (
        "upload_texture_nil_folder_made",
        both(Answer::Text("asset type 0, inventory type 0, flags 0")),
    ),
    (
        "upload_texture_nil_folder_told",
        each(
            Answer::Text("a balance (success, no transaction, \"\")"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "upload_texture_noise",
        each(
            Answer::Text(
                "200: {error={identifier=\"Upload_InvalidAsset\",message=\"LLKDUMessageError::flush()\",success=false},message=\"LLKDUMessageError::flush()\",state=\"failure\"}",
            ),
            Answer::Text(
                "200: {inventory_item_flags=0,new_asset=\"<id>\",new_everyone_mask=0,new_group_mask=0,new_inventory_item=<id>,new_next_owner_mask=581632,state=\"complete\"}",
            ),
        ),
    ),
    ("upload_texture_noise_charged", both(Answer::Count(0))),
    ("upload_texture_noise_told", both(Answer::Text("nothing"))),
    (
        "upload_texture_nothing",
        each(
            Answer::Text(
                "200: {error={identifier=\"Upload_InvalidAsset\",message=\"File is empty FILE: <file>\",success=false},message=\"File is empty FILE: <file>\",state=\"failure\"}",
            ),
            Answer::Text(
                "200: {inventory_item_flags=0,new_asset=\"<id>\",new_everyone_mask=0,new_group_mask=0,new_inventory_item=<id>,new_next_owner_mask=581632,state=\"complete\"}",
            ),
        ),
    ),
    ("upload_texture_nothing_charged", both(Answer::Count(0))),
    ("upload_texture_nothing_told", both(Answer::Text("nothing"))),
    (
        "upload_texture_odd_size",
        each(
            Answer::Text(
                "200: {error={identifier=\"Upload_InvalidAsset\",message=\"Invalid width: Value not a power of 2.\",success=false},message=\"Invalid width: Value not a power of 2.\",state=\"failure\"}",
            ),
            Answer::Text(
                "200: {inventory_item_flags=0,new_asset=\"<id>\",new_everyone_mask=0,new_group_mask=0,new_inventory_item=<id>,new_next_owner_mask=581632,state=\"complete\"}",
            ),
        ),
    ),
    ("upload_texture_odd_size_charged", both(Answer::Count(0))),
    (
        "upload_texture_odd_size_told",
        both(Answer::Text("nothing")),
    ),
    (
        "upload_texture_png",
        each(
            Answer::Text(
                "200: {error={identifier=\"Upload_InvalidAsset\",message=\"LLKDUMessageError::flush()\",success=false},message=\"LLKDUMessageError::flush()\",state=\"failure\"}",
            ),
            Answer::Text(
                "200: {inventory_item_flags=0,new_asset=\"<id>\",new_everyone_mask=0,new_group_mask=0,new_inventory_item=<id>,new_next_owner_mask=581632,state=\"complete\"}",
            ),
        ),
    ),
    ("upload_texture_png_charged", both(Answer::Count(0))),
    ("upload_texture_png_told", both(Answer::Text("nothing"))),
    (
        "upload_texture_small_large_cost",
        each(
            Answer::Text(
                "200: {error={expected_upload_price=50,identifier=\"Upload_UploadPriceDiffers\",message=\"The server expects a different upload fee\",upload_price=10},message=\"The server expects a different upload fee\",state=\"failure\"}",
            ),
            Answer::Text(
                "200: {inventory_item_flags=0,new_asset=\"<id>\",new_everyone_mask=0,new_group_mask=0,new_inventory_item=<id>,new_next_owner_mask=581632,state=\"complete\"}",
            ),
        ),
    ),
    (
        "upload_texture_small_large_cost_charged",
        both(Answer::Count(0)),
    ),
    (
        "upload_texture_small_large_cost_told",
        both(Answer::Text("nothing")),
    ),
    (
        "upload_texture_told",
        each(
            Answer::Text("a balance (success, no transaction, \"\")"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "upload_texture_unknown_folder",
        each(
            Answer::Text(
                "200: {balance=undef,inventory_flags=0,new_asset=<id>,new_base_mask=2147483647,new_everyone_mask=0,new_group_mask=0,new_inventory_item=<id>,new_next_owner_mask=581632,new_owner_mask=2147483647,state=\"complete\",transaction_id=<id>,upload_price=10}",
            ),
            Answer::Text(
                "200: {inventory_item_flags=0,new_asset=\"<id>\",new_everyone_mask=0,new_group_mask=0,new_inventory_item=<id>,new_next_owner_mask=581632,state=\"complete\"}",
            ),
        ),
    ),
    (
        "upload_texture_unknown_folder_charged",
        each(Answer::Count(10), Answer::Count(0)),
    ),
    (
        "upload_texture_unknown_folder_made",
        both(Answer::Text("asset type 0, inventory type 0, flags 0")),
    ),
    (
        "upload_texture_unknown_folder_told",
        each(
            Answer::Text("a balance (success, no transaction, \"\")"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "uploader_again",
        each(
            Answer::Text("404: no media type \"cap not found: '<id>'\""),
            Answer::Text("200: no body"),
        ),
    ),
    ("uploader_again_charged", both(Answer::Count(0))),
    (
        "uploader_get",
        each(
            Answer::Text("501: text/html \"message Not implemented error\""),
            Answer::Text("404: text/html \"404 Page not found\""),
        ),
    ),
    ("uploader_on_the_capability_host", both(Answer::Flag(true))),
];

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

/// The questions neither fake flavour is held to, by the end of the metric's
/// name:
///
/// - what an upload was **charged** and whether a balance is answered at
///   all: the fake grid answers no balance request (that is the money
///   ledger's work, `gridspec-money`);
/// - somebody who **cannot pay**: with no ledger nobody can give their L$
///   away. The refusal itself is `sl-proto`'s to test, and is;
/// - a **mesh**'s cost data and what is made of its bytes
///   (`server-fake-grid-mesh-upload-cost`);
/// - a thumbnail **read back** from the item: the fake grid's inventory has
///   no place for one yet.
const NOT_IMITATED: &[&str] = &[
    "_charged",
    "balance_answered",
    "pauper_can_pay",
    "step1_pauper",
    "upload_pauper",
    "upload_pauper_told",
    "step1_pauper_mesh",
    "step1_mesh",
    "step1_mesh_no_physics",
    "step1_mesh_noise",
    "thumbnail_item_read_back",
];

/// What a Second-Life-flavoured fake grid is not held to besides: the
/// balance Second Life pushes after it charged one (the ledger's again), and
/// its verifier's words for an animation it cannot read, which are a Python
/// interpreter's.
const NOT_IMITATED_AS_SECOND_LIFE: &[&str] = &[
    "_told",
    "upload_animation_cut_short",
    "upload_animation_nothing",
];

/// A census of the upload capabilities as HTTP.
#[derive(Debug)]
pub struct UploadHttp;

impl GridTest for UploadHttp {
    fn name(&self) -> &'static str {
        "upload-http"
    }

    fn description(&self) -> &'static str {
        "What the upload capabilities answer, what an upload costs and what is said of it"
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
            let mut residue = Residue::default();
            let outcome = census(ctx, &mut metrics, &mut residue).await;
            ctx.metrics().merge(metrics.clone());
            // Take away what the census made, whatever became of it.
            let session = ctx.primary();
            if !residue.items.is_empty() {
                session
                    .send(Command::RemoveInventoryItems(residue.items.clone()))
                    .await
                    .ok();
            }
            if !residue.folders.is_empty() {
                session
                    .send(Command::RemoveInventoryFolders(residue.folders.clone()))
                    .await
                    .ok();
            }
            outcome?;
            let held: Vec<(&str, Measured<Answer>)> = MEASURED
                .iter()
                .filter(|(key, _measured)| {
                    let left_out =
                        |suffixes: &[&str]| suffixes.iter().any(|suffix| key.ends_with(suffix));
                    !(grid.is_fake() && left_out(NOT_IMITATED)
                        || grid == Grid::FakeSl && left_out(NOT_IMITATED_AS_SECOND_LIFE))
                })
                .copied()
                .collect();
            hold_to_measured(grid, &metrics, &held, SOURCE)
        })
    }
}

/// What the census made in the first avatar's inventory.
#[derive(Debug, Default)]
struct Residue {
    /// The items uploads created.
    items: Vec<InventoryKey>,
    /// The folders the case made.
    folders: Vec<InventoryFolderKey>,
}

/// What a capability answered one request with.
#[derive(Debug, Clone)]
struct Reply {
    /// The HTTP status.
    status: u16,
    /// The media type the answer states, without its parameters.
    media_type: String,
    /// The body, as sent.
    body: Vec<u8>,
}

impl Reply {
    /// The body as LLSD, where it is LLSD.
    fn llsd(&self) -> Option<Llsd> {
        // A page of well-formed HTML reads as an LLSD document with nothing
        // in it, so the document has to say it is one.
        std::str::from_utf8(&self.body)
            .ok()
            .filter(|text| text.contains("<llsd"))
            .and_then(|text| parse_llsd_xml(text).ok())
    }

    /// The uploader the answer names, where it names one.
    fn uploader(&self) -> Option<String> {
        match self.llsd()?.get("uploader")? {
            Llsd::String(url) | Llsd::Uri(url) if !url.is_empty() => Some(url.clone()),
            _other => None,
        }
    }

    /// The id the answer states under `key`, where it is one and not nil.
    fn id(&self, key: &str) -> Option<Uuid> {
        let id = match self.llsd()?.get(key)? {
            Llsd::Uuid(id) => *id,
            Llsd::String(text) => Uuid::parse_str(text).ok()?,
            _other => return None,
        };
        (!id.is_nil()).then_some(id)
    }

    /// The answer as a metric records it: the status and the body, an LLSD
    /// body key by key and any other as its words.
    fn said(&self) -> String {
        if self.body.is_empty() {
            return format!("{}: no body", self.status);
        }
        match self.llsd() {
            Some(body) => format!("{}: {}", self.status, rendered(&body, None)),
            None => {
                let words = match std::str::from_utf8(&self.body) {
                    // A handler's stack trace names its own lines.
                    Ok(text) if text.contains("stack-trace") => "a stack trace".to_owned(),
                    Ok(text) => {
                        // The first words of a page say which page it is.
                        let text = without_tags(text);
                        let words: Vec<&str> = text.split_whitespace().take(4).collect();
                        format!("\"{}\"", without_ids(&words.join(" ")))
                    }
                    Err(_) => format!("{} bytes", self.body.len()),
                };
                format!("{}: {} {words}", self.status, self.media_type)
            }
        }
    }
}

/// An LLSD value as a metric writes it: a map key by key in order, an id as
/// `<id>`, an address as `<url>`, a number as it stands.
fn rendered(value: &Llsd, key: Option<&str>) -> String {
    match value {
        Llsd::Map(map) => {
            let mut entries: Vec<(&String, &Llsd)> = map.iter().collect();
            entries.sort_unstable_by_key(|(key, _value)| *key);
            let entries: Vec<String> = entries
                .into_iter()
                .map(|(key, value)| format!("{key}={}", rendered(value, Some(key))))
                .collect();
            format!("{{{}}}", entries.join(","))
        }
        Llsd::Array(items) if items.len() > 8 => format!("[{} entries]", items.len()),
        Llsd::Array(items) => {
            let items: Vec<String> = items.iter().map(|item| rendered(item, None)).collect();
            format!("[{}]", items.join(","))
        }
        Llsd::Undef => "undef".to_owned(),
        Llsd::Boolean(flag) => flag.to_string(),
        // A balance is the avatar's and differs from run to run.
        Llsd::Integer(_) if key == Some("balance") => "an amount".to_owned(),
        Llsd::Integer(number) => number.to_string(),
        Llsd::Real(number) => format!("{number:.3}"),
        Llsd::String(text) | Llsd::Uri(text) => {
            if text.is_empty() {
                "\"\"".to_owned()
            } else if key == Some("uploader") || text.starts_with("http") {
                "<url>".to_owned()
            } else if Uuid::parse_str(text).is_ok_and(|id| id.is_nil()) {
                "nil".to_owned()
            } else {
                let text: String = without_ids(&without_files(text))
                    .chars()
                    .take(160)
                    .collect();
                format!("\"{text}\"")
            }
        }
        Llsd::Uuid(id) if id.is_nil() => "nil".to_owned(),
        Llsd::Uuid(_) => "<id>".to_owned(),
        Llsd::Binary(_) => "binary".to_owned(),
        Llsd::Date(_) => "date".to_owned(),
    }
}

/// `text` with every word that names a file under `/tmp` replaced by
/// `<file>`: a grid's verifier names the scratch file it read.
fn without_files(text: &str) -> String {
    text.split(' ')
        .map(|word| {
            if word.starts_with("/tmp/") {
                if word.ends_with(':') {
                    "<file>:"
                } else {
                    "<file>"
                }
            } else {
                word
            }
        })
        .collect::<Vec<&str>>()
        .join(" ")
}

/// An LLSD map of `entries`.
fn map<const N: usize>(entries: [(&str, Llsd); N]) -> Llsd {
    Llsd::Map(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
    )
}

/// An LLSD text.
fn text(value: &str) -> Llsd {
    Llsd::String(value.to_owned())
}

/// An LLSD array of reals.
fn reals<const N: usize>(values: [f64; N]) -> Llsd {
    Llsd::Array(values.into_iter().map(Llsd::Real).collect())
}

/// The first request of a `NewFileAgentInventory` upload, as the reference
/// viewer writes it.
fn new_file(
    folder: Uuid,
    asset_type: &str,
    inventory_type: &str,
    name: &str,
    cost: i64,
) -> HashMap<String, Llsd> {
    let Llsd::Map(body) = map([
        ("folder_id", Llsd::Uuid(folder)),
        ("asset_type", text(asset_type)),
        ("inventory_type", text(inventory_type)),
        ("name", text(name)),
        ("description", text("sl-conformance upload-http")),
        ("next_owner_mask", Llsd::Integer(NEXT_OWNER)),
        ("group_mask", Llsd::Integer(0)),
        ("everyone_mask", Llsd::Integer(0)),
        (
            "expected_upload_cost",
            Llsd::Integer(i32::try_from(cost).unwrap_or(i32::MAX)),
        ),
    ]) else {
        return HashMap::new();
    };
    body
}

/// The requests the case sends itself.
#[derive(Debug)]
struct Asker {
    /// An HTTP client that follows no redirect.
    http: reqwest::Client,
}

impl Asker {
    /// A client that follows no redirect, so that one is an answer.
    fn new() -> Result<Self, TestFailure> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| {
                TestFailure::Assertion(format!("building an HTTP client failed: {error}"))
            })?;
        Ok(Self { http })
    }

    /// Sends `request` and reads the answer.
    async fn send(&self, request: reqwest::RequestBuilder) -> Result<Reply, TestFailure> {
        let response = request.send().await.map_err(|error| {
            TestFailure::Assertion(format!("an upload request failed: {error}"))
        })?;
        let status = response.status().as_u16();
        let media_type = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .map_or_else(
                || "no media type".to_owned(),
                |value| {
                    value
                        .split(';')
                        .next()
                        .unwrap_or(value)
                        .trim()
                        .to_ascii_lowercase()
                },
            );
        let body = response
            .bytes()
            .await
            .map_err(|error| {
                TestFailure::Assertion(format!("reading an upload answer failed: {error}"))
            })?
            .to_vec();
        Ok(Reply {
            status,
            media_type,
            body,
        })
    }

    /// Posts `body` as LLSD.
    async fn post_llsd(&self, url: &str, body: &Llsd) -> Result<Reply, TestFailure> {
        self.post(url, "application/llsd+xml", body.to_llsd_xml().into_bytes())
            .await
    }

    /// Posts `body` under `content_type`.
    async fn post(
        &self,
        url: &str,
        content_type: &str,
        body: Vec<u8>,
    ) -> Result<Reply, TestFailure> {
        self.send(
            self.http
                .post(url)
                .header("Content-Type", content_type)
                .header("Accept", "application/llsd+xml")
                .body(body),
        )
        .await
    }

    /// GETs `url`.
    async fn get(&self, url: &str) -> Result<Reply, TestFailure> {
        self.send(self.http.get(url).header("Accept", "application/llsd+xml"))
            .await
    }
}

/// What a session was sent, unasked, during a window: every balance, alert
/// and item announcement, as a metric writes them.
async fn told(session: &mut Session, window: Duration) -> Result<String, TestFailure> {
    let mut lines: Vec<String> = Vec::new();
    let started = std::time::Instant::now();
    loop {
        let remaining = window.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            break;
        }
        let outcome = session
            .wait_for(remaining, |event| match event {
                Event::MoneyBalance(balance) => Some(match &balance.transaction {
                    Some(transaction) => format!(
                        "a balance ({}, type {}, L$ {}, \"{}\")",
                        if balance.success {
                            "success"
                        } else {
                            "failure"
                        },
                        transaction.transaction_type,
                        transaction.amount.0,
                        without_ids(&transaction.item_description),
                    ),
                    None => format!(
                        "a balance ({}, no transaction, \"{}\")",
                        if balance.success {
                            "success"
                        } else {
                            "failure"
                        },
                        without_ids(&balance.description),
                    ),
                }),
                Event::AlertMessage {
                    message,
                    alert_info,
                    ..
                } => Some(format!(
                    "an alert \"{}\"",
                    without_ids(&alert_info.first().map_or_else(
                        || message.clone(),
                        |info| format!("{message} [{}]", info.message),
                    ))
                )),
                Event::AgentAlertMessage { message, .. } => {
                    Some(format!("an agent alert \"{}\"", without_ids(message)))
                }
                Event::InventoryItemCreated { .. } => Some("an item (legacy)".to_owned()),
                Event::InventoryBulkUpdate { items, .. } => {
                    Some(format!("{} item(s) (bulk)", items.len()))
                }
                _ => None,
            })
            .await;
        match outcome {
            Ok(line) => lines.push(line),
            Err(TestFailure::Timeout(_)) => break,
            Err(other) => return Err(other),
        }
    }
    Ok(if lines.is_empty() {
        "nothing".to_owned()
    } else {
        lines.join("; ")
    })
}

/// The agent's balance, where the grid answers a request for one.
async fn balance_of(session: &mut Session, known: bool) -> Result<Option<i64>, TestFailure> {
    if !known {
        return Ok(None);
    }
    current_balance(session).await.map(Some)
}

/// Everything the legs share.
struct Census<'a> {
    /// The client.
    asker: Asker,
    /// How long a leg listens after an upload.
    quiet: Duration,
    /// This run's mark, in every name the case gives.
    tag: String,
    /// The balance of the avatar the legs are run as, where the grid states
    /// one.
    balance: Option<i64>,
    /// What is recorded.
    metrics: &'a mut Metrics,
    /// What is made.
    residue: &'a mut Residue,
}

impl Census<'_> {
    /// Records `value` under `key`.
    fn record(&mut self, key: &str, value: impl Into<crate::record::MetricValue>) {
        self.metrics.set(key, value);
    }

    /// Posts the first request `body` to `cap` and records the answer as
    /// `step1_<key>`.
    async fn step1(&mut self, cap: &str, key: &str, body: &Llsd) -> Result<Reply, TestFailure> {
        let reply = self.asker.post_llsd(cap, body).await?;
        self.record(&format!("step1_{key}"), reply.said());
        Ok(reply)
    }

    /// Posts `bytes` to the uploader `first` named, if it named one, and
    /// records the answer as `upload_<key>`, what the session was then told
    /// as `upload_<key>_told` and what it was charged as
    /// `upload_<key>_charged`.
    async fn complete(
        &mut self,
        session: &mut Session,
        key: &str,
        first: &Reply,
        content_type: &str,
        bytes: Vec<u8>,
    ) -> Result<Option<Reply>, TestFailure> {
        let Some(uploader) = first.uploader() else {
            for part in ["", "_told", "_charged"] {
                self.record(&format!("upload_{key}{part}"), NO_UPLOADER);
            }
            return Ok(None);
        };
        let reply = self.asker.post(&uploader, content_type, bytes).await?;
        if let Some(item) = reply.id("new_inventory_item") {
            self.residue.items.push(InventoryKey::from(item));
        }
        self.record(&format!("upload_{key}"), reply.said());
        let heard = told(session, self.quiet).await?;
        self.record(&format!("upload_{key}_told"), heard);
        self.charge(session, &format!("upload_{key}_charged"))
            .await?;
        Ok(Some(reply))
    }

    /// Records under `key` what the balance fell by since it was last read.
    async fn charge(&mut self, session: &mut Session, key: &str) -> Result<(), TestFailure> {
        match self.balance {
            None => self.record(key, NO_BALANCE),
            Some(before) => {
                let after = current_balance(session).await?;
                self.balance = Some(after);
                self.record(key, before.saturating_sub(after));
            }
        }
        Ok(())
    }

    /// One upload: the first request `body` to `cap`, then `bytes` to the
    /// uploader. Records the first answer too where it brought no uploader.
    async fn upload(
        &mut self,
        session: &mut Session,
        cap: &str,
        key: &str,
        body: &Llsd,
        bytes: Vec<u8>,
    ) -> Result<Option<Reply>, TestFailure> {
        let first = self.asker.post_llsd(cap, body).await?;
        if first.uploader().is_none() {
            self.record(
                &format!("upload_{key}"),
                format!("refused before the bytes: {}", first.said()),
            );
            for part in ["_told", "_charged"] {
                self.record(&format!("upload_{key}{part}"), NO_UPLOADER);
            }
            return Ok(None);
        }
        self.complete(session, key, &first, "application/octet-stream", bytes)
            .await
    }

    /// Records as `upload_<key>_made` the classes of the item the upload
    /// `reply` says it made, as the inventory states them.
    async fn made(
        &mut self,
        session: &Session,
        key: &str,
        reply: Option<&Reply>,
    ) -> Result<(), TestFailure> {
        let Some(item) = reply.and_then(|reply| reply.id("new_inventory_item")) else {
            self.record(&format!("upload_{key}_made"), "no item named");
            return Ok(());
        };
        let (Some(cap), Some(agent)) = (session.cap("FetchInventory2"), session.agent_id()) else {
            self.record(&format!("upload_{key}_made"), NO_CAPABILITY);
            return Ok(());
        };
        let body = map([
            ("agent_id", Llsd::Uuid(agent.uuid())),
            (
                "items",
                Llsd::Array(vec![map([
                    ("owner_id", Llsd::Uuid(agent.uuid())),
                    ("item_id", Llsd::Uuid(item)),
                ])]),
            ),
        ]);
        let answer = self.asker.post_llsd(&cap, &body).await?;
        let stated = answer.llsd();
        let stated =
            stated
                .as_ref()
                .and_then(|body| body.get("items"))
                .and_then(|items| match items {
                    Llsd::Array(items) => items.first(),
                    _other => None,
                });
        let made = match stated {
            None => format!("{}: no such item", answer.status),
            Some(stated) => {
                let field = |name: &str| {
                    stated
                        .get(name)
                        .map_or_else(|| "none".to_owned(), |value| rendered(value, None))
                };
                format!(
                    "asset type {}, inventory type {}, flags {}",
                    field("type"),
                    field("inv_type"),
                    field("flags")
                )
            }
        };
        self.record(&format!("upload_{key}_made"), made);
        Ok(())
    }

    /// The name of the item the leg `key` makes.
    fn name(&self, key: &str) -> String {
        format!("conf-{}-{key}", self.tag)
    }
}

/// The prices a viewer would send on this grid.
#[derive(Debug, Clone, Copy)]
struct Prices {
    /// A texture below the large-texture area.
    texture: i64,
    /// A texture above it.
    large_texture: i64,
    /// A sound.
    sound: i64,
    /// An animation.
    animation: i64,
}

/// Reads the prices a viewer would send: the benefits package's where the
/// login carried one, the `EconomyData` upload price otherwise.
async fn prices(session: &mut Session, metrics: &mut Metrics) -> Result<Prices, TestFailure> {
    session.send(Command::RequestEconomyData).await?;
    let economy = session
        .wait_for(REPLY_TIMEOUT, |event| match event {
            Event::EconomyData(data) => Some((**data).clone()),
            _ => None,
        })
        .await;
    let economy = match economy {
        Ok(data) => {
            metrics.set(
                "economy_price_upload",
                i64::try_from(data.price_upload.0).unwrap_or(-1),
            );
            Some(data)
        }
        Err(TestFailure::Timeout(_)) => {
            metrics.set("economy_price_upload", "no answer");
            None
        }
        Err(other) => return Err(other),
    };
    let benefits = session
        .login_account()
        .and_then(|account| account.benefits.as_ref());
    // What the client's own price list says a viewer states for each class.
    let price = |class: UploadClass| {
        i64::try_from(expected_upload_cost(class, benefits, economy.as_ref()).0).unwrap_or(-1)
    };
    let prices = Prices {
        texture: price(UploadClass::Texture {
            width: 64,
            height: 64,
        }),
        large_texture: price(UploadClass::Texture {
            width: 2048,
            height: 2048,
        }),
        sound: price(UploadClass::Sound),
        animation: price(UploadClass::Animation),
    };
    metrics.set(
        "price_list",
        if benefits.is_some() {
            "the benefits package"
        } else {
            "EconomyData"
        },
    );
    metrics.set(
        "prices",
        format!(
            "texture {}, large texture {}, sound {}, animation {}",
            prices.texture, prices.large_texture, prices.sound, prices.animation
        ),
    );
    Ok(prices)
}

/// A checker texture `width` by `height`, as JPEG-2000.
fn texture(width: u32, height: u32) -> Result<Vec<u8>, TestFailure> {
    let mut pixels = Vec::new();
    for y in 0..height {
        for x in 0..width {
            let odd = (x / 8).wrapping_add(y / 8) % 2 == 1;
            pixels.extend_from_slice(if odd {
                &[0x11, 0x11, 0x11, 0xFF]
            } else {
                &[0xFF, 0x00, 0x99, 0xFF]
            });
        }
    }
    sl_test_assets::RgbaImage {
        width,
        height,
        pixels,
    }
    .j2c()
    .map_err(|error| TestFailure::Assertion(format!("j2c encode failed: {error}")))
}

/// A tone of `seconds` at `rate`, as Ogg Vorbis.
fn tone(seconds: f32, rate: u32) -> Result<Vec<u8>, TestFailure> {
    let rate = NonZeroU32::new(rate)
        .ok_or_else(|| TestFailure::Assertion("a sample rate of zero".to_owned()))?;
    sl_test_assets::sound::tone(sl_test_assets::sound::tones::MID, seconds, rate)
        .map_err(|error| TestFailure::Assertion(format!("vorbis encode failed: {error}")))
}

/// Sixteen bytes of a RIFF header: a sound in a format no grid stores.
fn wav() -> Vec<u8> {
    let mut out = b"RIFF".to_vec();
    out.extend_from_slice(&[0x24, 0x08, 0x00, 0x00]);
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&[
        0x10, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x44, 0xac, 0x00, 0x00, 0x88, 0x58, 0x01,
        0x00, 0x02, 0x00, 0x10, 0x00,
    ]);
    out.extend_from_slice(b"data");
    out.extend_from_slice(&[0x00, 0x08, 0x00, 0x00]);
    out.resize(out.len().saturating_add(2048), 0);
    out
}

/// The eight bytes every PNG begins with, and a little after them.
fn png() -> Vec<u8> {
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    out.extend_from_slice(&[0x00, 0x00, 0x00, 0x0d]);
    out.extend_from_slice(b"IHDR");
    out.resize(out.len().saturating_add(64), 0);
    out
}

/// Bytes that are no asset of any class.
fn noise() -> Vec<u8> {
    (0_u32..512)
        .map(|index| u8::try_from(index.wrapping_mul(37) % 251).unwrap_or(0))
        .collect()
}

/// The fixture animation with its stated length set to `seconds`.
#[expect(
    clippy::little_endian_bytes,
    reason = "every multi-byte field of a keyframe motion is little-endian"
)]
fn animation_lasting(seconds: f32) -> Vec<u8> {
    let mut bytes = sl_test_assets::anim::chest_twist_animation_asset();
    // The header: version and subversion (two bytes each), the priority
    // (four), the duration (four), the emote's name (empty, one byte), the
    // loop's start (four) and its end (four).
    for at in [8_usize, 17] {
        if let Some(field) = bytes.get_mut(at..at.saturating_add(4)) {
            field.copy_from_slice(&seconds.to_bits().to_le_bytes());
        }
    }
    bytes
}

/// The fixture body of `class`, where the fixture table has one.
fn fixture_body(class: AssetType) -> Option<Vec<u8>> {
    sl_test_assets::inventory::seeded_assets()
        .ok()?
        .into_iter()
        .find(|asset| asset.asset_type == class)
        .map(|asset| asset.body)
}

/// A unit cube as a mesh asset, with a physics shape — its own eight corners
/// as the bounding hull — where `physics` asks for one.
fn cube(physics: bool) -> Result<Vec<u8>, TestFailure> {
    let mut model = sl_mesh::MeshModel::default()
        .with_every_lod(vec![sl_test_assets::mesh::unit_cube_submesh()]);
    if physics {
        let mut corners = Vec::new();
        for x in [-0.5_f32, 0.5] {
            for y in [-0.5_f32, 0.5] {
                for z in [-0.5_f32, 0.5] {
                    corners.push([x, y, z]);
                }
            }
        }
        model = model.with_physics_convex(sl_mesh::PhysicsConvex {
            hulls: Vec::new(),
            bounding_verts: corners,
            min: [-0.5, -0.5, -0.5],
            max: [0.5, 0.5, 0.5],
        });
    }
    sl_mesh::encode_mesh(&model)
        .map_err(|error| TestFailure::Assertion(format!("mesh encode failed: {error}")))
}

/// The resources of a model upload, as the reference viewer writes them: the
/// `meshes`, no texture, and one instance of the first mesh where
/// `instances` asks for one.
fn model_resources(meshes: Vec<Vec<u8>>, instances: bool) -> Llsd {
    let instance = map([
        ("position", reals([0.0, 0.0, 0.0])),
        ("rotation", reals([0.0, 0.0, 0.0, 1.0])),
        ("scale", reals([1.0, 1.0, 1.0])),
        ("material", Llsd::Integer(3)),
        ("physics_shape_type", Llsd::Integer(2)),
        ("mesh", Llsd::Integer(0)),
        ("mesh_name", text("cube")),
        (
            "face_list",
            Llsd::Array(vec![map([
                ("diffuse_color", reals([1.0, 1.0, 1.0, 1.0])),
                ("fullbright", Llsd::Boolean(false)),
            ])]),
        ),
    ]);
    map([
        (
            "mesh_list",
            Llsd::Array(meshes.into_iter().map(Llsd::Binary).collect()),
        ),
        ("texture_list", Llsd::Array(Vec::new())),
        (
            "instance_list",
            Llsd::Array(if instances {
                vec![instance]
            } else {
                Vec::new()
            }),
        ),
        ("source_format", map([("high", Llsd::Integer(0))])),
        ("metric", text("MUT_Unspecified")),
    ])
}

/// The first request of a model upload: the fee request.
fn model_request(folder: Uuid, texture_folder: Uuid, name: &str, resources: Option<Llsd>) -> Llsd {
    let mut body = new_file(folder, "mesh", "object", name, 0);
    let _cost = body.remove("expected_upload_cost");
    let _previous = body.insert("texture_folder_id".to_owned(), Llsd::Uuid(texture_folder));
    let _previous = body.insert("description".to_owned(), text("(No Description)"));
    if let Some(resources) = resources {
        let _previous = body.insert("asset_resources".to_owned(), resources);
    }
    Llsd::Map(body)
}

/// The folders the legs upload into.
#[derive(Debug, Clone, Copy)]
struct Destinations {
    /// Textures.
    textures: Uuid,
    /// Sounds.
    sounds: Uuid,
    /// Animations.
    animations: Uuid,
    /// Objects.
    objects: Uuid,
    /// The inventory's root.
    root: Uuid,
}

/// The system folder of `folder_type`, or the root.
async fn folder_of(session: &mut Session, folder_type: FolderType) -> Result<Uuid, TestFailure> {
    destination_folder(session, folder_type)
        .await?
        .map(|folder| folder.uuid())
        .ok_or_else(|| TestFailure::Assertion("no inventory root to upload into".to_owned()))
}

/// The whole census.
async fn census(
    ctx: &mut TestContext,
    metrics: &mut Metrics,
    residue: &mut Residue,
) -> Result<(), TestFailure> {
    let grid = ctx.grid();
    let postcard_to = ctx.postcard_to().map(ToOwned::to_owned);
    let quiet = if grid.is_fake() { FAKE_QUIET } else { QUIET };
    let tag = format!(
        "{:08}",
        Uuid::new_v4()
            .as_u128()
            .checked_rem(100_000_000)
            .unwrap_or(0)
    );

    let session = ctx.primary();
    session.wait_for_region(REGION_TIMEOUT).await?;
    session
        .send(Command::SetThrottle(Throttle::preset_1000()))
        .await?;
    let Some(cap) = session.cap("NewFileAgentInventory") else {
        return Err(TestFailure::Assertion(
            "no NewFileAgentInventory capability offered".to_owned(),
        ));
    };
    let primary_id = session
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("the primary has no agent id".to_owned()))?;
    let prices = prices(session, metrics).await?;
    let destinations = Destinations {
        textures: folder_of(session, FolderType::Texture).await?,
        sounds: folder_of(session, FolderType::Sound).await?,
        animations: folder_of(session, FolderType::Animation).await?,
        objects: folder_of(session, FolderType::Object).await?,
        root: {
            session.send(Command::QueryInventoryRoots).await?;
            session
                .wait_for(LONG_TIMEOUT, |event| match event {
                    Event::InventoryRoots { agent_root, .. } => Some(*agent_root),
                    _other => None,
                })
                .await?
                .map(|root| root.uuid())
                .ok_or_else(|| TestFailure::Assertion("no inventory root".to_owned()))?
        },
    };
    // Whether the grid answers a balance request at all: asked once, with a
    // short wait, so a grid with no money module costs one timeout.
    session.send(Command::RequestMoneyBalance).await?;
    let balance = match session
        .wait_for(Duration::from_secs(10), |event| match event {
            Event::MoneyBalance(balance) => Some(i64::try_from(balance.balance.0).unwrap_or(-1)),
            _other => None,
        })
        .await
    {
        Ok(balance) => Some(balance),
        Err(TestFailure::Timeout(_)) => None,
        Err(other) => return Err(other),
    };
    metrics.set("balance_answered", balance.is_some());
    let spent_from = balance;

    let mut census = Census {
        asker: Asker::new()?,
        quiet,
        tag,
        balance,
        metrics,
        residue,
    };

    let small = texture(64, 64)?;
    price_legs(&mut census, session, &cap, destinations, prices, &small).await?;
    request_legs(&mut census, &cap, destinations, prices).await?;
    class_legs(&mut census, session, &cap, destinations, &small).await?;
    let thumbnail_item =
        texture_legs(&mut census, session, &cap, destinations, prices, &small).await?;
    sound_legs(&mut census, session, &cap, destinations, prices).await?;
    animation_legs(&mut census, session, &cap, destinations, prices).await?;
    mesh_legs(&mut census, session, &cap, destinations).await?;
    flag_legs(&mut census, session).await?;
    thumbnail_legs(&mut census, session, destinations, thumbnail_item).await?;
    postcard_legs(&mut census, session, postcard_to.as_deref()).await?;
    command_legs(&mut census, session, thumbnail_item).await?;
    object_leg(&mut census, session, &cap, destinations, prices).await?;

    let spent = match (spent_from, census.balance) {
        (Some(before), Some(after)) => Some(before.saturating_sub(after)),
        _ => None,
    };
    let quiet = census.quiet;
    let tag = census.tag.clone();
    drop(census);
    if let Some(spent) = spent {
        metrics.set("spent", spent);
    }

    pauper_legs(ctx, metrics, primary_id.uuid(), prices, &small, quiet, &tag).await
}

/// The price legs: one class, the texture's, asked for with every
/// `expected_upload_cost` a viewer might send — and uploaded where the grid
/// hands out an uploader, to see what it is charged.
async fn price_legs(
    census: &mut Census<'_>,
    session: &mut Session,
    cap: &str,
    destinations: Destinations,
    prices: Prices,
    small: &[u8],
) -> Result<(), TestFailure> {
    let right = prices.texture;
    let costs: [(&str, Option<Llsd>); 8] = [
        ("cost_right", Some(Llsd::Integer(narrow(right)))),
        ("cost_none", None),
        ("cost_zero", Some(Llsd::Integer(0))),
        ("cost_one", Some(Llsd::Integer(1))),
        (
            "cost_over",
            Some(Llsd::Integer(narrow(right.saturating_add(1)))),
        ),
        ("cost_huge", Some(Llsd::Integer(1_000_000_000))),
        ("cost_negative", Some(Llsd::Integer(-1))),
        ("cost_text", Some(text("ten"))),
    ];
    for (key, cost) in costs {
        let mut body = new_file(
            destinations.textures,
            "texture",
            "texture",
            &census.name(key),
            0,
        );
        match cost {
            Some(cost) => {
                let _previous = body.insert("expected_upload_cost".to_owned(), cost);
            }
            None => {
                let _previous = body.remove("expected_upload_cost");
            }
        }
        let first = census.step1(cap, key, &Llsd::Map(body)).await?;
        if key == "cost_right" {
            // Where the uploader is, against the capability.
            let same = first.uploader().is_some_and(|uploader| {
                authority_of(&uploader).is_some() && authority_of(&uploader) == authority_of(cap)
            });
            census.record("uploader_on_the_capability_host", same);
        }
        let _reply = census
            .complete(
                session,
                key,
                &first,
                "application/octet-stream",
                small.to_vec(),
            )
            .await?;
    }
    Ok(())
}

/// The scheme, host and port of a URL, as one text.
fn authority_of(url: &str) -> Option<String> {
    let url = url::Url::parse(url).ok()?;
    Some(format!(
        "{}://{}:{}",
        url.scheme(),
        url.host_str()?,
        url.port_or_known_default()?
    ))
}

/// An L$ amount as an LLSD integer.
fn narrow(amount: i64) -> i32 {
    i32::try_from(amount).unwrap_or(i32::MAX)
}

/// The request legs: first requests that are not what the reference viewer
/// writes. None is completed.
async fn request_legs(
    census: &mut Census<'_>,
    cap: &str,
    destinations: Destinations,
    prices: Prices,
) -> Result<(), TestFailure> {
    let good = |census: &Census<'_>, key: &str| {
        new_file(
            destinations.textures,
            "texture",
            "texture",
            &census.name(key),
            prices.texture,
        )
    };
    let mut body = good(census, "no_folder");
    let _previous = body.remove("folder_id");
    let _reply = census.step1(cap, "no_folder", &Llsd::Map(body)).await?;

    let mut body = good(census, "nil_folder");
    let _previous = body.insert("folder_id".to_owned(), Llsd::Uuid(Uuid::nil()));
    let _reply = census.step1(cap, "nil_folder", &Llsd::Map(body)).await?;

    let mut body = good(census, "unknown_folder");
    let _previous = body.insert(
        "folder_id".to_owned(),
        Llsd::Uuid(Uuid::from_u128(0x0bad_f01d_e000_4000_8000_0000_0000_dead)),
    );
    let _reply = census
        .step1(cap, "unknown_folder", &Llsd::Map(body))
        .await?;

    let mut body = good(census, "no_name");
    let _previous = body.remove("name");
    let _reply = census.step1(cap, "no_name", &Llsd::Map(body)).await?;

    let mut body = good(census, "no_asset_type");
    let _previous = body.remove("asset_type");
    let _reply = census.step1(cap, "no_asset_type", &Llsd::Map(body)).await?;

    let mut body = good(census, "no_inventory_type");
    let _previous = body.remove("inventory_type");
    let _reply = census
        .step1(cap, "no_inventory_type", &Llsd::Map(body))
        .await?;

    let mut body = good(census, "classes_disagree");
    let _previous = body.insert("inventory_type".to_owned(), text("sound"));
    let _reply = census
        .step1(cap, "classes_disagree", &Llsd::Map(body))
        .await?;

    let reply = census
        .asker
        .post_llsd(cap, &Llsd::Map(HashMap::new()))
        .await?;
    census.record("step1_empty_map", reply.said());
    let reply = census
        .asker
        .post(cap, "application/llsd+xml", Vec::new())
        .await?;
    census.record("step1_no_body", reply.said());
    let reply = census
        .asker
        .post(cap, "application/llsd+xml", b"not llsd at all".to_vec())
        .await?;
    census.record("step1_not_llsd", reply.said());
    let reply = census.asker.get(cap).await?;
    census.record("step1_get", reply.said());
    Ok(())
}

/// The class legs: every class a viewer holds in inventory but the three
/// with legs of their own, asked for at no cost and uploaded where the grid
/// takes it and the fixture table has a body.
async fn class_legs(
    census: &mut Census<'_>,
    session: &mut Session,
    cap: &str,
    destinations: Destinations,
    small: &[u8],
) -> Result<(), TestFailure> {
    let classes: [(&str, &str, &str, Option<Vec<u8>>); 12] = [
        (
            "notecard",
            "notecard",
            "notecard",
            fixture_body(AssetType::Notecard),
        ),
        (
            "script",
            "lsltext",
            "script",
            fixture_body(AssetType::ScriptText),
        ),
        (
            "bodypart",
            "bodypart",
            "wearable",
            fixture_body(AssetType::Bodypart),
        ),
        (
            "clothing",
            "clothing",
            "wearable",
            fixture_body(AssetType::Clothing),
        ),
        (
            "gesture",
            "gesture",
            "gesture",
            fixture_body(AssetType::Gesture),
        ),
        (
            "landmark",
            "landmark",
            "landmark",
            fixture_body(AssetType::Landmark),
        ),
        (
            "settings",
            "settings",
            "settings",
            fixture_body(AssetType::Settings),
        ),
        (
            "material",
            "material",
            "material",
            fixture_body(AssetType::Material),
        ),
        ("callingcard", "callcard", "callcard", None),
        ("snapshot", "texture", "snapshot", Some(small.to_vec())),
        ("mesh_asset", "mesh", "mesh", None),
        ("unknown", "nonsense", "nonsense", None),
    ];
    for (key, asset_type, inventory_type, body) in classes {
        let key = format!("class_{key}");
        let request = Llsd::Map(new_file(
            destinations.root,
            asset_type,
            inventory_type,
            &census.name(&key),
            0,
        ));
        let first = census.step1(cap, &key, &request).await?;
        match body {
            Some(body) => {
                let reply = census
                    .complete(session, &key, &first, "application/octet-stream", body)
                    .await?;
                census.made(session, &key, reply.as_ref()).await?;
            }
            None => census.record(&format!("upload_{key}"), "not asked"),
        }
    }
    Ok(())
}

/// The object leg, which comes last: an object's class asked for and
/// uploaded, and then a texture asked for again — OpenSim's uploader does not
/// come back from an object that is no model.
async fn object_leg(
    census: &mut Census<'_>,
    session: &mut Session,
    cap: &str,
    destinations: Destinations,
    prices: Prices,
) -> Result<(), TestFailure> {
    let request = Llsd::Map(new_file(
        destinations.objects,
        "object",
        "object",
        &census.name("class_object"),
        0,
    ));
    let first = census.step1(cap, "class_object", &request).await?;
    match fixture_body(AssetType::Object) {
        Some(body) => {
            let reply = census
                .complete(
                    session,
                    "class_object",
                    &first,
                    "application/octet-stream",
                    body,
                )
                .await?;
            census.made(session, "class_object", reply.as_ref()).await?;
        }
        None => census.record("upload_class_object", "not asked"),
    }
    let request = Llsd::Map(new_file(
        destinations.textures,
        "texture",
        "texture",
        &census.name("after_object"),
        prices.texture,
    ));
    let _reply = census.step1(cap, "after_object", &request).await?;
    Ok(())
}

/// The texture legs: bytes that are a texture and bytes that are not. Returns
/// the item the good upload made, which the thumbnail legs use.
async fn texture_legs(
    census: &mut Census<'_>,
    session: &mut Session,
    cap: &str,
    destinations: Destinations,
    prices: Prices,
    small: &[u8],
) -> Result<Option<Uuid>, TestFailure> {
    let request = |census: &Census<'_>, key: &str, cost: i64| {
        Llsd::Map(new_file(
            destinations.textures,
            "texture",
            "texture",
            &census.name(key),
            cost,
        ))
    };
    // The good one, and the mechanics of its uploader.
    let body = request(census, "texture", prices.texture);
    let first = census.asker.post_llsd(cap, &body).await?;
    let mut item = None;
    if let Some(uploader) = first.uploader() {
        let done = census
            .complete(
                session,
                "texture",
                &first,
                "application/octet-stream",
                small.to_vec(),
            )
            .await?;
        census.made(session, "texture", done.as_ref()).await?;
        item = done.and_then(|done| done.id("new_inventory_item"));
        // The same uploader, a second time.
        let again = census
            .asker
            .post(&uploader, "application/octet-stream", small.to_vec())
            .await?;
        if let Some(item) = again.id("new_inventory_item") {
            census.residue.items.push(InventoryKey::from(item));
        }
        census.record("uploader_again", again.said());
        census.charge(session, "uploader_again_charged").await?;
    } else {
        census.record("upload_texture", first.said());
        census.record("uploader_again", NO_UPLOADER);
    }
    // An uploader asked with a `GET`, and then posted to after all.
    let body = request(census, "after_get", prices.texture);
    let first = census.asker.post_llsd(cap, &body).await?;
    match first.uploader() {
        Some(uploader) => {
            let reply = census.asker.get(&uploader).await?;
            census.record("uploader_get", reply.said());
            let _reply = census
                .complete(
                    session,
                    "after_get",
                    &first,
                    "application/octet-stream",
                    small.to_vec(),
                )
                .await?;
        }
        None => {
            census.record("uploader_get", NO_UPLOADER);
            census.record("upload_after_get", NO_UPLOADER);
        }
    }

    let legs: [(&str, Vec<u8>, i64); 6] = [
        ("texture_nothing", Vec::new(), prices.texture),
        ("texture_noise", noise(), prices.texture),
        ("texture_png", png(), prices.texture),
        ("texture_odd_size", texture(100, 60)?, prices.texture),
        (
            "texture_large_flat_cost",
            texture(2048, 2048)?,
            prices.texture,
        ),
        (
            "texture_small_large_cost",
            small.to_vec(),
            prices.large_texture,
        ),
    ];
    for (key, bytes, cost) in legs {
        let body = request(census, key, cost);
        let _reply = census.upload(session, cap, key, &body, bytes).await?;
    }
    // Into the nil folder, and into one nobody has.
    for (key, folder) in [
        ("texture_nil_folder", Uuid::nil()),
        (
            "texture_unknown_folder",
            Uuid::from_u128(0x0bad_f01d_e000_4000_8000_0000_0000_dead),
        ),
    ] {
        let body = Llsd::Map(new_file(
            folder,
            "texture",
            "texture",
            &census.name(key),
            prices.texture,
        ));
        let reply = census
            .upload(session, cap, key, &body, small.to_vec())
            .await?;
        census.made(session, key, reply.as_ref()).await?;
    }
    Ok(item)
}

/// The sound legs.
async fn sound_legs(
    census: &mut Census<'_>,
    session: &mut Session,
    cap: &str,
    destinations: Destinations,
    prices: Prices,
) -> Result<(), TestFailure> {
    let samples = sl_test_assets::sound::tone_samples(
        sl_test_assets::sound::tones::MID,
        0.25,
        sl_sound::SL_SAMPLE_RATE,
    );
    let stereo = sl_sound::encode_vorbis(
        &[samples.as_slice(), samples.as_slice()],
        sl_sound::SL_SAMPLE_RATE,
        sl_sound::SL_QUALITY,
    )
    .map_err(|error| TestFailure::Assertion(format!("vorbis encode failed: {error}")))?;
    let legs: [(&str, Vec<u8>); 7] = [
        ("sound", tone(0.25, 44_100)?),
        ("sound_nothing", Vec::new()),
        ("sound_noise", noise()),
        ("sound_wav", wav()),
        ("sound_stereo", stereo),
        ("sound_22k", tone(0.25, 22_050)?),
        ("sound_long", tone(31.0, 44_100)?),
    ];
    for (key, bytes) in legs {
        let body = Llsd::Map(new_file(
            destinations.sounds,
            "sound",
            "sound",
            &census.name(key),
            prices.sound,
        ));
        if key == "sound" {
            let first = census.step1(cap, key, &body).await?;
            let reply = census
                .complete(session, key, &first, "application/octet-stream", bytes)
                .await?;
            census.made(session, key, reply.as_ref()).await?;
        } else {
            let _reply = census.upload(session, cap, key, &body, bytes).await?;
        }
    }
    Ok(())
}

/// The animation legs.
async fn animation_legs(
    census: &mut Census<'_>,
    session: &mut Session,
    cap: &str,
    destinations: Destinations,
    prices: Prices,
) -> Result<(), TestFailure> {
    let good = sl_test_assets::anim::chest_twist_animation_asset();
    let cut: Vec<u8> = good.iter().copied().take(good.len() / 2).collect();
    let legs: [(&str, Vec<u8>); 5] = [
        ("animation", good),
        ("animation_nothing", Vec::new()),
        ("animation_noise", noise()),
        ("animation_cut_short", cut),
        ("animation_long", animation_lasting(61.0)),
    ];
    for (key, bytes) in legs {
        let body = Llsd::Map(new_file(
            destinations.animations,
            "animatn",
            "animation",
            &census.name(key),
            prices.animation,
        ));
        if key == "animation" {
            let first = census.step1(cap, key, &body).await?;
            let reply = census
                .complete(session, key, &first, "application/octet-stream", bytes)
                .await?;
            census.made(session, key, reply.as_ref()).await?;
        } else {
            let _reply = census.upload(session, cap, key, &body, bytes).await?;
        }
    }
    Ok(())
}

/// The mesh legs: the fee request for a model and for what is not one, and
/// one upload.
async fn mesh_legs(
    census: &mut Census<'_>,
    session: &mut Session,
    cap: &str,
    destinations: Destinations,
) -> Result<(), TestFailure> {
    let request = |census: &Census<'_>, key: &str, resources: Option<Llsd>| {
        model_request(
            destinations.objects,
            destinations.textures,
            &census.name(key),
            resources,
        )
    };
    let body = request(census, "mesh_no_resources", None);
    let _reply = census.step1(cap, "mesh_no_resources", &body).await?;
    let body = request(
        census,
        "mesh_no_meshes",
        Some(model_resources(Vec::new(), false)),
    );
    let _reply = census.step1(cap, "mesh_no_meshes", &body).await?;
    let body = request(
        census,
        "mesh_noise",
        Some(model_resources(vec![noise()], true)),
    );
    let _reply = census.step1(cap, "mesh_noise", &body).await?;
    let body = request(
        census,
        "mesh_no_instances",
        Some(model_resources(vec![cube(true)?], false)),
    );
    let _reply = census.step1(cap, "mesh_no_instances", &body).await?;
    let body = request(
        census,
        "mesh_no_physics",
        Some(model_resources(vec![cube(false)?], true)),
    );
    let _reply = census.step1(cap, "mesh_no_physics", &body).await?;

    // The model, and its upload: the second request carries the resources
    // alone, as LLSD.
    let resources = model_resources(vec![cube(true)?], true);
    let body = request(census, "mesh", Some(resources.clone()));
    let first = census.step1(cap, "mesh", &body).await?;
    let reply = census
        .complete(
            session,
            "mesh",
            &first,
            "application/llsd+xml",
            resources.to_llsd_xml().into_bytes(),
        )
        .await?;
    census.made(session, "mesh", reply.as_ref()).await?;
    Ok(())
}

/// `MeshUploadFlag`.
async fn flag_legs(census: &mut Census<'_>, session: &Session) -> Result<(), TestFailure> {
    let Some(cap) = session.cap("MeshUploadFlag") else {
        census.record("mesh_flag_get", NO_CAPABILITY);
        census.record("mesh_flag_post", NO_CAPABILITY);
        return Ok(());
    };
    let reply = census.asker.get(&cap).await?;
    census.record("mesh_flag_get", flag_said(&reply));
    census.record("mesh_flag_status", flag_status(&reply));
    let reply = census
        .asker
        .post_llsd(&cap, &Llsd::Map(HashMap::new()))
        .await?;
    census.record("mesh_flag_post", flag_said(&reply));
    Ok(())
}

/// A `MeshUploadFlag` answer as a metric records it: the status, whether it
/// states `mesh_upload_status`, and whether it states anything else. Second
/// Life's states the agent's names, which a metric must not.
fn flag_said(reply: &Reply) -> String {
    match reply.llsd() {
        Some(Llsd::Map(body)) => {
            let status = if body.contains_key("mesh_upload_status") {
                "mesh_upload_status"
            } else {
                "no mesh_upload_status"
            };
            let others = body
                .len()
                .saturating_sub(usize::from(body.contains_key("mesh_upload_status")));
            let others = if others == 0 {
                "nothing else"
            } else {
                "the agent's name record"
            };
            format!("{}: {status}, {others}", reply.status)
        }
        _other => reply.said(),
    }
}

/// The status a `MeshUploadFlag` answer states: the account's, and so
/// recorded and not held.
fn flag_status(reply: &Reply) -> String {
    reply
        .llsd()
        .as_ref()
        .and_then(|body| body.get("mesh_upload_status"))
        .map_or_else(|| "none stated".to_owned(), |status| rendered(status, None))
}

/// The client's own commands: the flag asked for, a thumbnail uploaded.
async fn command_legs(
    census: &mut Census<'_>,
    session: &mut Session,
    item: Option<Uuid>,
) -> Result<(), TestFailure> {
    let has_flag = session.cap("MeshUploadFlag").is_some();
    session.send(Command::RequestMeshUploadFlag).await?;
    let answer = session
        .wait_for(Duration::from_secs(15), |event| match event {
            Event::MeshUploadFlag(status) => Some(status.clone()),
            _ => None,
        })
        .await;
    let answer = match answer {
        Ok(status) => {
            census.record(
                "command_mesh_flag_allows",
                match &status {
                    MeshUploadStatus::Valid | MeshUploadStatus::Unstated => "an upload".to_owned(),
                    MeshUploadStatus::Other(other) => format!("no upload ({other})"),
                },
            );
            "answered"
        }
        Err(TestFailure::Timeout(_)) if !has_flag => NO_CAPABILITY,
        Err(TestFailure::Timeout(_)) => "not answered",
        Err(other) => return Err(other),
    };
    census.record("command_mesh_flag", answer);

    let Some(item) = item else {
        census.record("command_thumbnail", "no item to give a thumbnail");
        return Ok(());
    };
    let target = ThumbnailTarget::Item(InventoryKey::from(item));
    session
        .send(Command::UploadInventoryThumbnail {
            target,
            data: texture(64, 64)?,
        })
        .await?;
    let outcome = session
        .wait_for(LONG_TIMEOUT, |event| match event {
            Event::InventoryThumbnailUploaded { target: of, .. } if *of == target => {
                Some("uploaded".to_owned())
            }
            Event::InventoryThumbnailUploadFailed { target: of, reason } if *of == target => {
                Some(format!("failed: {}", without_ids(reason)))
            }
            _ => None,
        })
        .await?;
    census.record("command_thumbnail", outcome);
    Ok(())
}

/// The questions the thumbnail legs record an answer to.
const THUMBNAIL_QUESTIONS: &[&str] = &[
    "thumbnail_step1_item",
    "thumbnail_upload_item",
    "thumbnail_upload_item_charged",
    "thumbnail_upload_item_told",
    "thumbnail_item_read_back",
    "thumbnail_step1_folder",
    "thumbnail_upload_folder",
    "thumbnail_step1_task",
    "thumbnail_step1_unknown_item",
    "thumbnail_step1_nil_item",
    "thumbnail_step1_no_id",
    "thumbnail_step1_both_ids",
    "thumbnail_upload_small",
    "thumbnail_upload_large",
    "thumbnail_upload_oblong",
    "thumbnail_upload_noise",
    "thumbnail_upload_nothing",
    "thumbnail_get",
    "thumbnail_upload_unknown_item",
    "thumbnail_upload_no_id",
    "thumbnail_upload_task",
];

/// `InventoryThumbnailUpload`: an item, a folder, ids nobody has, and images
/// the reference viewer would and would not send.
async fn thumbnail_legs(
    census: &mut Census<'_>,
    session: &mut Session,
    destinations: Destinations,
    item: Option<Uuid>,
) -> Result<(), TestFailure> {
    let Some(cap) = session.cap("InventoryThumbnailUpload") else {
        for question in THUMBNAIL_QUESTIONS {
            census.record(question, NO_CAPABILITY);
        }
        return Ok(());
    };
    let Some(item) = item else {
        for question in THUMBNAIL_QUESTIONS {
            census.record(question, "no item to give a thumbnail");
        }
        return Ok(());
    };
    // A folder of the case's own to give a thumbnail.
    let folder = InventoryFolderKey::from(Uuid::new_v4());
    session
        .send(Command::CreateInventoryFolder {
            folder_id: folder,
            parent_id: InventoryFolderKey::from(destinations.root),
            folder_type: FolderType::None,
            name: census.name("thumbnail-folder"),
        })
        .await?;
    census.residue.folders.push(folder);
    let _heard = told(session, census.quiet).await?;

    let image = texture(256, 256)?;
    let of_item = map([("item_id", Llsd::Uuid(item))]);

    let first = census.asker.post_llsd(&cap, &of_item).await?;
    census.record("thumbnail_step1_item", first.said());
    match first.uploader() {
        Some(uploader) => {
            let reply = census
                .asker
                .post(&uploader, "application/jp2", image.clone())
                .await?;
            census.record("thumbnail_upload_item", reply.said());
            let heard = told(session, census.quiet).await?;
            census.record("thumbnail_upload_item_told", heard);
            census
                .charge(session, "thumbnail_upload_item_charged")
                .await?;
            // The item read back over the inventory API, where there is one.
            let read_back = match session.cap("InventoryAPIv3") {
                None => NO_CAPABILITY.to_owned(),
                Some(api) => {
                    let answer = census.asker.get(&format!("{api}/item/{item}")).await?;
                    let new_asset = reply.id("new_asset");
                    match answer
                        .llsd()
                        .as_ref()
                        .and_then(|body| body.get("thumbnail"))
                    {
                        None => format!("{}: no thumbnail stated", answer.status),
                        Some(thumbnail) => {
                            let stated = match thumbnail.get("asset_id") {
                                Some(Llsd::Uuid(id)) => Some(*id),
                                Some(Llsd::String(id)) => Uuid::parse_str(id).ok(),
                                _other => None,
                            };
                            if stated.is_some() && stated == new_asset {
                                format!("{}: the uploaded asset", answer.status)
                            } else {
                                format!("{}: {}", answer.status, rendered(thumbnail, None))
                            }
                        }
                    }
                }
            };
            census.record("thumbnail_item_read_back", read_back);
        }
        None => {
            census.record("thumbnail_upload_item", NO_UPLOADER);
            census.record("thumbnail_upload_item_told", NO_UPLOADER);
            census.record("thumbnail_upload_item_charged", NO_UPLOADER);
            census.record("thumbnail_item_read_back", NO_UPLOADER);
        }
    }

    let first = census
        .asker
        .post_llsd(&cap, &map([("category_id", Llsd::Uuid(folder.uuid()))]))
        .await?;
    census.record("thumbnail_step1_folder", first.said());
    let answer = match first.uploader() {
        Some(uploader) => census
            .asker
            .post(&uploader, "application/jp2", image.clone())
            .await?
            .said(),
        None => NO_UPLOADER.to_owned(),
    };
    census.record("thumbnail_upload_folder", answer);

    let nobody = Uuid::from_u128(0x0bad_1d00_0000_4000_8000_0000_0000_dead);
    let asked: [(&str, Llsd); 5] = [
        (
            "thumbnail_step1_task",
            map([
                ("item_id", Llsd::Uuid(item)),
                ("task_id", Llsd::Uuid(nobody)),
            ]),
        ),
        (
            "thumbnail_step1_unknown_item",
            map([("item_id", Llsd::Uuid(nobody))]),
        ),
        (
            "thumbnail_step1_nil_item",
            map([("item_id", Llsd::Uuid(Uuid::nil()))]),
        ),
        ("thumbnail_step1_no_id", Llsd::Map(HashMap::new())),
        (
            "thumbnail_step1_both_ids",
            map([
                ("item_id", Llsd::Uuid(item)),
                ("category_id", Llsd::Uuid(folder.uuid())),
            ]),
        ),
    ];
    for (key, body) in asked {
        let reply = census.asker.post_llsd(&cap, &body).await?;
        census.record(key, reply.said());
    }
    let reply = census.asker.get(&cap).await?;
    census.record("thumbnail_get", reply.said());
    // An image for what is not there.
    let targets: [(&str, Llsd); 3] = [
        (
            "thumbnail_upload_unknown_item",
            map([("item_id", Llsd::Uuid(nobody))]),
        ),
        ("thumbnail_upload_no_id", Llsd::Map(HashMap::new())),
        (
            "thumbnail_upload_task",
            map([
                ("item_id", Llsd::Uuid(item)),
                ("task_id", Llsd::Uuid(nobody)),
            ]),
        ),
    ];
    for (key, body) in targets {
        let first = census.asker.post_llsd(&cap, &body).await?;
        let answer = match first.uploader() {
            Some(uploader) => census
                .asker
                .post(&uploader, "application/jp2", image.clone())
                .await?
                .said(),
            None => format!("refused before the bytes: {}", first.said()),
        };
        census.record(key, answer);
    }

    let images: [(&str, Vec<u8>); 5] = [
        ("thumbnail_upload_small", texture(32, 32)?),
        ("thumbnail_upload_large", texture(512, 512)?),
        ("thumbnail_upload_oblong", texture(256, 128)?),
        ("thumbnail_upload_noise", noise()),
        ("thumbnail_upload_nothing", Vec::new()),
    ];
    for (key, bytes) in images {
        let first = census.asker.post_llsd(&cap, &of_item).await?;
        let answer = match first.uploader() {
            Some(uploader) => census
                .asker
                .post(&uploader, "application/jp2", bytes)
                .await?
                .said(),
            None => format!("refused before the bytes: {}", first.said()),
        };
        census.record(key, answer);
    }
    Ok(())
}

/// The questions the postcard legs record an answer to.
const POSTCARD_QUESTIONS: &[&str] = &[
    "postcard_step1",
    "postcard_step1_no_address",
    "postcard_step1_not_an_address",
    "postcard_step1_empty",
    "postcard_get",
    "postcard_upload",
];

/// `SendPostcard`: the first request, and — only where the fixtures name an
/// address to send to — one postcard.
async fn postcard_legs(
    census: &mut Census<'_>,
    session: &mut Session,
    to: Option<&str>,
) -> Result<(), TestFailure> {
    let Some(cap) = session.cap("SendPostcard") else {
        for question in POSTCARD_QUESTIONS {
            census.record(question, NO_CAPABILITY);
        }
        return Ok(());
    };
    let card = |to: Option<&str>| {
        let mut body = HashMap::from([
            ("pos-global".to_owned(), reals([256_000.0, 256_000.0, 25.0])),
            ("name".to_owned(), text("sl-conformance")),
            ("subject".to_owned(), text("sl-conformance upload-http")),
            (
                "msg".to_owned(),
                text("A postcard the conformance suite sent."),
            ),
        ]);
        if let Some(to) = to {
            let _previous = body.insert("to".to_owned(), text(to));
        }
        Llsd::Map(body)
    };
    // An address in a domain reserved for never existing: the first request
    // sends nothing, and nothing is posted to its uploader.
    let first = census
        .asker
        .post_llsd(&cap, &card(Some("nobody@example.invalid")))
        .await?;
    census.record("postcard_step1", first.said());
    let reply = census.asker.post_llsd(&cap, &card(None)).await?;
    census.record("postcard_step1_no_address", reply.said());
    let reply = census
        .asker
        .post_llsd(&cap, &card(Some("not an address")))
        .await?;
    census.record("postcard_step1_not_an_address", reply.said());
    let reply = census
        .asker
        .post_llsd(&cap, &Llsd::Map(HashMap::new()))
        .await?;
    census.record("postcard_step1_empty", reply.said());
    let reply = census.asker.get(&cap).await?;
    census.record("postcard_get", reply.said());

    let answer = match to {
        None => "not sent: the fixtures name no address".to_owned(),
        Some(to) => {
            let first = census.asker.post_llsd(&cap, &card(Some(to))).await?;
            match first.uploader() {
                Some(uploader) => {
                    let reply = census
                        .asker
                        .post(
                            &uploader,
                            "application/octet-stream",
                            POSTCARD_JPEG.to_vec(),
                        )
                        .await?;
                    let heard = told(session, census.quiet).await?;
                    format!("{}; told {heard}", reply.said())
                }
                None => format!("refused before the bytes: {}", first.said()),
            }
        }
    };
    census.record("postcard_upload", answer);
    Ok(())
}

/// The legs of somebody who cannot pay: the second avatar gives the first all
/// but [`PAUPER_KEEPS`] of its L$, asks for uploads, and is given them back.
async fn pauper_legs(
    ctx: &mut TestContext,
    metrics: &mut Metrics,
    primary_id: Uuid,
    prices: Prices,
    small: &[u8],
    quiet: Duration,
    tag: &str,
) -> Result<(), TestFailure> {
    let no_secondary =
        || TestFailure::Assertion("two-account test ran without a secondary".to_owned());
    let secondary = ctx.secondary().ok_or_else(no_secondary)?;
    secondary.wait_for_region(REGION_TIMEOUT).await?;
    let secondary_id = secondary
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("the secondary has no agent id".to_owned()))?
        .uuid();
    let cap = secondary.cap("NewFileAgentInventory");
    let folder = folder_of(secondary, FolderType::Texture).await?;
    let objects = folder_of(secondary, FolderType::Object).await?;

    secondary.send(Command::RequestMoneyBalance).await?;
    let before = match secondary
        .wait_for(Duration::from_secs(10), |event| match event {
            Event::MoneyBalance(balance) => Some(i64::try_from(balance.balance.0).unwrap_or(-1)),
            _other => None,
        })
        .await
    {
        Ok(balance) => Some(balance),
        Err(TestFailure::Timeout(_)) => None,
        Err(other) => return Err(other),
    };
    // Give away all but a little.
    let moved = before
        .map(|before| before.saturating_sub(PAUPER_KEEPS))
        .filter(|moved| *moved > 0);
    let lent = match moved {
        Some(moved) => transfer(secondary, primary_id, moved, "upload-http (lent)").await,
        None => Ok(()),
    };
    // From here to the return of the L$ nothing leaves early: a failure is
    // kept and reported once the second avatar has its money again.
    let outcome = async {
        lent?;
        let Some(cap) = cap else {
            return Err(TestFailure::Assertion(
                "the secondary was offered no NewFileAgentInventory".to_owned(),
            ));
        };
        let poor = balance_of(secondary, before.is_some()).await?;
        metrics.set(
            "pauper_can_pay",
            match poor {
                Some(poor) => {
                    if poor >= prices.texture {
                        "yes"
                    } else {
                        "no"
                    }
                }
                None => NO_BALANCE,
            },
        );
        let mut items = Residue::default();
        let mut census = Census {
            asker: Asker::new()?,
            quiet,
            tag: tag.to_owned(),
            balance: poor,
            metrics,
            residue: &mut items,
        };
        let body = Llsd::Map(new_file(
            folder,
            "texture",
            "texture",
            &census.name("pauper"),
            prices.texture,
        ));
        let first = census.step1(&cap, "pauper", &body).await?;
        let _reply = census
            .complete(
                secondary,
                "pauper",
                &first,
                "application/octet-stream",
                small.to_vec(),
            )
            .await?;
        let body = model_request(
            objects,
            folder,
            &census.name("pauper_mesh"),
            Some(model_resources(vec![cube(true)?], true)),
        );
        let _reply = census.step1(&cap, "pauper_mesh", &body).await?;
        // Whether this avatar may upload a model: the flag is the account's.
        let flag = match secondary.cap("MeshUploadFlag") {
            Some(flag) => flag_status(&census.asker.get(&flag).await?),
            None => NO_CAPABILITY.to_owned(),
        };
        census.record("second_avatar_mesh_flag_status", flag);
        drop(census);
        // What the second avatar made is its own to take away.
        if !items.items.is_empty() {
            secondary
                .send(Command::RemoveInventoryItems(items.items.clone()))
                .await
                .ok();
        }
        Ok(())
    }
    .await;

    // Give it back, whatever became of the legs.
    if let Some(moved) = moved {
        transfer(ctx.primary(), secondary_id, moved, "upload-http (returned)").await?;
    }
    outcome
}

/// Pays `amount` L$ from `session`'s avatar to `to` and waits for the payer
/// to be told of it. A grid that tells a payer nothing is not an error.
async fn transfer(
    session: &mut Session,
    to: Uuid,
    amount: i64,
    description: &str,
) -> Result<(), TestFailure> {
    let amount = LindenAmount(
        u64::try_from(amount)
            .map_err(|_negative| TestFailure::Assertion("a negative transfer".to_owned()))?,
    );
    session
        .send(Command::SendMoneyTransfer {
            dest: to,
            amount,
            kind: MoneyTransactionType::Gift,
            description: format!("sl-conformance {description}"),
        })
        .await?;
    match session
        .wait_for(Duration::from_secs(15), |event| match event {
            Event::MoneyBalance(balance) if balance.transaction.is_some() => Some(()),
            _ => None,
        })
        .await
    {
        Ok(()) | Err(TestFailure::Timeout(_)) => Ok(()),
        Err(other) => Err(other),
    }
}

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;

    use super::*;

    /// An answer is written without anything that differs from run to run.
    #[test]
    fn an_answer_is_rendered_without_ids_or_addresses() {
        let reply = Reply {
            status: 200,
            media_type: "application/llsd+xml".to_owned(),
            body: b"<llsd><map><key>state</key><string>upload</string>\
                    <key>uploader</key><string>https://sim.example/cap/1234</string>\
                    <key>upload_price</key><integer>10</integer>\
                    <key>new_asset</key><uuid>3c115e51-04f4-523c-9fa6-98aff1034730</uuid>\
                    </map></llsd>"
                .to_vec(),
        };
        assert_eq!(
            reply.said(),
            "200: {new_asset=<id>,state=\"upload\",upload_price=10,uploader=<url>}"
        );
        assert_eq!(
            reply.uploader().as_deref(),
            Some("https://sim.example/cap/1234")
        );
    }

    /// The length a fixture animation states is where the legs write it: a
    /// viewer's decoder refuses the lengthened one for its length and nothing
    /// else, and reads one lengthened less.
    #[test]
    fn a_lengthened_animation_states_its_length() -> Result<(), Box<dyn core::error::Error>> {
        assert!(matches!(
            sl_anim::Motion::from_bytes(&animation_lasting(61.0)),
            Err(sl_anim::AnimDecodeError::InvalidDuration { .. })
        ));
        let motion = sl_anim::Motion::from_bytes(&animation_lasting(30.0))?;
        assert!((motion.duration - 30.0).abs() < f32::EPSILON);
        assert!((motion.loop_out_point - 30.0).abs() < f32::EPSILON);
        Ok(())
    }

    /// The model the mesh legs send decodes as a mesh with a physics shape.
    #[test]
    fn the_model_has_a_physics_shape() -> Result<(), Box<dyn core::error::Error>> {
        let with = cube(true)?;
        let without = cube(false)?;
        assert!(with.len() > without.len());
        Ok(())
    }
}
