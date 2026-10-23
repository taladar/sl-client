//! A census of notecards and gestures: what each grid makes a new notecard
//! and a new gesture of, what it keeps of a saved body and of an item
//! embedded in one, what `CopyInventoryFromNotecard` answers and files, and
//! where a gesture's active state is kept — the item's flags, the next
//! login's `gestures` array, or both.
//!
//! One avatar, in a run folder `F` under its root. Every leg records:
//!
//! - **what arrived** in a window after it (`_push`, `_push_detail`), as the
//!   inventory censuses record it: every inventory message by name and road,
//!   every id named by its role in the run;
//! - **what the grid holds**, read back raw: the item through the item fetch
//!   (its asset, flags and masks), and its asset through both roads a viewer
//!   has to one — the `ViewerAsset` capability (`_viewer_asset`) and the UDP
//!   `TransferRequest` of an inventory item, the reference's road for a
//!   notecard (`_transfer`) — each described against the bytes the run wrote.
//!
//! The legs:
//!
//! 1. **A new notecard**: `CreateInventoryItem`, nothing saved; its asset and
//!    what either road reads of it.
//! 2. **A saved body**: a plain body saved over
//!    `UpdateNotecardAgentInventory`, read back.
//! 3. **An embedded item**: a body embedding the saved notecard of leg 2 (an
//!    item of the avatar's, full permission), read back and compared field by
//!    field with what was written; then a body embedding an item nobody has
//!    (ids the run made up).
//! 4. **`CopyInventoryFromNotecard`** of the embedded item into `F`, into the
//!    folder the grid picks (a nil `folder-id`), of an id the notecard does not
//!    hold, and out of the forged notecard of leg 3 — each answer, its push,
//!    where the copy is filed and what it is (asset, masks, name).
//! 5. **Gestures**: three gestures made and saved over
//!    `UpdateGestureAgentInventory`; all three activated, one deactivated, one
//!    removed while active; the flags after each; then a relogin
//!    without asking for `gestures` (whether the array comes at all), then one
//!    asking, and which of the three its array names, under which asset.
//!
//! What each live grid answered is stated as a [`Measured`] and written up in
//! `book/src/gridspec/inventory.md` § Notecards and gestures; every run is
//! held to it.
//!
//! `1av`, live and fake grids.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use reqwest::header::ACCEPT;
use sl_client_tokio::{
    AssetType, AssetUpdateLocation, CircuitProbe, Command, Event, FolderType, GestureActivation,
    InventoryFolderKey, InventoryKey, InventoryType, LoginRequest, NewInventoryItem, StartLocation,
    UpdatableAssetType, Uuid, WearableType,
};
use sl_notecard::{
    AssetIdEncoding, InventoryItem as EmbeddedItem, Notecard, NotecardVersion, PermissionMask,
    Permissions, SaleInfo, SaleType,
};
use sl_types::key::Key;
use sl_wire::messages::{
    RemoveInventoryItem, RemoveInventoryItemAgentDataBlock, RemoveInventoryItemInventoryDataBlock,
};
use sl_wire::{Llsd, LoginList};

use crate::cases::asset_http::without_ids;
use crate::cases::inventory_fetch::{
    CAP_WAIT, Census, FI2, FID2, HTTP_TIMEOUT, Made, POLL, Raw, SETTLE, View, clean_up,
    listed_folders, map, uuid_of,
};
use crate::cases::inventory_mutations::{
    Heard, ItemState, Roles, item_state, listen_on, mask_text,
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
    "book/src/gridspec/inventory.md § Notecards and gestures (notecards-gestures, 2026-10-23)";

/// How long what a grid pushes after an operation is listened for.
const PUSH_WINDOW: Duration = Duration::from_secs(5);

/// How long what an earlier step caused is let arrive before an operation.
const DRAIN_WINDOW: Duration = Duration::from_secs(2);

/// How long late pushes are listened for after the read-back.
const LATE_WINDOW: Duration = Duration::from_secs(1);

/// How long a body's save may take.
const SAVE_TIMEOUT: Duration = Duration::from_secs(60);

/// How long a UDP asset transfer may take.
const TRANSFER_TIMEOUT: Duration = Duration::from_secs(20);

/// The next-owner mask of every item the run makes: move, copy, modify,
/// transfer.
const NEXT_OWNER_FULL: u32 = 0x0008_e000;

/// The callback id the copies are asked under, so its echo can be told from
/// the zero a grid writes by default.
const COPY_CALLBACK: i32 = 7;

/// The capability a viewer reads an asset through.
const VIEWER_ASSET: &str = "ViewerAsset";

/// The capability that copies an item out of a notecard.
const COPY_FROM_NOTECARD: &str = "CopyInventoryFromNotecard";

/// The trigger of the run's gestures.
const TRIGGER: &str = "/confgesture";

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
/// local OpenSim, 2026-10-23), every key of which held still between them.
const MEASURED: &[(&str, Measured<Answer>)] = &[
    ("activate_push", both(Answer::Text("nothing"))),
    ("activate_push_detail", both(Answer::Text("nothing"))),
    ("activated_asset", both(Answer::Text("the saved asset"))),
    (
        "activated_flags",
        each(Answer::Text("0x0"), Answer::Text("0x1")),
    ),
    ("activated_listed", both(Answer::Flag(true))),
    (
        "activated_masks",
        each(
            Answer::Text("0x7fffffff 0x7fffffff"),
            Answer::Text("0x0009e000 0x0009e000"),
        ),
    ),
    (
        "copy_absent_answer",
        each(Answer::Text("200: undef"), Answer::Text("400: no LLSD")),
    ),
    ("copy_absent_asset", both(Answer::Text("no copy"))),
    (
        "copy_absent_callback",
        both(Answer::Text("no announcement")),
    ),
    ("copy_absent_filed", both(Answer::Text("no copy"))),
    ("copy_absent_masks", both(Answer::Text("no copy"))),
    ("copy_absent_name", both(Answer::Text("no copy"))),
    ("copy_absent_next_mask", both(Answer::Text("no copy"))),
    (
        "copy_absent_push",
        each(Answer::Text("nothing"), Answer::Text("AlertMessage (UDP)")),
    ),
    (
        "copy_absent_push_detail",
        each(
            Answer::Text("nothing"),
            Answer::Text("alert: Failed to retrieve item"),
        ),
    ),
    (
        "copy_answer",
        each(Answer::Text("200: undef"), Answer::Text("200: no LLSD")),
    ),
    ("copy_asset", both(Answer::Text("the saved asset"))),
    ("copy_callback", both(Answer::Text("none"))),
    (
        "copy_default_answer",
        each(Answer::Text("200: undef"), Answer::Text("200: no LLSD")),
    ),
    ("copy_default_asset", both(Answer::Text("the saved asset"))),
    ("copy_default_callback", both(Answer::Text("none"))),
    ("copy_default_filed", both(Answer::Text("Notecards"))),
    (
        "copy_default_masks",
        each(
            Answer::Text("0x7fffffff 0x7fffffff"),
            Answer::Text("0x0009e000 0x0009e000"),
        ),
    ),
    (
        "copy_default_name",
        both(Answer::Text("the embedded item's")),
    ),
    (
        "copy_default_next_mask",
        each(Answer::Text("0x7fffffff"), Answer::Text("0x0008e000")),
    ),
    (
        "copy_default_push",
        both(Answer::Text("BulkUpdateInventory (event queue)")),
    ),
    (
        "copy_default_push_detail",
        both(Answer::Text(
            "BulkUpdateInventory: folders [] items [the copy default copy]",
        )),
    ),
    ("copy_filed", both(Answer::Text("F"))),
    (
        "copy_forged_answer",
        each(Answer::Text("200: undef"), Answer::Text("400: no LLSD")),
    ),
    ("copy_forged_asset", both(Answer::Text("no copy"))),
    (
        "copy_forged_callback",
        both(Answer::Text("no announcement")),
    ),
    ("copy_forged_filed", both(Answer::Text("no copy"))),
    ("copy_forged_masks", both(Answer::Text("no copy"))),
    ("copy_forged_name", both(Answer::Text("no copy"))),
    ("copy_forged_next_mask", both(Answer::Text("no copy"))),
    (
        "copy_forged_push",
        each(Answer::Text("nothing"), Answer::Text("AlertMessage (UDP)")),
    ),
    (
        "copy_forged_push_detail",
        each(
            Answer::Text("nothing"),
            Answer::Text("alert: Failed to retrieve item"),
        ),
    ),
    (
        "copy_masks",
        each(
            Answer::Text("0x7fffffff 0x7fffffff"),
            Answer::Text("0x0009e000 0x0009e000"),
        ),
    ),
    ("copy_name", both(Answer::Text("the embedded item's"))),
    (
        "copy_next_mask",
        each(Answer::Text("0x7fffffff"), Answer::Text("0x0008e000")),
    ),
    (
        "copy_push",
        both(Answer::Text("BulkUpdateInventory (event queue)")),
    ),
    (
        "copy_push_detail",
        both(Answer::Text(
            "BulkUpdateInventory: folders [] items [the copy copy]",
        )),
    ),
    ("deactivate_push", both(Answer::Text("nothing"))),
    ("deactivate_push_detail", both(Answer::Text("nothing"))),
    ("deactivated_asset", both(Answer::Text("the saved asset"))),
    ("deactivated_flags", both(Answer::Text("0x0"))),
    ("deactivated_listed", both(Answer::Flag(true))),
    (
        "deactivated_masks",
        each(
            Answer::Text("0x7fffffff 0x7fffffff"),
            Answer::Text("0x0009e000 0x0009e000"),
        ),
    ),
    (
        "embed_read_item",
        each(
            Answer::Text(
                "differs in item_id another; group_mask 0x00000000 to 0x7fffffff; everyone_mask 0x00000000 to 0x7fffbfff; next_owner_mask 0x0008e000 to 0x7fffffff",
            ),
            Answer::Text("as written"),
        ),
    ),
    ("embed_save", both(Answer::Text("saved"))),
    (
        "embed_transfer",
        each(
            Answer::Text("received: a notecard, 1 embedded items, some text"),
            Answer::Text("received: the bytes written"),
        ),
    ),
    (
        "embed_viewer_asset",
        each(Answer::Text("503"), Answer::Text("200: the bytes written")),
    ),
    (
        "forged_save",
        each(
            Answer::Text("failed: Internal error from verifier."),
            Answer::Text("saved"),
        ),
    ),
    ("gesture_save", both(Answer::Text("saved"))),
    ("login_gesture_asset", both(Answer::Text("the saved asset"))),
    ("login_gestures", both(Answer::Text("the active gesture"))),
    ("login_gestures_asked", both(Answer::Text("a list"))),
    (
        "login_gestures_unasked",
        each(Answer::Text("absent"), Answer::Text("a list")),
    ),
    ("new_gesture_asset", both(Answer::Text("an asset"))),
    (
        "new_gesture_body",
        each(
            Answer::Text(
                "2\\n255\\n0\\n/hey\\nHey!\\n2\\n0\\nWave\\r\\n9b29cd61-c45b-5689-ded2-91756b8d76a9\\n0\\n1\\nhey_male.wav\\n3a3fa800-81a0-6b88-d671-6faf0b457a72\\n0\\n\\x00",
            ),
            Answer::Text("\\x00"),
        ),
    ),
    ("new_gesture_created_asset", both(Answer::Text("an asset"))),
    ("new_gesture_flags", both(Answer::Text("0x0"))),
    ("new_gesture_listed", both(Answer::Flag(true))),
    (
        "new_gesture_masks",
        each(
            Answer::Text("0x7fffffff 0x7fffffff"),
            Answer::Text("0x0009e000 0x0009e000"),
        ),
    ),
    (
        "new_gesture_push",
        both(Answer::Text("UpdateCreateInventoryItem (UDP)")),
    ),
    (
        "new_gesture_push_detail",
        both(Answer::Text(
            "UpdateCreateInventoryItem: the active gesture in F, with a callback id",
        )),
    ),
    (
        "new_gesture_transfer",
        each(
            Answer::Text("received: 122 bytes, first line \"2\""),
            Answer::Text("received: 1 zero bytes"),
        ),
    ),
    (
        "new_gesture_viewer_asset",
        each(
            Answer::Text("200: 122 bytes, first line \"2\""),
            Answer::Text("200: 1 zero bytes"),
        ),
    ),
    (
        "new_note_asset",
        each(Answer::Text("nil"), Answer::Text("an asset")),
    ),
    (
        "new_note_created_asset",
        each(Answer::Text("nil"), Answer::Text("an asset")),
    ),
    ("new_note_flags", both(Answer::Text("0x0"))),
    ("new_note_listed", both(Answer::Flag(true))),
    (
        "new_note_masks",
        each(
            Answer::Text("0x7fffffff 0x7fffffff"),
            Answer::Text("0x0009e000 0x0009e000"),
        ),
    ),
    (
        "new_note_push",
        both(Answer::Text("UpdateCreateInventoryItem (UDP)")),
    ),
    (
        "new_note_push_detail",
        both(Answer::Text(
            "UpdateCreateInventoryItem: the new notecard in F, with a callback id",
        )),
    ),
    (
        "new_note_transfer",
        each(
            Answer::Text("failed: UnknownSource"),
            Answer::Text("received: 1 zero bytes"),
        ),
    ),
    (
        "new_note_viewer_asset",
        each(Answer::Text("no asset"), Answer::Text("200: 1 zero bytes")),
    ),
    ("plain_asset", both(Answer::Text("the saved asset"))),
    ("plain_flags", both(Answer::Text("0x0"))),
    ("plain_listed", both(Answer::Flag(true))),
    (
        "plain_masks",
        each(
            Answer::Text("0x7fffffff 0x7fffffff"),
            Answer::Text("0x0009e000 0x0009e000"),
        ),
    ),
    ("plain_save", both(Answer::Text("saved"))),
    (
        "plain_transfer",
        both(Answer::Text("received: the bytes written")),
    ),
    (
        "plain_viewer_asset",
        each(Answer::Text("503"), Answer::Text("200: the bytes written")),
    ),
    (
        "relogged_active_asset",
        both(Answer::Text("the saved asset")),
    ),
    (
        "relogged_active_flags",
        each(Answer::Text("0x0"), Answer::Text("0x1")),
    ),
    ("relogged_active_listed", both(Answer::Flag(true))),
    (
        "relogged_active_masks",
        each(
            Answer::Text("0x7fffffff 0x7fffffff"),
            Answer::Text("0x0009e000 0x0009e000"),
        ),
    ),
    (
        "relogged_deactivated_asset",
        both(Answer::Text("the saved asset")),
    ),
    ("relogged_deactivated_flags", both(Answer::Text("0x0"))),
    ("relogged_deactivated_listed", both(Answer::Flag(true))),
    (
        "relogged_deactivated_masks",
        each(
            Answer::Text("0x7fffffff 0x7fffffff"),
            Answer::Text("0x0009e000 0x0009e000"),
        ),
    ),
    ("remove_active_gone", both(Answer::Flag(true))),
    ("remove_active_push", both(Answer::Text("nothing"))),
    ("remove_active_push_detail", both(Answer::Text("nothing"))),
    ("saved_gesture_asset", both(Answer::Text("the saved asset"))),
    ("saved_gesture_flags", both(Answer::Text("0x0"))),
    ("saved_gesture_listed", both(Answer::Flag(true))),
    (
        "saved_gesture_masks",
        each(
            Answer::Text("0x7fffffff 0x7fffffff"),
            Answer::Text("0x0009e000 0x0009e000"),
        ),
    ),
    (
        "saved_gesture_transfer",
        both(Answer::Text("received: the bytes written")),
    ),
    (
        "saved_gesture_viewer_asset",
        both(Answer::Text("200: the bytes written")),
    ),
];

/// The metrics a fake flavour is not held to yet, by prefix, and why.
const FAKE_EXEMPT: &[(&str, &str)] = &[(
    "relogged_",
    "the fake grid seeds each login's inventory afresh, so the run's gestures are gone after \
     the relogin (server-fake-grid-inventory-persists-across-logins)",
)];

/// The census of notecards and gestures.
#[derive(Debug)]
pub struct NotecardsGestures;

impl GridTest for NotecardsGestures {
    fn name(&self) -> &'static str {
        "notecards-gestures"
    }

    fn description(&self) -> &'static str {
        "What each grid makes of a new notecard and gesture, a saved body, an embedded item and \
         its copy out of the notecard, and where a gesture's active state is kept"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi, Grid::FakeSl, Grid::FakeOpensim]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();
            let mut metrics = Metrics::new();
            let outcome = census(ctx.primary(), &mut metrics).await;
            ctx.metrics().merge(metrics.clone());
            let held: Vec<(&str, Measured<Answer>)> = MEASURED
                .iter()
                .filter(|(key, _measured)| {
                    !grid.is_fake()
                        || !FAKE_EXEMPT
                            .iter()
                            .any(|(prefix, _why)| key.starts_with(prefix))
                })
                .copied()
                .collect();
            outcome.and_then(|()| hold_to_measured(grid, &metrics, &held, SOURCE))
        })
    }
}

/// What the run made that the folder clean-up does not take with it.
#[derive(Debug, Default)]
struct Strays {
    /// Items filed outside `F`.
    items: Vec<Uuid>,
    /// Gestures left active.
    active: Vec<Uuid>,
}

/// Everything the legs share.
struct Run<'a> {
    /// The census handle.
    census: Census<'a>,
    /// The roles of the run's ids.
    roles: Roles,
    /// The inventory root.
    root: Uuid,
    /// The Trash.
    trash: Uuid,
    /// The Notecards folder, as the skeleton names it.
    notecards: Uuid,
    /// The run folder `F`.
    folder: Uuid,
    /// The folders the run made, for the clean-up.
    made: Made,
    /// What the clean-up has to find elsewhere.
    strays: Strays,
}

/// A gesture or notecard the run made, as the grid answered its creation.
#[derive(Debug, Clone, Copy)]
struct Made1 {
    /// Its item id.
    item: Uuid,
    /// The asset its creation named.
    asset: Uuid,
}

impl Run<'_> {
    /// The index of the next diagnostic the session records.
    fn now(&self) -> usize {
        self.census.a.diagnostics().len()
    }

    /// Lets what an earlier step caused arrive and be dropped, then marks
    /// where the next operation's window begins.
    async fn begin(&mut self) -> usize {
        let _drained = self
            .census
            .a
            .wait_for(DRAIN_WINDOW, |_event| None::<()>)
            .await;
        self.now()
    }

    /// Listens for `window`, then reads what the circuit probe saw since
    /// `mark`.
    async fn listen(&mut self, mark: usize, window: Duration) -> Heard {
        listen_on(self.census.a, mark, window).await
    }

    /// Records the pushes `heard` under `leg`.
    fn record_push(&self, metrics: &mut Metrics, leg: &str, heard: &Heard) {
        metrics.set(&format!("{leg}_push"), heard.push());
        metrics.set(&format!("{leg}_push_detail"), heard.detail(&self.roles));
    }

    /// The item fetch's raw entry for `item`.
    async fn item_raw(&self, item: Uuid) -> Option<Llsd> {
        let raw = self.census.fetch_item(FI2, self.census.agent, item).await;
        raw.body
            .as_ref()
            .and_then(|body| body.get("items"))
            .and_then(Llsd::as_array)
            .and_then(<[Llsd]>::first)
            .cloned()
    }

    /// The item fetch's statement of `item`.
    async fn item(&self, item: Uuid) -> Option<ItemState> {
        self.item_raw(item).await.as_ref().map(item_state)
    }

    /// The items the folder fetch lists in `folder`: name and id.
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

    /// Polls `folder` until it lists an item named `name` that is none of
    /// `known`; its id.
    async fn new_item_named(&self, folder: Uuid, name: &str, known: &[Uuid]) -> Option<Uuid> {
        let started = Instant::now();
        loop {
            if let Some((_name, id)) = self
                .named_items(folder)
                .await
                .into_iter()
                .find(|(listed, id)| listed == name && !known.contains(id))
            {
                return Some(id);
            }
            if started.elapsed() > SETTLE {
                return None;
            }
            tokio::time::sleep(POLL).await;
        }
    }

    /// Makes the run folder `F` under the root.
    async fn make_folder(&mut self) -> Result<Uuid, TestFailure> {
        let name = self.census.name("f");
        self.census
            .a
            .send(Command::CreateInventoryFolder {
                folder_id: InventoryFolderKey::from(Uuid::new_v4()),
                parent_id: InventoryFolderKey::from(self.root),
                folder_type: FolderType::None,
                name: name.clone(),
            })
            .await?;
        let listed = self
            .census
            .settle(self.root, |children| {
                children.iter().any(|(listed, _id)| *listed == name)
            })
            .await;
        if !listed {
            return Err(TestFailure::Assertion(
                "the grid never listed the run folder".to_owned(),
            ));
        }
        let id = self
            .census
            .children(self.root)
            .await
            .into_iter()
            .find(|(listed, _id)| *listed == name)
            .map(|(_name, id)| id)
            .ok_or_else(|| {
                TestFailure::Assertion("the grid never listed the run folder".to_owned())
            })?;
        self.roles.name(id, "F");
        self.made.folders.push(id);
        Ok(id)
    }

    /// Makes an item of `asset_type` named `word` in `F` with
    /// `CreateInventoryItem`, named `role`; what the grid answered and the
    /// window's pushes.
    async fn make_item(
        &mut self,
        word: &str,
        asset_type: AssetType,
        inv_type: InventoryType,
        role: &str,
    ) -> Result<(Option<Made1>, Heard), TestFailure> {
        let name = self.census.name(word);
        let mark = self.begin().await;
        self.census
            .a
            .send(Command::CreateInventoryItem(NewInventoryItem {
                folder_id: InventoryFolderKey::from(self.folder),
                transaction_id: Uuid::nil(),
                next_owner_mask: NEXT_OWNER_FULL,
                asset_type,
                inv_type,
                wearable_type: WearableType::Shape,
                name: name.clone(),
                description: "sl-conformance notecards-gestures".to_owned(),
            }))
            .await?;
        let mut heard = self.listen(mark, PUSH_WINDOW).await;
        let started = Instant::now();
        while !heard.items().iter().any(|item| item.name == name)
            && started.elapsed() < HTTP_TIMEOUT
        {
            heard.extend(self.listen(self.now(), LATE_WINDOW).await);
        }
        let made = heard
            .items()
            .into_iter()
            .find(|item| item.name == name)
            .map(|item| Made1 {
                item: item.item_id.uuid(),
                asset: item.asset_id,
            });
        if let Some(made) = made {
            self.roles.name(made.item, role);
        }
        Ok((made, heard))
    }

    /// Saves `body` into `item` over the update capability of `kind`; the
    /// answer and the asset the save named.
    async fn save(
        &mut self,
        item: Uuid,
        kind: UpdatableAssetType,
        body: Vec<u8>,
    ) -> Result<(String, Option<Uuid>), TestFailure> {
        if self.census.a.cap(kind.cap()).is_none() {
            return Ok(("no capability".to_owned(), None));
        }
        let _mark = self.begin().await;
        self.census
            .a
            .send(Command::UpdateInventoryAsset {
                location: AssetUpdateLocation::AgentInventory {
                    item_id: InventoryKey::from(item),
                },
                asset_type: kind,
                data: body,
            })
            .await?;
        let outcome = self
            .census
            .a
            .wait_for(SAVE_TIMEOUT, |event| match event {
                Event::AssetUploaded { new_asset, .. } => Some(if new_asset.is_nil() {
                    ("saved a nil asset".to_owned(), None)
                } else {
                    ("saved".to_owned(), Some(*new_asset))
                }),
                Event::AssetUploadFailed { reason } => {
                    Some((format!("failed: {}", without_ids(reason)), None))
                }
                _ => None,
            })
            .await;
        match outcome {
            Ok(answer) => Ok(answer),
            Err(TestFailure::Timeout(_)) => Ok(("no answer".to_owned(), None)),
            Err(other) => Err(other),
        }
    }

    /// Reads `asset` of `kind` through `ViewerAsset`: the status and the
    /// bytes, where it answered `200`.
    async fn viewer_asset(&self, asset: Uuid, kind: AssetType) -> (String, Option<Vec<u8>>) {
        if asset.is_nil() {
            return ("no asset".to_owned(), None);
        }
        let (Some(url), Some(key)) = (self.census.a.cap(VIEWER_ASSET), kind.get_asset_query_key())
        else {
            return ("no capability".to_owned(), None);
        };
        let request = self
            .census
            .http
            .get(format!("{url}?{key}={asset}"))
            .header(ACCEPT, "*/*");
        match request.send().await {
            Ok(response) => {
                let status = response.status().as_u16();
                let bytes = response.bytes().await.map(|bytes| bytes.to_vec()).ok();
                if status == 200 {
                    ("200".to_owned(), bytes)
                } else {
                    (status.to_string(), None)
                }
            }
            Err(error) => (
                format!("no answer: {}", without_ids(&error.to_string())),
                None,
            ),
        }
    }

    /// Reads the asset of the inventory item `item` with the UDP
    /// `TransferRequest` the reference reads a notecard with (an inventory
    /// item's source, no task): the outcome and the bytes.
    async fn transfer(
        &mut self,
        item: Uuid,
        asset: Uuid,
        kind: AssetType,
    ) -> Result<(String, Option<Vec<u8>>), TestFailure> {
        let _mark = self.begin().await;
        self.census
            .a
            .send(Command::FetchInventoryItemAsset {
                item_id: InventoryKey::from(item),
                asset_id: sl_client_tokio::AssetKey::from(asset),
                asset_type: kind,
            })
            .await?;
        let wanted = InventoryKey::from(item);
        let outcome = self
            .census
            .a
            .wait_for(TRANSFER_TIMEOUT, |event| match event {
                Event::InventoryItemAssetReceived {
                    item: received,
                    data,
                    ..
                } if *received == wanted => Some(("received".to_owned(), Some(data.clone()))),
                Event::TransferFailed { status, .. } => Some((format!("failed: {status:?}"), None)),
                _ => None,
            })
            .await;
        match outcome {
            Ok(answer) => Ok(answer),
            Err(TestFailure::Timeout(_)) => Ok(("no answer".to_owned(), None)),
            Err(other) => Err(other),
        }
    }

    /// Reads `asset` through both roads and records each under `leg`
    /// (`_viewer_asset`, `_transfer`), described against `written`; the bytes
    /// the first road that answered read.
    async fn read_back(
        &mut self,
        metrics: &mut Metrics,
        leg: &str,
        item: Uuid,
        asset: Uuid,
        kind: AssetType,
        written: Option<&[u8]>,
    ) -> Result<Option<Vec<u8>>, TestFailure> {
        let (viewer_status, viewer_bytes) = self.viewer_asset(asset, kind).await;
        metrics.set(
            &format!("{leg}_viewer_asset"),
            road_text(&viewer_status, viewer_bytes.as_deref(), written),
        );
        let (transfer_status, transfer_bytes) = self.transfer(item, asset, kind).await?;
        metrics.set(
            &format!("{leg}_transfer"),
            road_text(&transfer_status, transfer_bytes.as_deref(), written),
        );
        Ok(transfer_bytes.or(viewer_bytes))
    }

    /// Records what the item fetch states of `item` under `leg`: whether it
    /// is there, its asset against `saved`, its flags and masks.
    async fn record_item(&self, metrics: &mut Metrics, leg: &str, item: Uuid, saved: Option<Uuid>) {
        let state = self.item(item).await;
        metrics.set(&format!("{leg}_listed"), state.is_some());
        let asset = state.as_ref().and_then(|state| state.asset);
        metrics.set(&format!("{leg}_asset"), asset_role(asset, saved));
        metrics.set(
            &format!("{leg}_flags"),
            state
                .as_ref()
                .and_then(|state| state.flags)
                .map_or_else(|| "none".to_owned(), |flags| format!("{flags:#x}")),
        );
        metrics.set(
            &format!("{leg}_masks"),
            state.as_ref().map_or_else(
                || "none".to_owned(),
                |state| format!("{} {}", mask_text(state.masks.0), mask_text(state.masks.1)),
            ),
        );
    }

    /// POSTs a `CopyInventoryFromNotecard` of `embedded` out of `notecard`
    /// into `folder` (nil: the grid's pick), and records the answer, the
    /// pushes and the copy under `leg`; the copy's id, where one was found.
    async fn copy_out(
        &mut self,
        metrics: &mut Metrics,
        leg: &str,
        notecard: Uuid,
        embedded: &EmbeddedItem,
        folder: Uuid,
    ) -> Result<Option<Uuid>, TestFailure> {
        let body = map([
            ("notecard-id", Llsd::Uuid(notecard)),
            ("object-id", Llsd::Uuid(Uuid::nil())),
            ("item-id", Llsd::Uuid(embedded.item_id.0)),
            ("folder-id", Llsd::Uuid(folder)),
            ("callback-id", Llsd::Integer(COPY_CALLBACK)),
        ]);
        let mark = self.begin().await;
        let raw = self.census.post(COPY_FROM_NOTECARD, &body).await;
        metrics.set(&format!("{leg}_answer"), raw_text(&raw));
        let mut heard = self.listen(mark, PUSH_WINDOW).await;
        // The copy: what a push named, or a new item of the embedded item's
        // name in the folder asked for (or the Notecards folder).
        let known: Vec<Uuid> = self.roles_ids();
        let pushed = heard
            .items()
            .into_iter()
            .find(|item| !known.contains(&item.item_id.uuid()))
            .map(|item| item.item_id.uuid());
        let found = match pushed {
            Some(id) => Some(id),
            None if raw.status == Ok(200) => {
                let look_in = if folder.is_nil() {
                    self.notecards
                } else {
                    folder
                };
                self.new_item_named(look_in, &embedded.name, &known).await
            }
            None => None,
        };
        heard.extend(self.listen(self.now(), LATE_WINDOW).await);
        if let Some(id) = found {
            self.roles
                .name(id, &format!("the {} copy", leg.replace('_', " ")));
        }
        self.record_push(metrics, leg, &heard);
        metrics.set(&format!("{leg}_callback"), callback_text(&heard));
        let Some(id) = found else {
            for key in ["filed", "asset", "masks", "next_mask", "name"] {
                metrics.set(&format!("{leg}_{key}"), "no copy");
            }
            return Ok(None);
        };
        let state = self.item(id).await;
        let parent = state.as_ref().and_then(|state| state.parent);
        metrics.set(
            &format!("{leg}_filed"),
            parent.map_or_else(|| "not fetched".to_owned(), |parent| self.roles.of(parent)),
        );
        metrics.set(
            &format!("{leg}_asset"),
            asset_role(
                state.as_ref().and_then(|state| state.asset),
                Some(embedded.asset_id.0),
            ),
        );
        metrics.set(
            &format!("{leg}_masks"),
            state.as_ref().map_or_else(
                || "none".to_owned(),
                |state| format!("{} {}", mask_text(state.masks.0), mask_text(state.masks.1)),
            ),
        );
        metrics.set(
            &format!("{leg}_next_mask"),
            mask_text(state.as_ref().and_then(|state| state.next_mask)),
        );
        metrics.set(
            &format!("{leg}_name"),
            match state.as_ref().map(|state| state.name.as_str()) {
                None => "not fetched".to_owned(),
                Some(name) if name == embedded.name => "the embedded item's".to_owned(),
                Some(_other) => "another".to_owned(),
            },
        );
        if parent != Some(self.folder) {
            self.strays.items.push(id);
        }
        Ok(Some(id))
    }

    /// Every id the run has named.
    fn roles_ids(&self) -> Vec<Uuid> {
        self.roles.ids()
    }

    /// Sends a raw `RemoveInventoryItem` of `item`.
    async fn remove_item(&self, item: Uuid) -> Result<(), TestFailure> {
        self.census
            .send_raw(sl_client_tokio::AnyMessage::RemoveInventoryItem(
                RemoveInventoryItem {
                    agent_data: RemoveInventoryItemAgentDataBlock {
                        agent_id: self.census.agent,
                        session_id: self.census.session_id,
                    },
                    inventory_data: vec![RemoveInventoryItemInventoryDataBlock { item_id: item }],
                },
            ))
            .await
    }
}

/// How an asset id stands to `saved`: the saved one, another, nil or none.
fn asset_role(asset: Option<Uuid>, saved: Option<Uuid>) -> String {
    match asset {
        None => "none".to_owned(),
        Some(asset) if asset.is_nil() => "nil".to_owned(),
        Some(asset) if Some(asset) == saved => "the saved asset".to_owned(),
        Some(_asset) => "an asset".to_owned(),
    }
}

/// One road's answer as a metric: its status and, where it read bytes, what
/// they are.
fn road_text(status: &str, bytes: Option<&[u8]>, written: Option<&[u8]>) -> String {
    match bytes {
        Some(bytes) => format!("{status}: {}", describe_body(bytes, written)),
        None => status.to_owned(),
    }
}

/// What a body is, in words that name no id.
fn describe_body(bytes: &[u8], written: Option<&[u8]>) -> String {
    if written == Some(bytes) {
        return "the bytes written".to_owned();
    }
    if bytes.is_empty() {
        return "empty".to_owned();
    }
    if bytes.iter().all(|byte| *byte == 0) {
        return format!("{} zero bytes", bytes.len());
    }
    if let Ok(notecard) = Notecard::decode(bytes) {
        return format!(
            "a notecard, {} embedded items, {} text",
            notecard.items.len(),
            if notecard.text.trim().is_empty() {
                "no"
            } else {
                "some"
            }
        );
    }
    let first_line: String = String::from_utf8_lossy(bytes)
        .lines()
        .next()
        .unwrap_or_default()
        .chars()
        .take(24)
        .collect();
    format!(
        "{} bytes, first line {:?}{}",
        bytes.len(),
        first_line,
        if written.is_some() {
            ", not the bytes written"
        } else {
            ""
        }
    )
}

/// A short body as text, every byte that is not printable ASCII escaped.
fn escaped(bytes: &[u8]) -> String {
    bytes.escape_ascii().to_string()
}

/// A raw capability answer as a metric: status, top keys, ids scrubbed.
fn raw_text(raw: &Raw) -> String {
    match &raw.status {
        Err(reason) => reason.clone(),
        Ok(status) => match &raw.body {
            Some(Llsd::Undef) => format!("{status}: undef"),
            Some(body) if body.as_map().is_some() => raw.summary(""),
            Some(other) => format!("{status}: {}", without_ids(&format!("{other:?}"))),
            None => format!("{status}: no LLSD"),
        },
    }
}

/// Whether a window's announcements carried the copies' callback id.
fn callback_text(heard: &Heard) -> String {
    let mut seen = Vec::new();
    for event in &heard.events {
        match event {
            Event::InventoryItemCreated { callback_id, .. } => {
                seen.push(callback_id.map(|id| id.0));
            }
            Event::InventoryBulkUpdate {
                items,
                item_callbacks,
                ..
            } => {
                if item_callbacks.is_empty() {
                    seen.extend(items.iter().map(|_item| None));
                } else {
                    seen.extend(
                        item_callbacks
                            .iter()
                            .map(|(_item, callback)| Some(callback.0)),
                    );
                }
            }
            _ => {}
        }
    }
    if seen.is_empty() {
        return "no announcement".to_owned();
    }
    let mut words: Vec<&str> = seen
        .iter()
        .map(|id| match id {
            Some(id) if i64::from(*id) == i64::from(COPY_CALLBACK) => "echoed",
            Some(0) | None => "none",
            Some(_other) => "another",
        })
        .collect();
    words.sort_unstable();
    words.dedup();
    words.join(",")
}

/// The fields of `written` an embedded item read back as `read` differs in;
/// `as written` for none.
fn embedded_differences(written: &EmbeddedItem, read: &EmbeddedItem) -> String {
    let mut differing: Vec<String> = Vec::new();
    let id = |written: Key, read: Key| {
        (written != read).then(|| {
            if read.0.is_nil() {
                "nil".to_owned()
            } else {
                "another".to_owned()
            }
        })
    };
    let mask = |written: PermissionMask, read: PermissionMask| {
        (written != read).then(|| format!("{:#010x} to {:#010x}", written.0, read.0))
    };
    let written_perms = &written.permissions;
    let read_perms = &read.permissions;
    for (name, change) in [
        ("item_id", id(written.item_id, read.item_id)),
        ("parent_id", id(written.parent_id, read.parent_id)),
        (
            "base_mask",
            mask(written_perms.base_mask, read_perms.base_mask),
        ),
        (
            "owner_mask",
            mask(written_perms.owner_mask, read_perms.owner_mask),
        ),
        (
            "group_mask",
            mask(written_perms.group_mask, read_perms.group_mask),
        ),
        (
            "everyone_mask",
            mask(written_perms.everyone_mask, read_perms.everyone_mask),
        ),
        (
            "next_owner_mask",
            mask(written_perms.next_owner_mask, read_perms.next_owner_mask),
        ),
        (
            "creator_id",
            id(written_perms.creator_id, read_perms.creator_id),
        ),
        ("owner_id", id(written_perms.owner_id, read_perms.owner_id)),
        (
            "last_owner_id",
            id(written_perms.last_owner_id, read_perms.last_owner_id),
        ),
        ("group_id", id(written_perms.group_id, read_perms.group_id)),
        ("asset_id", id(written.asset_id, read.asset_id)),
        (
            "asset_id_encoding",
            (written.asset_id_encoding != read.asset_id_encoding)
                .then(|| format!("{:?}", read.asset_id_encoding)),
        ),
        (
            "flags",
            (written.flags != read.flags).then(|| format!("{:#x}", read.flags)),
        ),
        (
            "sale_info",
            (written.sale_info != read.sale_info).then(|| format!("{:?}", read.sale_info)),
        ),
        ("name", (written.name != read.name).then(String::new)),
        (
            "description",
            (written.description != read.description).then(String::new),
        ),
        (
            "creation_date",
            (written.creation_date != read.creation_date).then(String::new),
        ),
        (
            "unknown_fields",
            (written.unknown_fields != read.unknown_fields).then(|| read.unknown_fields.join("|")),
        ),
    ] {
        if let Some(change) = change {
            differing.push(if change.is_empty() {
                name.to_owned()
            } else {
                format!("{name} {change}")
            });
        }
    }
    if differing.is_empty() {
        "as written".to_owned()
    } else {
        format!("differs in {}", differing.join("; "))
    }
}

/// An embedded item made from the item fetch's entry for an item.
fn embedded_from(entry: &Llsd, agent: Uuid) -> EmbeddedItem {
    let state = item_state(entry);
    let permissions = entry.get("permissions");
    let id = |value: Option<&Llsd>| Key(value.and_then(uuid_of).unwrap_or_default());
    let mask = |key: &str| {
        PermissionMask(
            permissions
                .and_then(|map| map.get(key))
                .and_then(crate::cases::inventory_mutations::int_of)
                .and_then(|mask| u32::try_from(mask & 0xffff_ffff).ok())
                .unwrap_or_default(),
        )
    };
    let sale = entry.get("sale_info");
    EmbeddedItem {
        item_id: id(entry.get("item_id")),
        parent_id: id(entry.get("parent_id")),
        permissions: Permissions {
            base_mask: mask("base_mask"),
            owner_mask: mask("owner_mask"),
            group_mask: mask("group_mask"),
            everyone_mask: mask("everyone_mask"),
            next_owner_mask: mask("next_owner_mask"),
            creator_id: id(permissions.and_then(|map| map.get("creator_id"))),
            owner_id: Key(agent),
            last_owner_id: id(permissions.and_then(|map| map.get("last_owner_id"))),
            group_id: id(permissions.and_then(|map| map.get("group_id"))),
            group_owned: false,
        },
        metadata: None,
        asset_id: Key(state.asset.unwrap_or_default()),
        asset_id_encoding: AssetIdEncoding::Plain,
        asset_type: sl_notecard::AssetType::from_type_name("notecard"),
        inventory_type: sl_notecard::InventoryType::from_type_name("notecard"),
        flags: state
            .flags
            .and_then(|flags| u32::try_from(flags & 0xffff_ffff).ok())
            .unwrap_or_default(),
        sale_info: SaleInfo {
            sale_type: SaleType::from_type_name(
                match sale
                    .and_then(|sale| sale.get("sale_type"))
                    .and_then(crate::cases::inventory_mutations::int_of)
                {
                    Some(1) => "orig",
                    Some(2) => "copy",
                    Some(3) => "cntn",
                    _ => "not",
                },
            ),
            sale_price: sale
                .and_then(|sale| sale.get("sale_price"))
                .and_then(crate::cases::inventory_mutations::int_of)
                .and_then(|price| i32::try_from(price).ok())
                .unwrap_or_default(),
        },
        name: state.name,
        description: state.description,
        creation_date: entry
            .get("created_at")
            .and_then(crate::cases::inventory_mutations::int_of)
            .unwrap_or_default(),
        unknown_fields: Vec::new(),
    }
}

/// A notecard body of `text` embedding `items`, each marked once at the end.
fn notecard_body(text: &str, items: Vec<EmbeddedItem>) -> Vec<u8> {
    let markers: String = (0..items.len())
        .filter_map(|index| u32::try_from(index).ok())
        .filter_map(sl_notecard::embedded_char)
        .collect();
    Notecard {
        source_version: NotecardVersion::V2,
        items,
        text: format!("{text}{markers}\n"),
    }
    .encode()
}

/// A gesture body (`LLMultiGesture::serialize`): no key, the run's trigger,
/// one chat step, and the NUL the reference ends the asset with.
fn gesture_body(line: &str) -> Vec<u8> {
    format!("2\n0\n0\n{TRIGGER}\n\n1\n2\n{line}\n0\n\0").into_bytes()
}

/// Runs every leg, recording into `metrics`.
async fn census(a: &mut Session, metrics: &mut Metrics) -> Result<(), TestFailure> {
    a.wait_for_region(REGION_TIMEOUT).await?;
    wait_for_caps(a).await;
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
    let gestures = system(FolderType::Gesture);
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
    if let Some(gestures) = gestures {
        roles.name(gestures, "Gestures");
    }
    let mut run = Run {
        census: Census {
            a,
            agent,
            session_id,
            http,
            tag,
        },
        roles,
        root,
        trash,
        notecards,
        folder: Uuid::nil(),
        made: Made::default(),
        strays: Strays::default(),
    };
    run.census
        .a
        .send(Command::ProbeCircuits(CircuitProbe::Observe))
        .await?;
    let outcome = legs(&mut run, metrics).await;
    let cleaned = clean_up_run(&run).await;
    run.census
        .a
        .send(Command::ProbeCircuits(CircuitProbe::Off))
        .await?;
    outcome.and(cleaned)
}

/// Waits for the inventory capabilities the census reads through.
async fn wait_for_caps(a: &Session) {
    let started = Instant::now();
    while [FID2, FI2].iter().any(|name| a.cap(name).is_none()) && started.elapsed() < CAP_WAIT {
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

/// Whether a login answer carried the `gestures` array: absent, an empty
/// list, or how many entries.
fn gestures_presence(login: &sl_client_tokio::LoginSuccess) -> String {
    if !login.gestures.is_empty() {
        return "a list".to_owned();
    }
    if login.empty_lists.contains(&LoginList::Gestures) {
        "an empty list".to_owned()
    } else {
        "absent".to_owned()
    }
}

/// Every leg, in order.
async fn legs(run: &mut Run<'_>, metrics: &mut Metrics) -> Result<(), TestFailure> {
    // Folders an earlier run left behind go with this run's clean-up.
    for folder in [run.root, run.trash] {
        let leftovers: Vec<Uuid> = run
            .census
            .listing(folder, View::Fetch)
            .await
            .into_iter()
            .filter(|(name, _id)| name.starts_with("conf-inv-"))
            .map(|(_name, id)| id)
            .collect();
        run.made.folders.extend(leftovers);
    }
    let stray: Vec<Uuid> = run
        .named_items(run.notecards)
        .await
        .into_iter()
        .filter(|(name, _id)| name.starts_with("conf-inv-"))
        .map(|(_name, id)| id)
        .collect();
    run.strays.items.extend(stray);
    run.folder = run.make_folder().await?;
    notecards(run, metrics).await?;
    gestures(run, metrics).await
}

/// Legs 1 to 4: notecards, their embedded items and the copies out of them.
async fn notecards(run: &mut Run<'_>, metrics: &mut Metrics) -> Result<(), TestFailure> {
    // Leg 1: a new notecard, nothing saved.
    let (fresh, heard) = run
        .make_item(
            "new",
            AssetType::Notecard,
            InventoryType::Notecard,
            "the new notecard",
        )
        .await?;
    run.record_push(metrics, "new_note", &heard);
    let fresh = fresh.ok_or_else(|| {
        TestFailure::Assertion("the grid never answered the notecard's creation".to_owned())
    })?;
    metrics.set(
        "new_note_created_asset",
        asset_role(Some(fresh.asset), None),
    );
    run.record_item(metrics, "new_note", fresh.item, None).await;
    let fetched = run.item(fresh.item).await.and_then(|state| state.asset);
    let _bytes = run
        .read_back(
            metrics,
            "new_note",
            fresh.item,
            fetched.unwrap_or_default(),
            AssetType::Notecard,
            None,
        )
        .await?;

    // Leg 2: a plain body saved.
    let (plain, _heard) = run
        .make_item(
            "plain",
            AssetType::Notecard,
            InventoryType::Notecard,
            "the embedded notecard",
        )
        .await?;
    let plain = plain.ok_or_else(|| {
        TestFailure::Assertion("the grid never answered the second notecard".to_owned())
    })?;
    let plain_body = notecard_body("sl-conformance notecards-gestures\n", Vec::new());
    let (answer, saved) = run
        .save(plain.item, UpdatableAssetType::Notecard, plain_body.clone())
        .await?;
    metrics.set("plain_save", answer);
    run.record_item(metrics, "plain", plain.item, saved).await;
    let _bytes = run
        .read_back(
            metrics,
            "plain",
            plain.item,
            saved.unwrap_or_default(),
            AssetType::Notecard,
            Some(&plain_body),
        )
        .await?;

    // Leg 3: a body embedding the saved notecard.
    let entry = run.item_raw(plain.item).await.ok_or_else(|| {
        TestFailure::Assertion("the item fetch did not state the saved notecard".to_owned())
    })?;
    let embedded = embedded_from(&entry, run.census.agent);
    let (holder, _heard) = run
        .make_item(
            "holder",
            AssetType::Notecard,
            InventoryType::Notecard,
            "the holding notecard",
        )
        .await?;
    let holder = holder.ok_or_else(|| {
        TestFailure::Assertion("the grid never answered the holding notecard".to_owned())
    })?;
    let holder_body = notecard_body("an embedded notecard: ", vec![embedded.clone()]);
    let (answer, holder_saved) = run
        .save(
            holder.item,
            UpdatableAssetType::Notecard,
            holder_body.clone(),
        )
        .await?;
    metrics.set("embed_save", answer);
    let read = run
        .read_back(
            metrics,
            "embed",
            holder.item,
            holder_saved.unwrap_or_default(),
            AssetType::Notecard,
            Some(&holder_body),
        )
        .await?;
    // What the grid stored is what a viewer copies out by: the embedded item
    // as it reads back, where it does.
    let decoded = read.as_deref().map(Notecard::decode);
    metrics.set(
        "embed_read_item",
        match &decoded {
            None => "not read".to_owned(),
            Some(Err(_error)) => "not a notecard".to_owned(),
            Some(Ok(notecard)) => match notecard.items.first() {
                None => "no embedded item".to_owned(),
                Some(item) => embedded_differences(&embedded, item),
            },
        },
    );
    let stored = decoded
        .and_then(Result::ok)
        .and_then(|notecard| notecard.items.into_iter().next())
        .unwrap_or_else(|| embedded.clone());

    // A body embedding an item nobody has.
    let forged = EmbeddedItem {
        item_id: Key(Uuid::new_v4()),
        asset_id: Key(Uuid::new_v4()),
        name: run.census.name("forged"),
        ..embedded.clone()
    };
    let (forger, _heard) = run
        .make_item(
            "forger",
            AssetType::Notecard,
            InventoryType::Notecard,
            "the forged notecard",
        )
        .await?;
    let forger = forger.ok_or_else(|| {
        TestFailure::Assertion("the grid never answered the forged notecard".to_owned())
    })?;
    let forged_body = notecard_body("a made-up item: ", vec![forged.clone()]);
    let (answer, _forged_saved) = run
        .save(forger.item, UpdatableAssetType::Notecard, forged_body)
        .await?;
    metrics.set("forged_save", answer);

    // Leg 4: the copies out of the notecard.
    let folder = run.folder;
    let _copy = run
        .copy_out(metrics, "copy", holder.item, &stored, folder)
        .await?;
    let _copy = run
        .copy_out(metrics, "copy_default", holder.item, &stored, Uuid::nil())
        .await?;
    let absent = EmbeddedItem {
        item_id: Key(Uuid::new_v4()),
        ..stored.clone()
    };
    let _copy = run
        .copy_out(metrics, "copy_absent", holder.item, &absent, folder)
        .await?;
    let _copy = run
        .copy_out(metrics, "copy_forged", forger.item, &forged, folder)
        .await?;
    Ok(())
}

/// Leg 5: gestures.
async fn gestures(run: &mut Run<'_>, metrics: &mut Metrics) -> Result<(), TestFailure> {
    let mut made = BTreeMap::new();
    for (word, role) in [
        ("on", "the active gesture"),
        ("off", "the deactivated gesture"),
        ("gone", "the removed gesture"),
    ] {
        let (gesture, heard) = run
            .make_item(word, AssetType::Gesture, InventoryType::Gesture, role)
            .await?;
        let gesture = gesture.ok_or_else(|| {
            TestFailure::Assertion(format!("the grid never answered the {word} gesture"))
        })?;
        if word == "on" {
            run.record_push(metrics, "new_gesture", &heard);
            metrics.set(
                "new_gesture_created_asset",
                asset_role(Some(gesture.asset), None),
            );
            run.record_item(metrics, "new_gesture", gesture.item, None)
                .await;
            let fetched = run.item(gesture.item).await.and_then(|state| state.asset);
            let bytes = run
                .read_back(
                    metrics,
                    "new_gesture",
                    gesture.item,
                    fetched.unwrap_or_default(),
                    AssetType::Gesture,
                    None,
                )
                .await?;
            metrics.set(
                "new_gesture_body",
                bytes
                    .as_deref()
                    .map_or_else(|| "not read".to_owned(), escaped),
            );
        }
        let body = gesture_body(&format!("sl-conformance {word}"));
        let (answer, saved) = run
            .save(gesture.item, UpdatableAssetType::Gesture, body.clone())
            .await?;
        if word == "on" {
            metrics.set("gesture_save", answer);
            run.record_item(metrics, "saved_gesture", gesture.item, saved)
                .await;
            let _bytes = run
                .read_back(
                    metrics,
                    "saved_gesture",
                    gesture.item,
                    saved.unwrap_or_default(),
                    AssetType::Gesture,
                    Some(&body),
                )
                .await?;
        }
        let asset = match saved {
            Some(asset) => asset,
            None => run
                .item(gesture.item)
                .await
                .and_then(|state| state.asset)
                .unwrap_or_default(),
        };
        let _previous = made.insert(word, (gesture.item, asset));
    }
    let (Some(&(on, on_asset)), Some(&(off, off_asset)), Some(&(gone, _gone_asset))) =
        (made.get("on"), made.get("off"), made.get("gone"))
    else {
        return Err(TestFailure::Assertion("a gesture went missing".to_owned()));
    };

    // Activate all three.
    let mark = run.begin().await;
    run.census
        .a
        .send(Command::ActivateGestures {
            gestures: made
                .values()
                .map(|&(item, asset)| GestureActivation {
                    item_id: InventoryKey::from(item),
                    asset_id: asset,
                })
                .collect(),
        })
        .await?;
    run.strays
        .active
        .extend(made.values().map(|&(item, _asset)| item));
    let heard = run.listen(mark, PUSH_WINDOW).await;
    run.record_push(metrics, "activate", &heard);
    run.record_item(metrics, "activated", on, Some(on_asset))
        .await;

    // Deactivate one.
    let mark = run.begin().await;
    run.census
        .a
        .send(Command::DeactivateGestures {
            item_ids: vec![InventoryKey::from(off)],
        })
        .await?;
    let heard = run.listen(mark, PUSH_WINDOW).await;
    run.record_push(metrics, "deactivate", &heard);
    run.record_item(metrics, "deactivated", off, Some(off_asset))
        .await;

    // Remove one while it is active.
    let mark = run.begin().await;
    run.remove_item(gone).await?;
    let heard = run.listen(mark, PUSH_WINDOW).await;
    run.record_push(metrics, "remove_active", &heard);
    metrics.set("remove_active_gone", run.item(gone).await.is_none());

    // The next login, asking for the array.
    // Once not asking for the array, once asking.
    relogin(run, false).await?;
    let unasked = run
        .census
        .a
        .login_success()
        .cloned()
        .ok_or_else(|| TestFailure::Assertion("no login answer kept".to_owned()))?;
    metrics.set("login_gestures_unasked", gestures_presence(&unasked));
    relogin(run, true).await?;
    let login = run
        .census
        .a
        .login_success()
        .cloned()
        .ok_or_else(|| TestFailure::Assertion("no login answer kept".to_owned()))?;
    metrics.set("login_gestures_asked", gestures_presence(&login));
    let mut listed: Vec<String> = login
        .gestures
        .iter()
        .filter(|entry| [on, off, gone].contains(&entry.item_id.uuid()))
        .map(|entry| run.roles.of(entry.item_id.uuid()))
        .collect();
    listed.sort_unstable();
    metrics.set(
        "login_gestures",
        if listed.is_empty() {
            "none of the run's".to_owned()
        } else {
            listed.join(",")
        },
    );
    metrics.set(
        "login_gesture_asset",
        login
            .gestures
            .iter()
            .find(|entry| entry.item_id.uuid() == on)
            .map_or_else(
                || "not listed".to_owned(),
                |entry| asset_role(Some(entry.asset_id), Some(on_asset)),
            ),
    );
    run.record_item(metrics, "relogged_active", on, Some(on_asset))
        .await;
    run.record_item(metrics, "relogged_deactivated", off, Some(off_asset))
        .await;
    Ok(())
}

/// Logs the avatar in again, asking for what the client asks for — less
/// `gestures` unless `gestures` — and picks up the new session.
async fn relogin(run: &mut Run<'_>, gestures: bool) -> Result<(), TestFailure> {
    let mut options = LoginRequest::new("", "", "", StartLocation::Last, "", "").options;
    options.retain(|option| option != "gestures");
    if gestures {
        options.push("gestures".to_owned());
    }
    run.census.a.relogin_with_options(Some(options)).await?;
    wait_for_caps(run.census.a).await;
    run.census.session_id =
        run.census.a.session_id().ok_or_else(|| {
            TestFailure::Assertion("the relogin reported no session id".to_owned())
        })?;
    run.census
        .a
        .send(Command::ProbeCircuits(CircuitProbe::Observe))
        .await?;
    Ok(())
}

/// Deactivates the run's gestures and takes away what it made.
async fn clean_up_run(run: &Run<'_>) -> Result<(), TestFailure> {
    if !run.strays.active.is_empty() {
        run.census
            .a
            .send(Command::DeactivateGestures {
                item_ids: run
                    .strays
                    .active
                    .iter()
                    .copied()
                    .map(InventoryKey::from)
                    .collect(),
            })
            .await?;
    }
    if !run.strays.items.is_empty() {
        run.census
            .a
            .send(Command::RemoveInventoryItems(
                run.strays
                    .items
                    .iter()
                    .copied()
                    .map(InventoryKey::from)
                    .collect(),
            ))
            .await?;
    }
    clean_up(&run.census, &run.made, run.root, run.trash, false).await
}

#[cfg(test)]
mod tests {
    use super::{describe_body, gesture_body, notecard_body};
    use pretty_assertions::assert_eq;

    /// A body is described against what was written, and by its shape where
    /// it is not that.
    #[test]
    fn a_body_is_described_by_its_shape() {
        let written = notecard_body("text\n", Vec::new());
        assert_eq!(describe_body(&written, Some(&written)), "the bytes written");
        assert_eq!(describe_body(&[0], None), "1 zero bytes");
        assert_eq!(
            describe_body(&notecard_body("", Vec::new()), None),
            "a notecard, 0 embedded items, no text"
        );
        assert_eq!(
            describe_body(&gesture_body("hi"), None),
            "30 bytes, first line \"2\""
        );
    }
}
