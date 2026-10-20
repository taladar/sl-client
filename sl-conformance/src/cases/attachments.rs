//! A census of attachments: what each grid answers every way of wearing and
//! taking off an object, what the agent and a second viewer beside it are
//! sent, and what becomes of the inventory.
//!
//! Avatar **A** wears; **B** stands beside it and only listens. A cube is
//! rezzed and copied into A's inventory as many times as the legs need, each
//! copy known by a role (`cube1`, `cube2`, `hud`, …), and every attachment
//! that arrives is told apart by the `AttachItemID` it carries (or, where it
//! carries none, by the object it was). The legs, in order:
//!
//! 1. **The arrival**: how many attachments A already wears (the aditi
//!    avatars wear an animation overrider), the limit the login's benefits
//!    and the region's `SimulatorFeatures` state.
//! 2. **Wearing** (`RezSingleAttachmentFromInv`): one item on a free point,
//!    a second added to it, a third replacing them, the third again on
//!    another point, and an item nobody has. The first arrival is recorded
//!    field by field, as A and as B see it.
//! 3. **Taking off**: `ObjectDetach` by the object, `DetachAttachmentIntoInv`
//!    by the item, and both of two worn at once.
//! 4. **Several at once** (`RezMultipleAttachmentsFromInv`).
//! 5. **A HUD**, which B must not be shown.
//! 6. **A drop** (`ObjectDrop`): what the object becomes and what the item.
//! 7. **An object attached from the world** (`ObjectAttach`) and taken off
//!    again: whether it keeps its id and which item it is filed as.
//! 8. **A script**: a cube's probe asks for `PERMISSION_ATTACH` and attaches
//!    it with `llAttachToAvatarTemp`, then detaches; a second cube's with
//!    `llAttachToAvatar`.
//! 9. **The limit**: as many items as fill the limit and one more, worn in
//!    one compound message.
//! 10. **A teleport** to a neighbouring region and back with an item worn:
//!     whether the attachment comes along, under which ids, and what each
//!     region tells B.
//! 11. **A new login** with an item worn: whether the grid puts it back on.
//!
//! What each live grid answered is stated as a [`Measured`] and written up in
//! `book/src/gridspec/appearance.md` § Attachments; every run is held to it.
//!
//! `2av`, live grids only: the fake grid wears nothing yet
//! (`fake-grid-own-attachments-and-region-moves`). Everything rezzed is
//! derezzed to the Trash and every item made is removed, also when a leg
//! fails; every item still worn is taken off first.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::{Duration, Instant};

use sl_client_tokio::{
    AgentKey, AssetType, AttachmentMode, AttachmentPoint, AvatarAppearance, ChatChannel, ChatType,
    Command, DeRezDestination, DetachOrder, Event, GridCoordinates, InventoryItem, InventoryKey,
    InventoryType, Object, ObjectKey, RegionHandle, RegionLocalObjectId, RestoreItem,
    RezAttachment, RezScriptParams, Rotation, ScopedObjectId, ScriptPermissionRequest,
    ScriptPermissions, ScriptTarget, ScriptUploadLocation, TransactionId, Uuid, Vector,
};

use crate::cases::asset_http::without_ids;
use crate::cases::groups::names_of;
use crate::cases::object_rez_derez::{
    Answer, Folders, Pace, REZ_LIFT_M, Residue, Run, SETTLE_WINDOW, clean_up, rez_cube,
};
use crate::cases::task_inventory::{hold_to_measured, listing, login_folders};
use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::metrics::Metrics;
use crate::registry::{GridTest, TestFuture};
use crate::support::{ANNOUNCED_BULK, ANNOUNCED_LEGACY, REGION_TIMEOUT, is_opensim};
use crate::teleport_trace::{neighbouring_region, region_named, request_teleport};

/// Where the measurements below are written down.
const SOURCE: &str = "book/src/gridspec/appearance.md § Attachments (attachments, 2026-10-20)";

/// Where both avatars log in on OpenSim: the level square in the block's
/// north-eastern region, where a cube may be rezzed.
const OPENSIM_START: &str = "uri:Northeast Region&70&180&26";

/// How long a wear, a detach or the probe's answer may take.
const STEP: Duration = Duration::from_secs(30);

/// How long a leg that may be answered with nothing listens.
const MAYBE: Duration = Duration::from_secs(10);

/// How long a teleport may take.
const TELEPORT: Duration = Duration::from_secs(60);

/// How long the limit leg's arrivals may take.
const LIMIT_WINDOW: Duration = Duration::from_secs(60);

/// How far beside A the first cube stands, in metres.
const CUBE_AWAY_M: f32 = 2.0;

/// How long B's queue is read after A's window.
const DRAIN: Duration = Duration::from_secs(1);

/// What the cube and every copy of it is called.
const CUBE_NAME: &str = "SLClientAttachment";

/// What the scripted cubes are called.
const SCRIPTED_NAME: &str = "SLClientScriptedAttachment";

/// An item id nobody's inventory holds.
const MADE_UP_ITEM: Uuid = Uuid::from_u128(0x3a77_ac4e_0d51_4b0e_9c2f_61d8_e4a0_7b19);

/// The channel the probe takes its orders on.
const PROBE_CHANNEL: i32 = 7_734_302;

/// What every line the probe says begins with.
const PROBE_TAG: &str = "SLCAT";

/// The name of the probe script.
const PROBE_NAME: &str = "SLClientAttachmentProbe";

/// Body points the census wears on, in the order it prefers them: points
/// little used, so that an avatar's own outfit is rarely on them. The first
/// two A wears nothing on are taken.
const BODY_POINTS: [AttachmentPoint; 8] = [
    AttachmentPoint::RightPec,
    AttachmentPoint::LeftPec,
    AttachmentPoint::LeftRingFinger,
    AttachmentPoint::RightRingFinger,
    AttachmentPoint::Tongue,
    AttachmentPoint::Groin,
    AttachmentPoint::AltLeftEar,
    AttachmentPoint::AltRightEar,
];

/// HUD points, likewise.
const HUD_POINTS: [AttachmentPoint; 4] = [
    AttachmentPoint::HudBottomRight,
    AttachmentPoint::HudBottomLeft,
    AttachmentPoint::HudTopRight,
    AttachmentPoint::HudCenter2,
];

/// The first HUD point's id.
const HUD_FIRST: u8 = 31;

/// The last HUD point's id.
const HUD_LAST: u8 = 38;

/// The name-value key a grid names the worn item under.
const ATTACH_ITEM_ID: &str = "AttachItemID";

/// The inventory type of a link (`LLAssetType::AT_LINK`).
const LINK_TYPE: i8 = 24;

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

/// What each grid was measured to answer, by the metric the census records
/// it under. How many attachments A arrives with, and so how many a new login
/// restores, is the aditi avatars' own outfit.
const MEASURED: &[(&str, Measured<Answer>)] = &[
    ("arrival_b_shown_all_but_huds", both(Answer::Flag(true))),
    ("arrival_worn", each(Answer::Any, Answer::Count(0))),
    ("attach_world_alerts", both(Answer::Text("none"))),
    ("attach_world_announced", both(Answer::Text("world (udp)"))),
    ("attach_world_appearance", both(Answer::Text("none"))),
    ("attach_world_arrived", both(Answer::Text("world"))),
    ("attach_world_attach_item", both(Answer::Text("the item"))),
    ("attach_world_b_appearance", both(Answer::Text("none"))),
    ("attach_world_b_arrived", both(Answer::Text("world"))),
    ("attach_world_b_attach_item", both(Answer::Text("the item"))),
    ("attach_world_b_killed", both(Answer::Text("none"))),
    ("attach_world_b_left", both(Answer::Text("none"))),
    (
        "attach_world_b_name_values",
        both(Answer::Text("AttachItemID")),
    ),
    ("attach_world_b_owner", both(Answer::Text("nil"))),
    ("attach_world_b_parent", both(Answer::Text("the avatar"))),
    (
        "attach_world_b_point",
        both(Answer::Text("the second point")),
    ),
    ("attach_world_b_shown", both(Answer::Flag(true))),
    (
        "attach_world_b_update_flags",
        each(Answer::Text("0x10020810"), Answer::Text("0x00020810")),
    ),
    ("attach_world_b_worn", both(Answer::Text("world"))),
    ("attach_world_detach_alerts", both(Answer::Text("none"))),
    (
        "attach_world_detach_announced",
        each(Answer::Text("world (bulk)"), Answer::Text("world (udp)")),
    ),
    ("attach_world_detach_appearance", both(Answer::Text("none"))),
    ("attach_world_detach_arrived", both(Answer::Text("none"))),
    (
        "attach_world_detach_b_appearance",
        both(Answer::Text("none")),
    ),
    ("attach_world_detach_b_arrived", both(Answer::Text("none"))),
    ("attach_world_detach_b_killed", both(Answer::Text("world"))),
    ("attach_world_detach_b_left", both(Answer::Text("none"))),
    ("attach_world_detach_b_worn", both(Answer::Text("none"))),
    ("attach_world_detach_killed", both(Answer::Text("world"))),
    ("attach_world_detach_left", both(Answer::Text("none"))),
    ("attach_world_detach_removed", both(Answer::Text("none"))),
    ("attach_world_detach_worn", both(Answer::Text("none"))),
    (
        "attach_world_form",
        both(Answer::Text("sent again attached, same ids")),
    ),
    ("attach_world_killed", both(Answer::Text("none"))),
    ("attach_world_left", both(Answer::Text("none"))),
    (
        "attach_world_name_values",
        both(Answer::Text("AttachItemID")),
    ),
    ("attach_world_owner", both(Answer::Text("nil"))),
    ("attach_world_parent", both(Answer::Text("the avatar"))),
    ("attach_world_point", both(Answer::Text("the second point"))),
    ("attach_world_removed", both(Answer::Text("none"))),
    ("attach_world_same_local_id", both(Answer::Flag(true))),
    ("attach_world_shown", both(Answer::Flag(true))),
    (
        "attach_world_update_flags",
        both(Answer::Text("0x1002093c")),
    ),
    ("attach_world_worn", both(Answer::Text("world"))),
    ("detach_alerts", both(Answer::Text("none"))),
    (
        "detach_announced",
        each(Answer::Text("cube3 (bulk)"), Answer::Text("cube3 (udp)")),
    ),
    ("detach_appearance", both(Answer::Text("none"))),
    ("detach_arrived", both(Answer::Text("none"))),
    ("detach_b_appearance", both(Answer::Text("none"))),
    ("detach_b_arrived", both(Answer::Text("none"))),
    ("detach_b_killed", both(Answer::Text("cube3"))),
    ("detach_b_left", both(Answer::Text("none"))),
    ("detach_b_worn", both(Answer::Text("none"))),
    ("detach_item_alerts", both(Answer::Text("none"))),
    ("detach_item_announced", both(Answer::Text("none"))),
    ("detach_item_appearance", both(Answer::Text("none"))),
    ("detach_item_arrived", both(Answer::Text("none"))),
    ("detach_item_b_appearance", both(Answer::Text("none"))),
    ("detach_item_b_arrived", both(Answer::Text("none"))),
    ("detach_item_b_killed", both(Answer::Text("cube1"))),
    ("detach_item_b_left", both(Answer::Text("none"))),
    ("detach_item_b_worn", both(Answer::Text("none"))),
    ("detach_item_killed", both(Answer::Text("cube1"))),
    ("detach_item_left", both(Answer::Text("none"))),
    ("detach_item_removed", both(Answer::Text("none"))),
    ("detach_item_worn", both(Answer::Text("none"))),
    ("detach_killed", both(Answer::Text("cube3"))),
    ("detach_left", both(Answer::Text("none"))),
    ("detach_multi_alerts", both(Answer::Text("none"))),
    (
        "detach_multi_announced",
        each(Answer::Text("cube2 (bulk)"), Answer::Text("cube2 (udp)")),
    ),
    ("detach_multi_appearance", both(Answer::Text("none"))),
    ("detach_multi_arrived", both(Answer::Text("none"))),
    ("detach_multi_b_appearance", both(Answer::Text("none"))),
    ("detach_multi_b_arrived", both(Answer::Text("none"))),
    ("detach_multi_b_killed", both(Answer::Text("cube1+cube2"))),
    ("detach_multi_b_left", both(Answer::Text("none"))),
    ("detach_multi_b_worn", both(Answer::Text("none"))),
    ("detach_multi_killed", both(Answer::Text("cube1+cube2"))),
    ("detach_multi_left", both(Answer::Text("none"))),
    ("detach_multi_removed", both(Answer::Text("none"))),
    ("detach_multi_worn", both(Answer::Text("none"))),
    ("detach_removed", both(Answer::Text("none"))),
    ("detach_worn", both(Answer::Text("none"))),
    ("drop_alerts", both(Answer::Text("none"))),
    ("drop_announced", both(Answer::Text("none"))),
    ("drop_appearance", both(Answer::Text("none"))),
    ("drop_arrived", both(Answer::Text("none"))),
    ("drop_b_appearance", both(Answer::Text("none"))),
    ("drop_b_arrived", both(Answer::Text("none"))),
    ("drop_b_killed", both(Answer::Text("none"))),
    ("drop_b_left", both(Answer::Text("drop"))),
    ("drop_b_worn", both(Answer::Text("none"))),
    (
        "drop_form",
        both(Answer::Text("sent again unattached, same ids")),
    ),
    ("drop_killed", both(Answer::Text("none"))),
    ("drop_left", both(Answer::Text("drop"))),
    ("drop_local_id_kept", both(Answer::Flag(true))),
    ("drop_name_values", both(Answer::Text("none"))),
    ("drop_owner", both(Answer::Text("nil"))),
    ("drop_removed", both(Answer::Text("drop"))),
    ("drop_worn", both(Answer::Text("none"))),
    ("hud_alerts", both(Answer::Text("none"))),
    ("hud_announced", both(Answer::Text("none"))),
    ("hud_appearance", both(Answer::Text("none"))),
    ("hud_arrived", both(Answer::Text("hud"))),
    ("hud_attach_item", both(Answer::Text("the item"))),
    ("hud_b_appearance", both(Answer::Text("none"))),
    ("hud_b_arrived", both(Answer::Text("none"))),
    ("hud_b_killed", both(Answer::Text("none"))),
    ("hud_b_left", both(Answer::Text("none"))),
    ("hud_b_sent_the_object", both(Answer::Flag(false))),
    ("hud_b_shown", both(Answer::Flag(false))),
    ("hud_b_worn", both(Answer::Text("none"))),
    ("hud_detach_alerts", both(Answer::Text("none"))),
    (
        "hud_detach_announced",
        each(Answer::Text("hud (bulk)"), Answer::Text("hud (udp)")),
    ),
    ("hud_detach_appearance", both(Answer::Text("none"))),
    ("hud_detach_arrived", both(Answer::Text("none"))),
    ("hud_detach_b_appearance", both(Answer::Text("none"))),
    ("hud_detach_b_arrived", both(Answer::Text("none"))),
    ("hud_detach_b_killed", both(Answer::Text("none"))),
    ("hud_detach_b_left", both(Answer::Text("none"))),
    (
        "hud_detach_b_sent_a_kill",
        each(Answer::Flag(true), Answer::Flag(false)),
    ),
    ("hud_detach_b_worn", both(Answer::Text("none"))),
    ("hud_detach_killed", both(Answer::Text("hud"))),
    ("hud_detach_left", both(Answer::Text("none"))),
    ("hud_detach_removed", both(Answer::Text("none"))),
    ("hud_detach_worn", both(Answer::Text("none"))),
    ("hud_killed", both(Answer::Text("none"))),
    ("hud_left", both(Answer::Text("none"))),
    ("hud_name_values", both(Answer::Text("AttachItemID"))),
    ("hud_owner", both(Answer::Text("nil"))),
    ("hud_parent", both(Answer::Text("the avatar"))),
    ("hud_point", both(Answer::Text("the HUD point"))),
    ("hud_removed", both(Answer::Text("none"))),
    ("hud_shown", both(Answer::Flag(true))),
    ("hud_update_flags", both(Answer::Text("0x1002093c"))),
    ("hud_worn", both(Answer::Text("hud"))),
    ("limit_alerts", both(Answer::Text("none"))),
    ("limit_announced", both(Answer::Text("none"))),
    ("limit_b_appearance", both(Answer::Text("none"))),
    ("limit_b_short", both(Answer::Count(1))),
    (
        "limit_benefits",
        each(Answer::Text("38"), Answer::Text("absent")),
    ),
    ("limit_features", both(Answer::Text("38"))),
    ("limit_short", both(Answer::Count(1))),
    ("limit_worn_after", both(Answer::Text("the limit"))),
    ("limit_worn_before_is_arrival", both(Answer::Flag(true))),
    ("move_alerts", both(Answer::Text("none"))),
    ("move_announced", both(Answer::Text("none"))),
    (
        "move_appearance",
        each(
            Answer::Text("sent, the last listing 1 of the census's attachments"),
            Answer::Text("sent, the last listing 0 of the census's attachments"),
        ),
    ),
    ("move_arrived", both(Answer::Text("cube1"))),
    ("move_attach_item", both(Answer::Text("the item"))),
    ("move_b_appearance", both(Answer::Text("none"))),
    ("move_b_arrived", both(Answer::Text("none"))),
    ("move_b_attach_item", both(Answer::Text("the item"))),
    ("move_b_killed", both(Answer::Text("none"))),
    (
        "move_b_killed_the_avatar",
        each(Answer::Flag(true), Answer::Flag(false)),
    ),
    ("move_b_left", both(Answer::Text("none"))),
    ("move_b_name_values", both(Answer::Text("AttachItemID"))),
    ("move_b_owner", both(Answer::Text("nil"))),
    ("move_b_parent", both(Answer::Text("the avatar"))),
    ("move_b_point", both(Answer::Text("the first point"))),
    ("move_b_shown", both(Answer::Flag(true))),
    (
        "move_b_update_flags",
        each(Answer::Text("0x10020810"), Answer::Text("0x00020810")),
    ),
    ("move_b_worn", both(Answer::Text("cube1"))),
    ("move_back_alerts", both(Answer::Text("none"))),
    ("move_back_announced", both(Answer::Text("none"))),
    (
        "move_back_appearance",
        each(
            Answer::Text("sent, the last listing 1 of the census's attachments"),
            Answer::Text("sent, the last listing 0 of the census's attachments"),
        ),
    ),
    ("move_back_arrived", both(Answer::Text("cube1"))),
    ("move_back_attach_item", both(Answer::Text("the item"))),
    ("move_back_b_appearance", both(Answer::Text("none"))),
    (
        "move_back_b_arrived",
        each(Answer::Text("none"), Answer::Text("cube1")),
    ),
    ("move_back_b_attach_item", both(Answer::Text("the item"))),
    ("move_back_b_killed", both(Answer::Text("none"))),
    ("move_back_b_killed_the_avatar", both(Answer::Flag(false))),
    ("move_back_b_left", both(Answer::Text("none"))),
    (
        "move_back_b_name_values",
        both(Answer::Text("AttachItemID")),
    ),
    ("move_back_b_owner", both(Answer::Text("nil"))),
    ("move_back_b_parent", both(Answer::Text("the avatar"))),
    ("move_back_b_point", both(Answer::Text("the first point"))),
    ("move_back_b_shown", both(Answer::Flag(true))),
    (
        "move_back_b_update_flags",
        each(Answer::Text("0x10020810"), Answer::Text("0x00020810")),
    ),
    ("move_back_b_worn", both(Answer::Text("cube1"))),
    ("move_back_killed", both(Answer::Text("none"))),
    ("move_back_left", both(Answer::Text("none"))),
    ("move_back_name_values", both(Answer::Text("AttachItemID"))),
    (
        "move_back_old_region_killed_it_first",
        both(Answer::Flag(false)),
    ),
    (
        "move_back_old_region_killed_the_avatar_first",
        each(Answer::Flag(true), Answer::Flag(false)),
    ),
    ("move_back_owner", both(Answer::Text("nil"))),
    ("move_back_parent", both(Answer::Text("the avatar"))),
    ("move_back_point", both(Answer::Text("the first point"))),
    ("move_back_removed", both(Answer::Text("none"))),
    ("move_back_same_full_id", both(Answer::Flag(true))),
    (
        "move_back_same_local_id",
        each(Answer::Flag(false), Answer::Flag(true)),
    ),
    ("move_back_shown", both(Answer::Flag(true))),
    ("move_back_teleport", both(Answer::Text("arrived"))),
    ("move_back_update_flags", both(Answer::Text("0x1002093c"))),
    ("move_back_worn", both(Answer::Text("cube1"))),
    ("move_killed", both(Answer::Text("none"))),
    ("move_left", both(Answer::Text("none"))),
    ("move_name_values", both(Answer::Text("AttachItemID"))),
    ("move_old_region_killed_it_first", both(Answer::Flag(false))),
    (
        "move_old_region_killed_the_avatar_first",
        each(Answer::Flag(true), Answer::Flag(false)),
    ),
    ("move_owner", both(Answer::Text("nil"))),
    ("move_parent", both(Answer::Text("the avatar"))),
    ("move_point", both(Answer::Text("the first point"))),
    ("move_removed", both(Answer::Text("none"))),
    ("move_same_full_id", both(Answer::Flag(true))),
    ("move_same_local_id", both(Answer::Flag(false))),
    ("move_shown", both(Answer::Flag(true))),
    ("move_teleport", both(Answer::Text("arrived"))),
    ("move_update_flags", both(Answer::Text("0x1002093c"))),
    ("move_worn", both(Answer::Text("cube1"))),
    ("relogin_any_worn", each(Answer::Any, Answer::Count(1))),
    (
        "relogin_appearance",
        each(
            Answer::Text("sent, the last listing 1 of the census's attachments"),
            Answer::Text("sent, the last listing 0 of the census's attachments"),
        ),
    ),
    ("relogin_arrived", both(Answer::Text("cube1"))),
    ("relogin_point", both(Answer::Text("the first point"))),
    ("relogin_worn", both(Answer::Text("cube1"))),
    ("script_attach_alerts", both(Answer::Text("none"))),
    (
        "script_attach_announced",
        both(Answer::Text("scripted (udp)")),
    ),
    ("script_attach_appearance", both(Answer::Text("none"))),
    ("script_attach_arrived", both(Answer::Text("scripted"))),
    ("script_attach_attach_item", both(Answer::Text("the item"))),
    ("script_attach_b_appearance", both(Answer::Text("none"))),
    ("script_attach_b_arrived", both(Answer::Text("scripted"))),
    (
        "script_attach_b_attach_item",
        both(Answer::Text("the item")),
    ),
    ("script_attach_b_killed", both(Answer::Text("none"))),
    ("script_attach_b_left", both(Answer::Text("none"))),
    (
        "script_attach_b_name_values",
        both(Answer::Text("AttachItemID")),
    ),
    ("script_attach_b_owner", both(Answer::Text("nil"))),
    ("script_attach_b_parent", both(Answer::Text("the avatar"))),
    (
        "script_attach_b_point",
        both(Answer::Text("the second point")),
    ),
    ("script_attach_b_shown", both(Answer::Flag(true))),
    (
        "script_attach_b_update_flags",
        each(Answer::Text("0x10020050"), Answer::Text("0x00020050")),
    ),
    ("script_attach_b_worn", both(Answer::Text("scripted"))),
    ("script_attach_detach_alerts", both(Answer::Text("none"))),
    (
        "script_attach_detach_announced",
        each(
            Answer::Text("scripted (bulk)"),
            Answer::Text("scripted (udp)"),
        ),
    ),
    (
        "script_attach_detach_appearance",
        both(Answer::Text("none")),
    ),
    ("script_attach_detach_arrived", both(Answer::Text("none"))),
    (
        "script_attach_detach_b_appearance",
        both(Answer::Text("none")),
    ),
    ("script_attach_detach_b_arrived", both(Answer::Text("none"))),
    (
        "script_attach_detach_b_killed",
        both(Answer::Text("scripted")),
    ),
    ("script_attach_detach_b_left", both(Answer::Text("none"))),
    ("script_attach_detach_b_worn", both(Answer::Text("none"))),
    (
        "script_attach_detach_killed",
        both(Answer::Text("scripted")),
    ),
    ("script_attach_detach_left", both(Answer::Text("none"))),
    ("script_attach_detach_removed", both(Answer::Text("none"))),
    ("script_attach_detach_worn", both(Answer::Text("none"))),
    (
        "script_attach_form",
        both(Answer::Text("sent again attached, same ids")),
    ),
    ("script_attach_killed", both(Answer::Text("none"))),
    ("script_attach_left", both(Answer::Text("none"))),
    (
        "script_attach_name_values",
        both(Answer::Text("AttachItemID")),
    ),
    ("script_attach_owner", both(Answer::Text("nil"))),
    ("script_attach_parent", both(Answer::Text("the avatar"))),
    (
        "script_attach_permission_asked",
        both(Answer::Text("asked for 32")),
    ),
    (
        "script_attach_permission_granted",
        both(Answer::Text("P|32")),
    ),
    (
        "script_attach_point",
        both(Answer::Text("the second point")),
    ),
    ("script_attach_removed", both(Answer::Text("none"))),
    (
        "script_attach_said",
        both(Answer::Text("A|1|the point asked")),
    ),
    ("script_attach_same_local_id", both(Answer::Flag(true))),
    ("script_attach_shown", both(Answer::Flag(true))),
    (
        "script_attach_update_flags",
        both(Answer::Text("0x1002017c")),
    ),
    ("script_attach_worn", both(Answer::Text("scripted"))),
    ("script_temp_alerts", both(Answer::Text("none"))),
    ("script_temp_announced", both(Answer::Text("none"))),
    ("script_temp_appearance", both(Answer::Text("none"))),
    ("script_temp_arrived", both(Answer::Text("temp"))),
    (
        "script_temp_attach_item",
        both(Answer::Text("the object's own id")),
    ),
    ("script_temp_b_appearance", both(Answer::Text("none"))),
    ("script_temp_b_arrived", both(Answer::Text("temp"))),
    (
        "script_temp_b_attach_item",
        both(Answer::Text("the object's own id")),
    ),
    ("script_temp_b_killed", both(Answer::Text("none"))),
    ("script_temp_b_left", both(Answer::Text("none"))),
    (
        "script_temp_b_name_values",
        both(Answer::Text("AttachItemID")),
    ),
    ("script_temp_b_owner", both(Answer::Text("nil"))),
    ("script_temp_b_parent", both(Answer::Text("the avatar"))),
    (
        "script_temp_b_point",
        both(Answer::Text("the second point")),
    ),
    ("script_temp_b_shown", both(Answer::Flag(true))),
    (
        "script_temp_b_update_flags",
        each(Answer::Text("0x10020050"), Answer::Text("0x00020050")),
    ),
    ("script_temp_b_worn", both(Answer::Text("temp"))),
    ("script_temp_detach_alerts", both(Answer::Text("none"))),
    ("script_temp_detach_announced", both(Answer::Text("none"))),
    ("script_temp_detach_appearance", both(Answer::Text("none"))),
    ("script_temp_detach_arrived", both(Answer::Text("none"))),
    (
        "script_temp_detach_b_appearance",
        both(Answer::Text("none")),
    ),
    ("script_temp_detach_b_arrived", both(Answer::Text("none"))),
    ("script_temp_detach_b_killed", both(Answer::Text("temp"))),
    ("script_temp_detach_b_left", both(Answer::Text("none"))),
    ("script_temp_detach_b_worn", both(Answer::Text("none"))),
    (
        "script_temp_detach_back_in_world",
        both(Answer::Flag(false)),
    ),
    ("script_temp_detach_killed", both(Answer::Text("temp"))),
    ("script_temp_detach_left", both(Answer::Text("none"))),
    ("script_temp_detach_removed", both(Answer::Text("none"))),
    ("script_temp_detach_worn", both(Answer::Text("none"))),
    (
        "script_temp_form",
        both(Answer::Text("sent again attached, same ids")),
    ),
    ("script_temp_killed", both(Answer::Text("none"))),
    ("script_temp_left", both(Answer::Text("none"))),
    (
        "script_temp_name_values",
        both(Answer::Text("AttachItemID")),
    ),
    ("script_temp_owner", both(Answer::Text("nil"))),
    ("script_temp_parent", both(Answer::Text("the avatar"))),
    (
        "script_temp_permission_asked",
        both(Answer::Text("asked for 32")),
    ),
    ("script_temp_permission_granted", both(Answer::Text("P|32"))),
    ("script_temp_point", both(Answer::Text("the second point"))),
    ("script_temp_removed", both(Answer::Text("none"))),
    (
        "script_temp_said",
        both(Answer::Text("A|1|the point asked")),
    ),
    ("script_temp_same_local_id", both(Answer::Flag(true))),
    ("script_temp_shown", both(Answer::Flag(true))),
    ("script_temp_update_flags", both(Answer::Text("0x1002017c"))),
    ("script_temp_worn", both(Answer::Text("temp"))),
    ("wear_add_alerts", both(Answer::Text("none"))),
    ("wear_add_announced", both(Answer::Text("none"))),
    (
        "wear_add_appearance",
        each(
            Answer::Text("sent, the last listing 2 of the census's attachments"),
            Answer::Text("none"),
        ),
    ),
    ("wear_add_arrived", both(Answer::Text("cube2"))),
    ("wear_add_b_appearance", both(Answer::Text("none"))),
    ("wear_add_b_arrived", both(Answer::Text("cube2"))),
    ("wear_add_b_killed", both(Answer::Text("none"))),
    ("wear_add_b_left", both(Answer::Text("none"))),
    ("wear_add_b_worn", both(Answer::Text("cube1+cube2"))),
    ("wear_add_killed", both(Answer::Text("none"))),
    ("wear_add_left", both(Answer::Text("none"))),
    ("wear_add_removed", both(Answer::Text("none"))),
    ("wear_add_worn", both(Answer::Text("cube1+cube2"))),
    ("wear_again_alerts", both(Answer::Text("none"))),
    ("wear_again_announced", both(Answer::Text("none"))),
    ("wear_again_appearance", both(Answer::Text("none"))),
    ("wear_again_arrived", both(Answer::Text("none"))),
    ("wear_again_b_appearance", both(Answer::Text("none"))),
    ("wear_again_b_arrived", both(Answer::Text("none"))),
    ("wear_again_b_killed", both(Answer::Text("none"))),
    ("wear_again_b_left", both(Answer::Text("none"))),
    (
        "wear_again_b_worn",
        each(Answer::Text("cube2+cube3"), Answer::Text("cube3")),
    ),
    ("wear_again_killed", both(Answer::Text("none"))),
    ("wear_again_left", both(Answer::Text("none"))),
    ("wear_again_point", both(Answer::Text("the first point"))),
    ("wear_again_removed", both(Answer::Text("none"))),
    (
        "wear_again_worn",
        each(Answer::Text("cube2+cube3"), Answer::Text("cube3")),
    ),
    ("wear_alerts", both(Answer::Text("none"))),
    ("wear_announced", both(Answer::Text("none"))),
    (
        "wear_appearance",
        each(
            Answer::Text("sent, the last listing 1 of the census's attachments"),
            Answer::Text("none"),
        ),
    ),
    ("wear_arrived", both(Answer::Text("cube1"))),
    ("wear_attach_item", both(Answer::Text("the item"))),
    ("wear_b_appearance", both(Answer::Text("none"))),
    ("wear_b_arrived", both(Answer::Text("cube1"))),
    ("wear_b_attach_item", both(Answer::Text("the item"))),
    ("wear_b_killed", both(Answer::Text("none"))),
    ("wear_b_left", both(Answer::Text("none"))),
    ("wear_b_name_values", both(Answer::Text("AttachItemID"))),
    ("wear_b_owner", both(Answer::Text("nil"))),
    ("wear_b_parent", both(Answer::Text("the avatar"))),
    ("wear_b_point", both(Answer::Text("the first point"))),
    ("wear_b_shown", both(Answer::Flag(true))),
    (
        "wear_b_update_flags",
        each(Answer::Text("0x10020810"), Answer::Text("0x00020810")),
    ),
    ("wear_b_worn", both(Answer::Text("cube1"))),
    ("wear_killed", both(Answer::Text("none"))),
    ("wear_left", both(Answer::Text("none"))),
    (
        "wear_missing_alerts",
        each(
            Answer::Text("Attempt to rez an object failed. [RezAttemptFailed]"),
            Answer::Text("none"),
        ),
    ),
    ("wear_missing_announced", both(Answer::Text("none"))),
    ("wear_missing_any_arrived", both(Answer::Count(0))),
    ("wear_missing_appearance", both(Answer::Text("none"))),
    ("wear_missing_arrived", both(Answer::Text("none"))),
    ("wear_missing_b_appearance", both(Answer::Text("none"))),
    ("wear_missing_b_arrived", both(Answer::Text("none"))),
    ("wear_missing_b_killed", both(Answer::Text("none"))),
    ("wear_missing_b_left", both(Answer::Text("none"))),
    (
        "wear_missing_b_worn",
        each(Answer::Text("cube2+cube3"), Answer::Text("cube3")),
    ),
    ("wear_missing_killed", both(Answer::Text("none"))),
    ("wear_missing_left", both(Answer::Text("none"))),
    ("wear_missing_removed", both(Answer::Text("none"))),
    (
        "wear_missing_worn",
        each(Answer::Text("cube2+cube3"), Answer::Text("cube3")),
    ),
    ("wear_multi_alerts", both(Answer::Text("none"))),
    ("wear_multi_announced", both(Answer::Text("none"))),
    ("wear_multi_appearance", both(Answer::Text("none"))),
    ("wear_multi_arrived", both(Answer::Text("cube1+cube2"))),
    ("wear_multi_b_appearance", both(Answer::Text("none"))),
    ("wear_multi_b_arrived", both(Answer::Text("cube1+cube2"))),
    ("wear_multi_b_killed", both(Answer::Text("none"))),
    ("wear_multi_b_left", both(Answer::Text("none"))),
    ("wear_multi_b_worn", both(Answer::Text("cube1+cube2"))),
    ("wear_multi_killed", both(Answer::Text("none"))),
    ("wear_multi_left", both(Answer::Text("none"))),
    ("wear_multi_removed", both(Answer::Text("none"))),
    ("wear_multi_worn", both(Answer::Text("cube1+cube2"))),
    ("wear_name_values", both(Answer::Text("AttachItemID"))),
    ("wear_owner", both(Answer::Text("nil"))),
    ("wear_parent", both(Answer::Text("the avatar"))),
    ("wear_point", both(Answer::Text("the first point"))),
    ("wear_removed", both(Answer::Text("none"))),
    ("wear_replace_alerts", both(Answer::Text("none"))),
    (
        "wear_replace_announced",
        each(
            Answer::Text("cube1 (bulk)"),
            Answer::Text("cube1 (udp)+cube2 (udp)"),
        ),
    ),
    (
        "wear_replace_appearance",
        each(
            Answer::Text("sent, the last listing 2 of the census's attachments"),
            Answer::Text("none"),
        ),
    ),
    ("wear_replace_arrived", both(Answer::Text("cube3"))),
    ("wear_replace_b_appearance", both(Answer::Text("none"))),
    ("wear_replace_b_arrived", both(Answer::Text("cube3"))),
    (
        "wear_replace_b_killed",
        each(Answer::Text("cube1"), Answer::Text("cube1+cube2")),
    ),
    ("wear_replace_b_left", both(Answer::Text("none"))),
    (
        "wear_replace_b_worn",
        each(Answer::Text("cube2+cube3"), Answer::Text("cube3")),
    ),
    (
        "wear_replace_killed",
        each(Answer::Text("cube1"), Answer::Text("cube1+cube2")),
    ),
    ("wear_replace_left", both(Answer::Text("none"))),
    ("wear_replace_point", both(Answer::Text("the first point"))),
    ("wear_replace_removed", both(Answer::Text("none"))),
    (
        "wear_replace_worn",
        each(Answer::Text("cube2+cube3"), Answer::Text("cube3")),
    ),
    ("wear_same_local_id", both(Answer::Flag(true))),
    ("wear_shown", both(Answer::Flag(true))),
    ("wear_update_flags", both(Answer::Text("0x1002093c"))),
    ("wear_worn", both(Answer::Text("cube1"))),
];

/// A census of attachments.
#[derive(Debug)]
pub struct Attachments;

impl GridTest for Attachments {
    fn name(&self) -> &'static str {
        "attachments"
    }

    fn description(&self) -> &'static str {
        "Record what each grid answers every way of wearing and taking off an attachment, \
         what the agent and a second viewer are sent, a HUD, a drop, a script's attach, the \
         limit, a teleport and a new login with one worn"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn accounts(&self) -> u8 {
        2
    }

    fn rezzes_objects(&self) -> bool {
        true
    }

    fn start_location(&self, grid: Grid) -> &'static str {
        if is_opensim(grid) {
            OPENSIM_START
        } else {
            "last"
        }
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(1800)
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();
            let folders = login_folders(ctx.primary()).await?;
            let mut metrics = Metrics::new();
            let mut left = Leftovers::default();
            let outcome = census(ctx, folders, &mut metrics, &mut left).await;
            ctx.metrics().merge(metrics.clone());
            // Take off and take away what is left, whatever became of the
            // census.
            let cleanup = tidy(ctx.primary(), &left, folders).await;
            outcome
                .and_then(|()| hold_to_measured(grid, &metrics, MEASURED, SOURCE))
                .and(cleanup)
        })
    }
}

/// What the census leaves behind until it is taken away.
#[derive(Debug, Default)]
struct Leftovers {
    /// The objects standing and the items made.
    residue: Residue,
    /// The items that may still be worn.
    worn: Vec<InventoryKey>,
    /// Whether the agent is in the region the census began in, where the
    /// standing objects are.
    home: bool,
}

/// Takes off every item that may still be worn and takes away what the census
/// rezzed and made.
async fn tidy(
    session: &mut Session,
    left: &Leftovers,
    folders: Folders,
) -> Result<(), TestFailure> {
    for item in &left.worn {
        session
            .send(Command::DetachAttachmentIntoInventory { item_id: *item })
            .await?;
    }
    if !left.worn.is_empty() {
        let _heard = listen(session, DRAIN, DRAIN, |_heard| false).await?;
    }
    let residue = if left.home {
        Residue {
            roots: left.residue.roots.clone(),
            items: left.residue.items.clone(),
        }
    } else {
        Residue {
            roots: Vec::new(),
            items: left.residue.items.clone(),
        }
    };
    clean_up(session, &residue, folders.trash).await
}

/// One thing a session was sent, in arrival order.
#[derive(Debug, Clone)]
enum Sighting {
    /// An object of the agent's region, sent anew (`true`) or again.
    Object(Box<Object>, bool),
    /// A `KillObject` of the agent's region.
    Kill(Vec<RegionLocalObjectId>),
    /// The agent arrived in another region.
    Moved,
}

/// What a session was sent during a window.
#[derive(Debug, Default)]
struct Heard {
    /// The objects and kills of the agent's region, in arrival order.
    sightings: Vec<Sighting>,
    /// Kills sent down another circuit than the agent's own.
    elsewhere_kills: Vec<RegionLocalObjectId>,
    /// Every inventory item announced, and the shape it came in.
    announced: Vec<(&'static str, InventoryItem)>,
    /// Items the grid said were removed from the inventory.
    removed: Vec<InventoryKey>,
    /// The text of every alert.
    alerts: Vec<String>,
    /// What the probe said, its tag taken off.
    said: Vec<String>,
    /// Every script permission request.
    questions: Vec<ScriptPermissionRequest>,
    /// Every appearance of the actor.
    appearances: Vec<AvatarAppearance>,
    /// How a script save ended.
    uploads: Vec<Result<bool, String>>,
    /// Whether a teleport ended in another region.
    arrived_elsewhere: bool,
    /// A teleport's refusal.
    teleport_failed: Option<String>,
    /// The kills of the agent's region that came before a teleport ended in
    /// another region.
    kills_before_move: Vec<RegionLocalObjectId>,
}

impl Heard {
    /// Whether an object worn from `item` was sent.
    fn wore(&self, item: InventoryKey) -> bool {
        self.objects()
            .any(|object| attach_item(object) == Some(item.uuid()))
    }

    /// Every object sent, in arrival order.
    fn objects(&self) -> impl DoubleEndedIterator<Item = &Object> {
        self.sightings.iter().filter_map(|sighting| match sighting {
            Sighting::Object(object, _new) => Some(&**object),
            Sighting::Kill(_) | Sighting::Moved => None,
        })
    }

    /// Whether `local_id` was killed.
    fn killed(&self, local_id: RegionLocalObjectId) -> bool {
        self.sightings.iter().any(|sighting| match sighting {
            Sighting::Kill(ids) => ids.contains(&local_id),
            Sighting::Object(..) | Sighting::Moved => false,
        })
    }

    /// Whether the probe said `line`.
    fn said(&self, line: &str) -> bool {
        self.said.iter().any(|said| said == line)
    }

    /// Whether the probe said a line beginning with `start`.
    fn said_starting(&self, start: &str) -> bool {
        self.said.iter().any(|said| said.starts_with(start))
    }

    /// The object items announced.
    fn object_items(&self) -> impl Iterator<Item = &InventoryItem> {
        let object = i8::try_from(AssetType::Object.to_code()).unwrap_or(-1);
        self.announced
            .iter()
            .map(|(_shape, item)| item)
            .filter(move |item| item.item_type == object)
    }
}

/// The item an attachment says it was worn from.
fn attach_item(object: &Object) -> Option<Uuid> {
    object
        .name_value_data(ATTACH_ITEM_ID)
        .and_then(|value| value.trim().parse::<Uuid>().ok())
}

/// Listens on `session` until `done` says the leg has what it came for and
/// `quiet` more has passed, or for `limit` at most.
async fn listen(
    session: &mut Session,
    limit: Duration,
    quiet: Duration,
    mut done: impl FnMut(&Heard) -> bool,
) -> Result<Heard, TestFailure> {
    let mut heard = Heard::default();
    let actor = session.agent_id().map(|agent| agent.uuid());
    let mut root = session.circuit_id();
    let started = Instant::now();
    let mut done_at: Option<Instant> = None;
    loop {
        if done_at.is_none() && done(&heard) {
            done_at = Some(Instant::now());
        }
        let remaining = done_at.map_or_else(
            || limit.saturating_sub(started.elapsed()),
            |at| quiet.saturating_sub(at.elapsed()),
        );
        if remaining.is_zero() {
            return Ok(heard);
        }
        let outcome = session
            .wait_for(remaining, |event| {
                match event {
                    Event::ObjectStreamBatch(batch)
                        if batch.form == sl_client_tokio::ObjectUpdateForm::Kill =>
                    {
                        let ids: Vec<RegionLocalObjectId> =
                            batch.entries.iter().map(|entry| entry.local_id).collect();
                        if batch.child {
                            heard.elsewhere_kills.extend(ids);
                        } else {
                            if !heard.arrived_elsewhere {
                                heard.kills_before_move.extend(ids.iter().copied());
                            }
                            heard.sightings.push(Sighting::Kill(ids));
                        }
                    }
                    Event::ObjectAdded(object)
                        if root.is_none_or(|root| object.circuit == root) =>
                    {
                        heard.sightings.push(Sighting::Object(object.clone(), true));
                    }
                    Event::ObjectUpdated(object)
                        if root.is_none_or(|root| object.circuit == root) =>
                    {
                        heard
                            .sightings
                            .push(Sighting::Object(object.clone(), false));
                    }
                    Event::RegionChanged { circuit, .. } => {
                        root = Some(*circuit);
                        heard.arrived_elsewhere = true;
                        heard.sightings.push(Sighting::Moved);
                    }
                    Event::TeleportFailed { reason, .. } => {
                        heard.teleport_failed = Some(reason.clone());
                    }
                    Event::InventoryItemCreated { item, .. } => {
                        heard.announced.push((ANNOUNCED_LEGACY, item.clone()));
                    }
                    Event::InventoryBulkUpdate { items, .. } => heard
                        .announced
                        .extend(items.iter().map(|item| (ANNOUNCED_BULK, item.clone()))),
                    Event::InventoryItemsRemoved { items }
                    | Event::InventoryObjectsRemoved { items, .. } => {
                        heard.removed.extend(items.iter().copied());
                    }
                    Event::AlertMessage {
                        message,
                        alert_info,
                        ..
                    } => heard.alerts.push(alert_info.first().map_or_else(
                        || message.clone(),
                        |info| format!("{message} [{}]", info.message),
                    )),
                    Event::AgentAlertMessage { message, .. } => heard.alerts.push(message.clone()),
                    Event::ChatReceived(chat) => {
                        if let Some(line) = chat
                            .message
                            .strip_prefix(PROBE_TAG)
                            .and_then(|rest| rest.strip_prefix('|'))
                        {
                            heard.said.push(line.to_owned());
                        }
                    }
                    Event::ScriptPermissionRequest(request) => {
                        heard.questions.push((**request).clone());
                    }
                    Event::AvatarAppearance(appearance)
                        if Some(appearance.avatar_id.uuid()) == actor =>
                    {
                        heard.appearances.push((**appearance).clone());
                    }
                    Event::ScriptUploaded { compiled, .. } => heard.uploads.push(Ok(*compiled)),
                    Event::AssetUploadFailed { reason } => heard.uploads.push(Err(reason.clone())),
                    _ => return None,
                }
                Some(())
            })
            .await;
        match outcome {
            Ok(()) => {}
            Err(TestFailure::Timeout(_)) => return Ok(heard),
            Err(other) => return Err(other),
        }
    }
}

/// What one viewer holds of A: its avatar and what A wears.
#[derive(Debug, Default)]
struct View {
    /// A's avatar's local id in the region the viewer is shown.
    avatar: Option<RegionLocalObjectId>,
    /// Every attachment of A's, by its local id: the census's and A's own.
    all_worn: BTreeMap<RegionLocalObjectId, Object>,
    /// The census's attachments, by role.
    worn: BTreeMap<String, Object>,
}

impl View {
    /// The roles worn, `+`-joined, or `none`.
    fn worn_roles(&self) -> String {
        joined(self.worn.keys().cloned().collect())
    }
}

/// What a fold of one window into a [`View`] found.
#[derive(Debug, Default)]
struct Change {
    /// The roles whose attachment was sent anew.
    arrived: Vec<String>,
    /// The roles whose attachment was killed.
    killed: Vec<String>,
    /// The roles whose object was sent again no longer attached.
    left: Vec<String>,
    /// Every attachment of A's sent anew, the census's or not.
    any_arrived: usize,
}

/// `parts`, sorted and `+`-joined, or `none`.
fn joined(mut parts: Vec<String>) -> String {
    if parts.is_empty() {
        return "none".to_owned();
    }
    parts.sort();
    parts.join("+")
}

/// Who is who: the items and objects the census knows by role.
#[derive(Debug, Default)]
struct Roles {
    /// The items, by id.
    items: HashMap<InventoryKey, String>,
    /// The objects, by id.
    objects: HashMap<ObjectKey, String>,
}

impl Roles {
    /// The role `object` plays, if it is one of the census's.
    fn of(&self, object: &Object) -> Option<String> {
        attach_item(object)
            .and_then(|item| self.items.get(&InventoryKey::from(item)))
            .or_else(|| self.objects.get(&object.full_id))
            .cloned()
    }

    /// The item of `role`.
    fn item(&self, role: &str) -> Option<InventoryKey> {
        self.items
            .iter()
            .find(|(_item, named)| named.as_str() == role)
            .map(|(item, _named)| *item)
    }
}

/// Folds `heard` into `view`, learning which objects play which role.
fn fold(view: &mut View, roles: &mut Roles, actor: Uuid, heard: &Heard) -> Change {
    let mut change = Change::default();
    for sighting in &heard.sightings {
        match sighting {
            Sighting::Moved => {
                view.avatar = None;
                view.all_worn.clear();
                view.worn.clear();
            }
            Sighting::Object(object, new) => {
                if object.full_id.uuid() == actor {
                    view.avatar = Some(object.local_id);
                    continue;
                }
                let role = roles.of(object);
                if let Some(role) = &role {
                    roles.objects.insert(object.full_id, role.clone());
                }
                let on_avatar = view.avatar.is_some_and(|avatar| object.parent_id == avatar)
                    && object.attachment_point_id().is_some();
                if on_avatar {
                    let fresh = !view.all_worn.contains_key(&object.local_id);
                    if *new || fresh {
                        change.any_arrived = change.any_arrived.saturating_add(1);
                    }
                    view.all_worn.insert(object.local_id, (**object).clone());
                    if let Some(role) = role {
                        if *new || !view.worn.contains_key(&role) {
                            change.arrived.push(role.clone());
                        }
                        view.worn.insert(role, (**object).clone());
                    }
                } else if let Some(role) = role
                    && view
                        .worn
                        .get(&role)
                        .is_some_and(|worn| worn.full_id == object.full_id)
                {
                    view.worn.remove(&role);
                    view.all_worn.remove(&object.local_id);
                    change.left.push(role);
                }
            }
            Sighting::Kill(ids) => {
                for id in ids {
                    view.all_worn.remove(id);
                }
                let gone: Vec<String> = view
                    .worn
                    .iter()
                    .filter(|(_role, object)| ids.contains(&object.local_id))
                    .map(|(role, _object)| role.clone())
                    .collect();
                for role in gone {
                    view.worn.remove(&role);
                    change.killed.push(role);
                }
            }
        }
    }
    change
}

/// The points the census wears on.
#[derive(Debug, Clone, Copy)]
struct Points {
    /// The first free body point.
    first: AttachmentPoint,
    /// The second.
    second: AttachmentPoint,
    /// The first free HUD point.
    hud: AttachmentPoint,
}

/// Everything the legs share.
struct Census<'a> {
    /// The avatar that wears.
    a: &'a mut Session,
    /// The avatar that watches.
    b: &'a mut Session,
    /// A's id.
    actor: Uuid,
    /// The rez bookkeeping.
    run: Run,
    /// Who is who.
    roles: Roles,
    /// What A holds of itself.
    own: View,
    /// What B holds of A.
    seen: View,
    /// The points.
    points: Points,
    /// A's and B's names, each with the word that replaces it in a metric.
    names: Vec<(String, &'static str)>,
    /// The cube the copies are taken of.
    source: Object,
    /// The role an item the grid makes in the next window plays.
    adopt: Option<String>,
}

impl Census<'_> {
    /// Sends `commands` as A, listens to A until `done` (or `limit`), then
    /// reads B's queue, and folds both.
    async fn ask(
        &mut self,
        commands: Vec<Command>,
        limit: Duration,
        done: impl FnMut(&Heard) -> bool,
    ) -> Result<(Heard, Heard, Change, Change), TestFailure> {
        for command in commands {
            self.a.send(command).await?;
        }
        let heard = listen(self.a, limit, self.run.pace.quiet, done).await?;
        let theirs = listen(self.b, DRAIN, DRAIN, |_heard| false).await?;
        // An item the grid made in this window is the adopted role's: the
        // object it then sends worn from that item is too.
        if let Some(role) = self.adopt.take() {
            for item in heard.object_items() {
                self.roles.items.insert(item.item_id, role.clone());
            }
        }
        let change = fold(&mut self.own, &mut self.roles, self.actor, &heard);
        let their_change = fold(&mut self.seen, &mut self.roles, self.actor, &theirs);
        Ok((heard, theirs, change, their_change))
    }

    /// Records what a leg changed under `prefix`.
    fn record(
        &self,
        metrics: &mut Metrics,
        prefix: &str,
        heard: &Heard,
        theirs: &Heard,
        change: &Change,
        their_change: &Change,
    ) {
        metrics.set(&format!("{prefix}_arrived"), joined(change.arrived.clone()));
        metrics.set(&format!("{prefix}_killed"), joined(change.killed.clone()));
        metrics.set(&format!("{prefix}_left"), joined(change.left.clone()));
        metrics.set(&format!("{prefix}_worn"), self.own.worn_roles());
        metrics.set(
            &format!("{prefix}_b_arrived"),
            joined(their_change.arrived.clone()),
        );
        metrics.set(
            &format!("{prefix}_b_killed"),
            joined(their_change.killed.clone()),
        );
        metrics.set(
            &format!("{prefix}_b_left"),
            joined(their_change.left.clone()),
        );
        metrics.set(&format!("{prefix}_b_worn"), self.seen.worn_roles());
        metrics.set(&format!("{prefix}_announced"), self.announced(heard));
        metrics.set(&format!("{prefix}_removed"), self.removed(heard));
        metrics.set(&format!("{prefix}_alerts"), self.alerts(heard));
        metrics.set(&format!("{prefix}_appearance"), self.appearance(heard));
        metrics.set(&format!("{prefix}_b_appearance"), self.appearance(theirs));
    }

    /// The items `heard` announced, in words: the shape, and the role, a link
    /// or something else.
    fn announced(&self, heard: &Heard) -> String {
        joined(
            heard
                .announced
                .iter()
                .map(|(shape, item)| {
                    let what = if item.item_type == LINK_TYPE {
                        let to = self
                            .roles
                            .items
                            .get(&InventoryKey::from(item.asset_id))
                            .map_or("something else", String::as_str);
                        format!("a link to {to}")
                    } else if let Some(role) = self.roles.items.get(&item.item_id) {
                        role.clone()
                    } else {
                        format!("a new item of type {}", item.item_type)
                    };
                    let shape = if *shape == ANNOUNCED_LEGACY {
                        "udp"
                    } else {
                        "bulk"
                    };
                    format!("{what} ({shape})")
                })
                .collect(),
        )
    }

    /// The items `heard` said were removed, by role.
    fn removed(&self, heard: &Heard) -> String {
        joined(
            heard
                .removed
                .iter()
                .map(|item| {
                    self.roles
                        .items
                        .get(item)
                        .cloned()
                        .unwrap_or_else(|| "another item".to_owned())
                })
                .collect(),
        )
    }

    /// The alerts, scrubbed of ids and names, `|`-joined, or `none`.
    fn alerts(&self, heard: &Heard) -> String {
        if heard.alerts.is_empty() {
            return "none".to_owned();
        }
        heard
            .alerts
            .iter()
            .map(|alert| self.scrub(alert))
            .collect::<Vec<_>>()
            .join(" | ")
    }

    /// `text` with every id and every name of A's and B's replaced.
    fn scrub(&self, text: &str) -> String {
        let mut out = without_ids(text);
        for (name, word) in &self.names {
            if !name.is_empty() {
                out = out.replace(name.as_str(), word);
            }
        }
        out
    }

    /// Whether an appearance of A came, and how many of the census's worn
    /// attachments the last one lists.
    fn appearance(&self, heard: &Heard) -> String {
        let Some(last) = heard.appearances.last() else {
            return "none".to_owned();
        };
        let ours: HashSet<ObjectKey> = self.roles.objects.keys().copied().collect();
        let listed = last
            .attachments
            .iter()
            .filter(|attachment| ours.contains(&attachment.id))
            .count();
        // How many: Second Life sent one after a teleport in one run and
        // two in the next.
        format!("sent, the last listing {listed} of the census's attachments")
    }

    /// Records how one attachment came under `prefix`, as A and B see it.
    fn describe(&self, metrics: &mut Metrics, prefix: &str, role: &str) {
        for (view, side) in [(&self.own, ""), (&self.seen, "_b")] {
            let key = |name: &str| format!("{prefix}{side}_{name}");
            let Some(object) = view.worn.get(role) else {
                metrics.set(&key("shown"), false);
                continue;
            };
            metrics.set(&key("shown"), true);
            metrics.set(
                &key("parent"),
                if view.avatar == Some(object.parent_id) {
                    "the avatar"
                } else {
                    "something else"
                },
            );
            let item = self.roles.item(role);
            metrics.set(
                &key("attach_item"),
                match object.name_value_data(ATTACH_ITEM_ID) {
                    None => "absent",
                    Some(value) => match value.trim().parse::<Uuid>() {
                        Ok(id) if id.is_nil() => "nil",
                        Ok(id) if Some(InventoryKey::from(id)) == item => "the item",
                        Ok(id) if id == object.full_id.uuid() => "the object's own id",
                        Ok(_) => "another item",
                        Err(_) => "not an id",
                    },
                },
            );
            let mut names: Vec<String> = object
                .name_values()
                .into_iter()
                .map(|pair| pair.name)
                .collect();
            names.sort();
            metrics.set(&key("name_values"), names.join(","));
            metrics.set(
                &key("owner"),
                if object.owner_id.is_nil() {
                    "nil"
                } else if object.owner_id == self.actor {
                    "A"
                } else {
                    "another"
                },
            );
            metrics.set(
                &key("update_flags"),
                format!("0x{:08x}", object.update_flags),
            );
            metrics.set(&key("point"), point_word(object, self.points));
        }
        if let (Some(own), Some(seen)) = (self.own.worn.get(role), self.seen.worn.get(role)) {
            metrics.set(
                &format!("{prefix}_same_local_id"),
                own.local_id == seen.local_id,
            );
        }
    }

    /// Makes `role` an item of A's: a copy of the source cube.
    async fn copy(&mut self, role: &str) -> Result<InventoryKey, TestFailure> {
        self.a
            .send(Command::DerezObjects {
                local_ids: vec![self.source.scoped_id()],
                destination: DeRezDestination::AcquireToAgentInventory(self.run.folders.objects),
                transaction_id: TransactionId::from(Uuid::new_v4()),
                group_id: None,
            })
            .await?;
        let heard = listen(self.a, STEP, Duration::ZERO, |heard| {
            heard.object_items().next().is_some()
        })
        .await?;
        let item = heard.object_items().next().cloned().ok_or_else(|| {
            TestFailure::Assertion(format!("a copy of the cube for {role} was never announced"))
        })?;
        self.roles.items.insert(item.item_id, role.to_owned());
        Ok(item.item_id)
    }

    /// The wear request of `role` on `point`.
    fn wear(&self, role: &str, point: AttachmentPoint, mode: AttachmentMode) -> RezAttachment {
        RezAttachment {
            item_id: self
                .roles
                .item(role)
                .unwrap_or_else(|| InventoryKey::from(MADE_UP_ITEM)),
            owner_id: self.actor,
            attachment_point: point,
            mode,
            name: CUBE_NAME.to_owned(),
            description: String::new(),
        }
    }

    /// The item of `role`, which the census made.
    fn item(&self, role: &str) -> Result<InventoryKey, TestFailure> {
        self.roles
            .item(role)
            .ok_or_else(|| TestFailure::State(format!("{role} has no item")))
    }

    /// Wears `role` without recording the leg, and fails when it never comes.
    async fn put_on(&mut self, role: &str, point: AttachmentPoint) -> Result<(), TestFailure> {
        let item = self.item(role)?;
        let rez = self.wear(role, point, AttachmentMode::Add);
        let _answer = self
            .ask(vec![Command::RezAttachment(rez)], STEP, |heard| {
                heard.wore(item)
            })
            .await?;
        if self.own.worn.contains_key(role) {
            Ok(())
        } else {
            Err(TestFailure::Assertion(format!("{role} was never worn")))
        }
    }

    /// Takes off every census attachment A wears but `keep`'s, without
    /// recording it.
    async fn strip(&mut self, keep: Option<&str>) -> Result<(), TestFailure> {
        let worn: Vec<(ScopedObjectId, RegionLocalObjectId)> = self
            .own
            .worn
            .iter()
            .filter(|(role, _object)| Some(role.as_str()) != keep)
            .map(|(_role, object)| (object.scoped_id(), object.local_id))
            .collect();
        if worn.is_empty() {
            return Ok(());
        }
        let ids: Vec<ScopedObjectId> = worn.iter().map(|(scoped, _id)| *scoped).collect();
        let _answer = self
            .ask(
                vec![Command::DetachObjects { local_ids: ids }],
                STEP,
                |heard| worn.iter().all(|(_scoped, id)| heard.killed(*id)),
            )
            .await?;
        Ok(())
    }

    /// Tells the probe `order` and listens until `done`.
    async fn order(
        &mut self,
        order: &str,
        limit: Duration,
        done: impl FnMut(&Heard) -> bool,
    ) -> Result<(Heard, Heard, Change, Change), TestFailure> {
        self.ask(
            vec![Command::Chat {
                message: order.to_owned(),
                chat_type: ChatType::Normal,
                channel: ChatChannel(PROBE_CHANNEL),
            }],
            limit,
            done,
        )
        .await
    }

    /// Rezzes a cube, names it, puts the probe in it and waits for it to say
    /// it runs.
    async fn scripted_cube(
        &mut self,
        role: &str,
        slot: u8,
        residue: &mut Residue,
    ) -> Result<Object, TestFailure> {
        let cube = rez_cube(self.a, &mut self.run, residue, slot).await?;
        self.roles.objects.insert(cube.full_id, role.to_owned());
        self.a
            .send(Command::SetObjectName {
                local_id: cube.scoped_id(),
                name: SCRIPTED_NAME.to_owned(),
            })
            .await?;
        self.a
            .send(Command::RezScript {
                target: cube.scoped_id(),
                params: Box::new(RezScriptParams {
                    group_id: None,
                    enabled: true,
                    item: RestoreItem::new_script(
                        AgentKey::from(self.actor),
                        cube.full_id,
                        PROBE_NAME,
                        Uuid::new_v4(),
                    ),
                }),
            })
            .await?;
        let started = Instant::now();
        let item = loop {
            let _heard = listen(
                self.a,
                Duration::from_millis(500),
                Duration::ZERO,
                |_heard| false,
            )
            .await?;
            let held = listing(self.a, cube.scoped_id(), cube.full_id).await?;
            if let Some(item) = held
                .items
                .iter()
                .find(|item| item.name == PROBE_NAME && item.inv_type == InventoryType::Script)
            {
                break item.item_id;
            }
            if started.elapsed() >= STEP {
                return Err(TestFailure::Assertion(
                    "the new script never appeared in the cube".to_owned(),
                ));
            }
        };
        self.a
            .send(Command::UploadScript {
                location: ScriptUploadLocation::TaskInventory {
                    task_id: cube.full_id,
                    item_id: item,
                    running: true,
                    experience: None,
                },
                target: ScriptTarget::Mono,
                source: probe_source().into_bytes(),
            })
            .await?;
        // The script may speak before the capability's answer is read, so one
        // window hears both.
        let heard = listen(self.a, STEP, Duration::ZERO, |heard| {
            !heard.uploads.is_empty() && heard.said("R")
        })
        .await?;
        match (heard.uploads.first(), heard.said("R")) {
            (Some(Ok(true)), true) => Ok(cube),
            (Some(Ok(true)), false) => Err(TestFailure::Assertion(
                "the probe script never said it runs".to_owned(),
            )),
            (Some(Ok(false)), _) => Err(TestFailure::Assertion(
                "the probe script did not compile".to_owned(),
            )),
            (Some(Err(reason)), _) => Err(TestFailure::Assertion(format!(
                "saving the probe script failed: {reason}"
            ))),
            (None, _) => Err(TestFailure::Assertion(
                "saving the probe script was never answered".to_owned(),
            )),
        }
    }

    /// Asks the probe for `PERMISSION_ATTACH`, grants it when the grid asks,
    /// and records how it went under `prefix`.
    async fn grant(&mut self, metrics: &mut Metrics, prefix: &str) -> Result<(), TestFailure> {
        let (heard, ..) = self
            .order("ask", STEP, |heard| {
                !heard.questions.is_empty() || heard.said_starting("P|")
            })
            .await?;
        let request = heard.questions.first().cloned();
        metrics.set(
            &format!("{prefix}_asked"),
            request.as_ref().map_or_else(
                || "granted unasked".to_owned(),
                |request| format!("asked for {}", request.permissions.0),
            ),
        );
        let Some(request) = request else {
            return Ok(());
        };
        self.a
            .send(Command::AnswerScriptPermissions {
                task_id: request.task_id,
                item_id: request.item_id,
                permissions: ScriptPermissions(ScriptPermissions::ATTACH),
                experience_id: request.experience_id,
            })
            .await?;
        let heard = listen(self.a, STEP, Duration::ZERO, |heard| {
            heard.said_starting("P|")
        })
        .await?;
        metrics.set(
            &format!("{prefix}_granted"),
            heard
                .said
                .iter()
                .find(|line| line.starts_with("P|"))
                .cloned()
                .unwrap_or_else(|| "nothing".to_owned()),
        );
        Ok(())
    }
}

/// The word a metric names `point` by: which of the census's points it is,
/// since which points are free differs from avatar to avatar.
fn point_word(object: &Object, points: Points) -> String {
    let Some(id) = object.attachment_point_id() else {
        return "none".to_owned();
    };
    if id == points.first.to_code() {
        "the first point".to_owned()
    } else if id == points.second.to_code() {
        "the second point".to_owned()
    } else if id == points.hud.to_code() {
        "the HUD point".to_owned()
    } else {
        format!("point {id}")
    }
}

/// The probe: a script that asks for the permission to attach, attaches its
/// object and takes it off again on its owner's word, and says what it did
/// and what it was told.
fn probe_source() -> String {
    format!(
        r#"default
{{
    state_entry()
    {{
        llListen({PROBE_CHANNEL}, "", llGetOwner(), "");
        llOwnerSay("{PROBE_TAG}|R");
    }}
    listen(integer channel, string name, key id, string what)
    {{
        list parts = llParseString2List(what, [":"], []);
        string verb = llList2String(parts, 0);
        integer point = llList2Integer(parts, 1);
        llOwnerSay("{PROBE_TAG}|D|" + what);
        if (verb == "ask") llRequestPermissions(llGetOwner(), PERMISSION_ATTACH);
        else if (verb == "temp") llAttachToAvatarTemp(point);
        else if (verb == "attach") llAttachToAvatar(point);
        else if (verb == "detach") llDetachFromAvatar();
    }}
    run_time_permissions(integer granted)
    {{
        llOwnerSay("{PROBE_TAG}|P|" + (string)granted);
    }}
    attach(key id)
    {{
        llOwnerSay("{PROBE_TAG}|A|" + (string)(id != NULL_KEY) + "|" + (string)llGetAttached());
    }}
}}
"#
    )
}

/// Picks the first two body points and the first HUD point nothing of A's is
/// worn on.
fn free_points(view: &View) -> Result<Points, TestFailure> {
    let taken: HashSet<u8> = view
        .all_worn
        .values()
        .filter_map(Object::attachment_point_id)
        .collect();
    let mut body = BODY_POINTS
        .iter()
        .filter(|point| !taken.contains(&point.to_code()));
    let first = body.next().copied();
    let second = body.next().copied();
    let hud = HUD_POINTS
        .iter()
        .find(|point| !taken.contains(&point.to_code()))
        .copied();
    match (first, second, hud) {
        (Some(first), Some(second), Some(hud)) => Ok(Points { first, second, hud }),
        _ => Err(TestFailure::State(
            "A wears something on every point the census may use".to_owned(),
        )),
    }
}

/// The census: both avatars settled beside each other, then every leg.
#[expect(
    clippy::too_many_lines,
    reason = "one linear flow of legs, each a request and a window to hear its answer in"
)]
async fn census(
    ctx: &mut TestContext,
    folders: Folders,
    metrics: &mut Metrics,
    left: &mut Leftovers,
) -> Result<(), TestFailure> {
    let grid = ctx.grid();
    let build_position = ctx.build_position();
    let elsewhere = ctx.ban_line_regions().to_vec();
    let pace = Pace::of(grid);
    let Some((a, b)) = ctx.primary_and_secondary() else {
        return Err(TestFailure::State(
            "attachments needs a secondary avatar".to_owned(),
        ));
    };
    let owner = a
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;
    let actor = owner.uuid();
    let benefits_limit = a
        .login_account()
        .and_then(|account| account.benefits.as_ref())
        .map(|benefits| benefits.attachment_limit);
    a.wait_for_region(REGION_TIMEOUT).await?;
    b.wait_for_region(REGION_TIMEOUT).await?;
    a.send(Command::RequestSimulatorFeatures).await?;
    let features_limit = a
        .wait_for(STEP, |event| match event {
            Event::SimulatorFeatures(features) => Some(features.max_agent_attachments),
            _ => None,
        })
        .await?;

    // --- 1. The arrival. ---
    let mut arrival: Vec<Object> = Vec::new();
    let settled = crate::support::settle_scene_observing(
        a,
        grid,
        build_position.clone(),
        SETTLE_WINDOW,
        pace.settle_idle,
        |event| {
            if let Event::ObjectAdded(object) | Event::ObjectUpdated(object) = event {
                arrival.push((**object).clone());
            }
        },
    )
    .await?;
    let mut their_arrival: Vec<Object> = Vec::new();
    let _theirs = crate::support::settle_scene_observing(
        b,
        grid,
        build_position.clone(),
        SETTLE_WINDOW,
        pace.settle_idle,
        |event| {
            if let Event::ObjectAdded(object) | Event::ObjectUpdated(object) = event {
                their_arrival.push((**object).clone());
            }
        },
    )
    .await?;
    let mut roles = Roles::default();
    let mut own = View::default();
    let _change = fold(
        &mut own,
        &mut roles,
        actor,
        &Heard {
            sightings: arrival
                .into_iter()
                .map(|object| Sighting::Object(Box::new(object), true))
                .collect(),
            ..Heard::default()
        },
    );
    let mut seen = View::default();
    let _change = fold(
        &mut seen,
        &mut roles,
        actor,
        &Heard {
            sightings: their_arrival
                .into_iter()
                .map(|object| Sighting::Object(Box::new(object), true))
                .collect(),
            ..Heard::default()
        },
    );
    let already = own.all_worn.len();
    metrics.set("arrival_worn", i64::try_from(already).unwrap_or(-1));
    metrics.set(
        "arrival_b_shown_all_but_huds",
        seen.all_worn.len() == own.all_worn.values().filter(|worn| !is_hud(worn)).count(),
    );
    metrics.set(
        "limit_benefits",
        benefits_limit.map_or_else(|| "absent".to_owned(), |limit| limit.to_string()),
    );
    metrics.set(
        "limit_features",
        features_limit.map_or_else(|| "absent".to_owned(), |limit| limit.to_string()),
    );
    let points = free_points(&own)?;
    // The cubes stand beside A, within earshot of what A tells the probe.
    let anchor = settled
        .avatar
        .clone()
        .or_else(|| settled.anchor.clone())
        .ok_or_else(|| {
            TestFailure::Assertion(
                "the arrival streamed nothing to place a cube against".to_owned(),
            )
        })?;
    let mut names: Vec<(String, &'static str)> = Vec::new();
    for name in names_of(&a.avatar_legacy_name()) {
        names.push((name, "<A>"));
    }
    for name in names_of(&b.avatar_legacy_name()) {
        names.push((name, "<B>"));
    }
    names.sort_by_key(|(name, _word)| std::cmp::Reverse(name.len()));
    let mut run = Run {
        folders,
        owner,
        pace,
        position: Vector {
            x: anchor.x + CUBE_AWAY_M,
            y: anchor.y,
            z: anchor.z + REZ_LIFT_M,
        },
        seen: settled.seen,
    };
    left.home = true;
    let source = rez_cube(a, &mut run, &mut left.residue, 0).await?;
    a.send(Command::SetObjectName {
        local_id: source.scoped_id(),
        name: CUBE_NAME.to_owned(),
    })
    .await?;
    let mut census = Census {
        a,
        b,
        actor,
        run,
        roles,
        own,
        seen,
        points,
        names,
        source,
        adopt: None,
    };
    for role in ["cube1", "cube2", "cube3", "hud", "drop"] {
        let item = census.copy(role).await?;
        left.residue.items.push(item);
        left.worn.push(item);
    }
    let (first, second, hud) = (points.first, points.second, points.hud);

    // --- 2. Wearing. ---
    let cube1 = census.item("cube1")?;
    let rez = census.wear("cube1", first, AttachmentMode::Replace);
    let started = Instant::now();
    let (heard, theirs, change, their_change) = census
        .ask(vec![Command::RezAttachment(rez)], STEP, |heard| {
            heard.wore(cube1)
        })
        .await?;
    metrics.set_timing("wear_secs", started.elapsed().as_secs_f64());
    census.record(metrics, "wear", &heard, &theirs, &change, &their_change);
    census.describe(metrics, "wear", "cube1");

    let cube2 = census.item("cube2")?;
    let rez = census.wear("cube2", first, AttachmentMode::Add);
    let (heard, theirs, change, their_change) = census
        .ask(vec![Command::RezAttachment(rez)], STEP, |heard| {
            heard.wore(cube2)
        })
        .await?;
    census.record(metrics, "wear_add", &heard, &theirs, &change, &their_change);

    let cube3 = census.item("cube3")?;
    let rez = census.wear("cube3", first, AttachmentMode::Replace);
    let (heard, theirs, change, their_change) = census
        .ask(vec![Command::RezAttachment(rez)], STEP, |heard| {
            heard.wore(cube3)
        })
        .await?;
    census.record(
        metrics,
        "wear_replace",
        &heard,
        &theirs,
        &change,
        &their_change,
    );
    if let Some(worn) = census.own.worn.get("cube3") {
        metrics.set("wear_replace_point", point_word(worn, points));
    }

    let rez = census.wear("cube3", second, AttachmentMode::Add);
    let (heard, theirs, change, their_change) = census
        .ask(vec![Command::RezAttachment(rez)], MAYBE, |_heard| false)
        .await?;
    census.record(
        metrics,
        "wear_again",
        &heard,
        &theirs,
        &change,
        &their_change,
    );
    metrics.set(
        "wear_again_point",
        census
            .own
            .worn
            .get("cube3")
            .map_or_else(|| "none".to_owned(), |worn| point_word(worn, points)),
    );

    let rez = RezAttachment {
        item_id: InventoryKey::from(MADE_UP_ITEM),
        ..census.wear("cube1", second, AttachmentMode::Add)
    };
    let (heard, theirs, change, their_change) = census
        .ask(vec![Command::RezAttachment(rez)], MAYBE, |_heard| false)
        .await?;
    census.record(
        metrics,
        "wear_missing",
        &heard,
        &theirs,
        &change,
        &their_change,
    );
    metrics.set(
        "wear_missing_any_arrived",
        i64::try_from(change.any_arrived).unwrap_or(-1),
    );

    // --- 3. Taking off. ---
    census.strip(Some("cube3")).await?;
    let detached = census
        .own
        .worn
        .get("cube3")
        .map(|object| (object.scoped_id(), object.local_id));
    if let Some((scoped, local)) = detached {
        let (heard, theirs, change, their_change) = census
            .ask(
                vec![Command::DetachObjects {
                    local_ids: vec![scoped],
                }],
                STEP,
                |heard| heard.killed(local),
            )
            .await?;
        census.record(metrics, "detach", &heard, &theirs, &change, &their_change);
    }

    census.put_on("cube1", first).await?;
    let worn = census
        .own
        .worn
        .get("cube1")
        .map(|object| object.local_id)
        .unwrap_or_default();
    let (heard, theirs, change, their_change) = census
        .ask(
            vec![Command::DetachAttachmentIntoInventory { item_id: cube1 }],
            STEP,
            |heard| heard.killed(worn),
        )
        .await?;
    census.record(
        metrics,
        "detach_item",
        &heard,
        &theirs,
        &change,
        &their_change,
    );

    // --- 4. Several at once. ---
    let compound = vec![
        census.wear("cube1", first, AttachmentMode::Add),
        census.wear("cube2", second, AttachmentMode::Add),
    ];
    let (heard, theirs, change, their_change) = census
        .ask(
            vec![Command::RezAttachments {
                compound_id: TransactionId::from(Uuid::new_v4()),
                detach: DetachOrder::Keep,
                attachments: compound,
            }],
            STEP,
            |heard| heard.wore(cube1) && heard.wore(cube2),
        )
        .await?;
    census.record(
        metrics,
        "wear_multi",
        &heard,
        &theirs,
        &change,
        &their_change,
    );
    let both: Vec<(ScopedObjectId, RegionLocalObjectId)> = ["cube1", "cube2"]
        .iter()
        .filter_map(|role| census.own.worn.get(*role))
        .map(|object| (object.scoped_id(), object.local_id))
        .collect();
    let (heard, theirs, change, their_change) = census
        .ask(
            vec![Command::DetachObjects {
                local_ids: both.iter().map(|(scoped, _id)| *scoped).collect(),
            }],
            STEP,
            |heard| both.iter().all(|(_scoped, id)| heard.killed(*id)),
        )
        .await?;
    census.record(
        metrics,
        "detach_multi",
        &heard,
        &theirs,
        &change,
        &their_change,
    );
    census.strip(None).await?;

    // --- 5. A HUD. ---
    let hud_item = census.item("hud")?;
    let rez = census.wear("hud", hud, AttachmentMode::Add);
    let (heard, theirs, change, their_change) = census
        .ask(vec![Command::RezAttachment(rez)], STEP, |heard| {
            heard.wore(hud_item)
        })
        .await?;
    census.record(metrics, "hud", &heard, &theirs, &change, &their_change);
    census.describe(metrics, "hud", "hud");
    metrics.set(
        "hud_b_sent_the_object",
        their_change.any_arrived > 0
            || theirs
                .objects()
                .any(|object| attach_item(object) == Some(hud_item.uuid())),
    );
    if let Some(worn) = census.own.worn.get("hud").cloned() {
        let (heard, theirs, change, their_change) = census
            .ask(
                vec![Command::DetachObjects {
                    local_ids: vec![worn.scoped_id()],
                }],
                STEP,
                |heard| heard.killed(worn.local_id),
            )
            .await?;
        census.record(
            metrics,
            "hud_detach",
            &heard,
            &theirs,
            &change,
            &their_change,
        );
        metrics.set("hud_detach_b_sent_a_kill", theirs.killed(worn.local_id));
    }

    // --- 6. A drop. ---
    census.put_on("drop", first).await?;
    if let Some(worn) = census.own.worn.get("drop").cloned() {
        let (heard, theirs, change, their_change) = census
            .ask(
                vec![Command::DropAttachments {
                    local_ids: vec![worn.scoped_id()],
                }],
                STEP,
                |heard| {
                    heard
                        .objects()
                        .any(|object| object.full_id == worn.full_id && object.parent_id.0 == 0)
                },
            )
            .await?;
        census.record(metrics, "drop", &heard, &theirs, &change, &their_change);
        let dropped = heard
            .objects()
            .rfind(|object| object.parent_id.0 == 0 && object.full_id == worn.full_id)
            .cloned();
        metrics.set("drop_form", drop_form(&heard, &worn, dropped.as_ref()));
        if let Some(dropped) = dropped {
            metrics.set("drop_local_id_kept", dropped.local_id == worn.local_id);
            metrics.set(
                "drop_owner",
                if dropped.owner_id.is_nil() {
                    "nil"
                } else if dropped.owner_id == actor {
                    "A"
                } else {
                    "another"
                },
            );
            metrics.set(
                "drop_name_values",
                joined(
                    dropped
                        .name_values()
                        .into_iter()
                        .map(|pair| pair.name)
                        .collect(),
                ),
            );
            left.residue.roots.push(dropped.scoped_id());
        }
    }

    // --- 7. An object attached from the world. ---
    let world = rez_cube(census.a, &mut census.run, &mut left.residue, 1).await?;
    census
        .roles
        .objects
        .insert(world.full_id, "world".to_owned());
    census.adopt = Some("world".to_owned());
    let (heard, theirs, change, their_change) = census
        .ask(
            vec![Command::AttachObject {
                local_id: world.scoped_id(),
                attachment_point: second,
                mode: AttachmentMode::Add,
                rotation: Rotation {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    s: 1.0,
                },
            }],
            STEP,
            |heard| {
                heard
                    .objects()
                    .any(|object| object.full_id == world.full_id && object.parent_id.0 != 0)
                    || heard.objects().any(|object| attach_item(object).is_some())
            },
        )
        .await?;
    for item in heard.object_items() {
        left.residue.items.push(item.item_id);
    }
    census.record(
        metrics,
        "attach_world",
        &heard,
        &theirs,
        &change,
        &their_change,
    );
    census.describe(metrics, "attach_world", "world");
    metrics.set("attach_world_form", attach_form(&heard, &world));
    if census.own.worn.contains_key("world") || heard.killed(world.local_id) {
        left.residue.roots.retain(|root| *root != world.scoped_id());
    }
    if let Some(worn) = census.own.worn.get("world").cloned() {
        if let Some(item) = census.roles.item("world") {
            left.worn.push(item);
        }
        let (heard, theirs, change, their_change) = census
            .ask(
                vec![Command::DetachObjects {
                    local_ids: vec![worn.scoped_id()],
                }],
                STEP,
                |heard| heard.killed(worn.local_id),
            )
            .await?;
        census.record(
            metrics,
            "attach_world_detach",
            &heard,
            &theirs,
            &change,
            &their_change,
        );
    }

    // --- 8. A script. ---
    let temp = census.scripted_cube("temp", 2, &mut left.residue).await?;
    census.grant(metrics, "script_temp_permission").await?;
    let order = format!("temp:{}", second.to_code());
    let (heard, theirs, change, their_change) = census
        .order(&order, STEP, |heard| {
            heard.said_starting("A|")
                || heard
                    .objects()
                    .any(|object| object.full_id == temp.full_id && object.parent_id.0 != 0)
        })
        .await?;
    census.record(
        metrics,
        "script_temp",
        &heard,
        &theirs,
        &change,
        &their_change,
    );
    census.describe(metrics, "script_temp", "temp");
    metrics.set("script_temp_said", said_after(&heard, "D|", second));
    metrics.set("script_temp_form", attach_form(&heard, &temp));
    if census.own.worn.contains_key("temp") || heard.killed(temp.local_id) {
        left.residue.roots.retain(|root| *root != temp.scoped_id());
    }
    for item in heard.object_items() {
        left.residue.items.push(item.item_id);
    }
    if let Some(worn) = census.own.worn.get("temp").cloned() {
        let (heard, theirs, change, their_change) = census
            .order("detach", STEP, |heard| heard.killed(worn.local_id))
            .await?;
        census.record(
            metrics,
            "script_temp_detach",
            &heard,
            &theirs,
            &change,
            &their_change,
        );
        metrics.set(
            "script_temp_detach_back_in_world",
            heard
                .objects()
                .any(|object| object.full_id == temp.full_id && object.parent_id.0 == 0),
        );
        for item in heard.object_items() {
            left.residue.items.push(item.item_id);
        }
    }

    let kept = census
        .scripted_cube("scripted", 3, &mut left.residue)
        .await?;
    census.grant(metrics, "script_attach_permission").await?;
    let order = format!("attach:{}", second.to_code());
    census.adopt = Some("scripted".to_owned());
    let (heard, theirs, change, their_change) = census
        .order(&order, STEP, |heard| {
            heard.said_starting("A|")
                || heard
                    .objects()
                    .any(|object| object.full_id == kept.full_id && object.parent_id.0 != 0)
        })
        .await?;
    for item in heard.object_items() {
        left.residue.items.push(item.item_id);
        left.worn.push(item.item_id);
    }
    census.record(
        metrics,
        "script_attach",
        &heard,
        &theirs,
        &change,
        &their_change,
    );
    census.describe(metrics, "script_attach", "scripted");
    metrics.set("script_attach_said", said_after(&heard, "D|", second));
    metrics.set("script_attach_form", attach_form(&heard, &kept));
    if census.own.worn.contains_key("scripted") || heard.killed(kept.local_id) {
        left.residue.roots.retain(|root| *root != kept.scoped_id());
    }
    if let Some(worn) = census.own.worn.get("scripted").cloned() {
        let (heard, theirs, change, their_change) = census
            .order("detach", STEP, |heard| heard.killed(worn.local_id))
            .await?;
        census.record(
            metrics,
            "script_attach_detach",
            &heard,
            &theirs,
            &change,
            &their_change,
        );
        for item in heard.object_items() {
            left.residue.items.push(item.item_id);
        }
    }

    // --- 9. The limit. ---
    let limit = benefits_limit
        .or(features_limit)
        .and_then(|limit| usize::try_from(limit).ok());
    if let Some(limit) = limit {
        let worn_now = census.own.all_worn.len();
        let need = limit.saturating_sub(worn_now).saturating_add(1);
        metrics.set("limit_worn_before_is_arrival", worn_now == already);
        let mut wanted = Vec::new();
        for n in 0..need {
            let role = format!("limit{n:02}");
            let item = census.copy(&role).await?;
            left.residue.items.push(item);
            left.worn.push(item);
            wanted.push(census.wear(&role, first, AttachmentMode::Add));
        }
        let (heard, theirs, change, their_change) = census
            .ask(
                vec![Command::RezAttachments {
                    compound_id: TransactionId::from(Uuid::new_v4()),
                    detach: DetachOrder::Keep,
                    attachments: wanted,
                }],
                LIMIT_WINDOW,
                |heard| {
                    heard
                        .objects()
                        .filter(|object| attach_item(object).is_some())
                        .count()
                        >= need
                },
            )
            .await?;
        let arrived = change
            .arrived
            .iter()
            .filter(|role| role.starts_with("limit"))
            .count();
        metrics.set(
            "limit_short",
            i64::try_from(need.saturating_sub(arrived)).unwrap_or(-1),
        );
        metrics.set(
            "limit_worn_after",
            if census.own.all_worn.len() == limit {
                "the limit".to_owned()
            } else {
                format!("{} against a limit of {limit}", census.own.all_worn.len())
            },
        );
        metrics.set(
            "limit_b_short",
            i64::try_from(
                need.saturating_sub(
                    their_change
                        .arrived
                        .iter()
                        .filter(|role| role.starts_with("limit"))
                        .count(),
                ),
            )
            .unwrap_or(-1),
        );
        metrics.set("limit_alerts", census.alerts(&heard));
        metrics.set("limit_announced", census.announced(&heard));
        metrics.set("limit_b_appearance", census.appearance(&theirs));
        census.strip(None).await?;
    } else {
        metrics.set("limit_short", "no limit stated");
    }

    // The world is cleared before the agent leaves the region; the items
    // stay, since the legs still wear some.
    let standing = Residue {
        roots: std::mem::take(&mut left.residue.roots),
        items: Vec::new(),
    };
    clean_up(census.a, &standing, folders.trash).await?;

    // --- 10. A teleport. ---
    census.put_on("cube1", first).await?;
    let origin = census
        .a
        .region_handle()
        .ok_or_else(|| TestFailure::State("A is in no region".to_owned()))?;
    let target = destination(census.a, origin, &elsewhere).await?;
    let before = census.own.worn.get("cube1").cloned();
    let avatar_before = census.own.avatar;
    left.home = false;
    request_teleport(census.a, target, (128.0, 128.0, 30.0), (129.0, 128.0, 30.0)).await?;
    record_move(
        &mut census,
        metrics,
        "move",
        before.as_ref(),
        avatar_before,
        cube1,
    )
    .await?;
    let before = census.own.worn.get("cube1").cloned();
    let avatar_before = census.own.avatar;
    let back = anchor.clone();
    request_teleport(
        census.a,
        origin,
        (back.x, back.y, back.z),
        (back.x + 1.0, back.y, back.z),
    )
    .await?;
    record_move(
        &mut census,
        metrics,
        "move_back",
        before.as_ref(),
        avatar_before,
        cube1,
    )
    .await?;
    left.home = census.a.region_handle() == Some(origin);

    // --- 11. A new login. ---
    if !census.own.worn.contains_key("cube1") {
        census.put_on("cube1", first).await?;
    }
    census.a.relogin().await?;
    census.own = View::default();
    let heard = listen(census.a, STEP, census.run.pace.quiet, |heard| {
        heard.wore(cube1)
    })
    .await?;
    let change = fold(&mut census.own, &mut census.roles, actor, &heard);
    metrics.set("relogin_arrived", joined(change.arrived.clone()));
    metrics.set("relogin_worn", census.own.worn_roles());
    metrics.set(
        "relogin_any_worn",
        i64::try_from(census.own.all_worn.len()).unwrap_or(-1),
    );
    metrics.set("relogin_appearance", census.appearance(&heard));
    if let Some(worn) = census.own.worn.get("cube1") {
        metrics.set("relogin_point", point_word(worn, points));
    }
    census.strip(None).await?;
    let _theirs = listen(census.b, DRAIN, DRAIN, |_heard| false).await?;
    // The teleport and the login left A where the region puts an arrival (a
    // telehub on aditi); the next case or viewer to log in at A's last
    // location expects the build location.
    let standing = heard
        .objects()
        .rfind(|object| object.full_id.uuid() == actor)
        .map(|avatar| avatar.motion.position.clone());
    if let (Some(build), Some(from)) = (build_position, standing) {
        let _arrived =
            crate::support::walk_within_region(census.a, from, &build, |_event| {}).await?;
    }
    Ok(())
}

/// Whether `object` is worn on a HUD point, which no other viewer is shown.
fn is_hud(object: &Object) -> bool {
    object
        .attachment_point_id()
        .is_some_and(|id| (HUD_FIRST..=HUD_LAST).contains(&id))
}

/// Where the teleport leg goes: a region next to `origin` on the map, or
/// where the map shows none (Mauve, on aditi), the first of the operator's
/// nearby regions the map knows.
async fn destination(
    session: &mut Session,
    origin: RegionHandle,
    elsewhere: &[String],
) -> Result<RegionHandle, TestFailure> {
    let neighbour = neighbouring_region(session, GridCoordinates::from(origin)).await;
    let failure = match neighbour {
        Ok(target) => return Ok(RegionHandle::from(target.grid_coordinates)),
        Err(failure) => failure,
    };
    for name in elsewhere {
        if let Ok(target) = region_named(session, name).await {
            let handle = RegionHandle::from(target.grid_coordinates);
            if handle != origin {
                return Ok(handle);
            }
        }
    }
    Err(failure)
}

/// How an object attached from the world came back: under its own id, or as
/// a new object.
fn attach_form(heard: &Heard, world: &Object) -> String {
    let killed = heard.killed(world.local_id);
    let same = heard
        .objects()
        .find(|object| object.full_id == world.full_id && object.parent_id.0 != 0);
    let other = heard.objects().any(|object| {
        object.full_id != world.full_id && object.parent_id.0 != 0 && attach_item(object).is_some()
    });
    match (same, other, killed) {
        (Some(object), _, false) if object.local_id == world.local_id => {
            "sent again attached, same ids".to_owned()
        }
        (Some(object), _, true) if object.local_id == world.local_id => {
            "killed, then sent again attached, same ids".to_owned()
        }
        (Some(_object), _, false) => "sent again attached, a new local id".to_owned(),
        (Some(_object), _, true) => "killed, then sent attached under a new local id".to_owned(),
        (None, true, true) => "killed, and a new object attached".to_owned(),
        (None, true, false) => "a new object attached, the old one left".to_owned(),
        (None, false, true) => "killed, nothing attached".to_owned(),
        (None, false, false) => "nothing".to_owned(),
    }
}

/// How a dropped attachment came down.
fn drop_form(heard: &Heard, worn: &Object, dropped: Option<&Object>) -> String {
    let killed = heard.killed(worn.local_id);
    match (dropped, killed) {
        (Some(object), false) if object.local_id == worn.local_id => {
            "sent again unattached, same ids".to_owned()
        }
        (Some(object), true) if object.local_id == worn.local_id => {
            "killed, then sent again unattached, same ids".to_owned()
        }
        (Some(_object), false) => "sent again unattached, a new local id".to_owned(),
        (Some(_object), true) => "killed, then sent unattached under a new local id".to_owned(),
        (None, true) => "killed, nothing in the world".to_owned(),
        (None, false) => "nothing".to_owned(),
    }
}

/// What the probe said but its `prefix` line, `|`-joined, the point it
/// was attached to named as the one asked for.
fn said_after(heard: &Heard, prefix: &str, asked: AttachmentPoint) -> String {
    let asked = format!("|{}", asked.to_code());
    let lines: Vec<String> = heard
        .said
        .iter()
        .filter(|line| !line.starts_with(prefix))
        .map(|line| {
            line.strip_suffix(asked.as_str())
                .map_or_else(|| line.clone(), |head| format!("{head}|the point asked"))
        })
        .collect();
    if lines.is_empty() {
        "nothing".to_owned()
    } else {
        lines.join(" | ")
    }
}

/// Listens to a teleport A asked for and records what became of `before`, the
/// attachment worn from `item`, under `prefix`.
async fn record_move(
    census: &mut Census<'_>,
    metrics: &mut Metrics,
    prefix: &str,
    before: Option<&Object>,
    avatar_before: Option<RegionLocalObjectId>,
    item: InventoryKey,
) -> Result<(), TestFailure> {
    let heard = listen(census.a, TELEPORT, census.run.pace.quiet, |heard| {
        heard.teleport_failed.is_some() || (heard.arrived_elsewhere && heard.wore(item))
    })
    .await?;
    let theirs = listen(census.b, DRAIN, DRAIN, |_heard| false).await?;
    let change = fold(&mut census.own, &mut census.roles, census.actor, &heard);
    let their_change = fold(&mut census.seen, &mut census.roles, census.actor, &theirs);
    if let Some(reason) = &heard.teleport_failed {
        metrics.set(
            &format!("{prefix}_teleport"),
            format!("refused: {}", census.scrub(reason)),
        );
        return Ok(());
    }
    metrics.set(
        &format!("{prefix}_teleport"),
        if heard.arrived_elsewhere {
            "arrived"
        } else {
            "never arrived"
        },
    );
    census.record(metrics, prefix, &heard, &theirs, &change, &their_change);
    if let Some(before) = before {
        metrics.set(
            &format!("{prefix}_old_region_killed_it_first"),
            heard.kills_before_move.contains(&before.local_id),
        );
        metrics.set(
            &format!("{prefix}_old_region_killed_the_avatar_first"),
            avatar_before.is_some_and(|avatar| heard.kills_before_move.contains(&avatar)),
        );
        metrics.set(
            &format!("{prefix}_b_killed_the_avatar"),
            avatar_before.is_some_and(|avatar| theirs.killed(avatar)),
        );
        if let Some(after) = census.own.worn.get("cube1") {
            metrics.set(
                &format!("{prefix}_same_full_id"),
                after.full_id == before.full_id,
            );
            metrics.set(
                &format!("{prefix}_same_local_id"),
                after.local_id == before.local_id,
            );
        }
    }
    census.describe(metrics, prefix, "cube1");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        Heard, Points, Roles, Sighting, View, attach_form, drop_form, fold, free_points,
        probe_source,
    };
    use pretty_assertions::assert_eq;
    use sl_client_tokio::{
        AttachmentPoint, CircuitId, InventoryKey, Object, ObjectExtraParams, ObjectKey,
        ObjectMotion, PrimShapeParams, RegionHandle, RegionLocalObjectId, Rotation, Uuid, Vector,
        pcode,
    };

    #[test]
    fn the_probe_compiles() -> Result<(), String> {
        let source = probe_source();
        sl_lsl_runtime::compile(&source)
            .map(|_program| ())
            .map_err(|errors| format!("the probe does not compile: {errors:?}\n{source}"))
    }

    /// A vector of zeroes.
    const fn zero() -> Vector {
        Vector {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }
    }

    /// An object of `local_id` and `full_id` under `parent`, worn on `point`
    /// from `item` where one is given.
    fn object(
        local_id: u32,
        full_id: u128,
        parent: u32,
        point: Option<AttachmentPoint>,
        item: Option<Uuid>,
    ) -> Object {
        Object {
            region_handle: RegionHandle(0),
            local_id: RegionLocalObjectId(local_id),
            circuit: CircuitId::default(),
            full_id: ObjectKey::from(Uuid::from_u128(full_id)),
            parent_id: RegionLocalObjectId(parent),
            pcode: if full_id == ACTOR {
                pcode::AVATAR
            } else {
                pcode::PRIMITIVE
            },
            state: point.map_or(0, |point| point.to_code().rotate_left(4)),
            crc: 0,
            material: 0,
            click_action: 0,
            update_flags: 0,
            scale: zero(),
            motion: ObjectMotion {
                position: zero(),
                velocity: zero(),
                acceleration: zero(),
                rotation: Rotation {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    s: 1.0,
                },
                angular_velocity: zero(),
                collision_plane: None,
            },
            owner_id: Uuid::nil(),
            sound: Uuid::nil(),
            gain: 0.0,
            sound_flags: 0,
            sound_radius: 0.0,
            text: String::new(),
            text_color: [0; 4],
            name_value: item.map_or_else(String::new, |item| {
                format!("AttachItemID STRING RW SV {item}")
            }),
            media_url: None,
            texture_entry: Vec::new(),
            texture_anim: Vec::new(),
            texture_animation: None,
            shape: PrimShapeParams::default(),
            particle_system: Vec::new(),
            particles: None,
            data: Vec::new(),
            extra_params: Vec::new(),
            extra: ObjectExtraParams::default(),
            properties: None,
            joint_type: 0,
            joint_pivot: zero(),
            joint_axis_or_anchor: zero(),
        }
    }

    /// A window that sent `sightings`.
    fn heard(sightings: Vec<Sighting>) -> Heard {
        Heard {
            sightings,
            ..Heard::default()
        }
    }

    const ACTOR: u128 = 0xA;

    #[test]
    fn a_worn_item_arrives_and_a_kill_takes_it_off() {
        let item = Uuid::from_u128(0x17);
        let mut roles = Roles::default();
        roles
            .items
            .insert(InventoryKey::from(item), "cube1".to_owned());
        let mut view = View::default();
        let arrival = heard(vec![
            Sighting::Object(Box::new(object(5, ACTOR, 0, None, None)), true),
            Sighting::Object(
                Box::new(object(
                    9,
                    0x99,
                    5,
                    Some(AttachmentPoint::RightPec),
                    Some(item),
                )),
                true,
            ),
        ]);
        let change = fold(&mut view, &mut roles, Uuid::from_u128(ACTOR), &arrival);
        assert_eq!(change.arrived, vec!["cube1".to_owned()]);
        assert_eq!(view.worn_roles(), "cube1");
        assert_eq!(
            roles.objects.get(&ObjectKey::from(Uuid::from_u128(0x99))),
            Some(&"cube1".to_owned())
        );
        let kill = heard(vec![Sighting::Kill(vec![RegionLocalObjectId(9)])]);
        let change = fold(&mut view, &mut roles, Uuid::from_u128(ACTOR), &kill);
        assert_eq!(change.killed, vec!["cube1".to_owned()]);
        assert_eq!(view.worn_roles(), "none");
        assert!(view.all_worn.is_empty());
    }

    #[test]
    fn an_object_sent_again_unattached_has_left() {
        let item = Uuid::from_u128(0x18);
        let mut roles = Roles::default();
        roles
            .items
            .insert(InventoryKey::from(item), "drop".to_owned());
        let mut view = View::default();
        let actor = Uuid::from_u128(ACTOR);
        let _change = fold(
            &mut view,
            &mut roles,
            actor,
            &heard(vec![
                Sighting::Object(Box::new(object(5, ACTOR, 0, None, None)), true),
                Sighting::Object(
                    Box::new(object(
                        9,
                        0x98,
                        5,
                        Some(AttachmentPoint::LeftPec),
                        Some(item),
                    )),
                    true,
                ),
            ]),
        );
        let dropped = object(9, 0x98, 0, None, None);
        let window = heard(vec![Sighting::Object(Box::new(dropped.clone()), false)]);
        let change = fold(&mut view, &mut roles, actor, &window);
        assert_eq!(change.left, vec!["drop".to_owned()]);
        assert_eq!(view.worn_roles(), "none");
        let worn = object(9, 0x98, 5, Some(AttachmentPoint::LeftPec), Some(item));
        assert_eq!(
            drop_form(&window, &worn, Some(&dropped)),
            "sent again unattached, same ids"
        );
    }

    #[test]
    fn a_region_move_forgets_the_old_regions_ids() {
        let mut roles = Roles::default();
        let mut view = View::default();
        let actor = Uuid::from_u128(ACTOR);
        let _change = fold(
            &mut view,
            &mut roles,
            actor,
            &heard(vec![
                Sighting::Object(Box::new(object(5, ACTOR, 0, None, None)), true),
                Sighting::Object(
                    Box::new(object(9, 0x97, 5, Some(AttachmentPoint::Chest), None)),
                    true,
                ),
            ]),
        );
        assert_eq!(view.all_worn.len(), 1);
        let _change = fold(&mut view, &mut roles, actor, &heard(vec![Sighting::Moved]));
        assert_eq!(view.avatar, None);
        assert!(view.all_worn.is_empty());
    }

    #[test]
    fn the_free_points_skip_what_is_worn() -> Result<(), String> {
        let mut view = View::default();
        let worn = object(9, 0x96, 5, Some(AttachmentPoint::RightPec), None);
        view.all_worn.insert(worn.local_id, worn);
        let hud = object(10, 0x95, 5, Some(AttachmentPoint::HudBottomRight), None);
        view.all_worn.insert(hud.local_id, hud);
        let Points { first, second, hud } =
            free_points(&view).map_err(|failure| failure.to_string())?;
        assert_eq!(first, AttachmentPoint::LeftPec);
        assert_eq!(second, AttachmentPoint::LeftRingFinger);
        assert_eq!(hud, AttachmentPoint::HudBottomLeft);
        Ok(())
    }

    #[test]
    fn an_object_attached_from_the_world_keeps_its_ids() {
        let world = object(12, 0x94, 0, None, None);
        let window = heard(vec![Sighting::Object(
            Box::new(object(
                12,
                0x94,
                5,
                Some(AttachmentPoint::LeftPec),
                Some(Uuid::from_u128(1)),
            )),
            false,
        )]);
        assert_eq!(
            attach_form(&window, &world),
            "sent again attached, same ids"
        );
    }
}
