//! A census of how an object leaves a region and comes back: taken, copied,
//! deleted, returned, expired — and rezzed again out of the item that made.
//!
//! "Rez from inventory" needs a rezzable object *item*, which a test avatar is
//! not guaranteed to hold, so the case makes one: it rezzes a cube with
//! `ObjectAdd`, names it, and from there asks one question a leg, listening
//! after each for everything the grid says about it — the kills, the
//! inventory announcements, a `DeRezAck`, alerts and instant messages:
//!
//! 1. **Take a copy** (`DRD_ACQUIRE_TO_AGENT_INVENTORY`): does the object
//!    stay, how is the item announced, and what does the item say.
//! 2. **Take** (`DRD_TAKE_INTO_AGENT_INVENTORY`): the same, and the kill.
//! 3. **Restore** the taken item (`RezRestoreToWorld`, the viewer's Restore
//!    to Last Position): does an object come back, and where.
//! 4. **Rez the taken item**: what arrives, whether the item survives, and
//!    what the new object's record says of where it came from — its creation
//!    date, its item and folder, its last owner.
//! 5. **`ObjectDelete`** of that object: the reference viewer's force-delete.
//! 6. **Delete** (`DRD_TRASH`), once naming the Trash and once naming another
//!    folder: where the item lands.
//! 7. **Return** (`DRD_RETURN_TO_OWNER`) of the agent's own object.
//! 8. **A linkset**: what a take of the root kills, how many items it makes
//!    and how many prims a rez of it puts back.
//! 9. **Temporary**: how long a prim flagged temporary stands.
//!
//! What each live grid answered is stated in `MEASURED` and written up in
//! `book/src/gridspec/building.md` § Rez and take; every run, live or fake,
//! is held to it. What a grid does on land that does not let the avatar
//! build, and with somebody else's object, is [`super::object_rez_land`]'s.
//!
//! `1av`, `[both]`, offline on both fake flavours. Everything left standing
//! is derezzed to the Trash at the end, also when a leg fails, and the items
//! the legs made are removed from the inventory.

use std::collections::HashSet;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use sl_client_tokio::{
    AgentKey, AssetType, Command, DeRezDestination, Event, FolderType, InventoryFolder,
    InventoryFolderKey, InventoryItem, InventoryKey, Object, ObjectFlagSettings,
    ObjectProperties as Properties, ObjectUpdateForm, OwnerKey, PrimShape, RegionLocalObjectId,
    RestoreItem, RezObjectParams, SaleType, ScopedObjectId, TransactionId, Uuid, Vector,
};

use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::metrics::Metrics;
use crate::record::MetricValue;
use crate::registry::{GridTest, TestFuture};
use crate::support::{
    ANNOUNCED_BULK, ANNOUNCED_LEGACY, ANNOUNCED_NOTHING, REGION_TIMEOUT, REPLY_TIMEOUT, is_opensim,
    is_own, secs_metric, settle_scene, wait_for_own_new_object,
};

/// The OpenSim start location: the "Default Region" (1000,1000), centred, where
/// this workspace's test object stands as the rez placement reference.
const OPENSIM_START: &str = "uri:Default Region&128&128&30";

/// The overall budget for settling the initial scene.
pub(crate) const SETTLE_WINDOW: Duration = Duration::from_secs(15);

/// The idle gap that ends the settle.
pub(crate) const SETTLE_IDLE: Duration = Duration::from_secs(5);

/// [`SETTLE_IDLE`] on a fake grid.
pub(crate) const FAKE_SETTLE_IDLE: Duration = Duration::from_secs(1);

/// How long to wait for a step's confirming event.
pub(crate) const STEP_TIMEOUT: Duration = Duration::from_secs(30);

/// How long a leg goes on listening once it has heard what it was waiting
/// for — and how long it listens for an answer that may never come.
pub(crate) const QUIET: Duration = Duration::from_secs(4);

/// How long a return's instant message is waited for. OpenSim sends its with
/// the region's next backup, every 200 frames.
const NOTICE_WINDOW: Duration = Duration::from_secs(45);

/// [`QUIET`] on a fake grid.
pub(crate) const FAKE_QUIET: Duration = Duration::from_millis(400);

/// How long the temporary prim is watched for. Second Life's stand a minute;
/// OpenSim's are swept on a timer of its own.
const TEMPORARY_WINDOW: Duration = Duration::from_secs(600);

/// How many prims the temporary leg flags, one after another.
const TEMPORARY_PRIMS: u8 = 3;

/// How long a temporary prim is said to stand.
const TEMPORARY_NOMINAL: Duration = Duration::from_secs(60);

/// How far above the anchor the cubes are rezzed, in metres.
const REZ_LIFT_M: f32 = 1.0;

/// How far apart along the region's x axis the cubes stand, in metres.
const SPACING_M: f32 = 1.5;

/// The name the cube is given, so that the items made from it can be told
/// from every other `Object` in an inventory.
const CUBE_NAME: &str = "SLClientRezDerez";

/// The description the cube is given.
const CUBE_DESCRIPTION: &str = "object-rez-derez";

/// How far from where the cube stood a restored object may be and still be
/// where it stood, in metres.
const RESTORE_SLACK_M: f32 = 0.5;

/// How far from an instant an item's or an object's date may be and still be
/// that instant's, in seconds: the two clocks are a grid's and ours.
const DATE_SLACK_SECS: u64 = 5;

/// Where the measurements below are written down.
const SOURCE: &str = "book/src/gridspec/building.md § Rez and take (object-rez-derez, 2026-10-10)";

/// One measured answer: what a metric the census recorded reads on each grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Answer {
    /// A text metric.
    Text(&'static str),
    /// A flag.
    Flag(bool),
    /// A count.
    Count(i64),
}

impl Answer {
    /// Whether the recorded `value` is this answer.
    pub(crate) fn is(self, value: &MetricValue) -> bool {
        match (self, value) {
            (Self::Text(expected), MetricValue::Text(actual)) => expected == actual,
            (Self::Flag(expected), MetricValue::Bool(actual)) => expected == *actual,
            (Self::Count(expected), MetricValue::Int(actual)) => expected == *actual,
            _ => false,
        }
    }
}

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
    ("cube_named", both(Answer::Flag(true))),
    // A copy: the object stays.
    ("take_copy_kills_the_object", both(Answer::Flag(false))),
    ("take_copy_kill_entries", both(Answer::Count(0))),
    ("take_copy_announcement", both(LEGACY)),
    (
        "take_copy_announcement_transaction",
        each(Answer::Text("another"), Answer::Text("nil")),
    ),
    ("take_copy_items", both(Answer::Count(1))),
    ("take_copy_item_lands_in", both(Answer::Text("objects"))),
    ("take_copy_derez_ack", both(NONE)),
    ("take_copy_alerts", both(NONE)),
    ("take_copy_told_by", both(Answer::Text("nothing"))),
    // The item a copy makes.
    (
        "take_copy_item_name_is_the_objects",
        both(Answer::Flag(true)),
    ),
    (
        "take_copy_item_description_is_the_objects",
        both(Answer::Flag(true)),
    ),
    (
        "take_copy_item_asset_id_nil",
        each(Answer::Flag(true), Answer::Flag(false)),
    ),
    ("take_copy_item_asset_type", both(Answer::Count(6))),
    ("take_copy_item_inventory_type", both(Answer::Count(6))),
    ("take_copy_item_flags", both(Answer::Text("0x00000000"))),
    ("take_copy_item_sale_type", both(Answer::Count(0))),
    (
        "take_copy_item_owner_is_the_agent",
        both(Answer::Flag(true)),
    ),
    (
        "take_copy_item_creator_is_the_agent",
        both(Answer::Flag(true)),
    ),
    ("take_copy_item_group_set", both(Answer::Flag(false))),
    ("take_copy_item_mask_base", each(SL_ALL, OPENSIM_FOLDED)),
    ("take_copy_item_mask_owner", each(SL_ALL, OPENSIM_FOLDED)),
    ("take_copy_item_mask_group", both(NO_BITS)),
    ("take_copy_item_mask_everyone", both(NO_BITS)),
    ("take_copy_item_mask_next_owner", both(MOVE_AND_TRANSFER)),
    (
        "take_copy_item_mask_base_is_the_objects",
        each(Answer::Flag(true), Answer::Flag(false)),
    ),
    (
        "take_copy_item_mask_next_owner_is_the_objects",
        both(Answer::Flag(true)),
    ),
    // A take: the same item, and the object goes.
    ("take_kills_the_object", both(Answer::Flag(true))),
    (
        "take_kill_entries",
        each(Answer::Count(1), Answer::Count(2)),
    ),
    (
        "take_kill_messages",
        each(Answer::Count(1), Answer::Count(2)),
    ),
    ("take_announcement", both(LEGACY)),
    (
        "take_announcement_transaction",
        each(Answer::Text("another"), Answer::Text("nil")),
    ),
    ("take_items", both(Answer::Count(1))),
    ("take_item_lands_in", both(Answer::Text("objects"))),
    ("take_derez_ack", both(NONE)),
    ("take_alerts", both(NONE)),
    ("take_told_by", both(Answer::Text("nothing"))),
    ("take_item_name_is_the_objects", both(Answer::Flag(true))),
    (
        "take_item_asset_id_nil",
        each(Answer::Flag(true), Answer::Flag(false)),
    ),
    ("take_item_mask_base", each(SL_ALL, OPENSIM_FOLDED)),
    ("take_item_mask_owner", each(SL_ALL, OPENSIM_FOLDED)),
    ("take_item_mask_next_owner", both(MOVE_AND_TRANSFER)),
    // A Restore to Last Position of the taken item.
    (
        "restore_brings_an_object",
        each(Answer::Flag(true), Answer::Flag(false)),
    ),
    (
        "restore_puts_it",
        each(Answer::Text("where it stood"), Answer::Text("nowhere")),
    ),
    ("restore_removes_the_item", both(Answer::Flag(false))),
    ("restore_alerts", both(NONE)),
    // A rez of the taken item.
    ("rez_brings", both(Answer::Text("full update"))),
    ("rez_announcement", both(NONE)),
    ("rez_removes_the_item", both(Answer::Flag(false))),
    ("rez_alerts", both(NONE)),
    (
        "rezzed_creation_date",
        both(Answer::Text("the original object's")),
    ),
    ("rezzed_name_is_the_items", both(Answer::Flag(true))),
    ("rezzed_description_is_the_items", both(Answer::Flag(true))),
    ("rezzed_item_id", both(Answer::Text("the item's"))),
    (
        "rezzed_folder_id",
        each(Answer::Text("the item's folder"), NONE),
    ),
    ("rezzed_from_task_id_set", both(Answer::Flag(false))),
    ("rezzed_owner_is_the_agent", both(Answer::Flag(true))),
    ("rezzed_creator_is_the_agent", both(Answer::Flag(true))),
    ("rezzed_last_owner", both(Answer::Text("the agent"))),
    (
        "rezzed_mask_base_is_the_originals",
        both(Answer::Flag(true)),
    ),
    (
        "rezzed_mask_owner_is_the_originals",
        both(Answer::Flag(true)),
    ),
    (
        "rezzed_mask_next_owner_is_the_originals",
        both(Answer::Flag(true)),
    ),
    ("rezzed_inventory_serial", both(Answer::Count(0))),
    // `ObjectDelete`.
    (
        "object_delete_kills",
        each(Answer::Flag(true), Answer::Flag(false)),
    ),
    ("object_delete_announcement", both(NONE)),
    ("object_delete_item_lands_in", both(Answer::Text("nowhere"))),
    ("object_delete_alerts", both(NONE)),
    // A delete to the Trash.
    ("trash_kills_the_object", both(Answer::Flag(true))),
    (
        "trash_kill_messages",
        each(Answer::Count(1), Answer::Count(2)),
    ),
    ("trash_announcement", both(LEGACY)),
    ("trash_items", both(Answer::Count(1))),
    ("trash_item_lands_in", both(Answer::Text("trash"))),
    ("trash_derez_ack", both(NONE)),
    ("trash_told_by", both(Answer::Text("nothing"))),
    (
        "trash_naming_objects_kills_the_object",
        both(Answer::Flag(true)),
    ),
    ("trash_naming_objects_announcement", both(LEGACY)),
    (
        "trash_naming_objects_item_lands_in",
        each(Answer::Text("objects"), Answer::Text("trash")),
    ),
    // A return of the agent's own object.
    ("return_kills_the_object", both(Answer::Flag(true))),
    (
        "return_kill_messages",
        each(Answer::Count(1), Answer::Count(2)),
    ),
    ("return_announcement", both(LEGACY)),
    ("return_items", both(Answer::Count(1))),
    ("return_item_lands_in", both(Answer::Text("lost and found"))),
    ("return_derez_ack", both(NONE)),
    ("return_alerts", both(NONE)),
    ("return_told_by", both(Answer::Text("an instant message"))),
    (
        "return_notice_from",
        each(Answer::Text("Second Life"), Answer::Text("Server")),
    ),
    (
        "return_notice_sender_has_an_id",
        each(Answer::Flag(true), Answer::Flag(false)),
    ),
    // A linkset.
    (
        "linkset_take_kill_names",
        each(Answer::Text("root+child"), Answer::Text("root")),
    ),
    ("linkset_take_kill_entries", both(Answer::Count(2))),
    (
        "linkset_take_kill_messages",
        each(Answer::Count(1), Answer::Count(2)),
    ),
    ("linkset_take_announcement", both(LEGACY)),
    ("linkset_take_items", both(Answer::Count(1))),
    ("linkset_take_item_lands_in", both(Answer::Text("objects"))),
    ("linkset_rez_prims", both(Answer::Count(2))),
];

/// What both live grids were measured to answer where the fake grid has
/// nothing to say: it stamps an item with a fixed date, so that a seeded
/// grid mints the same run twice, and it keeps no clock for a temporary prim.
const MEASURED_LIVE: &[(&str, Measured<Answer>)] = &[
    ("take_copy_item_date", both(Answer::Text("the derez's"))),
    ("take_item_date", both(Answer::Text("the derez's"))),
    ("temporary_prims_expired", both(Answer::Count(3))),
    ("temporary_life", both(Answer::Text("a minute or more"))),
    ("temporary_expiry_announcement", both(NONE)),
];

/// The legacy UDP announcement.
const LEGACY: Answer = Answer::Text(ANNOUNCED_LEGACY);

/// Nothing of the kind asked about.
const NONE: Answer = Answer::Text("none");

/// Second Life's "everything" mask.
const SL_ALL: Answer = Answer::Text("0x7fffffff");

/// The base and owner masks OpenSim gives the item of an unedited prim.
const OPENSIM_FOLDED: Answer = Answer::Text("0x0008e00f");

/// An empty mask.
const NO_BITS: Answer = Answer::Text("0x00000000");

/// The next-owner mask of an unedited prim on both grids.
const MOVE_AND_TRANSFER: Answer = Answer::Text("0x00082000");

/// Holds what the census recorded to [`MEASURED`], naming every metric that
/// reads otherwise — or was never recorded.
fn hold_to_measured(grid: Grid, metrics: &Metrics) -> Result<(), TestFailure> {
    let live: &[(&str, Measured<Answer>)] = if grid.is_fake() { &[] } else { MEASURED_LIVE };
    let failures: Vec<String> = MEASURED
        .iter()
        .chain(live)
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
        "{grid}: {verdict} {SOURCE}: {}",
        failures.join("; ")
    )))
}

/// One inventory announcement: its shape, the transaction it names and the
/// items it carried.
#[derive(Debug, Clone)]
pub(crate) struct Announced {
    /// [`ANNOUNCED_LEGACY`] or [`ANNOUNCED_BULK`].
    pub(crate) shape: &'static str,
    /// The transaction id the message states.
    pub(crate) transaction: Uuid,
    /// The object items it carried.
    pub(crate) items: Vec<InventoryItem>,
}

/// What the session was sent during a window.
#[derive(Debug, Default)]
pub(crate) struct Heard {
    /// Every `KillObject` of the agent's region: how long after the window
    /// opened it came, and the ids it named.
    pub(crate) kills: Vec<(Duration, Vec<RegionLocalObjectId>)>,
    /// Every inventory announcement that carried an object item.
    pub(crate) announced: Vec<Announced>,
    /// How long after the window opened the first announcement came.
    pub(crate) first_announced_after: Option<Duration>,
    /// The items the grid said were removed from the inventory.
    pub(crate) removed_items: Vec<InventoryKey>,
    /// Every `DeRezAck`: its transaction and whether it reports success.
    pub(crate) derez_acks: Vec<(TransactionId, bool)>,
    /// The text of every alert.
    pub(crate) alerts: Vec<String>,
    /// Every instant message, as its dialog, its sender and its text.
    pub(crate) messages: Vec<String>,
    /// Who each instant message said it was from: the name, and whether an
    /// id came with it.
    pub(crate) senders: Vec<(String, bool)>,
    /// How long after the window opened the first instant message came.
    pub(crate) first_message_after: Option<Duration>,
    /// Every `ObjectProperties` record.
    pub(crate) properties: Vec<Properties>,
    /// Objects of the agent's region that were sent anew, in arrival order.
    pub(crate) added: Vec<Object>,
    /// The objects of the agent's region the stream named, and the form each
    /// came in.
    pub(crate) updated: Vec<(RegionLocalObjectId, ObjectUpdateForm)>,
}

impl Heard {
    /// How many kill entries named `object`.
    pub(crate) fn kill_entries(&self, object: ScopedObjectId) -> usize {
        self.kills
            .iter()
            .flat_map(|(_after, killed)| killed)
            .filter(|killed| **killed == object.id())
            .count()
    }

    /// How long after the window opened `object` was first killed.
    pub(crate) fn killed_after(&self, object: ScopedObjectId) -> Option<Duration> {
        self.kills
            .iter()
            .find(|(_after, killed)| killed.contains(&object.id()))
            .map(|(after, _killed)| *after)
    }

    /// Whether `object` was killed.
    pub(crate) fn killed(&self, object: ScopedObjectId) -> bool {
        self.kill_entries(object) > 0
    }

    /// Every object item announced, in arrival order.
    pub(crate) fn items(&self) -> impl Iterator<Item = &InventoryItem> {
        self.announced
            .iter()
            .flat_map(|announced| announced.items.iter())
    }

    /// The shapes the items were announced in, `+`-joined, or
    /// [`ANNOUNCED_NOTHING`].
    pub(crate) fn announcement(&self) -> String {
        if self.announced.is_empty() {
            return ANNOUNCED_NOTHING.to_owned();
        }
        self.announced
            .iter()
            .map(|announced| announced.shape)
            .collect::<Vec<_>>()
            .join("+")
    }

    /// Adds what a later window heard of alerts and instant messages, its
    /// times counted from `offset` after this one opened.
    pub(crate) fn hear_more(&mut self, later: Self, offset: Duration) {
        if self.first_message_after.is_none() {
            self.first_message_after = later
                .first_message_after
                .map(|after| after.saturating_add(offset));
        }
        self.alerts.extend(later.alerts);
        self.messages.extend(later.messages);
        self.senders.extend(later.senders);
    }

    /// Records who the first instant message said it was from, and when it
    /// came, under `prefix`.
    pub(crate) fn record_notice(&self, metrics: &mut Metrics, prefix: &str) {
        let (from, with_id) = self
            .senders
            .first()
            .map_or(("nobody", false), |(name, with_id)| {
                (name.as_str(), *with_id)
            });
        metrics.set(&format!("{prefix}_notice_from"), from.to_owned());
        metrics.set(&format!("{prefix}_notice_sender_has_an_id"), with_id);
        if let Some(after) = self.first_message_after {
            metrics.set_timing(
                &secs_metric(&format!("{prefix}_notice")),
                after.as_secs_f64(),
            );
        }
    }

    /// How the agent was told of what happened, apart from the kill and the
    /// item: by an alert, by an instant message, by both or by nothing.
    pub(crate) const fn told_by(&self) -> &'static str {
        match (self.alerts.is_empty(), self.messages.is_empty()) {
            (true, true) => "nothing",
            (false, true) => "an alert",
            (true, false) => "an instant message",
            (false, false) => "an alert and an instant message",
        }
    }

    /// The forms `object` was sent in, `+`-joined, with `properties` where its
    /// record came too — or `nothing`.
    fn brought(&self, object: &Object) -> String {
        let mut parts = Vec::new();
        for (form, word) in [
            (ObjectUpdateForm::Full, "full update"),
            (ObjectUpdateForm::Compressed, "compressed update"),
            (ObjectUpdateForm::Terse, "terse update"),
        ] {
            if self.updated.contains(&(object.local_id, form)) {
                parts.push(word);
            }
        }
        if self
            .properties
            .iter()
            .any(|properties| properties.object_id == object.full_id)
        {
            parts.push("properties");
        }
        if parts.is_empty() {
            "nothing".to_owned()
        } else {
            parts.join("+")
        }
    }
}

/// Listens on `session` until `done` says the leg has what it came for and
/// `quiet` more has passed, or for `limit` at most, keeping everything a
/// rez or a derez may be answered with.
pub(crate) async fn listen(
    session: &mut Session,
    limit: Duration,
    quiet: Duration,
    mut done: impl FnMut(&Heard) -> bool,
) -> Result<Heard, TestFailure> {
    let mut heard = Heard::default();
    let object_type = AssetType::Object.to_code();
    let root = session.circuit_id();
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
                let since = started.elapsed();
                match event {
                    Event::ObjectStreamBatch(batch) if !batch.child => {
                        if batch.form == ObjectUpdateForm::Kill {
                            heard.kills.push((
                                since,
                                batch.entries.iter().map(|entry| entry.local_id).collect(),
                            ));
                        } else {
                            heard.updated.extend(
                                batch
                                    .entries
                                    .iter()
                                    .map(|entry| (entry.local_id, batch.form)),
                            );
                        }
                    }
                    Event::ObjectAdded(object)
                        if root.is_none_or(|root| object.circuit == root) =>
                    {
                        heard.added.push((**object).clone());
                    }
                    Event::ObjectProperties(properties) => {
                        heard.properties.push((**properties).clone());
                    }
                    Event::InventoryItemCreated {
                        item,
                        transaction_id,
                        ..
                    } if i32::from(item.item_type) == object_type => {
                        heard.first_announced_after.get_or_insert(since);
                        heard.announced.push(Announced {
                            shape: ANNOUNCED_LEGACY,
                            transaction: *transaction_id,
                            items: vec![item.clone()],
                        });
                    }
                    Event::InventoryBulkUpdate {
                        items,
                        transaction_id,
                        ..
                    } => {
                        let objects: Vec<InventoryItem> = items
                            .iter()
                            .filter(|item| i32::from(item.item_type) == object_type)
                            .cloned()
                            .collect();
                        if !objects.is_empty() {
                            heard.first_announced_after.get_or_insert(since);
                            heard.announced.push(Announced {
                                shape: ANNOUNCED_BULK,
                                transaction: *transaction_id,
                                items: objects,
                            });
                        }
                    }
                    Event::InventoryItemsRemoved { items }
                    | Event::InventoryObjectsRemoved { items, .. } => {
                        heard.removed_items.extend(items.iter().copied());
                    }
                    Event::DeRezAck {
                        transaction,
                        success,
                    } => heard.derez_acks.push((*transaction, *success)),
                    Event::AlertMessage {
                        message,
                        alert_info,
                        ..
                    } => heard.alerts.push(alert_info.first().map_or_else(
                        || message.clone(),
                        |info| format!("{message} [{}]", info.message),
                    )),
                    Event::AgentAlertMessage { message, .. } => {
                        heard.alerts.push(message.clone());
                    }
                    Event::InstantMessageReceived(message) => {
                        heard.first_message_after.get_or_insert(since);
                        heard.senders.push((
                            message.from_agent_name.clone(),
                            !message.from_agent_id.uuid().is_nil(),
                        ));
                        heard.messages.push(format!(
                            "{:?} from {:?} ({}): {}",
                            message.dialog,
                            message.from_agent_name,
                            message.from_agent_id,
                            message.message
                        ));
                    }
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

/// The objects the case rezzed and has to take away again, and the items its
/// legs made.
#[derive(Debug, Default)]
struct Residue {
    /// The roots still standing.
    roots: Vec<ScopedObjectId>,
    /// The inventory items the grid announced.
    items: Vec<InventoryKey>,
}

/// The folders of a login skeleton the case files things into.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Folders {
    /// The inventory root.
    pub(crate) root: InventoryFolderKey,
    /// Where a take puts the cube.
    pub(crate) objects: InventoryFolderKey,
    /// The Trash.
    pub(crate) trash: InventoryFolderKey,
    /// The Lost And Found.
    pub(crate) lost_and_found: InventoryFolderKey,
}

impl Folders {
    /// The well-known folders of a login skeleton, each falling back to its
    /// root.
    pub(crate) fn of(folders: &[InventoryFolder]) -> Option<Self> {
        let root = folders
            .iter()
            .find(|folder| folder.parent_id.is_none())
            .map(|folder| folder.folder_id)?;
        let of_type = |folder_type: FolderType| {
            folders
                .iter()
                .find(|folder| folder.folder_type == folder_type.to_code())
                .map_or(root, |folder| folder.folder_id)
        };
        Some(Self {
            root,
            objects: of_type(FolderType::Object),
            trash: of_type(FolderType::Trash),
            lost_and_found: of_type(FolderType::LostAndFound),
        })
    }

    /// The word a metric names `folder` by.
    pub(crate) fn word(&self, folder: InventoryFolderKey) -> &'static str {
        if folder == self.objects {
            "objects"
        } else if folder == self.trash {
            "trash"
        } else if folder == self.lost_and_found {
            "lost and found"
        } else if folder == self.root {
            "root"
        } else {
            "another folder"
        }
    }
}

/// The durations a run listens for, which a loopback grid shortens.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Pace {
    /// [`QUIET`].
    pub(crate) quiet: Duration,
    /// [`SETTLE_IDLE`].
    pub(crate) settle_idle: Duration,
    /// [`NOTICE_WINDOW`].
    pub(crate) notice: Duration,
}

impl Pace {
    /// The pace of a run on `grid`.
    pub(crate) const fn of(grid: Grid) -> Self {
        if grid.is_fake() {
            Self {
                quiet: FAKE_QUIET,
                settle_idle: FAKE_SETTLE_IDLE,
                notice: FAKE_QUIET,
            }
        } else {
            Self {
                quiet: QUIET,
                settle_idle: SETTLE_IDLE,
                notice: NOTICE_WINDOW,
            }
        }
    }
}

/// A census of rez and derez: what each way of taking an object out of a
/// region is answered with, and what a rez puts back.
#[derive(Debug)]
pub struct ObjectRezDerez;

impl GridTest for ObjectRezDerez {
    fn name(&self) -> &'static str {
        "object-rez-derez"
    }

    fn description(&self) -> &'static str {
        "What a take, a copy, a delete, a return and a rez are answered with, and how long a \
         temporary prim stands"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi, Grid::FakeSl, Grid::FakeOpensim]
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
        Duration::from_secs(1200)
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            // The folders come with the login, before the region does.
            let folders = {
                let folders = ctx
                    .primary()
                    .wait_for(REPLY_TIMEOUT, |event| match event {
                        Event::InventorySkeleton(folders) => Some(folders.clone()),
                        _ => None,
                    })
                    .await?;
                Folders::of(&folders).ok_or_else(|| {
                    TestFailure::Assertion("inventory skeleton had no root folder".to_owned())
                })?
            };
            let grid = ctx.grid();
            let mut residue = Residue::default();
            let mut metrics = Metrics::new();
            let outcome = census(ctx, folders, &mut residue, &mut metrics).await;
            let held = outcome.and_then(|()| hold_to_measured(grid, &metrics));
            ctx.metrics().merge(metrics);
            // Take away what is left whatever became of the census.
            let cleanup = clean_up(ctx.primary(), &residue, folders.trash).await;
            held.and(cleanup)
        })
    }
}

/// Everything a leg needs to know about the run.
#[derive(Debug)]
struct Run {
    /// The agent's folders.
    folders: Folders,
    /// The agent.
    owner: AgentKey,
    /// How long to listen.
    pace: Pace,
    /// Where the first cube stands; the others stand beside it.
    position: Vector,
    /// Every object sighted so far.
    seen: HashSet<ScopedObjectId>,
}

impl Run {
    /// The spot `slot` places along from the first cube's.
    fn spot(&self, slot: u8) -> Vector {
        Vector {
            x: f32::from(slot).mul_add(SPACING_M, self.position.x),
            y: self.position.y,
            z: self.position.z,
        }
    }
}

/// The legs, in order. Everything rezzed and every item made is noted in
/// `residue` as it appears, so the caller can take it away when a leg fails.
#[expect(
    clippy::too_many_lines,
    reason = "one linear flow of nine legs, each a request and a window to hear its answer in"
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
    let session = ctx.primary();
    let owner = session
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;

    session.wait_for_region(REGION_TIMEOUT).await?;
    let (seen, anchor) = settle_scene(
        session,
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
        position: Vector {
            x: anchor.x,
            y: anchor.y,
            z: anchor.z + REZ_LIFT_M,
        },
        seen,
    };

    // The cube everything else is made from, named so its items can be told.
    let cube = rez_cube(session, &mut run, residue, 0).await?;
    let cube_id = cube.scoped_id();
    session
        .send(Command::SetObjectName {
            local_id: cube_id,
            name: CUBE_NAME.to_owned(),
        })
        .await?;
    session
        .send(Command::SetObjectDescription {
            local_id: cube_id,
            description: CUBE_DESCRIPTION.to_owned(),
        })
        .await?;
    let original = select(session, &run, &cube).await?;
    metrics.set("cube_named", original.name == CUBE_NAME);

    // --- 1. Take a copy. ---
    let (heard, transaction) = derez(
        session,
        &run,
        cube_id,
        DeRezDestination::AcquireToAgentInventory(folders.objects),
    )
    .await?;
    record_derez(metrics, "take_copy", &run, &heard, &[cube_id], transaction)?;
    note_items(residue, &heard);
    if let Some(item) = heard.items().next() {
        record_item(metrics, "take_copy_item", &run, item, &original)?;
    }

    // --- 2. Take. ---
    let (heard, transaction) = derez(
        session,
        &run,
        cube_id,
        DeRezDestination::TakeIntoAgentInventory(folders.objects),
    )
    .await?;
    record_derez(metrics, "take", &run, &heard, &[cube_id], transaction)?;
    note_items(residue, &heard);
    if heard.killed(cube_id) {
        residue.roots.retain(|root| *root != cube_id);
    }
    if let Some(after) = heard.killed_after(cube_id) {
        metrics.set_timing(&secs_metric("take_kill"), after.as_secs_f64());
    }
    if let Some(after) = heard.first_announced_after {
        metrics.set_timing(&secs_metric("take_announced"), after.as_secs_f64());
    }
    let item = heard.items().next().cloned().ok_or_else(|| {
        TestFailure::Assertion("a take was answered with no inventory item to rez".to_owned())
    })?;
    record_item(metrics, "take_item", &run, &item, &original)?;

    // --- 3. Restore the taken item to where the cube stood. ---
    session
        .send(Command::RezRestoreToWorld {
            item: rez_params(&item, &run.spot(0)).item,
        })
        .await?;
    let known = run.seen.clone();
    let agent = owner.uuid();
    let is_restored = move |object: &Object| {
        is_own(object, agent)
            && object.parent_id == RegionLocalObjectId(0)
            && !known.contains(&object.scoped_id())
    };
    let heard = listen(session, pace.quiet.saturating_mul(3), pace.quiet, |heard| {
        heard.added.iter().any(&is_restored)
    })
    .await?;
    let restored = heard.added.iter().find(|object| is_restored(object));
    metrics.set("restore_brings_an_object", restored.is_some());
    metrics.set(
        "restore_puts_it",
        restored.map_or("nowhere", |object| {
            let at = &object.motion.position;
            let was = &cube.motion.position;
            let apart = (at.x - was.x).hypot(at.y - was.y).hypot(at.z - was.z);
            if apart < RESTORE_SLACK_M {
                "where it stood"
            } else {
                "somewhere else"
            }
        }),
    );
    metrics.set(
        "restore_removes_the_item",
        heard.removed_items.contains(&item.item_id),
    );
    metrics.set("restore_alerts", joined(&heard.alerts));
    run.seen.extend(heard.added.iter().map(Object::scoped_id));
    if let Some(object) = restored {
        residue.roots.push(object.scoped_id());
    }

    // --- 4. Rez the taken item. ---
    let rez_started = now_secs()?;
    let (rezzed, heard) = rez_item(session, &mut run, residue, &item, 1).await?;
    metrics.set("rez_brings", heard.brought(&rezzed));
    metrics.set("rez_announcement", heard.announcement());
    metrics.set(
        "rez_removes_the_item",
        heard.removed_items.contains(&item.item_id),
    );
    metrics.set("rez_alerts", joined(&heard.alerts));
    let record = select(session, &run, &rezzed).await?;
    record_rezzed(metrics, &run, &record, &original, &item, rez_started);

    // --- 5. `ObjectDelete`. ---
    let rezzed_id = rezzed.scoped_id();
    session
        .send(Command::DeleteObjects {
            local_ids: vec![rezzed_id],
        })
        .await?;
    let heard = listen(session, pace.quiet.saturating_mul(2), pace.quiet, |heard| {
        heard.killed(rezzed_id)
    })
    .await?;
    let deleted = heard.killed(rezzed_id);
    metrics.set("object_delete_kills", deleted);
    metrics.set("object_delete_announcement", heard.announcement());
    metrics.set(
        "object_delete_item_lands_in",
        heard
            .items()
            .next()
            .map_or("nowhere", |item| folders.word(item.folder_id)),
    );
    metrics.set("object_delete_alerts", joined(&heard.alerts));
    note_items(residue, &heard);
    let standing = if deleted {
        residue.roots.retain(|root| *root != rezzed_id);
        rez_item(session, &mut run, residue, &item, 1).await?.0
    } else {
        rezzed
    };

    // --- 6. Delete: a derez to the Trash, and to a folder that is not it. ---
    let standing_id = standing.scoped_id();
    let (heard, transaction) = derez(
        session,
        &run,
        standing_id,
        DeRezDestination::Trash(folders.trash),
    )
    .await?;
    record_derez(metrics, "trash", &run, &heard, &[standing_id], transaction)?;
    note_items(residue, &heard);
    if heard.killed(standing_id) {
        residue.roots.retain(|root| *root != standing_id);
    }
    let (misfiled, _heard) = rez_item(session, &mut run, residue, &item, 2).await?;
    let misfiled_id = misfiled.scoped_id();
    let (heard, transaction) = derez(
        session,
        &run,
        misfiled_id,
        DeRezDestination::Trash(folders.objects),
    )
    .await?;
    record_derez(
        metrics,
        "trash_naming_objects",
        &run,
        &heard,
        &[misfiled_id],
        transaction,
    )?;
    note_items(residue, &heard);
    if heard.killed(misfiled_id) {
        residue.roots.retain(|root| *root != misfiled_id);
    }

    // --- 7. Return, of the agent's own object. ---
    let (returned, _heard) = rez_item(session, &mut run, residue, &item, 3).await?;
    let returned_id = returned.scoped_id();
    let (mut heard, transaction) =
        derez(session, &run, returned_id, DeRezDestination::ReturnToOwner).await?;
    wait_for_notice(session, &mut heard, pace).await?;
    heard.record_notice(metrics, "return");
    record_derez(metrics, "return", &run, &heard, &[returned_id], transaction)?;
    note_items(residue, &heard);
    if heard.killed(returned_id) {
        residue.roots.retain(|root| *root != returned_id);
    }

    // --- 8. A linkset. ---
    let root = rez_cube(session, &mut run, residue, 4).await?;
    let child = rez_cube(session, &mut run, residue, 5).await?;
    let (root_id, child_id) = (root.scoped_id(), child.scoped_id());
    session
        .send(Command::LinkObjects {
            local_ids: vec![root_id, child_id],
        })
        .await?;
    session
        .wait_for(STEP_TIMEOUT, |event| match event {
            Event::ObjectUpdated(object)
                if object.scoped_id() == child_id && object.scoped_parent_id() == root_id =>
            {
                Some(())
            }
            _ => None,
        })
        .await
        .map_err(|failure| match failure {
            TestFailure::Timeout(_) => {
                TestFailure::Assertion("the second cube never came under the first".to_owned())
            }
            other => other,
        })?;
    residue.roots.retain(|standing| *standing != child_id);
    listen(session, pace.quiet, pace.quiet, |_heard| false).await?;
    let (heard, transaction) = derez(
        session,
        &run,
        root_id,
        DeRezDestination::TakeIntoAgentInventory(folders.objects),
    )
    .await?;
    record_derez(
        metrics,
        "linkset_take",
        &run,
        &heard,
        &[root_id, child_id],
        transaction,
    )?;
    note_items(residue, &heard);
    metrics.set(
        "linkset_take_kill_names",
        match (heard.killed(root_id), heard.killed(child_id)) {
            (true, true) => "root+child",
            (true, false) => "root",
            (false, true) => "child",
            (false, false) => "nothing",
        },
    );
    if heard.killed(root_id) {
        residue.roots.retain(|standing| *standing != root_id);
    }
    if let Some(linkset_item) = heard.items().next().cloned() {
        let (again, heard) = rez_item(session, &mut run, residue, &linkset_item, 4).await?;
        // The root, and whatever names it as its parent.
        let root = again.scoped_id();
        let prims: Vec<&Object> = heard
            .added
            .iter()
            .filter(|object| is_own(object, owner.uuid()))
            .filter(|object| object.scoped_id() == root || object.scoped_parent_id() == root)
            .collect();
        metrics.set(
            "linkset_rez_prims",
            i64::try_from(prims.len()).unwrap_or(-1),
        );
    }

    // --- 9. Temporary prims, flagged one after another. Live only: the
    // fake grid keeps no clock for a prim (`server-fake-grid-object-return`).
    if grid.is_fake() {
        return Ok(());
    }
    let mut flagged: Vec<(ScopedObjectId, Instant)> = Vec::new();
    for slot in 0..TEMPORARY_PRIMS {
        let temporary = rez_cube(session, &mut run, residue, slot.saturating_add(6)).await?;
        session
            .send(Command::SetObjectFlags {
                local_id: temporary.scoped_id(),
                flags: ObjectFlagSettings {
                    use_physics: false,
                    is_temporary: true,
                    is_phantom: false,
                    casts_shadows: false,
                },
            })
            .await?;
        flagged.push((temporary.scoped_id(), Instant::now()));
    }
    let mut expired: Vec<(Duration, Instant)> = Vec::new();
    let mut pending = flagged.clone();
    let watch_started = Instant::now();
    let mut announcements = Vec::new();
    let mut alerts = Vec::new();
    while !pending.is_empty() {
        let remaining = TEMPORARY_WINDOW.saturating_sub(watch_started.elapsed());
        if remaining.is_zero() {
            break;
        }
        let waiting = pending.clone();
        let heard = listen(session, remaining, Duration::ZERO, |heard| {
            waiting.iter().any(|(prim, _at)| heard.killed(*prim))
        })
        .await?;
        let now = Instant::now();
        pending.retain(|(prim, at)| {
            let gone = heard.killed(*prim);
            if gone {
                expired.push((now.duration_since(*at), now));
                residue.roots.retain(|standing| standing != prim);
            }
            !gone
        });
        announcements.push(heard.announcement());
        alerts.extend(heard.alerts.iter().cloned());
        note_items(residue, &heard);
    }
    metrics.set(
        "temporary_prims_expired",
        i64::try_from(expired.len()).unwrap_or(-1),
    );
    let lifetimes: Vec<f64> = expired
        .iter()
        .map(|(life, _at)| life.as_secs_f64())
        .collect();
    if let (Some(shortest), Some(longest)) = (
        lifetimes.iter().copied().reduce(f64::min),
        lifetimes.iter().copied().reduce(f64::max),
    ) {
        metrics.set_timing(&secs_metric("temporary_shortest_life"), shortest);
        metrics.set_timing(&secs_metric("temporary_longest_life"), longest);
        metrics.set(
            "temporary_life",
            if longest < TEMPORARY_NOMINAL.as_secs_f64() {
                "under a minute"
            } else if shortest >= TEMPORARY_NOMINAL.as_secs_f64() {
                "a minute or more"
            } else {
                "either side of a minute"
            },
        );
    }
    // Whether the prims went in one sweep or each when its own time was up:
    // they were flagged several seconds apart.
    let instants: Vec<Instant> = expired.iter().map(|(_life, at)| *at).collect();
    if let (Some(first), Some(last)) = (instants.iter().min(), instants.iter().max()) {
        metrics.set(
            "temporary_prims_go_together",
            last.duration_since(*first) < Duration::from_secs(1),
        );
    }
    if let (Some((_first, first_at)), Some((_last, last_at))) = (flagged.first(), flagged.last()) {
        metrics.set_timing(
            &secs_metric("temporary_flag_spread"),
            last_at.duration_since(*first_at).as_secs_f64(),
        );
    }
    metrics.set(
        "temporary_expiry_announcement",
        if announcements.iter().all(|shape| shape == ANNOUNCED_NOTHING) {
            ANNOUNCED_NOTHING.to_owned()
        } else {
            announcements.join("+")
        },
    );
    metrics.set("temporary_expiry_alerts", joined(&alerts));
    Ok(())
}

/// Goes on listening for the instant message that tells of a return where
/// `heard` holds none yet: OpenSim sends its with the region's next backup,
/// which may be a third of a minute away.
pub(crate) async fn wait_for_notice(
    session: &mut Session,
    heard: &mut Heard,
    pace: Pace,
) -> Result<(), TestFailure> {
    if !heard.messages.is_empty() {
        return Ok(());
    }
    let started = Instant::now();
    let later = listen(session, pace.notice, pace.quiet, |later| {
        !later.messages.is_empty()
    })
    .await?;
    // The window before this one closed a quiet spell after the kill.
    heard.hear_more(later, started.elapsed().min(pace.quiet));
    Ok(())
}

/// The current time, in seconds since the Unix epoch.
fn now_secs() -> Result<u64, TestFailure> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| TestFailure::State(format!("the clock is before the epoch: {error}")))?
        .as_secs())
}

/// `texts`, `|`-joined, or `none`.
pub(crate) fn joined(texts: &[String]) -> String {
    if texts.is_empty() {
        "none".to_owned()
    } else {
        texts.join(" | ")
    }
}

/// Notes every item `heard` announced, for the cleanup.
fn note_items(residue: &mut Residue, heard: &Heard) {
    residue.items.extend(heard.items().map(|item| item.item_id));
}

/// Rezzes a cube with `ObjectAdd` at `slot` and waits for it.
async fn rez_cube(
    session: &mut Session,
    run: &mut Run,
    residue: &mut Residue,
    slot: u8,
) -> Result<Object, TestFailure> {
    session
        .send(Command::RezObject {
            shape: PrimShape::cube(run.spot(slot)),
            group_id: None,
        })
        .await?;
    let cube = wait_for_own_new_object(session, &run.seen, STEP_TIMEOUT)
        .await?
        .map_err(|reason| {
            TestFailure::Assertion(format!("no new object appeared after RezObject: {reason}"))
        })?;
    run.seen.insert(cube.scoped_id());
    residue.roots.push(cube.scoped_id());
    // A rez is followed by more updates of the new object; they are not the
    // next leg's doing.
    listen(session, run.pace.quiet, run.pace.quiet, |_heard| false).await?;
    Ok(cube)
}

/// Rezzes `item` at `slot` and returns the root that appeared with
/// everything the grid sent around it.
async fn rez_item(
    session: &mut Session,
    run: &mut Run,
    residue: &mut Residue,
    item: &InventoryItem,
    slot: u8,
) -> Result<(Object, Heard), TestFailure> {
    session
        .send(Command::RezObjectFromInventory {
            params: Box::new(rez_params(item, &run.spot(slot))),
        })
        .await?;
    let owner = run.owner.uuid();
    let seen = run.seen.clone();
    let is_new_root = move |object: &Object| {
        is_own(object, owner)
            && object.parent_id == RegionLocalObjectId(0)
            && !seen.contains(&object.scoped_id())
    };
    let heard = listen(session, STEP_TIMEOUT, run.pace.quiet, |heard| {
        heard.added.iter().any(&is_new_root)
    })
    .await?;
    let root = heard
        .added
        .iter()
        .find(|object| is_new_root(object))
        .cloned()
        .ok_or_else(|| {
            TestFailure::Assertion(format!(
                "no new object appeared after RezObjectFromInventory; alerts: {}",
                joined(&heard.alerts)
            ))
        })?;
    run.seen.extend(heard.added.iter().map(Object::scoped_id));
    residue.roots.push(root.scoped_id());
    Ok((root, heard))
}

/// Selects `object`, returns its record and deselects it.
async fn select(
    session: &mut Session,
    run: &Run,
    object: &Object,
) -> Result<Properties, TestFailure> {
    let local_id = object.scoped_id();
    let object_id = object.full_id;
    session
        .send(Command::RequestObjectProperties {
            local_ids: vec![local_id],
        })
        .await?;
    let heard = listen(session, STEP_TIMEOUT, run.pace.quiet, |heard| {
        heard
            .properties
            .iter()
            .any(|properties| properties.object_id == object_id)
    })
    .await?;
    session
        .send(Command::DeselectObjects {
            local_ids: vec![local_id],
        })
        .await?;
    heard
        .properties
        .into_iter()
        .rfind(|properties| properties.object_id == object_id)
        .ok_or_else(|| {
            TestFailure::Assertion("a select of our own object went unanswered".to_owned())
        })
}

/// Derezzes `object` to `destination` and listens for the answer: until the
/// grid has both said what became of the object and announced an item, or
/// for twice the quiet window where it does only one of them.
async fn derez(
    session: &mut Session,
    run: &Run,
    object: ScopedObjectId,
    destination: DeRezDestination,
) -> Result<(Heard, TransactionId), TestFailure> {
    let transaction = TransactionId::from(Uuid::new_v4());
    session
        .send(Command::DerezObjects {
            local_ids: vec![object],
            destination,
            transaction_id: transaction,
            group_id: None,
        })
        .await?;
    let leaves = destination.removes_from_world();
    let heard = listen(session, STEP_TIMEOUT, run.pace.quiet, |heard| {
        // What every derez measured so far was answered with at the least.
        let filed = !heard.announced.is_empty() || !heard.derez_acks.is_empty();
        let gone = heard.killed(object);
        if leaves { gone && filed } else { filed }
    })
    .await?;
    Ok((heard, transaction))
}

/// Records what a derez of `objects` (a root, and its children after it) was
/// answered with, under `prefix`.
fn record_derez(
    metrics: &mut Metrics,
    prefix: &str,
    run: &Run,
    heard: &Heard,
    objects: &[ScopedObjectId],
    transaction: TransactionId,
) -> Result<(), TestFailure> {
    let root = objects
        .first()
        .copied()
        .ok_or_else(|| TestFailure::State("a derez leg recorded without an object".to_owned()))?;
    let count = |value: usize| i64::try_from(value).unwrap_or(-1);
    metrics.set(&format!("{prefix}_kills_the_object"), heard.killed(root));
    metrics.set(
        &format!("{prefix}_kill_entries"),
        count(
            objects
                .iter()
                .map(|object| heard.kill_entries(*object))
                .sum(),
        ),
    );
    metrics.set(
        &format!("{prefix}_kill_messages"),
        count(
            heard
                .kills
                .iter()
                .filter(|(_after, kill)| objects.iter().any(|object| kill.contains(&object.id())))
                .count(),
        ),
    );
    metrics.set(&format!("{prefix}_announcement"), heard.announcement());
    metrics.set(&format!("{prefix}_items"), count(heard.items().count()));
    metrics.set(
        &format!("{prefix}_item_lands_in"),
        heard
            .items()
            .next()
            .map_or("nowhere", |item| run.folders.word(item.folder_id)),
    );
    metrics.set(
        &format!("{prefix}_announcement_transaction"),
        match heard.announced.first() {
            Some(announced) if announced.transaction == transaction.get() => "the derez's",
            Some(announced) if announced.transaction.is_nil() => "nil",
            Some(_announced) => "another",
            None => "none",
        },
    );
    metrics.set(
        &format!("{prefix}_derez_ack"),
        match heard
            .derez_acks
            .iter()
            .find(|(acked, _success)| *acked == transaction)
        {
            Some((_acked, true)) => "success",
            Some((_acked, false)) => "failure",
            None if heard.derez_acks.is_empty() => "none",
            None => "of another transaction",
        },
    );
    metrics.set(&format!("{prefix}_alerts"), joined(&heard.alerts));
    metrics.set(&format!("{prefix}_messages"), joined(&heard.messages));
    metrics.set(&format!("{prefix}_told_by"), heard.told_by());
    Ok(())
}

/// Records what the item a derez made says, under `prefix`. `original` is the
/// record of the object it was made from.
fn record_item(
    metrics: &mut Metrics,
    prefix: &str,
    run: &Run,
    item: &InventoryItem,
    original: &Properties,
) -> Result<(), TestFailure> {
    let now = now_secs()?;
    let made = u64::try_from(item.creation_date).unwrap_or(0);
    let object_made = original.creation_date / 1_000_000;
    metrics.set(
        &format!("{prefix}_name_is_the_objects"),
        item.name == original.name,
    );
    metrics.set(
        &format!("{prefix}_description_is_the_objects"),
        item.description == original.description,
    );
    metrics.set(&format!("{prefix}_asset_id_nil"), item.asset_id.is_nil());
    metrics.set(&format!("{prefix}_asset_type"), i64::from(item.item_type));
    metrics.set(
        &format!("{prefix}_inventory_type"),
        i64::from(item.inv_type),
    );
    metrics.set(&format!("{prefix}_flags"), format!("{:#010x}", item.flags));
    metrics.set(&format!("{prefix}_sale_type"), i64::from(item.sale_type));
    metrics.set(
        &format!("{prefix}_owner_is_the_agent"),
        item.owner == OwnerKey::Agent(run.owner),
    );
    metrics.set(
        &format!("{prefix}_creator_is_the_agent"),
        item.creator_id == run.owner,
    );
    metrics.set(&format!("{prefix}_group_set"), item.group.is_some());
    // The derez happened within the quiet window of now, and the object was
    // made several windows before that.
    metrics.set(
        &format!("{prefix}_date"),
        if made == 0 {
            "zero"
        } else if now.abs_diff(made) <= DATE_SLACK_SECS.saturating_add(run.pace.quiet.as_secs()) {
            "the derez's"
        } else if object_made.abs_diff(made) <= DATE_SLACK_SECS {
            "the object's"
        } else {
            "neither"
        },
    );
    for (what, mask, objects) in [
        ("base", item.permissions.base, original.permissions.base),
        ("owner", item.permissions.owner, original.permissions.owner),
        ("group", item.permissions.group, original.permissions.group),
        (
            "everyone",
            item.permissions.everyone,
            original.permissions.everyone,
        ),
        (
            "next_owner",
            item.permissions.next_owner,
            original.permissions.next_owner,
        ),
    ] {
        metrics.set(
            &format!("{prefix}_mask_{what}"),
            format!("{:#010x}", mask.bits()),
        );
        metrics.set(
            &format!("{prefix}_mask_{what}_is_the_objects"),
            mask == objects,
        );
    }
    Ok(())
}

/// Records what the record of an object rezzed out of `item` says of where it
/// came from. `original` is the record of the object the item was made from,
/// and `rez_started` when the rez was asked for.
fn record_rezzed(
    metrics: &mut Metrics,
    run: &Run,
    record: &Properties,
    original: &Properties,
    item: &InventoryItem,
    rez_started: u64,
) {
    let made = record.creation_date / 1_000_000;
    metrics.set(
        "rezzed_creation_date",
        if record.creation_date == 0 {
            "zero"
        } else if record.creation_date == original.creation_date {
            "the original object's"
        } else if made.abs_diff(original.creation_date / 1_000_000) <= 1 {
            "the original object's, to the second"
        } else if made.abs_diff(rez_started) <= DATE_SLACK_SECS {
            "the rez's"
        } else {
            "neither"
        },
    );
    metrics.set("rezzed_name_is_the_items", record.name == item.name);
    metrics.set(
        "rezzed_description_is_the_items",
        record.description == item.description,
    );
    metrics.set(
        "rezzed_item_id",
        if record.item_id == item.item_id {
            "the item's"
        } else if record.item_id.uuid().is_nil() {
            "nil"
        } else {
            "another"
        },
    );
    metrics.set(
        "rezzed_folder_id",
        match record.folder_id {
            Some(folder) if folder == item.folder_id => "the item's folder",
            Some(_other) => "another",
            None => "none",
        },
    );
    metrics.set("rezzed_from_task_id_set", record.from_task_id.is_some());
    metrics.set(
        "rezzed_owner_is_the_agent",
        record.owner == OwnerKey::Agent(run.owner),
    );
    metrics.set(
        "rezzed_creator_is_the_agent",
        record.creator_id == run.owner,
    );
    metrics.set(
        "rezzed_last_owner",
        if record.last_owner_id.is_nil() {
            "nil"
        } else if record.last_owner_id == run.owner.uuid() {
            "the agent"
        } else {
            "somebody else"
        },
    );
    for (what, mask, originals) in [
        ("base", record.permissions.base, original.permissions.base),
        (
            "owner",
            record.permissions.owner,
            original.permissions.owner,
        ),
        (
            "next_owner",
            record.permissions.next_owner,
            original.permissions.next_owner,
        ),
    ] {
        metrics.set(
            &format!("rezzed_mask_{what}"),
            format!("{:#010x}", mask.bits()),
        );
        metrics.set(
            &format!("rezzed_mask_{what}_is_the_originals"),
            mask == originals,
        );
    }
    metrics.set(
        "rezzed_inventory_serial",
        i64::from(record.inventory_serial),
    );
}

/// Derezzes whatever the case left standing to the Trash, waits for each to
/// go, and removes the items its legs made from the inventory.
async fn clean_up(
    session: &mut Session,
    residue: &Residue,
    trash: InventoryFolderKey,
) -> Result<(), TestFailure> {
    let mut items = residue.items.clone();
    let outcome = if residue.roots.is_empty() {
        Ok(())
    } else {
        session
            .send(Command::DerezObjects {
                local_ids: residue.roots.clone(),
                destination: DeRezDestination::Trash(trash),
                transaction_id: TransactionId::from(Uuid::new_v4()),
                group_id: None,
            })
            .await?;
        let pending: HashSet<ScopedObjectId> = residue.roots.iter().copied().collect();
        let heard = listen(session, STEP_TIMEOUT, Duration::from_secs(1), |heard| {
            pending.iter().all(|root| heard.killed(*root))
        })
        .await?;
        items.extend(heard.items().map(|item| item.item_id));
        let left = pending.iter().filter(|root| !heard.killed(**root)).count();
        if left == 0 {
            Ok(())
        } else {
            Err(TestFailure::Assertion(format!(
                "{left} object(s) were never removed on cleanup"
            )))
        }
    };
    if !items.is_empty() {
        session.send(Command::RemoveInventoryItems(items)).await?;
    }
    outcome
}

/// Builds the [`RezObjectParams`] to rez `item` back into the world at
/// `position`, carrying the item's own permission masks and full payload. The
/// ray is bypassed so the object rezzes exactly at `position` (the headless rez
/// path), and the source item is left in inventory. The CRC is left `0`:
/// neither grid checks it.
pub(crate) fn rez_params(item: &InventoryItem, position: &Vector) -> RezObjectParams {
    RezObjectParams {
        group_id: None,
        from_task_id: None,
        bypass_raycast: true,
        ray_start: position.clone(),
        ray_end: position.clone(),
        ray_target_id: None,
        ray_end_is_intersection: false,
        rez_selected: false,
        remove_item: false,
        item_flags: item.flags,
        group_mask: item.permissions.group.bits(),
        everyone_mask: item.permissions.everyone.bits(),
        next_owner_mask: item.permissions.next_owner.bits(),
        item: RestoreItem {
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
        },
    }
}
