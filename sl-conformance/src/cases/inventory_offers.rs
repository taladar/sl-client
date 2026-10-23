//! A census of inventory offers: what each grid does with an item or a folder
//! one avatar gives another, with its acceptance, its refusal and an offer
//! left unanswered; with an offer to an avatar that is offline; with what an
//! object gives; and with a no-copy object that has changed hands, rezzed and
//! restored to its last position by its new owner.
//!
//! Avatar **A** is the primary test account, avatar **B** the secondary. Both
//! stand at the fixtures' build location (Second Life) or the Default
//! Region's centre (OpenSim). One question a leg; every leg records:
//!
//! - **the offer** as the recipient got it (`_offer`): its dialog, who it is
//!   from, its binary bucket (the asset type, and the id it names by its role
//!   in the run: the giver's original, an id new to the run, nothing), whether
//!   it was kept for an avatar that was offline, and what its id is;
//! - **where the gift is** in the recipient's inventory when the offer
//!   arrives and after the answer (`_at_offer`, `_after`), by the id the
//!   offer names (the item fetch) and by the name (`_named_…`: every system
//!   folder of the recipient's fetched at once), and where the giver's
//!   original is (`_original_…`);
//! - **what each avatar was told** in a window after the give and after the
//!   answer (`_told_a`, `_told_b`): instant messages, alerts, the probe's
//!   lines, inventory announcements and unread event-queue messages, every id
//!   and name described by whose it is.
//!
//! The legs:
//!
//! 1. **Agent gives**: A gives B a notecard B accepts, one B declines and one
//!    B leaves unanswered; B gives A a notecard A accepts (the other way); A
//!    gives B a notecard B's next owner may not copy, which B declines; A
//!    gives B a folder holding a notecard, which B accepts.
//! 2. **Offline**: B logs out, A gives it a notecard, B comes back, reads what
//!    was kept for it and accepts.
//! 3. **Object gives**: a cube of A's holds a notecard and a probe script that
//!    gives what A tells it to on a private channel: the notecard to B (B
//!    accepts, then once more and B declines), to its owner A (A accepts),
//!    and a folder of it to B (`llGiveInventoryList`, B accepts).
//! 4. **A no-copy object**: B rezzes a cube, takes it and gives the item to A
//!    (a new prim's next owner may not copy it); A rezzes it, takes it back
//!    and restores it to its last position. Whether each step leaves the item
//!    in A's inventory, and whether the restore brings the object back.
//!
//! Everything is answered as the reference viewer's legacy accept does
//! (`FSUseLegacyInventoryAcceptMessages`): an accept names the folder by type,
//! a decline the Trash. The default reference sends nothing for an agent's
//! offer, which the unanswered leg measures. What each live grid answered is
//! stated as a [`Measured`] and written up in `book/src/gridspec/inventory.md`
//! § Giving; every run is held to it. The fake grid's two flavours are held to
//! it too but for the object gives, which they skip: the fake grid runs no
//! scripts (`FAKE_EXEMPT`).
//!
//! `2av`, live and fake grids.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::{Duration, Instant};

use sl_client_tokio::{
    AgentKey, AssetType, ChatChannel, ChatSource, ChatType, Command, DeRezDestination, Diagnostic,
    Event, FolderType, ImDialog, InstantMessage, InventoryFolderKey, InventoryItem, InventoryKey,
    InventoryOffer, InventoryType, NewInventoryItem, Object, Permissions, PrimShape, RestoreItem,
    RezScriptParams, ScopedObjectId, ScriptTarget, ScriptUploadLocation, TaskInventoryKey,
    TransactionId, Uuid, Vector, WearableType,
};
use sl_wire::Llsd;

use crate::cases::asset_http::without_ids;
use crate::cases::groups::names_of;
use crate::cases::inventory_fetch::{
    AIS, CAP_WAIT, Census, FI2, FID2, HTTP_TIMEOUT, listed_folders, uuid_of,
};
use crate::cases::inventory_mutations::save_text;
use crate::cases::land_transactions::{region_named, teleport};
use crate::cases::object_rez_derez::{Answer, REZ_LIFT_M, SETTLE_IDLE, SETTLE_WINDOW, rez_params};
use crate::cases::task_inventory::{OPENSIM_START, drop_item, hold_to_measured, listing};
use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::metrics::Metrics;
use crate::registry::{GridTest, TestFuture};
use crate::support::{
    REGION_TIMEOUT, is_opensim, settle_scene, settle_scene_observing, wait_for_own_new_object,
};

/// Where the measurements below are written down.
const SOURCE: &str = "book/src/gridspec/inventory.md § Giving (inventory-offers, 2026-10-22)";

/// How long both avatars are listened to after a step.
const WINDOW: Duration = Duration::from_secs(6);

/// [`WINDOW`] on a fake grid, which answers at once.
const FAKE_WINDOW: Duration = Duration::from_secs(2);

/// How long an offer may take to reach the recipient.
const OFFER_TIMEOUT: Duration = Duration::from_secs(30);

/// How long a cube, a script or an item may take to appear.
const STEP: Duration = Duration::from_secs(30);

/// How long what was kept for an offline avatar is listened for after it
/// comes back, before it is asked for.
const UNASKED_WINDOW: Duration = Duration::from_secs(10);

/// [`UNASKED_WINDOW`] on a fake grid.
const FAKE_UNASKED_WINDOW: Duration = Duration::from_secs(3);

/// How far apart the census's cubes stand, in metres.
const SPACING_M: f32 = 1.5;

/// The channel the probe listens to its owner on: a positive one, since
/// Second Life passes on no avatar's chat on a negative channel.
const PROBE_CHANNEL: i32 = 7_310_455;

/// What every line the probe says begins with.
const PROBE_TAG: &str = "SLCOFFER";

/// The name of the probe script in the cube.
const PROBE_NAME: &str = "SLClientOfferProbe";

/// The next-owner mask of a full-permission gift: move, copy, modify,
/// transfer.
const NEXT_OWNER_FULL: u32 = 0x0008_e000;

/// The next-owner mask of a gift its next owner may not copy: move, modify,
/// transfer.
const NEXT_OWNER_NO_COPY: u32 = 0x0008_6000;

/// What every name the census gives begins with.
const NAME_PREFIX: &str = "conf-off-";

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
/// Second Life's `reverse_` keys where A's copy is filed are `Any`: the
/// primary test account's notecards are filed in a folder Second Life no
/// longer has (AIS3 answers it `404` "Category not found"), which is the
/// account's, not the grid's. Which system folders an account has twice is
/// recorded and not held.
const MEASURED: &[(&str, Measured<Answer>)] = &[
    ("accept_by_id_after", both(Answer::Text("B's Notecard"))),
    ("accept_by_id_at_offer", both(Answer::Text("B's Notecard"))),
    (
        "accept_named_after",
        both(Answer::Text("an item in B's Notecard")),
    ),
    (
        "accept_named_at_offer",
        both(Answer::Text("an item in B's Notecard")),
    ),
    (
        "accept_offer",
        each(
            Answer::Text(
                "InventoryOffered from A as \"<A>\", bucket type 7 and B's copy, id the give's transaction",
            ),
            Answer::Text(
                "InventoryOffered from A as \"<A>\", bucket type 7 and B's copy, id B's copy",
            ),
        ),
    ),
    ("accept_original_after", both(Answer::Text("A's Notecard"))),
    (
        "accept_original_at_offer",
        both(Answer::Text("A's Notecard")),
    ),
    ("accept_readable", both(Answer::Flag(true))),
    (
        "accept_reply_told_a",
        each(
            Answer::Text("nothing"),
            Answer::Text("im InventoryAccepted from B as \"<B>\" \"\", bucket 16 bytes"),
        ),
    ),
    ("accept_reply_told_b", both(Answer::Text("nothing"))),
    ("accept_told_a", both(Answer::Text("nothing"))),
    (
        "accept_told_b",
        each(
            Answer::Text(
                "im InventoryOffered from A as \"<A>\" \"conf-off-<n>-accept\", bucket type 7 and B's copy",
            ),
            Answer::Text(
                "bulk update: [\"conf-off-<n>-accept\" type 7 in B's Notecard]; im InventoryOffered from A as \"<A>\" \"conf-off-<n>-accept\", bucket type 7 and B's copy",
            ),
        ),
    ),
    (
        "decline_by_id_after",
        each(Answer::Text("B's Notecard"), Answer::Text("B's Trash")),
    ),
    ("decline_by_id_at_offer", both(Answer::Text("B's Notecard"))),
    (
        "decline_named_after",
        each(
            Answer::Text("an item in B's Notecard"),
            Answer::Text("an item in B's Trash"),
        ),
    ),
    (
        "decline_named_at_offer",
        both(Answer::Text("an item in B's Notecard")),
    ),
    (
        "decline_offer",
        each(
            Answer::Text(
                "InventoryOffered from A as \"<A>\", bucket type 7 and B's copy, id the give's transaction",
            ),
            Answer::Text(
                "InventoryOffered from A as \"<A>\", bucket type 7 and B's copy, id B's copy",
            ),
        ),
    ),
    ("decline_original_after", both(Answer::Text("A's Notecard"))),
    (
        "decline_original_at_offer",
        both(Answer::Text("A's Notecard")),
    ),
    ("decline_readable", both(Answer::Flag(true))),
    (
        "decline_reply_told_a",
        each(
            Answer::Text("nothing"),
            Answer::Text("im InventoryDeclined from B as \"<B>\" \"\", bucket 16 bytes"),
        ),
    ),
    (
        "decline_reply_told_b",
        each(
            Answer::Text("nothing"),
            Answer::Text("item created: \"conf-off-<n>-decline\" type 7 in B's Trash"),
        ),
    ),
    ("decline_told_a", both(Answer::Text("nothing"))),
    (
        "decline_told_b",
        each(
            Answer::Text(
                "im InventoryOffered from A as \"<A>\" \"conf-off-<n>-decline\", bucket type 7 and B's copy",
            ),
            Answer::Text(
                "bulk update: [\"conf-off-<n>-decline\" type 7 in B's Notecard]; im InventoryOffered from A as \"<A>\" \"conf-off-<n>-decline\", bucket type 7 and B's copy",
            ),
        ),
    ),
    (
        "folder_by_id_after",
        both(Answer::Text("B's RootInventory")),
    ),
    (
        "folder_by_id_at_offer",
        both(Answer::Text("B's RootInventory")),
    ),
    (
        "folder_named_after",
        both(Answer::Text("a folder in B's RootInventory")),
    ),
    (
        "folder_named_at_offer",
        both(Answer::Text("a folder in B's RootInventory")),
    ),
    (
        "folder_offer",
        each(
            Answer::Text(
                "InventoryOffered from A as \"<A>\", bucket type 8 and B's copy, id the give's transaction",
            ),
            Answer::Text(
                "InventoryOffered from A as \"<A>\", bucket type 8 and B's copy, id B's copy",
            ),
        ),
    ),
    (
        "folder_original_after",
        both(Answer::Text("a folder in A's RootInventory")),
    ),
    (
        "folder_original_at_offer",
        both(Answer::Text("a folder in A's RootInventory")),
    ),
    ("folder_readable", both(Answer::Flag(true))),
    (
        "folder_reply_told_a",
        each(
            Answer::Text("nothing"),
            Answer::Text("im InventoryAccepted from B as \"<B>\" \"\", bucket 16 bytes"),
        ),
    ),
    ("folder_reply_told_b", both(Answer::Text("nothing"))),
    ("folder_told_a", both(Answer::Text("nothing"))),
    (
        "folder_told_b",
        both(Answer::Text(
            "bulk update: [folder \"conf-off-<n>-folder\" in B's RootInventory]; im InventoryOffered from A as \"<A>\" \"conf-off-<n>-folder\", bucket type 8 and B's copy",
        )),
    ),
    ("ignore_by_id_after", both(Answer::Text("B's Notecard"))),
    ("ignore_by_id_at_offer", both(Answer::Text("B's Notecard"))),
    (
        "ignore_named_after",
        both(Answer::Text("an item in B's Notecard")),
    ),
    (
        "ignore_named_at_offer",
        both(Answer::Text("an item in B's Notecard")),
    ),
    (
        "ignore_offer",
        each(
            Answer::Text(
                "InventoryOffered from A as \"<A>\", bucket type 7 and B's copy, id the give's transaction",
            ),
            Answer::Text(
                "InventoryOffered from A as \"<A>\", bucket type 7 and B's copy, id B's copy",
            ),
        ),
    ),
    ("ignore_original_after", both(Answer::Text("A's Notecard"))),
    (
        "ignore_original_at_offer",
        both(Answer::Text("A's Notecard")),
    ),
    ("ignore_readable", both(Answer::Flag(true))),
    ("ignore_reply_told_a", both(Answer::Text("nothing"))),
    ("ignore_reply_told_b", both(Answer::Text("nothing"))),
    ("ignore_told_a", both(Answer::Text("nothing"))),
    (
        "ignore_told_b",
        each(
            Answer::Text(
                "im InventoryOffered from A as \"<A>\" \"conf-off-<n>-ignore\", bucket type 7 and B's copy",
            ),
            Answer::Text(
                "bulk update: [\"conf-off-<n>-ignore\" type 7 in B's Notecard]; im InventoryOffered from A as \"<A>\" \"conf-off-<n>-ignore\", bucket type 7 and B's copy",
            ),
        ),
    ),
    (
        "nocopy_decline_by_id_after",
        each(Answer::Text("B's Notecard"), Answer::Text("B's Trash")),
    ),
    (
        "nocopy_decline_by_id_at_offer",
        both(Answer::Text("B's Notecard")),
    ),
    (
        "nocopy_decline_named_after",
        each(
            Answer::Text("an item in B's Notecard"),
            Answer::Text("an item in B's Trash"),
        ),
    ),
    (
        "nocopy_decline_named_at_offer",
        both(Answer::Text("an item in B's Notecard")),
    ),
    (
        "nocopy_decline_offer",
        each(
            Answer::Text(
                "InventoryOffered from A as \"<A>\", bucket type 7 and B's copy, id the give's transaction",
            ),
            Answer::Text(
                "InventoryOffered from A as \"<A>\", bucket type 7 and B's copy, id B's copy",
            ),
        ),
    ),
    (
        "nocopy_decline_original_after",
        both(Answer::Text("A's Notecard")),
    ),
    (
        "nocopy_decline_original_at_offer",
        both(Answer::Text("A's Notecard")),
    ),
    ("nocopy_decline_readable", both(Answer::Flag(true))),
    (
        "nocopy_decline_reply_told_a",
        each(
            Answer::Text("nothing"),
            Answer::Text("im InventoryDeclined from B as \"<B>\" \"\", bucket 16 bytes"),
        ),
    ),
    (
        "nocopy_decline_reply_told_b",
        each(
            Answer::Text("nothing"),
            Answer::Text("item created: \"conf-off-<n>-nocopy_decline\" type 7 in B's Trash"),
        ),
    ),
    ("nocopy_decline_told_a", both(Answer::Text("nothing"))),
    (
        "nocopy_decline_told_b",
        each(
            Answer::Text(
                "im InventoryOffered from A as \"<A>\" \"conf-off-<n>-nocopy_decline\", bucket type 7 and B's copy",
            ),
            Answer::Text(
                "bulk update: [\"conf-off-<n>-nocopy_decline\" type 7 in B's Notecard]; im InventoryOffered from A as \"<A>\" \"conf-off-<n>-nocopy_decline\", bucket type 7 and B's copy",
            ),
        ),
    ),
    ("nocopy_object_a_holds", both(Answer::Flag(true))),
    (
        "nocopy_object_back_owner_can_copy",
        both(Answer::Flag(false)),
    ),
    (
        "nocopy_object_by_id_after",
        both(Answer::Text("A's Object")),
    ),
    (
        "nocopy_object_by_id_at_offer",
        both(Answer::Text("A's Object")),
    ),
    (
        "nocopy_object_item_after_restore",
        each(
            Answer::Text("not in its inventory"),
            Answer::Text("A's Object"),
        ),
    ),
    (
        "nocopy_object_item_after_rez",
        both(Answer::Text("not in its inventory")),
    ),
    (
        "nocopy_object_named_after",
        both(Answer::Text("an item in A's Object")),
    ),
    (
        "nocopy_object_named_at_offer",
        both(Answer::Text("an item in A's Object")),
    ),
    (
        "nocopy_object_offer",
        each(
            Answer::Text(
                "InventoryOffered from B as \"<B>\", bucket type 6 and A's copy, id the give's transaction",
            ),
            Answer::Text(
                "InventoryOffered from B as \"<B>\", bucket type 6 and A's copy, id A's copy",
            ),
        ),
    ),
    (
        "nocopy_object_original_after",
        both(Answer::Text("B's Object")),
    ),
    (
        "nocopy_object_original_at_offer",
        both(Answer::Text("B's Object")),
    ),
    ("nocopy_object_owner_can_copy", both(Answer::Flag(false))),
    ("nocopy_object_readable", both(Answer::Flag(true))),
    ("nocopy_object_reply_told_a", both(Answer::Text("nothing"))),
    (
        "nocopy_object_reply_told_b",
        each(
            Answer::Text("nothing"),
            Answer::Text("im InventoryAccepted from A as \"<A>\" \"\", bucket 16 bytes"),
        ),
    ),
    (
        "nocopy_object_restore",
        each(Answer::Text("an object"), Answer::Text("nothing")),
    ),
    (
        "nocopy_object_restore_told_a",
        each(
            Answer::Text("unread event RemoveInventoryObjects"),
            Answer::Text("nothing"),
        ),
    ),
    ("nocopy_object_rez", both(Answer::Text("an object"))),
    ("nocopy_object_take", both(Answer::Text("a new item"))),
    (
        "nocopy_object_told_a",
        each(
            Answer::Text(
                "im InventoryOffered from B as \"<B>\" \"conf-off-<n>-object\", bucket type 6 and A's copy",
            ),
            Answer::Text(
                "bulk update: [\"conf-off-<n>-object\" type 6 in A's Object]; im InventoryOffered from B as \"<B>\" \"conf-off-<n>-object\", bucket type 6 and A's copy",
            ),
        ),
    ),
    ("nocopy_object_told_b", both(Answer::Text("nothing"))),
    (
        "object_decline_named_after",
        each(
            Answer::Text("an item in B's Notecard (1 before)"),
            Answer::Text("an item in B's Notecard, an item in B's Trash (1 before)"),
        ),
    ),
    (
        "object_decline_named_at_offer",
        each(
            Answer::Text("an item in B's Notecard (1 before)"),
            Answer::Text("an item in B's Notecard, an item in B's Notecard (1 before)"),
        ),
    ),
    (
        "object_decline_offer",
        each(
            Answer::Text(
                "TaskInventoryOffered from A as \"Object\", bucket type 7 alone, id another id, kept",
            ),
            Answer::Text(
                "TaskInventoryOffered from A as \"Object\", bucket type 7 alone, id B's copy, kept",
            ),
        ),
    ),
    (
        "object_decline_offer_message",
        each(
            Answer::Text(
                "'conf-off-<n>-in-cube'  ( http://slurl.com/secondlife/<region>/<position> )",
            ),
            Answer::Text("'conf-off-<n>-in-cube'. (Object is located at <region> <position>)"),
        ),
    ),
    ("object_decline_readable", both(Answer::Flag(true))),
    ("object_decline_reply_told_a", both(Answer::Text("nothing"))),
    (
        "object_decline_reply_told_b",
        each(
            Answer::Text("nothing"),
            Answer::Text("item created: \"conf-off-<n>-in-cube\" type 7 in B's Trash"),
        ),
    ),
    (
        "object_decline_told_a",
        both(Answer::Text("chat by A's cube \"SLCOFFER|give\"")),
    ),
    (
        "object_decline_told_b",
        each(
            Answer::Text(
                "im TaskInventoryOffered from A as \"Object\" \"'conf-off-<n>-in-cube'  ( http://slurl.com/secondlife/<region>/<position> )\", bucket type 7 alone, kept",
            ),
            Answer::Text(
                "im TaskInventoryOffered from A as \"Object\" \"'conf-off-<n>-in-cube'. (Object is located at <region> <position>)\", bucket type 7 alone, kept; item created: \"conf-off-<n>-in-cube\" type 7 in B's Notecard",
            ),
        ),
    ),
    ("object_holds_the_notecard", both(Answer::Flag(true))),
    (
        "object_list_named_after",
        both(Answer::Text("a folder in B's RootInventory (0 before)")),
    ),
    (
        "object_list_named_at_offer",
        each(
            Answer::Text("nowhere (0 before)"),
            Answer::Text("a folder in B's RootInventory (0 before)"),
        ),
    ),
    (
        "object_list_offer",
        each(
            Answer::Text(
                "TaskInventoryOffered from A as \"Object\", bucket type 8 alone, id another id",
            ),
            Answer::Text(
                "TaskInventoryOffered from A as \"Object\", bucket type 8 alone, id B's copy",
            ),
        ),
    ),
    (
        "object_list_offer_message",
        each(
            Answer::Text(
                "'conf-off-<n>-list'  ( http://slurl.com/secondlife/<region>/<position> )",
            ),
            Answer::Text("'conf-off-<n>-list'"),
        ),
    ),
    ("object_list_readable", both(Answer::Flag(true))),
    ("object_list_reply_told_a", both(Answer::Text("nothing"))),
    (
        "object_list_reply_told_b",
        each(
            Answer::Text("bulk update: [folder \"conf-off-<n>-list\" in B's RootInventory]"),
            Answer::Text(
                "bulk update: [folder \"conf-off-<n>-list\" in B's RootInventory, \"conf-off-<n>-in-cube\" type 7 in B's copy]",
            ),
        ),
    ),
    (
        "object_list_told_a",
        both(Answer::Text("chat by A's cube \"SLCOFFER|list\"")),
    ),
    (
        "object_list_told_b",
        each(
            Answer::Text(
                "im TaskInventoryOffered from A as \"Object\" \"'conf-off-<n>-list'  ( http://slurl.com/secondlife/<region>/<position> )\", bucket type 8 alone",
            ),
            Answer::Text(
                "im TaskInventoryOffered from A as \"Object\" \"'conf-off-<n>-list'\", bucket type 8 alone",
            ),
        ),
    ),
    (
        "object_named_after",
        both(Answer::Text("an item in B's Notecard (0 before)")),
    ),
    (
        "object_named_at_offer",
        each(
            Answer::Text("nowhere (0 before)"),
            Answer::Text("an item in B's Notecard (0 before)"),
        ),
    ),
    (
        "object_offer",
        each(
            Answer::Text(
                "TaskInventoryOffered from A as \"Object\", bucket type 7 alone, id another id, kept",
            ),
            Answer::Text(
                "TaskInventoryOffered from A as \"Object\", bucket type 7 alone, id B's copy, kept",
            ),
        ),
    ),
    (
        "object_offer_message",
        each(
            Answer::Text(
                "'conf-off-<n>-in-cube'  ( http://slurl.com/secondlife/<region>/<position> )",
            ),
            Answer::Text("'conf-off-<n>-in-cube'. (Object is located at <region> <position>)"),
        ),
    ),
    (
        "object_owner_named_after",
        both(Answer::Text(
            "an item in A's Notecard, an item in A's Notecard (1 before)",
        )),
    ),
    (
        "object_owner_named_at_offer",
        each(
            Answer::Text("an item in A's Notecard (1 before)"),
            Answer::Text("an item in A's Notecard, an item in A's Notecard (1 before)"),
        ),
    ),
    (
        "object_owner_offer",
        each(
            Answer::Text(
                "TaskInventoryOffered from A as \"Object\", bucket type 7 alone, id another id, kept",
            ),
            Answer::Text(
                "TaskInventoryOffered from A as \"Object\", bucket type 7 alone, id A's copy, kept",
            ),
        ),
    ),
    (
        "object_owner_offer_message",
        each(
            Answer::Text(
                "'conf-off-<n>-in-cube'  ( http://slurl.com/secondlife/<region>/<position> )",
            ),
            Answer::Text("'conf-off-<n>-in-cube'. (Object is located at <region> <position>)"),
        ),
    ),
    ("object_owner_readable", both(Answer::Flag(true))),
    (
        "object_owner_reply_told_a",
        each(
            Answer::Text("item created: \"conf-off-<n>-in-cube\" type 7 in A's Notecard"),
            Answer::Text("nothing"),
        ),
    ),
    ("object_owner_reply_told_b", both(Answer::Text("nothing"))),
    (
        "object_owner_told_a",
        each(
            Answer::Text(
                "chat by A's cube \"SLCOFFER|give\"; im TaskInventoryOffered from A as \"Object\" \"'conf-off-<n>-in-cube'  ( http://slurl.com/secondlife/<region>/<position> )\", bucket type 7 alone, kept",
            ),
            Answer::Text(
                "chat by A's cube \"SLCOFFER|give\"; im TaskInventoryOffered from A as \"Object\" \"'conf-off-<n>-in-cube'. (Object is located at <region> <position>)\", bucket type 7 alone, kept; item created: \"conf-off-<n>-in-cube\" type 7 in A's Notecard",
            ),
        ),
    ),
    ("object_owner_told_b", both(Answer::Text("nothing"))),
    ("object_readable", both(Answer::Flag(true))),
    ("object_reply_told_a", both(Answer::Text("nothing"))),
    (
        "object_reply_told_b",
        each(
            Answer::Text("item created: \"conf-off-<n>-in-cube\" type 7 in B's Notecard"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "object_told_a",
        both(Answer::Text("chat by A's cube \"SLCOFFER|give\"")),
    ),
    (
        "object_told_b",
        each(
            Answer::Text(
                "im TaskInventoryOffered from A as \"Object\" \"'conf-off-<n>-in-cube'  ( http://slurl.com/secondlife/<region>/<position> )\", bucket type 7 alone, kept",
            ),
            Answer::Text(
                "im TaskInventoryOffered from A as \"Object\" \"'conf-off-<n>-in-cube'. (Object is located at <region> <position>)\", bucket type 7 alone, kept; item created: \"conf-off-<n>-in-cube\" type 7 in B's Notecard",
            ),
        ),
    ),
    (
        "offline_by_id_after",
        each(Answer::Text("no offer"), Answer::Text("B's Notecard")),
    ),
    (
        "offline_by_id_at_offer",
        each(Answer::Text("no id"), Answer::Text("B's Notecard")),
    ),
    ("offline_delivered", both(Answer::Text("asked"))),
    (
        "offline_ims",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "im InventoryOffered from A as \"<A>\" \"conf-off-<n>-offline\", bucket type 7 and B's copy",
            ),
        ),
    ),
    (
        "offline_named_after",
        each(
            Answer::Text("no offer"),
            Answer::Text("an item in B's Notecard"),
        ),
    ),
    (
        "offline_named_at_offer",
        both(Answer::Text("an item in B's Notecard")),
    ),
    (
        "offline_offer",
        each(
            Answer::Text("no offer"),
            Answer::Text(
                "InventoryOffered from A as \"<A>\", bucket type 7 and B's copy, id B's copy",
            ),
        ),
    ),
    (
        "offline_original_after",
        each(Answer::Text("no offer"), Answer::Text("A's Notecard")),
    ),
    (
        "offline_original_at_offer",
        both(Answer::Text("A's Notecard")),
    ),
    (
        "offline_readable",
        each(Answer::Text("no offer"), Answer::Flag(true)),
    ),
    (
        "offline_reply_told_a",
        each(
            Answer::Text("no offer"),
            Answer::Text("im InventoryAccepted from B as \"<B>\" \"\", bucket 16 bytes"),
        ),
    ),
    (
        "offline_reply_told_b",
        each(Answer::Text("no offer"), Answer::Text("nothing")),
    ),
    (
        "offline_told_a",
        each(
            Answer::Text("nothing"),
            Answer::Text("alert \"User not online. Inventory has been saved\""),
        ),
    ),
    (
        "reverse_by_id_after",
        each(Answer::Any, Answer::Text("A's Notecard")),
    ),
    (
        "reverse_by_id_at_offer",
        each(Answer::Any, Answer::Text("A's Notecard")),
    ),
    (
        "reverse_named_after",
        each(Answer::Any, Answer::Text("an item in A's Notecard")),
    ),
    (
        "reverse_named_at_offer",
        each(Answer::Any, Answer::Text("an item in A's Notecard")),
    ),
    (
        "reverse_offer",
        each(
            Answer::Any,
            Answer::Text(
                "InventoryOffered from B as \"<B>\", bucket type 7 and A's copy, id A's copy",
            ),
        ),
    ),
    ("reverse_original_after", both(Answer::Text("B's Notecard"))),
    (
        "reverse_original_at_offer",
        both(Answer::Text("B's Notecard")),
    ),
    ("reverse_readable", both(Answer::Flag(true))),
    ("reverse_reply_told_a", both(Answer::Text("nothing"))),
    (
        "reverse_reply_told_b",
        each(
            Answer::Text("nothing"),
            Answer::Text("im InventoryAccepted from A as \"<A>\" \"\", bucket 16 bytes"),
        ),
    ),
    (
        "reverse_told_a",
        each(
            Answer::Any,
            Answer::Text(
                "bulk update: [\"conf-off-<n>-reverse\" type 7 in A's Notecard]; im InventoryOffered from B as \"<B>\" \"conf-off-<n>-reverse\", bucket type 7 and A's copy",
            ),
        ),
    ),
    ("reverse_told_b", both(Answer::Text("nothing"))),
];

/// The metrics a fake flavour is not held to, by prefix, each with why.
const FAKE_EXEMPT: &[(&str, &str)] = &[("object_", "the fake grid runs no scripts")];

/// Why `grid` is not held to the metric `key`, if it is a fake flavour and
/// [`FAKE_EXEMPT`] names the metric.
fn fake_exemption(grid: Grid, key: &str) -> Option<&'static str> {
    if !grid.is_fake() {
        return None;
    }
    FAKE_EXEMPT
        .iter()
        .find(|(prefix, _why)| key.starts_with(prefix))
        .map(|(_prefix, why)| *why)
}

/// The probe: gives what its owner names on [`PROBE_CHANNEL`] — `give|<key>`
/// the notecard it holds, `list|<key>|<folder>` the notecard in a folder —
/// and says each request back.
fn probe_source() -> String {
    format!(
        "default\n\
         {{\n\
         \x20   state_entry()\n\
         \x20   {{\n\
         \x20       llListen({PROBE_CHANNEL}, \"\", llGetOwner(), \"\");\n\
         \x20       llOwnerSay(\"{PROBE_TAG}|ready\");\n\
         \x20   }}\n\
         \x20   listen(integer channel, string name, key speaker, string text)\n\
         \x20   {{\n\
         \x20       list parts = llParseString2List(text, [\"|\"], []);\n\
         \x20       string verb = llList2String(parts, 0);\n\
         \x20       key target = (key)llList2String(parts, 1);\n\
         \x20       string note = llGetInventoryName(INVENTORY_NOTECARD, 0);\n\
         \x20       if (verb == \"give\")\n\
         \x20       {{\n\
         \x20           llGiveInventory(target, note);\n\
         \x20       }}\n\
         \x20       else if (verb == \"list\")\n\
         \x20       {{\n\
         \x20           llGiveInventoryList(target, llList2String(parts, 2), [note]);\n\
         \x20       }}\n\
         \x20       llOwnerSay(\"{PROBE_TAG}|\" + verb);\n\
         \x20   }}\n\
         }}\n"
    )
}

/// The census of inventory offers.
#[derive(Debug)]
pub struct InventoryOffers;

impl GridTest for InventoryOffers {
    fn name(&self) -> &'static str {
        "inventory-offers"
    }

    fn description(&self) -> &'static str {
        "Giving inventory: an item or a folder accepted, declined or left unanswered, an offer \
         to an offline avatar, what an object gives, and a no-copy object rezzed and restored"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi, Grid::FakeSl, Grid::FakeOpensim]
    }

    fn accounts(&self) -> u8 {
        2
    }

    fn start_location(&self, grid: Grid) -> &'static str {
        if is_opensim(grid) {
            OPENSIM_START
        } else {
            "last"
        }
    }

    fn rezzes_objects(&self) -> bool {
        true
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(2400)
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();
            let mut metrics = Metrics::new();
            let outcome = census(ctx, &mut metrics).await;
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

/// Which avatar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Who {
    /// The primary.
    A,
    /// The secondary.
    B,
}

impl Who {
    /// The other avatar.
    const fn other(self) -> Self {
        match self {
            Self::A => Self::B,
            Self::B => Self::A,
        }
    }

    /// The avatar's letter.
    const fn letter(self) -> &'static str {
        match self {
            Self::A => "A",
            Self::B => "B",
        }
    }
}

/// Who and what is what, for describing ids and scrubbing names.
#[derive(Debug, Clone, Default)]
struct Scene {
    /// Avatar A.
    a: Uuid,
    /// Avatar B.
    b: Uuid,
    /// The system folders of each avatar, by id, with their words.
    folders: HashMap<Uuid, String>,
    /// Each avatar's system folders, by type.
    by_type: HashMap<(bool, i8), Uuid>,
    /// Every typed folder of each avatar's, where gifts are looked for.
    searched: Vec<(bool, Uuid)>,
    /// Which avatar has more than one system folder of a type, in words.
    doubled: Vec<String>,
    /// The region's name, as messages write it.
    region: String,
    /// The ids the run made or was given, with their words.
    ids: HashMap<Uuid, String>,
    /// Every way a grid may write A's or B's name, with the word that
    /// replaces it.
    names: Vec<(String, &'static str)>,
    /// This run's tag, which every name it gives carries.
    tag: String,
}

impl Scene {
    /// The agent id of `who`.
    const fn agent(&self, who: Who) -> Uuid {
        match who {
            Who::A => self.a,
            Who::B => self.b,
        }
    }

    /// `who`'s system folder of `folder_type`, if its skeleton named one.
    fn folder_of(&self, who: Who, folder_type: FolderType) -> Option<Uuid> {
        self.by_type
            .get(&(who == Who::A, folder_type.to_code()))
            .copied()
    }

    /// What `id` is, in words: an avatar, a folder, an id the run named, or
    /// `another id`.
    fn whose(&self, id: Uuid) -> String {
        if id.is_nil() {
            "nil".to_owned()
        } else if id == self.a {
            "A".to_owned()
        } else if id == self.b {
            "B".to_owned()
        } else if let Some(word) = self.folders.get(&id).or_else(|| self.ids.get(&id)) {
            word.clone()
        } else {
            "another id".to_owned()
        }
    }

    /// `text` with every id described and every name replaced.
    fn scrub(&self, text: &str) -> String {
        let mut out = text.to_owned();
        for (id, word) in [(self.a, "<A>"), (self.b, "<B>")] {
            if !id.is_nil() {
                out = out.replace(&id.to_string(), word);
            }
        }
        for (id, word) in &self.ids {
            out = out.replace(&id.to_string(), &format!("<{word}>"));
        }
        out = without_ids(&out);
        for (name, word) in &self.names {
            if !name.is_empty() {
                out = out.replace(name.as_str(), word);
            }
        }
        if !self.tag.is_empty() {
            out = out.replace(&self.tag, "<run>");
        }
        if !self.region.is_empty() {
            out = out.replace(&self.region, "<region>");
        }
        out = without_positions(&out);
        if let Some(position) = slurl_position(&out) {
            out = out.replace(&position, "/<position>");
        }
        out
    }

    /// A name this run gives, of `word`.
    fn name(&self, word: &str) -> String {
        format!("{NAME_PREFIX}{}-{word}", self.tag)
    }
}

/// `text` with every `<x, y, z>` position written `<position>`: where a
/// message says the object stands is the run's, not the grid's.
fn without_positions(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('<') {
        let (before, from) = rest.split_at(start);
        out.push_str(before);
        let end = from
            .find('>')
            .map_or(from.len(), |end| end.saturating_add(1));
        let (inside, after) = from.split_at(end);
        let body = inside.trim_start_matches('<').trim_end_matches('>');
        let numeric = body.split(',').count() == 3
            && body
                .split(',')
                .all(|part| part.trim().parse::<f64>().is_ok());
        out.push_str(if numeric { "<position>" } else { inside });
        rest = after;
    }
    out.push_str(rest);
    out
}

/// The `/x/y/z` a SLurl of the region states after `<region>`, if one does.
fn slurl_position(text: &str) -> Option<String> {
    const AT: &str = "<region>";
    let start = text.find(AT)?;
    let after = text
        .get(start.saturating_add(AT.len())..)
        .unwrap_or_default();
    let digits = after
        .find(|character: char| {
            !(character.is_ascii_digit()
                || character == '/'
                || character == '.'
                || character == '-')
        })
        .map_or(after, |end| after.get(..end).unwrap_or_default());
    let numbers = digits.split('/').filter(|part| !part.is_empty()).count();
    (numbers == 3 && digits.starts_with('/')).then(|| digits.trim_end_matches('/').to_owned())
}

/// An offer's binary bucket in words.
fn bucket_text(scene: &Scene, bucket: &[u8]) -> String {
    match bucket {
        [] => "empty".to_owned(),
        [kind] => format!("type {kind} alone"),
        [kind, id @ ..] if id.len() == 16 => {
            let id = <[u8; 16]>::try_from(id).map_or_else(|_err| Uuid::nil(), Uuid::from_bytes);
            format!("type {kind} and {}", scene.whose(id))
        }
        other => format!("{} bytes", other.len()),
    }
}

/// What one session was sent during a window.
#[derive(Debug, Default)]
struct Got {
    /// Every event the census reads, in arrival order.
    events: Vec<Event>,
    /// The instant messages, whole.
    ims: Vec<InstantMessage>,
    /// The event-queue messages the client did not read.
    unread: Vec<String>,
}

impl Got {
    /// Adds what `other` holds.
    fn merge(&mut self, other: Self) {
        self.events.extend(other.events);
        self.ims.extend(other.ims);
        self.unread.extend(other.unread);
    }
}

/// Folds one event into `got`; `None` for an event the census does not read.
fn fold(scene: &Scene, got: &mut Got, event: &Event) -> Option<()> {
    line(scene, event)?;
    if let Event::InstantMessageReceived(im) = event {
        got.ims.push((**im).clone());
    }
    got.events.push(event.clone());
    Some(())
}

/// One event in words; `None` for an event the census does not read.
fn line(scene: &Scene, event: &Event) -> Option<String> {
    Some(match event {
        Event::InstantMessageReceived(im) => {
            if matches!(im.dialog, ImDialog::TypingStart | ImDialog::TypingStop) {
                return None;
            }
            format!(
                "im {:?} from {} as \"{}\" \"{}\", bucket {}{}",
                im.dialog,
                scene.whose(im.from_agent_id.uuid()),
                scene.scrub(&im.from_agent_name),
                scene.scrub(&im.message),
                bucket_text(scene, &im.binary_bucket),
                if im.offline { ", kept" } else { "" }
            )
        }
        Event::AlertMessage { message, .. } => format!("alert \"{}\"", scene.scrub(message)),
        Event::AgentAlertMessage { message, .. } => {
            format!("agent alert \"{}\"", scene.scrub(message))
        }
        Event::ChatReceived(chat) if !chat.message.is_empty() => format!(
            "chat by {} \"{}\"",
            match &chat.source {
                ChatSource::System => "the system".to_owned(),
                ChatSource::Agent(agent) => scene.whose(agent.uuid()),
                ChatSource::Object(object) => scene.whose(object.uuid()),
                ChatSource::Unknown { source_type, .. } => format!("source {source_type}"),
            },
            scene.scrub(&chat.message)
        ),
        Event::InventoryItemCreated { item, .. } => {
            format!("item created: {}", item_text(scene, item))
        }
        Event::InventoryBulkUpdate { folders, items, .. } => {
            let mut parts: Vec<String> = folders
                .iter()
                .map(|folder| {
                    format!(
                        "folder \"{}\" in {}",
                        scene.scrub(&folder.name),
                        folder
                            .parent_id
                            .map_or_else(|| "none".to_owned(), |parent| scene.whose(parent.uuid()))
                    )
                })
                .collect();
            parts.extend(items.iter().map(|item| item_text(scene, item)));
            format!("bulk update: [{}]", parts.join(", "))
        }
        Event::InventoryItemsRemoved { items } => format!(
            "items removed: [{}]",
            items
                .iter()
                .map(|item| scene.whose(item.uuid()))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        _ => return None,
    })
}

/// An inventory item in words: its name, type and folder.
fn item_text(scene: &Scene, item: &InventoryItem) -> String {
    format!(
        "\"{}\" type {} in {}",
        scene.scrub(&item.name),
        item.item_type,
        scene.whose(item.folder_id.uuid())
    )
}

/// Listens on `session` for `window`, or until `until` says so.
async fn watch(
    scene: &Scene,
    session: &mut Session,
    window: Duration,
    until: impl Fn(&Got) -> bool + Send + Sync,
) -> Result<Got, TestFailure> {
    let mut got = Got::default();
    let outcome = session
        .wait_for(window, |event| {
            fold(scene, &mut got, event)?;
            until(&got).then_some(())
        })
        .await;
    match outcome {
        Ok(()) | Err(TestFailure::Timeout(_)) => Ok(got),
        Err(other) => Err(other),
    }
}

/// The names of the event-queue messages `session` was sent and did not read
/// since it had seen `seen` diagnostics, and the new count.
fn unread_since(session: &Session, seen: usize) -> (Vec<String>, usize) {
    let all = session.diagnostics();
    let names = all
        .iter()
        .skip(seen)
        .filter_map(|diagnostic| match diagnostic {
            Diagnostic::UnknownCapsEvent { message } => Some(message.clone()),
            _ => None,
        })
        .collect();
    (names, all.len())
}

/// What a window held, in words, sorted; `nothing` for an empty one.
fn told_text(scene: &Scene, got: &Got) -> String {
    let mut parts: Vec<String> = got
        .events
        .iter()
        .filter_map(|event| line(scene, event))
        .collect();
    parts.extend(got.unread.iter().map(|name| format!("unread event {name}")));
    parts.sort();
    if parts.is_empty() {
        "nothing".to_owned()
    } else {
        parts.join("; ")
    }
}

/// What is given.
#[derive(Debug, Clone)]
struct Given {
    /// The item's or the folder's id, in the giver's inventory.
    id: Uuid,
    /// The asset type ([`AssetType::Folder`] for a folder).
    asset_type: AssetType,
    /// Its name.
    name: String,
}

/// How the recipient answers an offer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reply {
    /// Accepts it into the folder of its type.
    Accept,
    /// Declines it, naming the Trash.
    Decline,
    /// Says nothing.
    Ignore,
}

/// The census's state.
struct Offers<'s> {
    /// Avatar A.
    a: &'s mut Session,
    /// Avatar B.
    b: &'s mut Session,
    /// Who and what is what.
    scene: Scene,
    /// The HTTP client of the fetches.
    http: reqwest::Client,
    /// How many diagnostics each session had shown when last looked at.
    seen: (usize, usize),
    /// The objects each avatar saw before the census rezzed any.
    objects_seen: (HashSet<ScopedObjectId>, HashSet<ScopedObjectId>),
    /// Where the first cube goes.
    cube_at: Vector,
    /// How many cubes have been rezzed.
    rezzed: u8,
    /// The objects still standing, by whose they are.
    standing: Vec<(Who, ScopedObjectId)>,
    /// The grid.
    grid: Grid,
    /// Where the fixtures say to build.
    build_point: Option<Vector>,
    /// How long both avatars are listened to after a step.
    window: Duration,
    /// How long what was kept for an offline avatar is listened for before
    /// it is asked for.
    unasked: Duration,
}

impl Offers<'_> {
    /// The session of `who`.
    const fn session(&mut self, who: Who) -> &mut Session {
        match who {
            Who::A => self.a,
            Who::B => self.b,
        }
    }

    /// A fetch handle for `who`.
    fn fetcher(&mut self, who: Who) -> Result<Census<'_>, TestFailure> {
        let http = self.http.clone();
        let session = self.session(who);
        let agent = session
            .agent_id()
            .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?
            .uuid();
        let session_id = session
            .session_id()
            .ok_or_else(|| TestFailure::Assertion("login reported no session id".to_owned()))?;
        Ok(Census {
            a: session,
            agent,
            session_id,
            http,
            tag: String::new(),
        })
    }

    /// The next spot along the row of cubes.
    fn next_spot(&mut self) -> Vector {
        let at = Vector {
            x: f32::from(self.rezzed).mul_add(SPACING_M, self.cube_at.x),
            y: self.cube_at.y,
            z: self.cube_at.z,
        };
        self.rezzed = self.rezzed.saturating_add(1);
        at
    }

    /// Listens to both avatars for `window`, or until `until` says the
    /// window of `who` is complete; what each was told.
    async fn hear(
        &mut self,
        window: Duration,
        who: Who,
        until: impl Fn(&Got) -> bool + Send + Sync,
    ) -> Result<(Got, Got), TestFailure> {
        let scene = self.scene.clone();
        let (mut got_a, mut got_b) = match who {
            Who::A => {
                let got_a = watch(&scene, self.a, window, &until).await?;
                let got_b = watch(&scene, self.b, Duration::from_millis(500), |_got| false).await?;
                (got_a, got_b)
            }
            Who::B => {
                let got_b = watch(&scene, self.b, window, &until).await?;
                let got_a = watch(&scene, self.a, Duration::from_millis(500), |_got| false).await?;
                (got_a, got_b)
            }
        };
        let (unread_a, seen_a) = unread_since(self.a, self.seen.0);
        let (unread_b, seen_b) = unread_since(self.b, self.seen.1);
        self.seen = (seen_a, seen_b);
        got_a.unread = unread_a;
        got_b.unread = unread_b;
        Ok((got_a, got_b))
    }

    /// Names every id `who` holds under `name` that the run has not named,
    /// as that avatar's copy.
    async fn name_copies(&mut self, who: Who, name: &str) -> Result<(), TestFailure> {
        for (id, _parent, _folder) in self.named_ids(who, name).await? {
            let _known = self
                .scene
                .ids
                .entry(id)
                .or_insert_with(|| format!("{}'s copy", who.letter()));
        }
        Ok(())
    }

    /// Lets both avatars hear out what the grid still had to say.
    async fn settle(&mut self) -> Result<(), TestFailure> {
        let scene = self.scene.clone();
        let window = self.window;
        let (got_a, got_b) = tokio::join!(
            watch(&scene, self.a, window, |_got| false),
            watch(&scene, self.b, window, |_got| false)
        );
        let (_got_a, _got_b) = (got_a?, got_b?);
        let (_unread, seen_a) = unread_since(self.a, self.seen.0);
        let (_unread, seen_b) = unread_since(self.b, self.seen.1);
        self.seen = (seen_a, seen_b);
        Ok(())
    }

    /// Makes a notecard named `word` for `who` in its Notecards, with a body,
    /// whose next owner gets `next_owner_mask`; the item, named `role`.
    async fn notecard(
        &mut self,
        who: Who,
        word: &str,
        next_owner_mask: u32,
        role: &str,
    ) -> Result<InventoryItem, TestFailure> {
        let folder = self
            .scene
            .folder_of(who, FolderType::Notecard)
            .ok_or_else(|| TestFailure::Assertion("the skeleton named no Notecards".to_owned()))?;
        self.make_item(who, folder, word, next_owner_mask, role)
            .await
    }

    /// Makes a notecard named `word` for `who` in `folder`, with a body, whose
    /// next owner gets `next_owner_mask`; the item, named `role`.
    async fn make_item(
        &mut self,
        who: Who,
        folder: Uuid,
        word: &str,
        next_owner_mask: u32,
        role: &str,
    ) -> Result<InventoryItem, TestFailure> {
        let name = self.scene.name(word);
        let session = self.session(who);
        session
            .send(Command::CreateInventoryItem(NewInventoryItem {
                folder_id: InventoryFolderKey::from(folder),
                transaction_id: Uuid::nil(),
                next_owner_mask,
                asset_type: AssetType::Notecard,
                inv_type: InventoryType::Notecard,
                wearable_type: WearableType::Shape,
                name: name.clone(),
                description: "sl-conformance inventory-offers".to_owned(),
            }))
            .await?;
        let wanted = name.clone();
        let item = session
            .wait_for(HTTP_TIMEOUT, |event| match event {
                Event::InventoryItemCreated { item, .. } if item.name == wanted => {
                    Some(item.clone())
                }
                _ => None,
            })
            .await?;
        // A notecard made on Second Life names no asset until a body is
        // saved, and a gift is one that has one.
        let saved = save_text(session, item.item_id.uuid()).await?;
        if saved != "saved" {
            return Err(TestFailure::Assertion(format!(
                "saving the body of {word} answered {saved}"
            )));
        }
        let _previous = self.scene.ids.insert(item.item_id.uuid(), role.to_owned());
        self.settle().await?;
        Ok(item)
    }

    /// Where the item `id` is in `who`'s inventory, by the item fetch.
    async fn item_at(&mut self, who: Who, id: Uuid) -> Result<String, TestFailure> {
        let owner = self.scene.agent(who);
        let census = self.fetcher(who)?;
        let raw = census.fetch_item(FI2, owner, id).await;
        let parent = raw
            .body
            .as_ref()
            .and_then(|body| body.get("items"))
            .and_then(Llsd::as_array)
            .and_then(<[Llsd]>::first)
            .and_then(|item| item.get("parent_id"))
            .and_then(uuid_of);
        let Some(parent) = parent else {
            return Ok("not in its inventory".to_owned());
        };
        if !self.scene.folders.contains_key(&parent) {
            let word = self.unlisted_folder(who, parent).await?;
            let _previous = self.scene.folders.insert(parent, word);
        }
        Ok(self.scene.whose(parent))
    }

    /// A folder of `who`'s its login skeleton did not list, in words, as
    /// AIS3 states it: its type, its name where it is a type's, and where
    /// it is.
    async fn unlisted_folder(&mut self, who: Who, folder: Uuid) -> Result<String, TestFailure> {
        let census = self.fetcher(who)?;
        if census.a.cap(AIS).is_none() {
            return Ok(format!(
                "a folder of {}'s outside its skeleton",
                who.letter()
            ));
        }
        let raw = census.get(AIS, &format!("/category/{folder}")).await;
        let fetched = census.fetch_folders(FID2, census.agent, &[folder]).await;
        let listed = listed_folders(fetched.body.as_ref())
            .next()
            .and_then(|entry| entry.get("version"))
            .and_then(Llsd::as_i32)
            .map_or_else(
                || {
                    format!(
                        "the folder fetch lists none ({})",
                        fetched.summary("refused")
                    )
                },
                |version| format!("the folder fetch lists it at version {version}"),
            );
        let Some(body) = raw.body.as_ref().filter(|body| body.get("name").is_some()) else {
            return Ok(format!(
                "a folder of {}'s outside its skeleton, which AIS3 answers {}; {listed}",
                who.letter(),
                self.scene.scrub(&raw.summary("refused"))
            ));
        };
        let kind = body
            .get("type_default")
            .and_then(Llsd::as_i32)
            .and_then(|code| i8::try_from(code).ok())
            .map_or_else(
                || "no type".to_owned(),
                |code| format!("{:?}", FolderType::from_code(code)),
            );
        let name = body
            .get("name")
            .and_then(Llsd::as_str)
            .unwrap_or_default()
            .to_owned();
        let parent = body.get("parent_id").and_then(uuid_of);
        Ok(format!(
            "a folder of {}'s outside its skeleton, of type {kind}, named \"{}\", in {}; {listed}",
            who.letter(),
            self.scene.scrub(&name),
            parent.map_or_else(|| "nothing".to_owned(), |parent| self.scene.whose(parent))
        ))
    }

    /// Every item and folder named `name` in `who`'s system folders, with
    /// the folder it is in.
    async fn named_ids(
        &mut self,
        who: Who,
        name: &str,
    ) -> Result<Vec<(Uuid, Uuid, bool)>, TestFailure> {
        let owner = self.scene.agent(who);
        let folders: Vec<Uuid> = self
            .scene
            .searched
            .iter()
            .filter(|(is_a, _id)| *is_a == (who == Who::A))
            .map(|(_is_a, id)| *id)
            .collect();
        let census = self.fetcher(who)?;
        let raw = census.fetch_folders(FID2, owner, &folders).await;
        let mut found = Vec::new();
        for entry in listed_folders(raw.body.as_ref()) {
            let Some(parent) = entry.get("folder_id").and_then(uuid_of) else {
                continue;
            };
            for item in entry
                .get("items")
                .and_then(Llsd::as_array)
                .unwrap_or_default()
            {
                if item.get("name").and_then(Llsd::as_str) == Some(name)
                    && let Some(id) = item.get("item_id").and_then(uuid_of)
                {
                    found.push((id, parent, false));
                }
            }
            for category in entry
                .get("categories")
                .and_then(Llsd::as_array)
                .unwrap_or_default()
            {
                if category.get("name").and_then(Llsd::as_str) == Some(name)
                    && let Some(id) = category
                        .get("category_id")
                        .or_else(|| category.get("folder_id"))
                        .and_then(uuid_of)
                {
                    found.push((id, parent, true));
                }
            }
        }
        Ok(found)
    }

    /// Where everything named `name` is in `who`'s system folders, in words;
    /// `nowhere` for nothing.
    async fn named(&mut self, who: Who, name: &str) -> Result<String, TestFailure> {
        let found = self.named_ids(who, name).await?;
        let mut words: Vec<String> = found
            .iter()
            .map(|(_id, parent, folder)| {
                format!(
                    "{} in {}",
                    if *folder { "a folder" } else { "an item" },
                    self.scene.whose(*parent)
                )
            })
            .collect();
        words.sort();
        Ok(if words.is_empty() {
            "nowhere".to_owned()
        } else {
            words.join(", ")
        })
    }

    /// Polls until `who` holds something named `name`, or `timeout` passes;
    /// where it is then.
    async fn named_settle(
        &mut self,
        who: Who,
        name: &str,
        timeout: Duration,
    ) -> Result<String, TestFailure> {
        let started = Instant::now();
        loop {
            let now = self.named(who, name).await?;
            if now != "nowhere" || started.elapsed() > timeout {
                return Ok(now);
            }
            let _idle = self
                .session(who)
                .wait_for(Duration::from_millis(750), |_event| None::<()>)
                .await;
        }
    }

    /// The offer `who` received for `name` in `got`, if one came.
    fn offer_in(got: &Got, name: &str) -> Option<InstantMessage> {
        got.ims
            .iter()
            .find(|im| {
                matches!(
                    im.dialog,
                    ImDialog::InventoryOffered | ImDialog::TaskInventoryOffered
                ) && im.message.contains(name)
            })
            .cloned()
    }

    /// Waits on `who` for the offer of `name`, keeping what else arrived;
    /// both windows' lines.
    async fn await_offer(
        &mut self,
        who: Who,
        name: &str,
    ) -> Result<(Option<InstantMessage>, Got, Got), TestFailure> {
        let wanted = name.to_owned();
        let (mut got_a, mut got_b) = self
            .hear(OFFER_TIMEOUT, who, move |got| {
                Self::offer_in(got, &wanted).is_some()
            })
            .await?;
        // What follows the offer belongs to the give too.
        let (late_a, late_b) = self.hear(self.window, who, |_got| false).await?;
        got_a.merge(late_a);
        got_b.merge(late_b);
        let offer = Self::offer_in(if who == Who::A { &got_a } else { &got_b }, name);
        Ok((offer, got_a, got_b))
    }

    /// The offer in words.
    fn offer_text(&self, im: &InstantMessage, transaction: Option<Uuid>) -> String {
        let id = if transaction == Some(im.id) {
            "the give's transaction".to_owned()
        } else {
            self.scene.whose(im.id)
        };
        format!(
            "{:?} from {} as \"{}\", bucket {}, id {id}{}",
            im.dialog,
            self.scene.whose(im.from_agent_id.uuid()),
            self.scene.scrub(&im.from_agent_name),
            bucket_text(&self.scene, &im.binary_bucket),
            if im.offline { ", kept" } else { "" }
        )
    }

    /// Answers `offer` as `who`, by `reply`.
    async fn answer(
        &mut self,
        who: Who,
        offer: InventoryOffer,
        reply: Reply,
    ) -> Result<(), TestFailure> {
        let folder_type = match offer.asset_type {
            AssetType::Folder => FolderType::RootInventory,
            AssetType::Object => FolderType::Object,
            _ => FolderType::Notecard,
        };
        let command = match reply {
            Reply::Accept => Command::AcceptInventoryOffer {
                offer,
                folder_id: InventoryFolderKey::from(
                    self.scene
                        .folder_of(who, folder_type)
                        .or_else(|| self.scene.folder_of(who, FolderType::RootInventory))
                        .unwrap_or_default(),
                ),
            },
            Reply::Decline => Command::DeclineInventoryOffer {
                offer,
                trash_folder_id: InventoryFolderKey::from(
                    self.scene
                        .folder_of(who, FolderType::Trash)
                        .unwrap_or_default(),
                ),
            },
            Reply::Ignore => return Ok(()),
        };
        self.session(who).send(command).await
    }

    /// One give from `giver` to the other avatar, answered by `reply`,
    /// recorded under `key`; the offer, if one came.
    async fn give(
        &mut self,
        metrics: &mut Metrics,
        key: &str,
        giver: Who,
        given: &Given,
        reply: Reply,
    ) -> Result<Option<InstantMessage>, TestFailure> {
        let receiver = giver.other();
        let to = AgentKey::from(self.scene.agent(receiver));
        let transaction = TransactionId::from(Uuid::new_v4());
        let command = if given.asset_type == AssetType::Folder {
            Command::GiveInventoryFolder {
                to_agent_id: to,
                folder_id: InventoryFolderKey::from(given.id),
                folder_name: given.name.clone(),
                transaction_id: transaction,
            }
        } else {
            Command::GiveInventory {
                to_agent_id: to,
                item_id: InventoryKey::from(given.id),
                asset_type: given.asset_type,
                item_name: given.name.clone(),
                transaction_id: transaction,
            }
        };
        self.session(giver).send(command).await?;
        let (offer, got_a, got_b) = self.await_offer(receiver, &given.name).await?;
        self.name_copies(receiver, &given.name).await?;
        metrics.set(&format!("{key}_told_a"), told_text(&self.scene, &got_a));
        metrics.set(&format!("{key}_told_b"), told_text(&self.scene, &got_b));
        self.record_where(metrics, key, "at_offer", giver, given, offer.as_ref())
            .await?;
        let Some(im) = offer else {
            no_offer(metrics, key, &AFTER_AN_ANSWER);
            return Ok(None);
        };
        metrics.set(
            &format!("{key}_offer"),
            self.offer_text(&im, Some(transaction.get())),
        );
        self.reply(metrics, key, receiver, &im, (&given.name, reply))
            .await?;
        self.record_where(metrics, key, "after", giver, given, Some(&im))
            .await?;
        Ok(Some(im))
    }

    /// Answers the offer `im` of the gift named `name` as `receiver` and
    /// records what each avatar was told under `key`.
    async fn reply(
        &mut self,
        metrics: &mut Metrics,
        key: &str,
        receiver: Who,
        im: &InstantMessage,
        (name, reply): (&str, Reply),
    ) -> Result<(), TestFailure> {
        match im.inventory_offer() {
            Some(offer) => {
                metrics.set(&format!("{key}_readable"), true);
                self.answer(receiver, offer, reply).await?;
            }
            None => metrics.set(&format!("{key}_readable"), false),
        }
        let (got_a, got_b) = self.hear(self.window, receiver, |_got| false).await?;
        self.name_copies(receiver, name).await?;
        metrics.set(
            &format!("{key}_reply_told_a"),
            told_text(&self.scene, &got_a),
        );
        metrics.set(
            &format!("{key}_reply_told_b"),
            told_text(&self.scene, &got_b),
        );
        Ok(())
    }

    /// Records, under `{key}_{when}`, where the gift is in the recipient's
    /// inventory by the id the offer names and by its name, and where the
    /// giver's original is.
    async fn record_where(
        &mut self,
        metrics: &mut Metrics,
        key: &str,
        when: &str,
        giver: Who,
        given: &Given,
        offer: Option<&InstantMessage>,
    ) -> Result<(), TestFailure> {
        let receiver = giver.other();
        let named = if when == "after" {
            self.named_settle(receiver, &given.name, Duration::from_secs(5))
                .await?
        } else {
            self.named(receiver, &given.name).await?
        };
        metrics.set(&format!("{key}_named_{when}"), named);
        let offered = offer.and_then(|im| {
            im.binary_bucket
                .get(1..17)
                .and_then(|bytes| <[u8; 16]>::try_from(bytes).ok())
                .map(Uuid::from_bytes)
        });
        let by_id = match offered {
            None => "no id".to_owned(),
            Some(id) if given.asset_type == AssetType::Folder => {
                let found = self.named_ids(receiver, &given.name).await?;
                found
                    .iter()
                    .find(|(found_id, _parent, _folder)| *found_id == id)
                    .map_or_else(
                        || "not under a system folder".to_owned(),
                        |(_id, parent, _folder)| self.scene.whose(*parent),
                    )
            }
            Some(id) => self.item_at(receiver, id).await?,
        };
        metrics.set(&format!("{key}_by_id_{when}"), by_id);
        let original = if given.asset_type == AssetType::Folder {
            self.named(giver, &given.name).await?
        } else {
            self.item_at(giver, given.id).await?
        };
        metrics.set(&format!("{key}_original_{when}"), original);
        Ok(())
    }

    /// Rezzes a cube as `who`; it, once it stands.
    async fn rez(&mut self, who: Who) -> Result<Object, TestFailure> {
        let at = self.next_spot();
        let session = match who {
            Who::A => &mut *self.a,
            Who::B => &mut *self.b,
        };
        session
            .send(Command::RezObject {
                shape: PrimShape::cube(at),
                group_id: None,
            })
            .await?;
        let seen = match who {
            Who::A => &self.objects_seen.0,
            Who::B => &self.objects_seen.1,
        };
        let object = wait_for_own_new_object(session, seen, STEP)
            .await?
            .map_err(|reason| {
                TestFailure::Assertion(format!(
                    "no cube appeared after RezObject: {}",
                    self.scene.scrub(&reason)
                ))
            })?;
        self.remember(who, &object);
        self.settle().await?;
        Ok(object)
    }

    /// Notes `object` as `who`'s, standing.
    fn remember(&mut self, who: Who, object: &Object) {
        match who {
            Who::A => self.objects_seen.0.insert(object.scoped_id()),
            Who::B => self.objects_seen.1.insert(object.scoped_id()),
        };
        self.standing.push((who, object.scoped_id()));
        let _previous = self
            .scene
            .ids
            .insert(object.full_id.uuid(), format!("{}'s cube", who.letter()));
    }

    /// Takes `object` of `who`'s into its Objects folder; the item the grid
    /// announced, if one.
    async fn take(
        &mut self,
        who: Who,
        object: ScopedObjectId,
    ) -> Result<Option<InventoryItem>, TestFailure> {
        let objects = InventoryFolderKey::from(
            self.scene
                .folder_of(who, FolderType::Object)
                .unwrap_or_default(),
        );
        let session = self.session(who);
        session
            .send(Command::DerezObjects {
                local_ids: vec![object],
                destination: DeRezDestination::TakeIntoAgentInventory(objects),
                transaction_id: TransactionId::from(Uuid::new_v4()),
                group_id: None,
            })
            .await?;
        let item = session
            .wait_for(STEP, |event| match event {
                Event::InventoryItemCreated { item, .. } => Some(item.clone()),
                Event::InventoryBulkUpdate { items, .. } => items.first().cloned(),
                _ => None,
            })
            .await;
        self.standing.retain(|(_who, standing)| *standing != object);
        match item {
            Ok(item) => Ok(Some(item)),
            Err(TestFailure::Timeout(_)) => Ok(None),
            Err(other) => Err(other),
        }
    }

    /// The item named `name` in `who`'s folder of `folder_type`, as the
    /// folder listing states it.
    async fn listed_item(
        &mut self,
        who: Who,
        folder_type: FolderType,
        name: &str,
    ) -> Result<Option<InventoryItem>, TestFailure> {
        let Some(folder) = self.scene.folder_of(who, folder_type) else {
            return Ok(None);
        };
        let folder = InventoryFolderKey::from(folder);
        let session = self.session(who);
        session.send(Command::RequestFolderContents(folder)).await?;
        let items = session
            .wait_for(STEP, |event| match event {
                Event::InventoryDescendents {
                    folder_id, items, ..
                } if *folder_id == folder => Some(items.clone()),
                _ => None,
            })
            .await?;
        Ok(items.into_iter().find(|item| item.name == name))
    }
}

/// The parts of a give's record that follow its offer, recorded as `no
/// offer` where none came, so every grid records every key.
const AFTER_AN_ANSWER: [&str; 6] = [
    "readable",
    "reply_told_a",
    "reply_told_b",
    "by_id_after",
    "named_after",
    "original_after",
];

/// Records the offer of `key` and each of its `parts` as `no offer`.
fn no_offer(metrics: &mut Metrics, key: &str, parts: &[&str]) {
    metrics.set(&format!("{key}_offer"), "no offer");
    for part in parts {
        metrics.set(&format!("{key}_{part}"), "no offer");
    }
}

/// Names `session`'s skeleton folders in `scene` as `who`'s: the root and
/// the first folder of each type by their type, a second folder of a type
/// as `other`, a folder of no type by its parent. Every typed folder is one
/// the census looks for gifts in.
fn name_folders(scene: &mut Scene, session: &Session, who: Who) -> Result<(), TestFailure> {
    let login = session
        .login_success()
        .ok_or_else(|| TestFailure::Assertion("no login answer kept".to_owned()))?;
    let is_a = who == Who::A;
    let mut counts: BTreeMap<i8, usize> = BTreeMap::new();
    for folder in &login.inventory_skeleton {
        let id = folder.folder_id.uuid();
        let code = if folder.parent_id.uuid().is_nil() {
            FolderType::RootInventory.to_code()
        } else {
            folder.type_default
        };
        if code < 0 {
            continue;
        }
        let kind = FolderType::from_code(code);
        let seen = counts.entry(code).or_insert(0);
        let word = if *seen == 0 {
            let _previous = scene.by_type.insert((is_a, code), id);
            format!("{}'s {kind:?}", who.letter())
        } else {
            format!("{}'s other {kind:?}", who.letter())
        };
        *seen = seen.saturating_add(1);
        let _previous = scene.folders.insert(id, word);
        scene.searched.push((is_a, id));
    }
    for folder in &login.inventory_skeleton {
        let id = folder.folder_id.uuid();
        if !scene.folders.contains_key(&id) {
            let parent = scene.whose(folder.parent_id.uuid());
            let _previous = scene
                .folders
                .insert(id, format!("a folder of {}'s in {parent}", who.letter()));
        }
    }
    let doubled: Vec<String> = counts
        .iter()
        .filter(|(_code, count)| **count > 1)
        .map(|(code, count)| format!("{count} {:?}", FolderType::from_code(*code)))
        .collect();
    scene.doubled.push(format!(
        "{}: {}",
        who.letter(),
        if doubled.is_empty() {
            "none".to_owned()
        } else {
            doubled.join(", ")
        }
    ));
    Ok(())
}

/// Sets up the scene and runs every leg, then cleans up.
async fn census(ctx: &mut TestContext, metrics: &mut Metrics) -> Result<(), TestFailure> {
    let grid = ctx.grid();
    let build_point = ctx.build_position();
    let somewhere = ctx
        .build_region()
        .zip(ctx.build_position())
        .map(|(region, point)| (region.to_owned(), point));
    let Some((a, b)) = ctx.primary_and_secondary() else {
        return Err(TestFailure::State(
            "inventory-offers needs a secondary avatar".to_owned(),
        ));
    };
    for session in [&mut *a, &mut *b] {
        session.wait_for_region(REGION_TIMEOUT).await?;
    }
    let id = |session: &Session| {
        session
            .agent_id()
            .map(|agent| agent.uuid())
            .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))
    };
    let (a_id, b_id) = (id(a)?, id(b)?);
    let mut names: Vec<(String, &'static str)> = Vec::new();
    for (session, word) in [(&*a, "<A>"), (&*b, "<B>")] {
        for name in names_of(&session.avatar_legacy_name()) {
            names.push((name, word));
        }
    }
    names.sort_by_key(|(name, _word)| std::cmp::Reverse(name.len()));
    let mut scene = Scene {
        a: a_id,
        b: b_id,
        names,
        tag: Uuid::new_v4()
            .simple()
            .to_string()
            .chars()
            .take(6)
            .collect(),
        ..Scene::default()
    };
    for (session, who) in [(&*a, Who::A), (&*b, Who::B)] {
        name_folders(&mut scene, session, who)?;
    }
    metrics.set("system_folders_doubled", scene.doubled.join("; "));
    if !is_opensim(grid) && !grid.is_fake() {
        let (region, point) = somewhere.ok_or_else(|| {
            TestFailure::State("inventory-offers needs a build_location in the fixtures".to_owned())
        })?;
        let here = |session: &Session| {
            session
                .region_identity()
                .and_then(|identity| identity.sim_name.as_ref())
                .is_some_and(|name| name.to_string().eq_ignore_ascii_case(&region))
        };
        if !(here(a) && here(b)) {
            let handle = region_named(a, &region).await?;
            for session in [&mut *a, &mut *b] {
                teleport(session, handle, &point).await?;
            }
        }
    }
    let (seen_b, _anchor) =
        settle_scene(b, grid, build_point.clone(), SETTLE_WINDOW, SETTLE_IDLE).await?;
    let (seen_a, anchor) =
        settle_scene(a, grid, build_point.clone(), SETTLE_WINDOW, SETTLE_IDLE).await?;
    let anchor = anchor.ok_or_else(|| {
        TestFailure::Assertion("the arrival streamed nothing to place a cube against".to_owned())
    })?;
    let started = Instant::now();
    while [FID2, FI2]
        .iter()
        .any(|name| a.cap(name).is_none() || b.cap(name).is_none())
        && started.elapsed() < CAP_WAIT
    {
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    let http = reqwest::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .pool_max_idle_per_host(0)
        .build()
        .map_err(|error| TestFailure::State(format!("building an HTTP client failed: {error}")))?;
    scene.region = a
        .region_identity()
        .and_then(|identity| identity.sim_name.as_ref())
        .map(ToString::to_string)
        .unwrap_or_default();
    let mut offers = Offers {
        a,
        b,
        scene,
        http,
        seen: (0, 0),
        objects_seen: (seen_a, seen_b),
        cube_at: Vector {
            x: anchor.x,
            y: anchor.y,
            z: anchor.z + REZ_LIFT_M,
        },
        rezzed: 0,
        standing: Vec::new(),
        grid,
        build_point,
        window: if grid.is_fake() { FAKE_WINDOW } else { WINDOW },
        unasked: if grid.is_fake() {
            FAKE_UNASKED_WINDOW
        } else {
            UNASKED_WINDOW
        },
    };
    offers.settle().await?;
    let outcome = legs(&mut offers, metrics).await;
    let cleaned = clean_up(&mut offers).await;
    outcome.and(cleaned)
}

/// Every leg, in order.
async fn legs(offers: &mut Offers<'_>, metrics: &mut Metrics) -> Result<(), TestFailure> {
    agent_gives(offers, metrics).await?;
    offline(offers, metrics).await?;
    if !offers.grid.is_fake() {
        object_gives(offers, metrics).await?;
    }
    no_copy_object(offers, metrics).await
}

/// Leg 1: agent gives.
async fn agent_gives(offers: &mut Offers<'_>, metrics: &mut Metrics) -> Result<(), TestFailure> {
    for (key, giver, reply, mask) in [
        ("accept", Who::A, Reply::Accept, NEXT_OWNER_FULL),
        ("decline", Who::A, Reply::Decline, NEXT_OWNER_FULL),
        ("ignore", Who::A, Reply::Ignore, NEXT_OWNER_FULL),
        ("reverse", Who::B, Reply::Accept, NEXT_OWNER_FULL),
        ("nocopy_decline", Who::A, Reply::Decline, NEXT_OWNER_NO_COPY),
    ] {
        let item = offers
            .notecard(
                giver,
                key,
                mask,
                &format!("{}'s {key} original", giver.letter()),
            )
            .await?;
        let given = Given {
            id: item.item_id.uuid(),
            asset_type: AssetType::Notecard,
            name: item.name.clone(),
        };
        let _offer = offers.give(metrics, key, giver, &given, reply).await?;
    }

    // A folder holding a notecard.
    let root = offers
        .scene
        .folder_of(Who::A, FolderType::RootInventory)
        .ok_or_else(|| TestFailure::Assertion("A's skeleton named no root".to_owned()))?;
    let folder_name = offers.scene.name("folder");
    let folder_id = Uuid::new_v4();
    offers
        .a
        .send(Command::CreateInventoryFolder {
            folder_id: InventoryFolderKey::from(folder_id),
            parent_id: InventoryFolderKey::from(root),
            folder_type: FolderType::None,
            name: folder_name.clone(),
        })
        .await?;
    offers.settle().await?;
    // The id the grid gave it: Second Life's AIS3 mints its own.
    let made = offers
        .named_ids(Who::A, &folder_name)
        .await?
        .into_iter()
        .find(|(_id, _parent, folder)| *folder)
        .map(|(id, _parent, _folder)| id);
    let Some(folder_id) = made else {
        metrics.set("folder_offer", "not made");
        return Ok(());
    };
    let _previous = offers.scene.ids.insert(folder_id, "A's folder".to_owned());
    let _inside = offers
        .make_item(
            Who::A,
            folder_id,
            "in-folder",
            NEXT_OWNER_FULL,
            "A's notecard in the folder",
        )
        .await?;
    let given = Given {
        id: folder_id,
        asset_type: AssetType::Folder,
        name: folder_name,
    };
    let _offer = offers
        .give(metrics, "folder", Who::A, &given, Reply::Accept)
        .await?;
    Ok(())
}

/// Leg 2: a give to B while B is offline.
async fn offline(offers: &mut Offers<'_>, metrics: &mut Metrics) -> Result<(), TestFailure> {
    let item = offers
        .notecard(Who::A, "offline", NEXT_OWNER_FULL, "A's offline original")
        .await?;
    let name = item.name.clone();
    offers.b.disconnect().await?;
    let transaction = TransactionId::from(Uuid::new_v4());
    offers
        .a
        .send(Command::GiveInventory {
            to_agent_id: AgentKey::from(offers.scene.b),
            item_id: item.item_id,
            asset_type: AssetType::Notecard,
            item_name: name.clone(),
            transaction_id: transaction,
        })
        .await?;
    let scene = offers.scene.clone();
    let mut got_a = watch(&scene, offers.a, offers.window, |_got| false).await?;
    let (unread_a, seen_a) = unread_since(offers.a, offers.seen.0);
    offers.seen.0 = seen_a;
    got_a.unread = unread_a;
    metrics.set("offline_told_a", told_text(&offers.scene, &got_a));
    metrics.set(
        "offline_original_at_offer",
        offers.item_at(Who::A, item.item_id.uuid()).await?,
    );

    // B comes back. Nothing waits for the region: that wait would throw
    // away what the arrival brings.
    offers.b.relogin().await?;
    offers.seen.1 = 0;
    // B came back wherever its last session left it: it is brought back
    // beside A for the legs that follow, and what the arrival brings is
    // kept as it goes, since a wait would throw it away.
    let mut unasked = Got::default();
    let settled = settle_scene_observing(
        offers.b,
        offers.grid,
        offers.build_point.clone(),
        SETTLE_WINDOW,
        SETTLE_IDLE,
        |event| {
            let _read = fold(&scene, &mut unasked, event);
        },
    )
    .await?;
    offers.objects_seen.1 = settled.seen;
    let wanted = name.clone();
    if Offers::offer_in(&unasked, &name).is_none() {
        unasked.merge(
            watch(&scene, offers.b, offers.unasked, move |got| {
                Offers::offer_in(got, &wanted).is_some()
            })
            .await?,
        );
    }
    let mut offer = Offers::offer_in(&unasked, &name);
    metrics.set(
        "offline_delivered",
        if offer.is_some() { "unasked" } else { "asked" },
    );
    let started = Instant::now();
    while offers.b.cap(FID2).is_none() && started.elapsed() < CAP_WAIT {
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    if offer.is_none() {
        offers.b.send(Command::RequestOfflineMessages).await?;
        let wanted = name.clone();
        let asked = watch(&scene, offers.b, OFFER_TIMEOUT, move |got| {
            Offers::offer_in(got, &wanted).is_some()
        })
        .await?;
        offer = Offers::offer_in(&asked, &name);
        unasked.merge(asked);
    }
    // Every message the return brought, the offer's or not.
    unasked
        .events
        .retain(|event| matches!(event, Event::InstantMessageReceived(_)));
    let outcome = offline_answer(offers, metrics, &item, transaction, offer).await;
    metrics.set("offline_ims", told_text(&offers.scene, &unasked));
    offers.seen.1 = offers.b.diagnostics().len();
    outcome
}

/// The rest of the offline leg: where the gift is, and B's acceptance of
/// `offer`, if it came.
async fn offline_answer(
    offers: &mut Offers<'_>,
    metrics: &mut Metrics,
    item: &InventoryItem,
    transaction: TransactionId,
    offer: Option<InstantMessage>,
) -> Result<(), TestFailure> {
    let name = item.name.clone();
    let given = Given {
        id: item.item_id.uuid(),
        asset_type: AssetType::Notecard,
        name,
    };
    offers.name_copies(Who::B, &given.name).await?;
    offers
        .record_where(
            metrics,
            "offline",
            "at_offer",
            Who::A,
            &given,
            offer.as_ref(),
        )
        .await?;
    let Some(im) = offer else {
        no_offer(metrics, "offline", &AFTER_AN_ANSWER);
        return Ok(());
    };
    metrics.set(
        "offline_offer",
        offers.offer_text(&im, Some(transaction.get())),
    );
    offers
        .reply(
            metrics,
            "offline",
            Who::B,
            &im,
            (&given.name, Reply::Accept),
        )
        .await?;
    offers
        .record_where(metrics, "offline", "after", Who::A, &given, Some(&im))
        .await
}

/// Leg 3: what an object gives.
async fn object_gives(offers: &mut Offers<'_>, metrics: &mut Metrics) -> Result<(), TestFailure> {
    let cube = offers.rez(Who::A).await?;
    // The notecard it gives, with a body, dropped into it.
    let note = offers
        .notecard(
            Who::A,
            "in-cube",
            NEXT_OWNER_FULL,
            "A's notecard for the cube",
        )
        .await?;
    offers
        .a
        .send(Command::UpdateTaskInventory {
            target: cube.scoped_id(),
            key: TaskInventoryKey::Item,
            item: Box::new(drop_item(&note, cube.full_id)?),
        })
        .await?;
    offers.settle().await?;
    // The probe, made new in the cube and saved over.
    offers
        .a
        .send(Command::RezScript {
            target: cube.scoped_id(),
            params: Box::new(RezScriptParams {
                group_id: None,
                enabled: true,
                item: RestoreItem::new_script(
                    AgentKey::from(offers.scene.a),
                    cube.full_id,
                    PROBE_NAME,
                    Uuid::new_v4(),
                ),
            }),
        })
        .await?;
    let started = Instant::now();
    let script = loop {
        offers.settle().await?;
        let held = listing(offers.a, cube.scoped_id(), cube.full_id).await?;
        metrics.set(
            "object_holds_the_notecard",
            held.items.iter().any(|item| item.name == note.name),
        );
        if let Some(item) = held.items.iter().find(|item| item.name == PROBE_NAME) {
            break item.item_id;
        }
        if started.elapsed() >= STEP {
            return Err(TestFailure::Assertion(
                "the probe script never appeared in the cube".to_owned(),
            ));
        }
    };
    offers
        .a
        .send(Command::UploadScript {
            location: ScriptUploadLocation::TaskInventory {
                task_id: cube.full_id,
                item_id: script,
                running: true,
                experience: None,
            },
            target: ScriptTarget::Mono,
            source: probe_source().into_bytes(),
        })
        .await?;
    let scene = offers.scene.clone();
    let ready = format!("{PROBE_TAG}|ready");
    let said_ready = |got: &Got| {
        got.events.iter().any(
            |event| matches!(event, Event::ChatReceived(chat) if chat.message.contains(&ready)),
        )
    };
    let got = watch(&scene, offers.a, STEP, said_ready).await?;
    if !said_ready(&got) {
        return Err(TestFailure::Assertion(format!(
            "the probe never said it started: {}",
            told_text(&offers.scene, &got)
        )));
    }
    offers.settle().await?;

    let b = offers.scene.b.to_string();
    let a = offers.scene.a.to_string();
    let list_name = offers.scene.name("list");
    for (key, words, receiver, reply, looked_for) in [
        (
            "object",
            format!("give|{b}"),
            Who::B,
            Reply::Accept,
            note.name.clone(),
        ),
        (
            "object_decline",
            format!("give|{b}"),
            Who::B,
            Reply::Decline,
            note.name.clone(),
        ),
        (
            "object_owner",
            format!("give|{a}"),
            Who::A,
            Reply::Accept,
            note.name.clone(),
        ),
        (
            "object_list",
            format!("list|{b}|{list_name}"),
            Who::B,
            Reply::Accept,
            list_name.clone(),
        ),
    ] {
        object_give(offers, metrics, key, &words, receiver, reply, &looked_for).await?;
    }
    Ok(())
}

/// One request to the probe, `words` on its channel, whose gift `receiver`
/// answers by `reply`; recorded under `key`. `looked_for` is the name the
/// gift goes by.
async fn object_give(
    offers: &mut Offers<'_>,
    metrics: &mut Metrics,
    key: &str,
    words: &str,
    receiver: Who,
    reply: Reply,
    looked_for: &str,
) -> Result<(), TestFailure> {
    let before = offers.named_ids(receiver, looked_for).await?.len();
    offers
        .a
        .send(Command::Chat {
            message: words.to_owned(),
            chat_type: ChatType::Normal,
            channel: ChatChannel(PROBE_CHANNEL),
        })
        .await?;
    let wanted = looked_for.to_owned();
    let (mut got_a, mut got_b) = offers
        .hear(OFFER_TIMEOUT, receiver, move |got| {
            Offers::offer_in(got, &wanted).is_some()
        })
        .await?;
    let (late_a, late_b) = offers.hear(offers.window, receiver, |_got| false).await?;
    got_a.merge(late_a);
    got_b.merge(late_b);
    offers.name_copies(receiver, looked_for).await?;
    metrics.set(&format!("{key}_told_a"), told_text(&offers.scene, &got_a));
    metrics.set(&format!("{key}_told_b"), told_text(&offers.scene, &got_b));
    let got = if receiver == Who::A { &got_a } else { &got_b };
    // An object's offer names the object, not the gift: take the first one.
    let offer = got
        .ims
        .iter()
        .find(|im| {
            matches!(
                im.dialog,
                ImDialog::InventoryOffered | ImDialog::TaskInventoryOffered
            )
        })
        .cloned();
    let at_offer = offers.named(receiver, looked_for).await?;
    metrics.set(
        &format!("{key}_named_at_offer"),
        format!("{at_offer} ({before} before)"),
    );
    let Some(im) = offer else {
        no_offer(
            metrics,
            key,
            &[
                "offer_message",
                "readable",
                "reply_told_a",
                "reply_told_b",
                "named_after",
            ],
        );
        return Ok(());
    };
    metrics.set(&format!("{key}_offer"), offers.offer_text(&im, None));
    metrics.set(
        &format!("{key}_offer_message"),
        offers.scene.scrub(&im.message),
    );
    offers
        .reply(metrics, key, receiver, &im, (looked_for, reply))
        .await?;
    let after = offers
        .named_settle(receiver, looked_for, Duration::from_secs(5))
        .await?;
    metrics.set(
        &format!("{key}_named_after"),
        format!("{after} ({before} before)"),
    );
    Ok(())
}

/// Leg 4: a no-copy object changes hands, is rezzed, taken and restored.
async fn no_copy_object(offers: &mut Offers<'_>, metrics: &mut Metrics) -> Result<(), TestFailure> {
    let cube = offers.rez(Who::B).await?;
    let spot = cube.motion.position.clone();
    let Some(taken) = offers.take(Who::B, cube.scoped_id()).await? else {
        metrics.set("nocopy_object_offer", "B's take was never announced");
        return Ok(());
    };
    // Named so the gift can be found by name.
    let name = offers.scene.name("object");
    offers
        .b
        .send(Command::UpdateInventoryItem {
            item: Box::new(InventoryItem {
                name: name.clone(),
                ..taken.clone()
            }),
            transaction_id: TransactionId::from(Uuid::nil()),
        })
        .await?;
    offers.settle().await?;
    let _previous = offers
        .scene
        .ids
        .insert(taken.item_id.uuid(), "B's object item".to_owned());
    let given = Given {
        id: taken.item_id.uuid(),
        asset_type: AssetType::Object,
        name: name.clone(),
    };
    let offer = offers
        .give(metrics, "nocopy_object", Who::B, &given, Reply::Accept)
        .await?;
    if offer.is_none() {
        return Ok(());
    }
    let Some(item) = offers
        .listed_item(Who::A, FolderType::Object, &name)
        .await?
    else {
        metrics.set("nocopy_object_a_holds", false);
        return Ok(());
    };
    metrics.set("nocopy_object_a_holds", true);
    let _previous = offers
        .scene
        .ids
        .insert(item.item_id.uuid(), "A's object item".to_owned());
    metrics.set(
        "nocopy_object_owner_can_copy",
        item.permissions.owner.contains(Permissions::COPY),
    );

    // A rezzes it.
    let at = offers.next_spot();
    offers
        .a
        .send(Command::RezObjectFromInventory {
            params: Box::new(rez_params(&item, &at)),
        })
        .await?;
    let rezzed = wait_for_own_new_object(offers.a, &offers.objects_seen.0, STEP).await?;
    let rezzed = match rezzed {
        Ok(object) => object,
        Err(reason) => {
            metrics.set(
                "nocopy_object_rez",
                format!("nothing: {}", offers.scene.scrub(&reason)),
            );
            return Ok(());
        }
    };
    offers.remember(Who::A, &rezzed);
    offers.settle().await?;
    metrics.set("nocopy_object_rez", "an object");
    metrics.set(
        "nocopy_object_item_after_rez",
        offers.item_at(Who::A, item.item_id.uuid()).await?,
    );

    // A takes it back.
    let Some(back) = offers.take(Who::A, rezzed.scoped_id()).await? else {
        metrics.set("nocopy_object_take", "never announced");
        return Ok(());
    };
    offers.settle().await?;
    metrics.set(
        "nocopy_object_take",
        if back.item_id == item.item_id {
            "the same item"
        } else {
            "a new item"
        },
    );
    let _previous = offers.scene.ids.insert(
        back.item_id.uuid(),
        "A's object item, taken back".to_owned(),
    );
    let back = offers
        .listed_item(Who::A, FolderType::Object, &name)
        .await?
        .unwrap_or(back);
    metrics.set(
        "nocopy_object_back_owner_can_copy",
        back.permissions.owner.contains(Permissions::COPY),
    );

    // A restores it to its last position.
    offers
        .a
        .send(Command::RezRestoreToWorld {
            item: rez_params(&back, &spot).item,
        })
        .await?;
    let restored = wait_for_own_new_object(offers.a, &offers.objects_seen.0, offers.window).await?;
    let scene = offers.scene.clone();
    let mut got = watch(&scene, offers.a, offers.window, |_got| false).await?;
    let (unread_a, seen_a) = unread_since(offers.a, offers.seen.0);
    offers.seen.0 = seen_a;
    got.unread = unread_a;
    metrics.set(
        "nocopy_object_restore_told_a",
        told_text(&offers.scene, &got),
    );
    match restored {
        Ok(object) => {
            offers.remember(Who::A, &object);
            metrics.set("nocopy_object_restore", "an object");
        }
        Err(_reason) => metrics.set("nocopy_object_restore", "nothing"),
    }
    metrics.set(
        "nocopy_object_item_after_restore",
        offers.item_at(Who::A, back.item_id.uuid()).await?,
    );
    Ok(())
}

/// Takes away every object still standing and everything the run named, in
/// both avatars' inventories.
async fn clean_up(offers: &mut Offers<'_>) -> Result<(), TestFailure> {
    offers.settle().await?;
    for who in [Who::A, Who::B] {
        let mine: Vec<ScopedObjectId> = offers
            .standing
            .iter()
            .filter(|(owner, _object)| *owner == who)
            .map(|(_owner, object)| *object)
            .collect();
        if mine.is_empty() {
            continue;
        }
        let trash = InventoryFolderKey::from(
            offers
                .scene
                .folder_of(who, FolderType::Trash)
                .unwrap_or_default(),
        );
        offers
            .session(who)
            .send(Command::DerezObjects {
                local_ids: mine,
                destination: DeRezDestination::Trash(trash),
                transaction_id: TransactionId::from(Uuid::new_v4()),
                group_id: None,
            })
            .await?;
    }
    offers.standing.clear();
    offers.settle().await?;
    let prefix = format!("{NAME_PREFIX}{}", offers.scene.tag);
    for who in [Who::A, Who::B] {
        let owner = offers.scene.agent(who);
        let folders: Vec<Uuid> = offers
            .scene
            .searched
            .iter()
            .filter(|(is_a, _id)| *is_a == (who == Who::A))
            .map(|(_is_a, id)| *id)
            .collect();
        let census = offers.fetcher(who)?;
        let raw = census.fetch_folders(FID2, owner, &folders).await;
        let mut items = Vec::new();
        let mut made = Vec::new();
        for entry in listed_folders(raw.body.as_ref()) {
            for item in entry
                .get("items")
                .and_then(Llsd::as_array)
                .unwrap_or_default()
            {
                let named = item.get("name").and_then(Llsd::as_str).unwrap_or_default();
                if named.starts_with(&prefix)
                    && let Some(id) = item.get("item_id").and_then(uuid_of)
                {
                    items.push(InventoryKey::from(id));
                }
            }
            for category in entry
                .get("categories")
                .and_then(Llsd::as_array)
                .unwrap_or_default()
            {
                let named = category
                    .get("name")
                    .and_then(Llsd::as_str)
                    .unwrap_or_default();
                if named.starts_with(&prefix)
                    && let Some(id) = category
                        .get("category_id")
                        .or_else(|| category.get("folder_id"))
                        .and_then(uuid_of)
                {
                    made.push(id);
                }
            }
        }
        if !items.is_empty() {
            census.a.send(Command::RemoveInventoryItems(items)).await?;
        }
        if !made.is_empty() {
            let root = offers
                .scene
                .folder_of(who, FolderType::RootInventory)
                .unwrap_or_default();
            let trash = offers
                .scene
                .folder_of(who, FolderType::Trash)
                .unwrap_or_default();
            let ais_road = offers
                .session(who)
                .cap(crate::cases::inventory_fetch::AIS)
                .is_some();
            let census = offers.fetcher(who)?;
            crate::cases::inventory_fetch::clean_up(
                &census,
                &crate::cases::inventory_fetch::Made { folders: made },
                root,
                trash,
                ais_road,
            )
            .await?;
        }
    }
    offers.settle().await
}

#[cfg(test)]
mod tests {
    use super::{Scene, bucket_text, probe_source, slurl_position, without_positions};
    use pretty_assertions::assert_eq;
    use sl_client_tokio::Uuid;

    #[test]
    fn the_probe_compiles() -> Result<(), String> {
        let source = probe_source();
        sl_lsl_runtime::compile(&source)
            .map(|_program| ())
            .map_err(|errors| format!("the probe does not compile: {errors:?}\n{source}"))
    }

    /// A position in a message is written `<position>`; other angle
    /// brackets stay.
    #[test]
    fn a_position_is_scrubbed_and_a_word_kept() {
        assert_eq!(
            without_positions("'x'. (Object is located at <region> <120, 128, 27.5>)"),
            "'x'. (Object is located at <region> <position>)"
        );
        assert_eq!(without_positions("<A> gave <B>"), "<A> gave <B>");
        let slurl = "( http://slurl.com/secondlife/<region>/52/62/34 )";
        assert_eq!(slurl_position(slurl).as_deref(), Some("/52/62/34"));
        assert_eq!(slurl_position("at <region> <position>"), None);
        assert_eq!(slurl_position("no region at all"), None);
    }

    /// Names go first, and a text with no SLurl is left as it is.
    #[test]
    fn a_name_is_scrubbed_from_a_text_without_a_slurl() {
        let scene = Scene {
            names: vec![("Some Resident".to_owned(), "<A>")],
            region: "Mauve".to_owned(),
            ..Scene::default()
        };
        assert_eq!(scene.scrub("Some Resident gave it"), "<A> gave it");
    }

    /// A bucket is described by its shape and the role of the id it names.
    #[test]
    fn a_bucket_names_its_type_and_id() {
        let original = Uuid::from_u128(0x17e);
        let mut scene = Scene::default();
        let _previous = scene.ids.insert(original, "A's original".to_owned());
        let mut bucket = vec![7_u8];
        bucket.extend_from_slice(original.as_bytes());
        assert_eq!(bucket_text(&scene, &bucket), "type 7 and A's original");
        assert_eq!(bucket_text(&scene, &[7_u8]), "type 7 alone");
        assert_eq!(bucket_text(&scene, &[]), "empty");
        assert_eq!(bucket_text(&scene, &[1, 2, 3]), "3 bytes");
    }
}
