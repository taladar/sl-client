//! A census of a prim's contents: how each grid lists them, what each way of
//! writing into them is answered with, and who is told.
//!
//! A prim carries an inventory of its own — the scripts, notecards and
//! objects a build drops into it. A viewer reads it two ways and writes it
//! five:
//!
//! - **the listing**, over the `RequestTaskInventory` capability where the
//!   region grants one ([`Command::FetchTaskInventory`]) and over the UDP
//!   `RequestTaskInventory` → `ReplyTaskInventory` → `Xfer` road everywhere;
//! - **a drop** of an inventory item ([`Command::UpdateTaskInventory`]), **a
//!   script** dropped or made new ([`Command::RezScript`]), **a remove**
//!   ([`Command::RemoveTaskInventory`]), **a move** out into the agent's
//!   inventory ([`Command::MoveTaskInventory`]), and **a save** of an item's
//!   asset over an `Update*TaskInventory` capability
//!   ([`Command::UpdateInventoryAsset`]).
//!
//! The case rezzes a cube as the primary avatar and asks one question a leg,
//! listening after each for what the grid sends — the record with its new
//! contents serial, a `ReplyTaskInventory` nobody asked for, an alert, an
//! inventory announcement — and then reading the listing back:
//!
//! 1. **An empty prim**, over both roads; the capability asked with the
//!    serial already held, with a stale one, about a prim nobody has, and
//!    about nothing.
//! 2. **A drop**: what lands, under which id and with which masks, and what a
//!    second drop of the same item makes.
//! 3. **A script**, made new and dropped out of the inventory: its name,
//!    whether it runs, and what it says.
//! 4. **Who is told.** Three times over — with both avatars holding the cube
//!    selected, with only the writer, and with nobody — a script made, an
//!    item dropped, an item removed and an item moved out.
//! 5. **Writes that cannot land**: a remove and a move of an item the prim
//!    does not hold, a drop and a script naming an item the agent does not.
//! 6. **A script's experience**, over `GetMetadata`.
//! 7. **A gesture and a material saved** into the prim over their
//!    capabilities.
//!
//! What each live grid answered is stated as a [`Measured`] and written up in
//! `book/src/gridspec/building.md` § Contents; every run, live or fake, is
//! held to it. What a second avatar may do to a prim that is not its own is
//! [`super::task_inventory_stranger`]'s.
//!
//! `2av`, `[both]`, offline on both fake flavours. The cube is derezzed to
//! the Trash and the items the case made are removed, also when a leg fails.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use sl_client_tokio::{
    AssetType, AssetUpdateLocation, Command, Event, InventoryItem, InventoryKey, InventoryType,
    NewInventoryItem, Object, ObjectKey, ObjectProperties as Properties, ObjectUpdateForm,
    Permissions, RegionLocalObjectId, RestoreItem, RezScriptParams, SaleType, ScopedObjectId,
    TaskInventoryItem, TaskInventoryKey, TaskInventoryReply, UpdatableAssetType, Uuid,
    WearableType, parse_task_inventory, parse_task_inventory_cap_reply,
    stamp_task_inventory_cap_reply, task_inventory_cap_url,
};
use sl_wire::{Llsd, parse_llsd_xml};

use crate::cases::object_edit::{count, hex};
use crate::cases::object_rez_derez::{
    Answer, Folders, Pace, REZ_LIFT_M, Residue, Run, SETTLE_WINDOW, STEP_TIMEOUT, clean_up,
    rez_cube,
};
use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::metrics::Metrics;
use crate::registry::{GridTest, TestFuture};
use crate::support::{
    ANNOUNCED_BULK, ANNOUNCED_LEGACY, REGION_TIMEOUT, REPLY_TIMEOUT, is_opensim, settle_scene,
};

/// The OpenSim start location: the "Default Region" (1000,1000), centred, where
/// this workspace's test object stands as the rez placement reference.
pub(crate) const OPENSIM_START: &str = "uri:Default Region&128&128&30";

/// How long the session that was *not* waited on is then read for: its events
/// were queued the whole time, so this only has to empty the queue.
pub(crate) const DRAIN: Duration = Duration::from_millis(500);

/// [`DRAIN`] on a fake grid.
pub(crate) const FAKE_DRAIN: Duration = Duration::from_millis(100);

/// The notecard the case drops.
const NOTECARD_NAME: &str = "SLClientContentsNote";

/// The script it drops out of the inventory.
const SCRIPT_NAME: &str = "SLClientContentsScript";

/// The script it makes new in the prim.
const NEW_SCRIPT_NAME: &str = "SLClientContentsNew";

/// The script it makes new and asks not to run.
const STOPPED_SCRIPT_NAME: &str = "SLClientContentsStopped";

/// The text of the notecard it drops.
const NOTECARD_TEXT: &str = "a note for a prim";

/// The gesture it saves into the prim.
const GESTURE_NAME: &str = "SLClientContentsGesture";

/// The material it saves into the prim.
const MATERIAL_NAME: &str = "SLClientContentsMaterial";

/// What a metric reads where the region grants no capability to ask.
pub(crate) const NO_CAPABILITY: &str = "no capability";

/// A gesture asset: version 2, no key, no modifier, a trigger, no replacement
/// text and no steps.
const GESTURE_BODY: &[u8] = b"2\n0\n0\nslclientcontents\n\n0\n";

/// Where the measurements below are written down.
const SOURCE: &str = "book/src/gridspec/building.md § Contents (task-inventory, 2026-10-12)";

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
/// under. Every run — live or fake — is held to all of it at the end, so a
/// run that disagrees says everything it disagrees about at once.
const MEASURED: &[(&str, Measured<Answer>)] = &[
    (
        "cap_at_another_serial_answers",
        each(
            Answer::Text("200: serial 0, 0 item(s)"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "cap_at_the_held_serial_answers",
        each(Answer::Text("304"), Answer::Text(NO_CAPABILITY)),
    ),
    (
        "cap_empty_answers",
        each(
            Answer::Text("200: serial 0, 0 item(s)"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "cap_item_shape",
        each(
            Answer::Text(
                "{asset_id:uuid,created_at:integer,desc:string,flags:integer,inv_type:string,item_id:uuid,metadata:{},name:string,parent_id:uuid,permissions:{base_mask:integer,creator_id:uuid,everyone_mask:integer,group_id:uuid,group_mask:integer,is_owner_group:boolean,last_owner_id:uuid,next_owner_mask:integer,owner_id:uuid,owner_mask:integer},sale_info:{sale_price:integer,sale_type:string},type:string}",
            ),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "cap_item_type_names",
        each(
            Answer::Text("notecard/notecard/not"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "cap_of_an_unknown_prim_answers",
        each(Answer::Text("404"), Answer::Text(NO_CAPABILITY)),
    ),
    (
        "cap_of_no_prim_answers",
        each(Answer::Text("404"), Answer::Text(NO_CAPABILITY)),
    ),
    ("drop_agent_item", both(Answer::Text("kept"))),
    ("drop_asset", both(Answer::Text("the agent item's"))),
    (
        "drop_brings",
        each(
            Answer::Text("told by full update, serial +1, items +1"),
            Answer::Text("told by properties, serial +1, items +1"),
        ),
    ),
    ("drop_group_owned", both(Answer::Flag(false))),
    ("drop_item_id", both(Answer::Text("a new one"))),
    ("drop_keeps_the_masks", both(Answer::Flag(true))),
    ("drop_keeps_the_name", both(Answer::Flag(true))),
    (
        "drop_of_an_unheld_item_brings",
        each(
            Answer::Text(
                "alert: Inventory creation on in-world object failed. [InventoryCreationInWorldObjectFailed]",
            ),
            Answer::Text("nothing"),
        ),
    ),
    ("drop_parent_is_the_prim", both(Answer::Flag(true))),
    ("drop_record_serial_steps", both(Answer::Text("+1"))),
    (
        "drop_with_both_selecting_told_other",
        each(
            Answer::Text("properties+full update"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "drop_with_both_selecting_told_writer",
        each(
            Answer::Text("properties+full update"),
            Answer::Text("properties"),
        ),
    ),
    (
        "drop_with_nobody_selecting_told_other",
        each(Answer::Text("full update"), Answer::Text("nothing")),
    ),
    (
        "drop_with_nobody_selecting_told_writer",
        each(Answer::Text("full update"), Answer::Text("properties")),
    ),
    (
        "drop_with_the_writer_selecting_told_other",
        each(Answer::Text("full update"), Answer::Text("nothing")),
    ),
    (
        "drop_with_the_writer_selecting_told_writer",
        each(
            Answer::Text("properties+full update"),
            Answer::Text("properties"),
        ),
    ),
    ("empty_reply_names_a_file", both(Answer::Flag(false))),
    ("empty_serial", both(Answer::Count(0))),
    (
        "fetch_answers_with",
        each(Answer::Text("contents"), Answer::Text("reply+contents")),
    ),
    (
        "fetch_at_the_held_serial_answers_with",
        each(Answer::Text("unchanged"), Answer::Text("reply+unchanged")),
    ),
    ("gesture_save", both(Answer::Text("saved"))),
    (
        "gesture_save_brings",
        each(
            Answer::Text("told by full update, serial +1"),
            Answer::Text("alert: Gesture updated, serial +1"),
        ),
    ),
    ("gesture_save_changes_the_asset", both(Answer::Flag(true))),
    (
        "gesture_save_names_the_listed_asset",
        both(Answer::Flag(true)),
    ),
    (
        "gesture_save_told_other",
        each(Answer::Text("full update"), Answer::Text("nothing")),
    ),
    ("gesture_task_cap_granted", both(Answer::Flag(true))),
    (
        "inventory_script_brings",
        each(
            Answer::Text("told by full update, serial +2, items +1"),
            Answer::Text("told by properties+full update, serial +1, items +1"),
        ),
    ),
    ("inventory_script_item_id", both(Answer::Text("a new one"))),
    ("inventory_script_keeps_the_name", both(Answer::Flag(true))),
    ("inventory_script_runs", both(Answer::Text("running"))),
    (
        "listing_cap_granted",
        each(Answer::Flag(true), Answer::Flag(false)),
    ),
    (
        "material_save",
        each(Answer::Text("saved"), Answer::Text(NO_CAPABILITY)),
    ),
    (
        "material_task_cap_granted",
        each(Answer::Flag(true), Answer::Flag(false)),
    ),
    (
        "metadata_answers",
        each(Answer::Text("200: undefined"), Answer::Text(NO_CAPABILITY)),
    ),
    (
        "metadata_cap_granted",
        each(Answer::Flag(true), Answer::Flag(false)),
    ),
    (
        "metadata_of_an_unknown_item_answers",
        each(Answer::Text("404: no body"), Answer::Text(NO_CAPABILITY)),
    ),
    (
        "move_announced_as",
        both(Answer::Text("update-create-inventory-item")),
    ),
    ("move_brings", both(Answer::Text("nothing"))),
    ("move_item_id", both(Answer::Text("a new one"))),
    (
        "move_item_name",
        both(Answer::Text("SLClientContentsNote 4")),
    ),
    ("move_lands_in", both(Answer::Text("root"))),
    (
        "move_leaves_the_prim",
        both(Answer::Text("holding the item")),
    ),
    (
        "move_of_an_unknown_item_brings",
        both(Answer::Text("nothing")),
    ),
    ("move_record_serial_steps", both(Answer::Text("none"))),
    (
        "move_with_both_selecting_told_other",
        both(Answer::Text("nothing")),
    ),
    (
        "move_with_both_selecting_told_writer",
        both(Answer::Text("nothing")),
    ),
    (
        "move_with_nobody_selecting_told_other",
        both(Answer::Text("nothing")),
    ),
    (
        "move_with_nobody_selecting_told_writer",
        both(Answer::Text("nothing")),
    ),
    (
        "move_with_the_writer_selecting_told_other",
        both(Answer::Text("nothing")),
    ),
    (
        "move_with_the_writer_selecting_told_writer",
        both(Answer::Text("nothing")),
    ),
    (
        "new_script_brings",
        each(
            Answer::Text("told by full update, serial +2, items +1"),
            Answer::Text("told by properties+full update, serial +1, items +1"),
        ),
    ),
    ("new_script_name", both(Answer::Text("SLClientContentsNew"))),
    ("new_script_runs", both(Answer::Text("running"))),
    (
        "note_remove_brings",
        each(
            Answer::Text("told by full update, serial +1, items -1"),
            Answer::Text("told by properties+full update, serial +1, items -1"),
        ),
    ),
    (
        "remove_brings",
        each(
            Answer::Text("told by full update, serial +2, items -1"),
            Answer::Text("told by properties+full update, serial +1, items -1"),
        ),
    ),
    (
        "remove_files_the_item_in",
        each(Answer::Text("no folder"), Answer::Text("trash")),
    ),
    ("remove_lands", both(Answer::Flag(true))),
    (
        "remove_of_an_unknown_item_brings",
        both(Answer::Text("nothing")),
    ),
    (
        "remove_record_serial_steps",
        each(Answer::Text("+2"), Answer::Text("+1")),
    ),
    (
        "remove_with_both_selecting_told_other",
        each(
            Answer::Text("properties+full update"),
            Answer::Text("full update"),
        ),
    ),
    (
        "remove_with_both_selecting_told_writer",
        both(Answer::Text("properties+full update")),
    ),
    (
        "remove_with_nobody_selecting_told_other",
        both(Answer::Text("full update")),
    ),
    (
        "remove_with_nobody_selecting_told_writer",
        each(
            Answer::Text("full update"),
            Answer::Text("properties+full update"),
        ),
    ),
    (
        "remove_with_the_writer_selecting_told_other",
        both(Answer::Text("full update")),
    ),
    (
        "remove_with_the_writer_selecting_told_writer",
        both(Answer::Text("properties+full update")),
    ),
    (
        "roads_agree",
        each(Answer::Text("they agree"), Answer::Text(NO_CAPABILITY)),
    ),
    (
        "script_experience",
        each(Answer::Text("none"), Answer::Text("no answer")),
    ),
    (
        "script_of_an_unheld_item_brings",
        both(Answer::Text("nothing")),
    ),
    (
        "script_with_both_selecting_told_other",
        each(
            Answer::Text("properties+reply+full update"),
            Answer::Text("full update"),
        ),
    ),
    (
        "script_with_both_selecting_told_writer",
        each(
            Answer::Text("properties+reply+full update"),
            Answer::Text("properties+full update"),
        ),
    ),
    (
        "script_with_nobody_selecting_told_other",
        both(Answer::Text("full update")),
    ),
    (
        "script_with_nobody_selecting_told_writer",
        each(
            Answer::Text("full update"),
            Answer::Text("properties+full update"),
        ),
    ),
    (
        "script_with_the_writer_selecting_told_other",
        both(Answer::Text("full update")),
    ),
    (
        "script_with_the_writer_selecting_told_writer",
        each(
            Answer::Text("properties+reply+full update"),
            Answer::Text("properties+full update"),
        ),
    ),
    (
        "second_drop_brings",
        each(
            Answer::Text("told by full update, serial +1, items +1"),
            Answer::Text("told by properties, serial +1, items +1"),
        ),
    ),
    (
        "second_drop_names",
        both(Answer::Text(
            "SLClientContentsNote | SLClientContentsNote 1",
        )),
    ),
    (
        "stopped_script_brings",
        each(
            Answer::Text("told by full update, serial +1, items +1"),
            Answer::Text("told by properties+full update, serial +1, items +1"),
        ),
    ),
    (
        "stopped_script_runs",
        each(Answer::Text("not running"), Answer::Text("running")),
    ),
    (
        "cap_reply_keys",
        each(
            Answer::Text("contents,inventory_serial"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "material_save_brings",
        each(
            Answer::Text("told by full update, serial +1"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "material_save_changes_the_asset",
        each(Answer::Flag(true), Answer::Text(NO_CAPABILITY)),
    ),
    (
        "material_save_names_the_listed_asset",
        each(Answer::Flag(true), Answer::Text(NO_CAPABILITY)),
    ),
    (
        "material_save_told_other",
        each(Answer::Text("full update"), Answer::Text(NO_CAPABILITY)),
    ),
];

/// Holds what a census recorded to `measured`, naming every metric that reads
/// otherwise — or was never recorded.
pub(crate) fn hold_to_measured(
    grid: Grid,
    metrics: &Metrics,
    measured: &[(&str, Measured<Answer>)],
    source: &str,
) -> Result<(), TestFailure> {
    let failures: Vec<String> = measured
        .iter()
        .filter_map(|(key, measured)| {
            let expected = measured.on(grid);
            match metrics.get(key) {
                Some(value) if expected.is(value) => None,
                Some(value) => Some(format!("{key} was {value:?}, measured {expected:?}")),
                None => Some(format!("{key} was not recorded, measured {expected:?}")),
            }
        })
        .collect();
    if failures.is_empty() {
        return Ok(());
    }
    let verdict = if grid.is_fake() {
        "the fake grid no longer imitates"
    } else {
        "the grid no longer matches"
    };
    Err(TestFailure::Assertion(format!(
        "{grid}: {verdict} {source}: {}",
        failures.join("; ")
    )))
}

/// What one session was sent during a window.
#[derive(Debug, Default)]
pub(crate) struct Heard {
    /// Every `ObjectProperties` record.
    pub(crate) properties: Vec<Properties>,
    /// Every `ReplyTaskInventory`.
    pub(crate) replies: Vec<TaskInventoryReply>,
    /// The text of every alert.
    pub(crate) alerts: Vec<String>,
    /// Every inventory item the grid announced, and the shape it came in.
    pub(crate) announced: Vec<(&'static str, InventoryItem)>,
    /// The items the grid said were removed from the agent's inventory.
    pub(crate) removed: Vec<InventoryKey>,
    /// The objects of the agent's region the stream named, and the form each
    /// came in.
    pub(crate) updated: Vec<(RegionLocalObjectId, ObjectUpdateForm)>,
    /// Every line of chat: who said it, how, and what.
    pub(crate) chats: Vec<String>,
    /// Objects the session already held, as each read once an update had
    /// changed it, in arrival order.
    pub(crate) changed: Vec<Object>,
    /// How each asset save ended: the asset stored, or the refusal.
    pub(crate) saves: Vec<Result<Uuid, String>>,
}

impl Heard {
    /// How the session was told that the contents of `object` changed from
    /// the serial `before`: by its record, by a `ReplyTaskInventory` nobody
    /// asked for, by the object sent again, by several of those — or by
    /// `nothing`.
    pub(crate) fn told(&self, object: &Object, before: i16) -> String {
        let mut parts = self.told_of(object.full_id, before);
        for (form, word) in [
            (ObjectUpdateForm::Full, "full update"),
            (ObjectUpdateForm::Compressed, "compressed update"),
            (ObjectUpdateForm::Terse, "terse update"),
        ] {
            if self.updated.contains(&(object.local_id, form)) {
                parts.push(word);
            }
        }
        words(&parts)
    }

    /// The record and the reply halves of [`told`](Self::told).
    fn told_of(&self, object: ObjectKey, before: i16) -> Vec<&'static str> {
        let mut parts = Vec::new();
        let mut records = self
            .properties
            .iter()
            .filter(|properties| properties.object_id == object)
            .peekable();
        if records.peek().is_some() {
            parts.push(
                if records.any(|properties| properties.inventory_serial != before) {
                    "properties"
                } else {
                    "properties at the old serial"
                },
            );
        }
        if self.replies.iter().any(|reply| reply.task == object) {
            parts.push("reply");
        }
        parts
    }

    /// How far from `before` each record of `object` put its contents
    /// serial, in arrival order and `,`-joined — or `none`.
    pub(crate) fn record_serial_steps(&self, object: ObjectKey, before: i16) -> String {
        let steps: Vec<String> = self
            .properties
            .iter()
            .filter(|properties| properties.object_id == object)
            .map(|properties| {
                format!(
                    "{:+}",
                    i32::from(properties.inventory_serial).saturating_sub(i32::from(before))
                )
            })
            .collect();
        if steps.is_empty() {
            "none".to_owned()
        } else {
            steps.join(",")
        }
    }

    /// The alerts, `|`-joined, or `none`.
    pub(crate) fn alerted(&self) -> String {
        if self.alerts.is_empty() {
            "none".to_owned()
        } else {
            self.alerts.join(" | ")
        }
    }
}

/// `+`-joined words, or `nothing`.
fn words(parts: &[&str]) -> String {
    if parts.is_empty() {
        "nothing".to_owned()
    } else {
        parts.join("+")
    }
}

/// Listens on `session` until `done` says the leg has what it came for and
/// `quiet` more has passed, or for `limit` at most.
pub(crate) async fn listen_for(
    session: &mut Session,
    limit: Duration,
    quiet: Duration,
    mut done: impl FnMut(&Heard) -> bool,
) -> Result<Heard, TestFailure> {
    let mut heard = Heard::default();
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
        // One event at a time, so that `done` sees each as it arrives.
        let outcome = session
            .wait_for(remaining, |event| {
                match event {
                    Event::ObjectProperties(properties) => {
                        heard.properties.push((**properties).clone());
                    }
                    Event::TaskInventoryReply(reply) => heard.replies.push(reply.clone()),
                    Event::AlertMessage {
                        message,
                        alert_info,
                        ..
                    } => heard.alerts.push(alert_info.first().map_or_else(
                        || message.clone(),
                        |info| format!("{message} [{}]", info.message),
                    )),
                    Event::AgentAlertMessage { message, .. } => heard.alerts.push(message.clone()),
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
                    Event::ObjectStreamBatch(batch) if !batch.child => heard.updated.extend(
                        batch
                            .entries
                            .iter()
                            .filter(|entry| entry.known)
                            .map(|entry| (entry.local_id, batch.form)),
                    ),
                    // A line out of earshot comes without its text.
                    Event::ChatReceived(chat) if !chat.message.is_empty() => {
                        heard.chats.push(chat.message.clone());
                    }
                    Event::ObjectUpdated(object) => heard.changed.push((**object).clone()),
                    Event::AssetUploaded { new_asset, .. } => heard.saves.push(Ok(*new_asset)),
                    Event::AssetUploadFailed { reason } => heard.saves.push(Err(reason.clone())),
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

/// Listens on `session` for `window`.
pub(crate) async fn listen(session: &mut Session, window: Duration) -> Result<Heard, TestFailure> {
    listen_for(session, window, window, |_heard| false).await
}

/// A prim's contents as the UDP road lists them.
#[derive(Debug, Clone)]
pub(crate) struct Listing {
    /// The serial the `ReplyTaskInventory` stated.
    pub(crate) serial: i16,
    /// Whether it named a file to download.
    pub(crate) named_a_file: bool,
    /// The items of the file.
    pub(crate) items: Vec<TaskInventoryItem>,
}

impl Listing {
    /// The items `earlier` did not have.
    pub(crate) fn new_since<'a>(&'a self, earlier: &Self) -> Vec<&'a TaskInventoryItem> {
        let known: HashSet<InventoryKey> = earlier.items.iter().map(|item| item.item_id).collect();
        self.items
            .iter()
            .filter(|item| !known.contains(&item.item_id))
            .collect()
    }

    /// Whether the prim holds the item `item_id`.
    pub(crate) fn holds(&self, item_id: InventoryKey) -> bool {
        self.items.iter().any(|item| item.item_id == item_id)
    }
}

/// Reads the contents of the prim `task`, known to `session` as `target`,
/// over the UDP road, whatever the region grants: a `RequestTaskInventory`,
/// and the `Xfer` of the file its reply names.
pub(crate) async fn listing(
    session: &mut Session,
    target: ScopedObjectId,
    task: ObjectKey,
) -> Result<Listing, TestFailure> {
    session
        .send(Command::RequestTaskInventory { target })
        .await?;
    let reply = session
        .wait_for(STEP_TIMEOUT, |event| match event {
            Event::TaskInventoryReply(reply) if reply.task == task => Some(reply.clone()),
            _ => None,
        })
        .await?;
    if reply.filename.is_empty() {
        return Ok(Listing {
            serial: reply.serial,
            named_a_file: false,
            items: Vec::new(),
        });
    }
    session
        .send(Command::RequestXfer {
            filename: reply.filename.clone(),
        })
        .await?;
    let data = session
        .wait_for(STEP_TIMEOUT, |event| match event {
            Event::XferDownloaded { data, .. } => Some(data.clone()),
            _ => None,
        })
        .await?;
    let items = parse_task_inventory(&data).map_err(|error| {
        TestFailure::Assertion(format!("the contents listing did not parse: {error}"))
    })?;
    Ok(Listing {
        serial: reply.serial,
        named_a_file: true,
        items,
    })
}

/// What a capability answered a request with.
#[derive(Debug)]
pub(crate) struct CapAnswer {
    /// The HTTP status.
    pub(crate) status: u16,
    /// The body, where it was LLSD.
    pub(crate) body: Option<Llsd>,
}

impl CapAnswer {
    /// The answer as a `RequestTaskInventory` listing of `task`, where it is
    /// one.
    pub(crate) fn listing(&self, task: ObjectKey) -> Option<(i16, Vec<TaskInventoryItem>)> {
        let stamped = stamp_task_inventory_cap_reply(self.body.clone()?, task);
        let reply = parse_task_inventory_cap_reply(&stamped).ok()?;
        Some((reply.serial, reply.items))
    }

    /// The answer as a metric records it: the status, and for a listing its
    /// serial and how many items it has.
    pub(crate) fn summary(&self, task: ObjectKey) -> String {
        match self.listing(task) {
            Some((serial, items)) => {
                format!("{}: serial {serial}, {} item(s)", self.status, items.len())
            }
            None if self.body.is_some() => format!("{}: no listing", self.status),
            None => self.status.to_string(),
        }
    }

    /// The keys of the answer's map, `,`-joined and in order.
    pub(crate) fn keys(&self) -> String {
        keys_of(self.body.as_ref())
    }

    /// The status and what kind of body came with it.
    pub(crate) fn shape(&self) -> String {
        let body = match &self.body {
            None => "no body".to_owned(),
            Some(Llsd::Map(map)) if map.is_empty() => "an empty map".to_owned(),
            Some(Llsd::Map(_)) => format!("a map of {}", self.keys()),
            Some(Llsd::Undef) => "undefined".to_owned(),
            Some(_) => "something else".to_owned(),
        };
        format!("{}: {body}", self.status)
    }
}

/// The keys of an LLSD map, `,`-joined and in order; `-` for anything else.
fn keys_of(value: Option<&Llsd>) -> String {
    match value {
        Some(Llsd::Map(map)) => {
            let mut keys: Vec<&str> = map.keys().map(String::as_str).collect();
            keys.sort_unstable();
            keys.join(",")
        }
        _ => "-".to_owned(),
    }
}

/// The shape of an LLSD value: the kind of a scalar, and for a map each key
/// with the shape of its value, in order.
pub(crate) fn shape_of(value: &Llsd) -> String {
    match value {
        Llsd::Map(map) => {
            let mut entries: Vec<(&String, &Llsd)> = map.iter().collect();
            entries.sort_unstable_by_key(|(key, _value)| *key);
            let entries: Vec<String> = entries
                .into_iter()
                .map(|(key, value)| format!("{key}:{}", shape_of(value)))
                .collect();
            format!("{{{}}}", entries.join(","))
        }
        Llsd::Array(items) => format!("[{}]", items.len()),
        Llsd::Undef => "undef".to_owned(),
        Llsd::Boolean(_) => "boolean".to_owned(),
        Llsd::Integer(_) => "integer".to_owned(),
        Llsd::Real(_) => "real".to_owned(),
        Llsd::String(_) => "string".to_owned(),
        Llsd::Uuid(_) => "uuid".to_owned(),
        Llsd::Binary(_) => "binary".to_owned(),
        Llsd::Date(_) => "date".to_owned(),
        Llsd::Uri(_) => "uri".to_owned(),
    }
}

/// An HTTP client for the capabilities the case asks itself.
pub(crate) fn http_client() -> Result<reqwest::Client, TestFailure> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|error| TestFailure::Assertion(format!("building an HTTP client failed: {error}")))
}

/// Reads what came back from a capability.
async fn answer_of(
    response: Result<reqwest::Response, reqwest::Error>,
) -> Result<CapAnswer, TestFailure> {
    let response = response
        .map_err(|error| TestFailure::Assertion(format!("a capability request failed: {error}")))?;
    let status = response.status().as_u16();
    let text = response.text().await.unwrap_or_default();
    Ok(CapAnswer {
        status,
        body: parse_llsd_xml(&text).ok(),
    })
}

/// GETs a capability URL.
pub(crate) async fn cap_get(http: &reqwest::Client, url: &str) -> Result<CapAnswer, TestFailure> {
    answer_of(
        http.get(url)
            .header("Accept", "application/llsd+xml")
            .send()
            .await,
    )
    .await
}

/// POSTs an LLSD body to a capability URL.
pub(crate) async fn cap_post(
    http: &reqwest::Client,
    url: &str,
    body: String,
) -> Result<CapAnswer, TestFailure> {
    answer_of(
        http.post(url)
            .header("Content-Type", "application/llsd+xml")
            .body(body)
            .send()
            .await,
    )
    .await
}

/// A census of a prim's contents: both roads of the listing, every way of
/// writing into it, and who is told.
#[derive(Debug)]
pub struct TaskInventory;

impl GridTest for TaskInventory {
    fn name(&self) -> &'static str {
        "task-inventory"
    }

    fn description(&self) -> &'static str {
        "How a prim's contents are listed, what each write into them brings, and who is told"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi, Grid::FakeSl, Grid::FakeOpensim]
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

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let folders = login_folders(ctx.primary()).await?;
            let mut residue = Residue::default();
            let mut metrics = Metrics::new();
            let outcome = census(ctx, folders, &mut residue, &mut metrics).await;
            let grid = ctx.grid();
            ctx.metrics().merge(metrics.clone());
            // Take the cube and the items away whatever became of the census.
            let cleanup = clean_up(ctx.primary(), &residue, folders.trash).await;
            outcome
                .and(cleanup)
                .and_then(|()| hold_to_measured(grid, &metrics, MEASURED, SOURCE))
        })
    }
}

/// The folders of the login skeleton, which comes before the region does.
pub(crate) async fn login_folders(session: &mut Session) -> Result<Folders, TestFailure> {
    let folders = session
        .wait_for(REPLY_TIMEOUT, |event| match event {
            Event::InventorySkeleton(folders) => Some(folders.clone()),
            _ => None,
        })
        .await?;
    Folders::of(&folders)
        .ok_or_else(|| TestFailure::Assertion("inventory skeleton had no root folder".to_owned()))
}

/// Everything the legs share.
struct Census<'a> {
    /// The avatar that owns the cube and writes into it.
    primary: &'a mut Session,
    /// The avatar that watches.
    secondary: &'a mut Session,
    /// The run.
    run: Run,
    /// How long the watcher's queue is read for.
    drain: Duration,
    /// The cube.
    cube: Object,
    /// The cube as the watcher knows it.
    theirs: ScopedObjectId,
    /// The cube's contents as last read.
    held: Listing,
}

impl Census<'_> {
    /// Sends `command`, a write into the cube, and returns what the writer
    /// and the watcher were sent, with the contents read back.
    async fn write(&mut self, command: Command) -> Result<Written, TestFailure> {
        self.primary.send(command).await?;
        let to_writer = listen(self.primary, self.run.pace.quiet).await?;
        self.written(to_writer).await
    }

    /// What a write that `to_writer` was heard after left behind.
    async fn written(&mut self, to_writer: Heard) -> Result<Written, TestFailure> {
        let before = self.held.clone();
        let to_other = listen(self.secondary, self.drain).await?;
        self.held = listing(self.primary, self.cube.scoped_id(), self.cube.full_id).await?;
        let latest = to_writer
            .changed
            .iter()
            .rfind(|object| object.full_id == self.cube.full_id)
            .cloned();
        let flag_bits = latest
            .as_ref()
            .map(|latest| latest.update_flags ^ self.cube.update_flags);
        if let Some(latest) = latest {
            self.cube = latest;
        }
        Ok(Written {
            to_writer,
            to_other,
            before,
            after: self.held.clone(),
            flag_bits,
        })
    }

    /// A drop of the agent's `item` into the cube.
    fn drop_of(&self, item: &InventoryItem) -> Result<Command, TestFailure> {
        Ok(Command::UpdateTaskInventory {
            target: self.cube.scoped_id(),
            key: TaskInventoryKey::Item,
            item: Box::new(drop_item(item, self.cube.full_id)?),
        })
    }

    /// The agent's script `item` dropped into the cube, to run.
    fn script_of(&self, item: &InventoryItem) -> Result<Command, TestFailure> {
        Ok(Command::RezScript {
            target: self.cube.scoped_id(),
            params: Box::new(RezScriptParams {
                group_id: None,
                enabled: true,
                item: drop_item(item, self.cube.full_id)?,
            }),
        })
    }

    /// A new script called `name` made in the cube, to run or not.
    fn new_script(&self, name: &str, enabled: bool) -> Command {
        Command::RezScript {
            target: self.cube.scoped_id(),
            params: Box::new(RezScriptParams {
                group_id: None,
                enabled,
                item: RestoreItem::new_script(
                    self.run.owner,
                    self.cube.full_id,
                    name,
                    Uuid::new_v4(),
                ),
            }),
        }
    }

    /// Whether the script `item_id` in the cube runs.
    async fn running(&mut self, item_id: InventoryKey) -> Result<&'static str, TestFailure> {
        let object_id = self.cube.full_id;
        self.primary
            .send(Command::RequestScriptRunning { object_id, item_id })
            .await?;
        let answer = self
            .primary
            .wait_for(self.run.pace.quiet.saturating_mul(2), |event| match event {
                Event::ScriptRunning {
                    object_id: of,
                    item_id: item,
                    running,
                } if *of == object_id && *item == item_id => Some(*running),
                _ => None,
            })
            .await;
        match answer {
            Ok(true) => Ok("running"),
            Ok(false) => Ok("not running"),
            Err(TestFailure::Timeout(_)) => Ok("no answer"),
            Err(other) => Err(other),
        }
    }
}

/// What one write into a prim brought.
pub(crate) struct Written {
    /// What the writer was sent.
    pub(crate) to_writer: Heard,
    /// What the other avatar was sent.
    pub(crate) to_other: Heard,
    /// The contents before.
    pub(crate) before: Listing,
    /// The contents after.
    pub(crate) after: Listing,
    /// The update flags the write changed, where the writer's own object was
    /// sent again.
    pub(crate) flag_bits: Option<u32>,
}

impl Written {
    /// The update flags the write changed, as a metric records them.
    fn flags_changed(&self) -> String {
        self.flag_bits
            .map_or_else(|| "no update".to_owned(), |bits| format!("{bits:#010x}"))
    }

    /// How far the contents serial moved.
    pub(crate) fn serial_step(&self) -> i64 {
        i64::from(self.after.serial).saturating_sub(i64::from(self.before.serial))
    }

    /// How many items the write added, or took away.
    pub(crate) fn items_step(&self) -> i64 {
        count(self.after.items.len()).saturating_sub(count(self.before.items.len()))
    }

    /// The first item the write added.
    pub(crate) fn added(&self) -> Option<TaskInventoryItem> {
        self.after.new_since(&self.before).first().copied().cloned()
    }

    /// What became of the write, as one metric: what the writer was told by,
    /// the alerts, and how the contents moved — or `nothing`.
    pub(crate) fn outcome(&self, told: &str) -> String {
        let mut parts = Vec::new();
        if told != "nothing" {
            parts.push(format!("told by {told}"));
        }
        if !self.to_writer.alerts.is_empty() {
            parts.push(format!("alert: {}", self.to_writer.alerted()));
        }
        if self.serial_step() != 0 {
            parts.push(format!("serial {:+}", self.serial_step()));
        }
        if self.items_step() != 0 {
            parts.push(format!("items {:+}", self.items_step()));
        }
        if parts.is_empty() {
            "nothing".to_owned()
        } else {
            parts.join(", ")
        }
    }

    /// [`outcome`](Self::outcome) of a write by the owner of `cube`.
    fn brought(&self, cube: &Object) -> String {
        self.outcome(&self.to_writer.told(cube, self.before.serial))
    }
}

/// The legs, in order. Everything rezzed and every item made is noted in
/// `residue` as it appears, so the caller can take it away when a leg fails.
#[expect(
    clippy::too_many_lines,
    reason = "one linear flow of seven legs, each a request and a window to hear its answer in"
)]
async fn census(
    ctx: &mut TestContext,
    folders: Folders,
    residue: &mut Residue,
    metrics: &mut Metrics,
) -> Result<(), TestFailure> {
    let grid = ctx.grid();
    let build_position = ctx.build_position();
    let pace = Pace::of(grid);
    let drain = if grid.is_fake() { FAKE_DRAIN } else { DRAIN };
    let Some((primary, secondary)) = ctx.primary_and_secondary() else {
        return Err(TestFailure::State(
            "task-inventory needs a secondary avatar".to_owned(),
        ));
    };
    let owner = primary
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;

    // Both avatars to the spot, each with its scene settled.
    primary.wait_for_region(REGION_TIMEOUT).await?;
    let (seen, anchor) = settle_scene(
        primary,
        grid,
        build_position.clone(),
        SETTLE_WINDOW,
        pace.settle_idle,
    )
    .await?;
    secondary.wait_for_region(REGION_TIMEOUT).await?;
    settle_scene(
        secondary,
        grid,
        build_position,
        SETTLE_WINDOW,
        pace.settle_idle,
    )
    .await?;
    let anchor = anchor.ok_or_else(|| {
        TestFailure::Assertion("the arrival streamed nothing to place a cube against".to_owned())
    })?;
    let mut run = Run {
        folders,
        owner,
        pace,
        position: sl_client_tokio::Vector {
            x: anchor.x,
            y: anchor.y,
            z: anchor.z + REZ_LIFT_M,
        },
        seen,
    };
    let http = http_client()?;

    // What the case drops: a notecard and a script of the agent's own.
    let notecard = create_item(
        primary,
        residue,
        &folders,
        AssetType::Notecard,
        InventoryType::Notecard,
        NOTECARD_NAME,
    )
    .await?;
    let mut notecard = notecard;
    primary
        .send(Command::UpdateInventoryAsset {
            location: AssetUpdateLocation::AgentInventory {
                item_id: notecard.item_id,
            },
            asset_type: UpdatableAssetType::Notecard,
            data: notecard_bytes(NOTECARD_TEXT),
        })
        .await?;
    let saved = listen_for(primary, STEP_TIMEOUT, pace.quiet, |heard| {
        !heard.saves.is_empty()
    })
    .await?;
    match saved.saves.first() {
        Some(Ok(asset)) => notecard.asset_id = *asset,
        other => {
            return Err(TestFailure::Assertion(format!(
                "the notecard's text was not saved: {other:?}"
            )));
        }
    }
    let script = create_item(
        primary,
        residue,
        &folders,
        AssetType::ScriptText,
        InventoryType::Script,
        SCRIPT_NAME,
    )
    .await?;
    metrics.set("agent_script_has_an_asset", !script.asset_id.is_nil());

    let cube = rez_cube(primary, &mut run, residue, 0).await?;
    let task = cube.full_id;
    metrics.set("cube", task.to_string());

    // --- 1. An empty prim, over both roads. ---
    let empty = listing(primary, cube.scoped_id(), task).await?;
    metrics.set("empty_serial", i64::from(empty.serial));
    metrics.set("empty_reply_names_a_file", empty.named_a_file);
    let listing_cap = primary.cap("RequestTaskInventory");
    metrics.set("listing_cap_granted", listing_cap.is_some());
    match &listing_cap {
        Some(base) => {
            let plain = cap_get(&http, &task_inventory_cap_url(base, task, None)).await?;
            metrics.set("cap_empty_answers", plain.summary(task));
            metrics.set("cap_reply_keys", plain.keys());
            let held = cap_get(
                &http,
                &task_inventory_cap_url(base, task, Some(empty.serial)),
            )
            .await?;
            metrics.set("cap_at_the_held_serial_answers", held.summary(task));
            let stale = cap_get(
                &http,
                &task_inventory_cap_url(base, task, Some(empty.serial.wrapping_add(7))),
            )
            .await?;
            metrics.set("cap_at_another_serial_answers", stale.summary(task));
            let nobody = ObjectKey::from(Uuid::from_u128(0x000B_1EC7_0000_DEAD));
            let unknown = cap_get(&http, &task_inventory_cap_url(base, nobody, None)).await?;
            metrics.set("cap_of_an_unknown_prim_answers", unknown.summary(nobody));
            let bare = cap_get(&http, base).await?;
            metrics.set("cap_of_no_prim_answers", bare.summary(nobody));
        }
        None => {
            for key in [
                "cap_empty_answers",
                "cap_reply_keys",
                "cap_at_the_held_serial_answers",
                "cap_at_another_serial_answers",
                "cap_of_an_unknown_prim_answers",
                "cap_of_no_prim_answers",
            ] {
                metrics.set(key, NO_CAPABILITY);
            }
        }
    }
    // The client's own fetch, which takes the capability where there is one.
    metrics.set(
        "fetch_answers_with",
        fetch(primary, &cube, None, pace.quiet).await?,
    );
    metrics.set(
        "fetch_at_the_held_serial_answers_with",
        fetch(primary, &cube, Some(empty.serial), pace.quiet).await?,
    );

    // The watcher finds the cube; nobody holds it selected yet.
    let theirs = find_object(secondary, task).await?;
    listen(secondary, drain).await?;
    let mut census = Census {
        primary,
        secondary,
        run,
        drain,
        cube,
        theirs,
        held: empty,
    };

    // --- 2. A drop. ---
    let dropped = census.write(census.drop_of(&notecard)?).await?;
    metrics.set("drop_brings", dropped.brought(&census.cube));
    metrics.set("drop_changes_flags", dropped.flags_changed());
    metrics.set(
        "drop_agent_item",
        if dropped.to_writer.removed.contains(&notecard.item_id) {
            "taken out of the inventory"
        } else {
            "kept"
        },
    );
    if let Some(item) = dropped.added() {
        metrics.set("drop_keeps_the_name", item.name == notecard.name);
        metrics.set(
            "drop_item_id",
            if item.item_id == notecard.item_id {
                "the agent item's"
            } else {
                "a new one"
            },
        );
        metrics.set("drop_parent_is_the_prim", item.parent_task == task);
        metrics.set(
            "drop_asset",
            match item.asset_id {
                Some(asset) if asset.uuid() == notecard.asset_id => "the agent item's",
                Some(_) => "another",
                None => "withheld",
            },
        );
        metrics.set(
            "drop_keeps_the_masks",
            item.permissions == notecard.permissions,
        );
        metrics.set(
            "drop_masks",
            format!(
                "{}/{}/{}/{}/{}",
                hex(item.permissions.base),
                hex(item.permissions.owner),
                hex(item.permissions.group),
                hex(item.permissions.everyone),
                hex(item.permissions.next_owner)
            ),
        );
        metrics.set(
            "agent_item_masks",
            format!(
                "{}/{}/{}/{}/{}",
                hex(notecard.permissions.base),
                hex(notecard.permissions.owner),
                hex(notecard.permissions.group),
                hex(notecard.permissions.everyone),
                hex(notecard.permissions.next_owner)
            ),
        );
        metrics.set("drop_group_owned", item.group_owned);
    }
    // The same contents over the capability.
    match &listing_cap {
        Some(base) => {
            let answer = cap_get(&http, &task_inventory_cap_url(base, task, None)).await?;
            metrics.set(
                "roads_agree",
                match answer.listing(task) {
                    Some((serial, items)) => {
                        differences(&items, &census.held.items, serial, census.held.serial)
                    }
                    None => format!("the capability answered {}", answer.summary(task)),
                },
            );
            let first = answer
                .body
                .as_ref()
                .and_then(|body| body.get("contents"))
                .and_then(Llsd::as_array)
                .and_then(<[Llsd]>::first);
            metrics.set(
                "cap_item_shape",
                first.map_or_else(|| "no item".to_owned(), shape_of),
            );
            let text = |key: &str| {
                first
                    .and_then(|item| item.get(key))
                    .and_then(Llsd::as_str)
                    .unwrap_or("-")
                    .to_owned()
            };
            metrics.set(
                "cap_item_type_names",
                format!(
                    "{}/{}/{}",
                    text("type"),
                    text("inv_type"),
                    first
                        .and_then(|item| item.get("sale_info"))
                        .and_then(|sale| sale.get("sale_type"))
                        .and_then(Llsd::as_str)
                        .unwrap_or("-")
                ),
            );
        }
        None => {
            metrics.set("roads_agree", NO_CAPABILITY);
            metrics.set("cap_item_shape", NO_CAPABILITY);
            metrics.set("cap_item_type_names", NO_CAPABILITY);
        }
    }
    // The same item, dropped again.
    let again = census.write(census.drop_of(&notecard)?).await?;
    metrics.set("second_drop_brings", again.brought(&census.cube));
    metrics.set(
        "second_drop_names",
        names(&census.held, AssetType::Notecard),
    );

    // --- 3. A script, made new and dropped out of the inventory. ---
    let made = census
        .write(census.new_script(NEW_SCRIPT_NAME, true))
        .await?;
    metrics.set("new_script_brings", made.brought(&census.cube));
    metrics.set("new_script_changes_flags", made.flags_changed());
    metrics.set("new_script_said", said(&made.to_writer));
    let new_script = made.added();
    match &new_script {
        Some(item) => {
            metrics.set("new_script_name", item.name.clone());
            metrics.set("new_script_runs", census.running(item.item_id).await?);
        }
        None => metrics.set("new_script_name", "none landed"),
    }
    let rezzed = census.write(census.script_of(&script)?).await?;
    metrics.set("inventory_script_brings", rezzed.brought(&census.cube));
    metrics.set("inventory_script_said", said(&rezzed.to_writer));
    match rezzed.added() {
        Some(item) => {
            metrics.set("inventory_script_keeps_the_name", item.name == script.name);
            metrics.set(
                "inventory_script_item_id",
                if item.item_id == script.item_id {
                    "the agent item's"
                } else {
                    "a new one"
                },
            );
            metrics.set("inventory_script_runs", census.running(item.item_id).await?);
        }
        None => metrics.set("inventory_script_item_id", "none landed"),
    }
    let stopped = census
        .write(census.new_script(STOPPED_SCRIPT_NAME, false))
        .await?;
    metrics.set("stopped_script_brings", stopped.brought(&census.cube));
    metrics.set("stopped_script_said", said(&stopped.to_writer));
    if let Some(item) = stopped.added() {
        metrics.set("stopped_script_runs", census.running(item.item_id).await?);
    }

    // --- 4. Who is told. ---
    for (index, who) in [(1_u8, "both"), (2, "the_writer"), (3, "nobody")] {
        match index {
            1 => {
                census
                    .primary
                    .send(Command::RequestObjectProperties {
                        local_ids: vec![census.cube.scoped_id()],
                    })
                    .await?;
                census
                    .secondary
                    .send(Command::RequestObjectProperties {
                        local_ids: vec![census.theirs],
                    })
                    .await?;
            }
            2 => {
                census
                    .secondary
                    .send(Command::DeselectObjects {
                        local_ids: vec![census.theirs],
                    })
                    .await?;
            }
            _ => {
                census
                    .primary
                    .send(Command::DeselectObjects {
                        local_ids: vec![census.cube.scoped_id()],
                    })
                    .await?;
            }
        }
        // Whatever the select or the deselect itself brought is not a
        // write's doing.
        listen(census.primary, census.run.pace.quiet).await?;
        listen(census.secondary, census.drain).await?;

        let script_name = format!("SLClientContentsRound{index}");
        let made = census.write(census.new_script(&script_name, true)).await?;
        note_told(metrics, "script", who, &made, &census.cube);
        let round_script = made.added().map(|item| item.item_id);

        let dropped = census.write(census.drop_of(&notecard)?).await?;
        note_told(metrics, "drop", who, &dropped, &census.cube);
        let round_note = dropped.added().map(|item| item.item_id);

        if let Some(item_id) = round_script {
            let removed = census
                .write(Command::RemoveTaskInventory {
                    target: census.cube.scoped_id(),
                    item_id,
                })
                .await?;
            note_told(metrics, "remove", who, &removed, &census.cube);
            residue.items.extend(
                removed
                    .to_writer
                    .announced
                    .iter()
                    .map(|(_shape, item)| item.item_id),
            );
            if index == 3 {
                metrics.set("remove_brings", removed.brought(&census.cube));
                metrics.set("remove_lands", !removed.after.holds(item_id));
                metrics.set(
                    "remove_files_the_item_in",
                    removed
                        .to_writer
                        .announced
                        .first()
                        .map_or("no folder", |(_shape, item)| folders.word(item.folder_id)),
                );
            }
        }
        if let Some(item_id) = round_note {
            let moved = census
                .write(Command::MoveTaskInventory {
                    target: census.cube.scoped_id(),
                    folder_id: folders.root,
                    item_id,
                })
                .await?;
            note_told(metrics, "move", who, &moved, &census.cube);
            residue.items.extend(
                moved
                    .to_writer
                    .announced
                    .iter()
                    .map(|(_shape, item)| item.item_id),
            );
            if index == 3 {
                metrics.set("move_brings", moved.brought(&census.cube));
                metrics.set(
                    "move_leaves_the_prim",
                    if moved.after.holds(item_id) {
                        "holding the item"
                    } else {
                        "without the item"
                    },
                );
                // By its type: a second copy of an item in a prim is given
                // a name of its own.
                let arrived = moved.to_writer.announced.iter().find(|(_shape, item)| {
                    i32::from(item.item_type) == AssetType::Notecard.to_code()
                });
                metrics.set(
                    "move_announced_as",
                    arrived.map_or("nothing", |(shape, _item)| *shape),
                );
                if let Some((_shape, item)) = arrived {
                    metrics.set("move_item_name", item.name.clone());
                    metrics.set(
                        "move_item_id",
                        if item.item_id == item_id {
                            "the prim item's"
                        } else if item.item_id == notecard.item_id {
                            "the agent item's"
                        } else {
                            "a new one"
                        },
                    );
                    metrics.set("move_lands_in", folders.word(item.folder_id));
                }
            }
        }
    }

    // --- 5. Writes that cannot land. Nobody holds the cube selected. ---
    let nothing = InventoryKey::from(Uuid::from_u128(0x000B_1EC7_0000_BEEF));
    let cube_id = census.cube.scoped_id();
    // First one that can, and is no script: a script's remove is also the
    // script's stop.
    let a_note = census
        .held
        .items
        .iter()
        .find(|item| item.asset_type == AssetType::Notecard)
        .map(|item| item.item_id);
    if let Some(item_id) = a_note {
        let removed = census
            .write(Command::RemoveTaskInventory {
                target: cube_id,
                item_id,
            })
            .await?;
        metrics.set("note_remove_brings", removed.brought(&census.cube));
        residue.items.extend(
            removed
                .to_writer
                .announced
                .iter()
                .map(|(_shape, item)| item.item_id),
        );
    }
    let removed = census
        .write(Command::RemoveTaskInventory {
            target: cube_id,
            item_id: nothing,
        })
        .await?;
    metrics.set(
        "remove_of_an_unknown_item_brings",
        removed.brought(&census.cube),
    );
    let moved = census
        .write(Command::MoveTaskInventory {
            target: cube_id,
            folder_id: folders.root,
            item_id: nothing,
        })
        .await?;
    metrics.set(
        "move_of_an_unknown_item_brings",
        moved.brought(&census.cube),
    );
    let mut unheld = notecard.clone();
    unheld.item_id = nothing;
    let dropped = census.write(census.drop_of(&unheld)?).await?;
    metrics.set(
        "drop_of_an_unheld_item_brings",
        dropped.brought(&census.cube),
    );
    let mut unheld_script = script.clone();
    unheld_script.item_id = nothing;
    let rezzed = census.write(census.script_of(&unheld_script)?).await?;
    metrics.set(
        "script_of_an_unheld_item_brings",
        rezzed.brought(&census.cube),
    );

    // --- 6. A script's experience. ---
    let metadata_cap = census.primary.cap("GetMetadata");
    metrics.set("metadata_cap_granted", metadata_cap.is_some());
    let asked_about = new_script.as_ref().map_or(nothing, |item| item.item_id);
    census
        .primary
        .send(Command::RequestScriptExperience {
            task,
            item_id: asked_about,
        })
        .await?;
    let experience = census
        .primary
        .wait_for(
            census.run.pace.quiet.saturating_mul(2),
            |event| match event {
                Event::ScriptExperience {
                    task: of,
                    item_id,
                    experience,
                } if *of == task && *item_id == asked_about => Some(*experience),
                _ => None,
            },
        )
        .await;
    metrics.set(
        "script_experience",
        match experience {
            Ok(Some(_)) => "an experience",
            Ok(None) => "none",
            Err(TestFailure::Timeout(_)) => "no answer",
            Err(other) => return Err(other),
        },
    );
    match &metadata_cap {
        Some(url) => {
            let known = cap_post(
                &http,
                url,
                sl_client_tokio::build_get_metadata_request(task, asked_about),
            )
            .await?;
            metrics.set("metadata_answers", known.shape());
            let unknown = cap_post(
                &http,
                url,
                sl_client_tokio::build_get_metadata_request(task, nothing),
            )
            .await?;
            metrics.set("metadata_of_an_unknown_item_answers", unknown.shape());
        }
        None => {
            metrics.set("metadata_answers", NO_CAPABILITY);
            metrics.set("metadata_of_an_unknown_item_answers", NO_CAPABILITY);
        }
    }

    // --- 7. A gesture and a material saved into the prim. ---
    for (what, asset_type, inv_type, updatable, name, body) in [
        (
            "gesture",
            AssetType::Gesture,
            InventoryType::Gesture,
            UpdatableAssetType::Gesture,
            GESTURE_NAME,
            GESTURE_BODY.to_vec(),
        ),
        (
            "material",
            AssetType::Material,
            InventoryType::Material,
            UpdatableAssetType::Material,
            MATERIAL_NAME,
            sl_test_assets::gltf_material_asset([0.25, 0.5, 0.75, 1.0], None),
        ),
    ] {
        let granted = census.primary.cap(updatable.task_cap()).is_some();
        metrics.set(&format!("{what}_task_cap_granted"), granted);
        if !granted {
            for metric in [
                "save",
                "save_brings",
                "save_told_other",
                "save_changes_the_asset",
                "save_names_the_listed_asset",
            ] {
                metrics.set(&format!("{what}_{metric}"), NO_CAPABILITY);
            }
            continue;
        }
        let item = create_item(
            census.primary,
            residue,
            &folders,
            asset_type,
            inv_type,
            name,
        )
        .await?;
        let dropped = census.write(census.drop_of(&item)?).await?;
        let Some(held) = dropped.added() else {
            metrics.set(&format!("{what}_save"), "the drop did not land");
            continue;
        };
        census
            .primary
            .send(Command::UpdateInventoryAsset {
                location: AssetUpdateLocation::TaskInventory {
                    task_id: task,
                    item_id: held.item_id,
                },
                asset_type: updatable,
                data: body,
            })
            .await?;
        let quiet = census.run.pace.quiet;
        let to_writer = listen_for(census.primary, STEP_TIMEOUT, quiet, |heard| {
            !heard.saves.is_empty()
        })
        .await?;
        let saved = census.written(to_writer).await?;
        metrics.set(
            &format!("{what}_save"),
            match saved.to_writer.saves.first() {
                Some(Ok(_asset)) => "saved".to_owned(),
                Some(Err(reason)) => format!("refused: {reason}"),
                None => "no answer".to_owned(),
            },
        );
        metrics.set(&format!("{what}_save_brings"), saved.brought(&census.cube));
        metrics.set(
            &format!("{what}_save_told_other"),
            saved.to_other.told(&census.cube, saved.before.serial),
        );
        let asset_now = saved
            .after
            .items
            .iter()
            .find(|item| item.item_id == held.item_id)
            .and_then(|item| item.asset_id);
        metrics.set(
            &format!("{what}_save_changes_the_asset"),
            asset_now.is_some() && asset_now != held.asset_id,
        );
        metrics.set(
            &format!("{what}_save_names_the_listed_asset"),
            match (saved.to_writer.saves.first(), asset_now) {
                (Some(Ok(saved)), Some(listed)) => *saved == listed.uuid(),
                _ => false,
            },
        );
    }

    metrics.set("final_serial", i64::from(census.held.serial));
    metrics.set("final_items", count(census.held.items.len()));
    Ok(())
}

/// Records who was told of one write of a round.
fn note_told(metrics: &mut Metrics, write: &str, who: &str, written: &Written, cube: &Object) {
    metrics.set(
        &format!("{write}_with_{who}_selecting_told_writer"),
        written.to_writer.told(cube, written.before.serial),
    );
    // The watcher stands in the same region, where the cube has the one
    // local id.
    metrics.set(
        &format!("{write}_with_{who}_selecting_told_other"),
        written.to_other.told(cube, written.before.serial),
    );
    if who == "both" {
        metrics.set(
            &format!("{write}_record_serial_steps"),
            written
                .to_writer
                .record_serial_steps(cube.full_id, written.before.serial),
        );
    }
}

/// What a script said in the window, `|`-joined, or `nothing`.
fn said(heard: &Heard) -> String {
    if heard.chats.is_empty() {
        "nothing".to_owned()
    } else {
        heard.chats.join(" | ")
    }
}

/// The names of a listing's items of one type, sorted and `|`-joined.
fn names(listing: &Listing, asset_type: AssetType) -> String {
    let mut names: Vec<&str> = listing
        .items
        .iter()
        .filter(|item| item.asset_type == asset_type)
        .map(|item| item.name.as_str())
        .collect();
    names.sort_unstable();
    names.join(" | ")
}

/// How the capability's listing of a prim differs from the UDP road's, or
/// `they agree`.
fn differences(
    over_cap: &[TaskInventoryItem],
    over_udp: &[TaskInventoryItem],
    cap_serial: i16,
    udp_serial: i16,
) -> String {
    let mut parts = Vec::new();
    if cap_serial != udp_serial {
        parts.push(format!("serial {cap_serial} against {udp_serial}"));
    }
    if over_cap.len() != over_udp.len() {
        parts.push(format!(
            "{} item(s) against {}",
            over_cap.len(),
            over_udp.len()
        ));
    }
    for item in over_cap {
        let Some(other) = over_udp.iter().find(|other| other.item_id == item.item_id) else {
            parts.push(format!("{} only over the capability", item.name));
            continue;
        };
        if other == item {
            continue;
        }
        tracing::info!(?item, ?other, "the two roads list an item differently");
        let mut fields = Vec::new();
        if item.parent_task != other.parent_task {
            fields.push("parent");
        }
        if item.permissions != other.permissions {
            fields.push("masks");
        }
        if item.creator_id != other.creator_id
            || item.owner != other.owner
            || item.last_owner_id != other.last_owner_id
            || item.group != other.group
            || item.group_owned != other.group_owned
        {
            fields.push("ownership");
        }
        if item.asset_id != other.asset_id {
            fields.push("asset");
        }
        if item.asset_type != other.asset_type || item.inv_type != other.inv_type {
            fields.push("type");
        }
        if item.flags != other.flags {
            fields.push("flags");
        }
        if item.sale_type != other.sale_type || item.sale_price != other.sale_price {
            fields.push("sale");
        }
        if item.name != other.name || item.description != other.description {
            fields.push("name or description");
        }
        if item.creation_date != other.creation_date {
            fields.push("date");
        }
        parts.push(format!("{} in {}", item.name, fields.join("+")));
    }
    if parts.is_empty() {
        "they agree".to_owned()
    } else {
        format!("they differ: {}", parts.join("; "))
    }
}

/// Asks for the contents of `object` the way a client does — over the
/// capability where the region grants it — and says what answered: a
/// `reply`, the `contents`, `unchanged`, or `nothing`.
async fn fetch(
    session: &mut Session,
    object: &Object,
    held_serial: Option<i16>,
    quiet: Duration,
) -> Result<String, TestFailure> {
    let task = object.full_id;
    session
        .send(Command::FetchTaskInventory {
            target: object.scoped_id(),
            held_serial,
        })
        .await?;
    let mut parts: Vec<&str> = Vec::new();
    let outcome = session
        .wait_for(
            STEP_TIMEOUT.min(quiet.saturating_mul(4)),
            |event| match event {
                Event::TaskInventoryReply(reply) if reply.task == task => {
                    parts.push("reply");
                    None
                }
                Event::TaskInventoryContents { task: of, .. } if *of == task => {
                    parts.push("contents");
                    Some(())
                }
                Event::TaskInventoryUnchanged { task: of, .. } if *of == task => {
                    parts.push("unchanged");
                    Some(())
                }
                _ => None,
            },
        )
        .await;
    match outcome {
        Ok(()) | Err(TestFailure::Timeout(_)) => {}
        Err(other) => return Err(other),
    }
    Ok(words(&parts))
}

/// Creates an item of the agent's own in the inventory root and notes it for
/// the cleanup.
pub(crate) async fn create_item(
    session: &mut Session,
    residue: &mut Residue,
    folders: &Folders,
    asset_type: AssetType,
    inv_type: InventoryType,
    name: &str,
) -> Result<InventoryItem, TestFailure> {
    session
        .send(Command::CreateInventoryItem(NewInventoryItem {
            folder_id: folders.root,
            transaction_id: Uuid::nil(),
            next_owner_mask: Permissions::ALL.bits(),
            asset_type,
            inv_type,
            wearable_type: WearableType::Shape,
            name: name.to_owned(),
            description: "sl-conformance task-inventory".to_owned(),
        }))
        .await?;
    let wanted = name.to_owned();
    let item = session
        .wait_for(STEP_TIMEOUT, move |event| match event {
            Event::InventoryItemCreated { item, .. } if item.name == wanted => Some(item.clone()),
            _ => None,
        })
        .await
        .map_err(|failure| match failure {
            TestFailure::Timeout(_) => {
                TestFailure::Assertion(format!("the grid never created the item {name}"))
            }
            other => other,
        })?;
    residue.items.push(item.item_id);
    Ok(item)
}

/// The id `session` knows the object `object_id` by, waiting for the region
/// to send it if it has not yet.
pub(crate) async fn find_object(
    session: &mut Session,
    object_id: ObjectKey,
) -> Result<ScopedObjectId, TestFailure> {
    session
        .wait_for(STEP_TIMEOUT, |event| match event {
            Event::ObjectAdded(object) | Event::ObjectUpdated(object)
                if object.full_id == object_id =>
            {
                Some(object.scoped_id())
            }
            _ => None,
        })
        .await
        .map_err(|failure| match failure {
            TestFailure::Timeout(_) => TestFailure::Assertion(
                "the other avatar was never sent the cube the first one rezzed".to_owned(),
            ),
            other => other,
        })
}

/// `text` as a notecard asset (`Linden text version 2`, nothing embedded).
pub(crate) fn notecard_bytes(text: &str) -> Vec<u8> {
    format!(
        "Linden text version 2\n{{\nLLEmbeddedItems version 1\n{{\ncount 0\n}}\nText length {}\n{}}}\n",
        text.len(),
        text,
    )
    .into_bytes()
}

/// The agent's `item` as a drop into the prim `task` carries it, with the
/// checksum Second Life holds a drop to.
pub(crate) fn drop_item(item: &InventoryItem, task: ObjectKey) -> Result<RestoreItem, TestFailure> {
    RestoreItem::for_task_drop(item, task, Uuid::new_v4())
        .map_err(|error| TestFailure::Assertion(format!("the item does not encode: {error}")))
}

/// Builds the [`RestoreItem`] for an [`Command::UpdateTaskInventory`] that drops
/// the taken agent inventory `item` into a prim, without the checksum: what
/// the object cases send, which OpenSim takes and the checksum-free legs of
/// their censuses rest on.
pub(super) fn task_item(item: &InventoryItem) -> RestoreItem {
    RestoreItem {
        item_id: item.item_id,
        folder_id: item.folder_id,
        creator_id: item.creator_id,
        owner: item.owner,
        group: item.group,
        permissions: item.permissions,
        transaction_id: Uuid::new_v4(),
        asset_type: item.item_type,
        inv_type: item.inv_type,
        flags: item.flags,
        sale_type: SaleType::from_code(item.sale_type),
        sale_price: item.sale_price.clone(),
        name: item.name.clone(),
        description: item.description.clone(),
        creation_date: item.creation_date,
        crc: 0,
    }
}
