//! Every asset id the grid hands out names bytes the grid can serve — and a
//! save changes which bytes.
//!
//! Two halves, one per direction:
//!
//! - **The read half.** Walk the agent's own inventory, and for every seeded
//!   fixture item ([`sl_test_assets::inventory`]) fetch the asset its item
//!   declares. The bytes must equal the fixture's own body, which — because
//!   `sl-test-assets` reads every one of those bodies back through the decoder
//!   that owns its format — is how this case asserts "of the item's declared
//!   class" without linking eleven decoders itself.
//! - **The write half.** For each class a viewer can save *in place*, save the
//!   fixture's second body over the item by the route that class really uses
//!   (an `Update*AgentInventory` capability, `UpdateScriptAgent`, or the legacy
//!   UDP transaction upload for a wearable, which has no capability), then
//!   fetch the id the grid returned and require the **edited** bytes.
//!
//! That second half is the point. A save is only observable as a re-fetch: a
//! grid that stored the bytes and a grid that answered `complete` and dropped
//! them look identical to a viewer that trusts its own in-memory copy, which is
//! precisely the bug the round trip exists to catch. Comparing against the
//! *edited* body rather than merely "some bytes came back" is what makes the
//! difference visible — an echo of the seeded body would pass a length check
//! and a non-empty check.
//!
//! The prim's task inventory is the third place an asset id lives, and it gets
//! the same treatment — through a prim this case rezzes and an item it drops
//! in, because that is the case nothing could reach before: a task copy is
//! minted a *fresh* item id, so no stated `(task, item)` fixture could ever have
//! carried its bytes.
//!
//! Fake-grid only, and deliberately so. The fixtures are the fake grid's seeded
//! inventory, which no live grid has; the live-grid question this case's shape
//! comes from — *what a real grid returns for the same save*, which for several
//! classes is very probably not what went in — is
//! `test-asset-save-mutation-survey`'s to measure. Until it has, this case
//! asserts an echo, which is what the fake grid implements.

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use sl_client_tokio::{
    AssetCacheLimits, AssetKey, AssetStore, AssetType, AssetUpdateLocation, Command, Event,
    InventoryFolderKey, InventoryItem, InventoryKey, ObjectKey, PrimShape, ReqwestAssetFetcher,
    RestoreItem, SaleType, ScopedObjectId, ScriptTarget, ScriptUploadLocation, TaskInventoryKey,
    Throttle, TransactionId, TransferId, UpdatableAssetType, Uuid, Vector,
};
use sl_test_assets::inventory::{SavePath, SeededAsset};

use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{LONG_TIMEOUT, REGION_TIMEOUT, check};

/// How long to wait for another object update before calling the arrival burst
/// finished. Only the fake grid runs this case, and it streams its scene in one
/// go, so this is a settling gap rather than a budget.
const SETTLE_IDLE: Duration = Duration::from_millis(400);

/// Where the container prim is rezzed — the stock arrival point, a little above
/// the fake grid's flat 25 m ground.
const CONTAINER_POSITION: Vector = Vector {
    x: 128.0,
    y: 128.0,
    z: 27.0,
};

/// Drives every seeded inventory class through a fetch, and every savable one
/// through a save and a re-fetch.
#[derive(Debug)]
pub struct AssetRoundTrip;

impl GridTest for AssetRoundTrip {
    fn name(&self) -> &'static str {
        "asset-round-trip"
    }

    fn description(&self) -> &'static str {
        "Every seeded inventory item's asset resolves, and a save is readable back"
    }

    fn grids(&self) -> &'static [Grid] {
        // Fake only: the fixtures are its seeded inventory, and what a *live*
        // grid returns for the same save is a measurement nobody has taken yet
        // (test-asset-save-mutation-survey).
        &[Grid::Fake]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;
            session
                .send(Command::SetThrottle(Throttle::preset_1000()))
                .await?;

            let cap = session.cap("ViewerAsset").ok_or_else(|| {
                TestFailure::Assertion("no ViewerAsset capability offered".to_owned())
            })?;
            let held = crawl_inventory(session).await?;

            let fixtures = sl_test_assets::inventory::seeded_assets()
                .map_err(|error| TestFailure::Assertion(error.to_string()))?;
            let mut read = 0_i64;
            let mut saved = 0_i64;
            for fixture in &fixtures {
                let item = held.get(fixture.name).ok_or_else(|| {
                    TestFailure::Assertion(format!(
                        "the grid's inventory has no item named {:?}; a seeded class is missing",
                        fixture.name
                    ))
                })?;
                check_seeded_item(item, fixture)?;

                // --- the read half.
                let body = fetch(&cap, AssetKey::from(item.asset_id), fixture.asset_type).await?;
                check(
                    body == fixture.body,
                    &format!(
                        "{}: the asset its item names is not the body of its class \
                         ({} bytes fetched, {} expected)",
                        fixture.name,
                        body.len(),
                        fixture.body.len()
                    ),
                )?;
                read = read.saturating_add(1);

                // --- the write half, where the class has an in-place save.
                let Some(new_asset) = save(ctx, item, fixture).await? else {
                    continue;
                };
                check(
                    new_asset != item.asset_id,
                    &format!(
                        "{}: the save reported the asset id the item already had, \
                         so nothing can tell it apart from a swallowed save",
                        fixture.name
                    ),
                )?;
                let after = fetch(&cap, AssetKey::from(new_asset), fixture.asset_type).await?;
                check(
                    after == fixture.edited_body,
                    &format!(
                        "{}: the id the save returned does not resolve to what was saved \
                         ({} bytes back, {} written)",
                        fixture.name,
                        after.len(),
                        fixture.edited_body.len()
                    ),
                )?;
                saved = saved.saturating_add(1);
            }

            // --- the third store: an item inside a prim.
            let task_bytes = task_inventory_round_trip(ctx).await?;

            let metrics = ctx.metrics();
            metrics.set("classes_read", read);
            metrics.set("classes_saved", saved);
            metrics.set("task_item_bytes", i64::try_from(task_bytes).unwrap_or(-1));
            metrics.set(
                "classes_without_a_fixture",
                i64::try_from(sl_test_assets::inventory::unsupported_classes().len()).unwrap_or(-1),
            );
            Ok(())
        })
    }
}

/// The item metadata a seeded fixture must carry. An item whose *declared*
/// class is wrong is exactly as broken as one whose asset is missing — a viewer
/// picks the decoder from the item, not from the bytes.
fn check_seeded_item(item: &InventoryItem, fixture: &SeededAsset) -> Result<(), TestFailure> {
    check(
        !item.asset_id.is_nil(),
        &format!("{}: the seeded item has a nil asset id", fixture.name),
    )?;
    check(
        i32::from(item.item_type) == fixture.asset_type.to_code(),
        &format!(
            "{}: the item declares asset class {} but the fixture is {:?}",
            fixture.name, item.item_type, fixture.asset_type
        ),
    )?;
    check(
        i32::from(item.inv_type) == fixture.inv_type.to_code(),
        &format!(
            "{}: the item declares inventory class {} but the fixture is {:?}",
            fixture.name, item.inv_type, fixture.inv_type
        ),
    )
}

/// Every item in the agent's inventory, by name.
///
/// Walks the whole tree rather than the folders the fixtures are filed in, so a
/// fixture that moved folder still reports "found in the wrong folder" through
/// its class check rather than as a missing item.
async fn crawl_inventory(
    session: &mut Session,
) -> Result<BTreeMap<String, InventoryItem>, TestFailure> {
    session.send(Command::QueryInventoryRoots).await?;
    let root = session
        .wait_for(LONG_TIMEOUT, |event| match event {
            Event::InventoryRoots { agent_root, .. } => Some(*agent_root),
            _other => None,
        })
        .await?
        .ok_or_else(|| TestFailure::Assertion("the grid reported no agent root".to_owned()))?;

    let mut items = BTreeMap::new();
    let mut queue = vec![root];
    while let Some(folder) = queue.pop() {
        for (name, item) in read_folder(session, folder, &mut queue).await? {
            let _replaced = items.insert(name, item);
        }
    }
    Ok(items)
}

/// One folder's items (by name), pushing its subfolders onto `queue`.
async fn read_folder(
    session: &mut Session,
    folder: InventoryFolderKey,
    queue: &mut Vec<InventoryFolderKey>,
) -> Result<Vec<(String, InventoryItem)>, TestFailure> {
    session.send(Command::RequestFolderContents(folder)).await?;
    let (folders, items) = session
        .wait_for(LONG_TIMEOUT, |event| match event {
            Event::InventoryDescendents {
                folder_id,
                folders,
                items,
                ..
            } if *folder_id == folder => Some((folders.clone(), items.clone())),
            _other => None,
        })
        .await?;
    queue.extend(folders.iter().map(|folder| folder.folder_id));
    Ok(items
        .into_iter()
        .map(|item| (item.name.clone(), item))
        .collect())
}

/// Saves `fixture.edited_body` onto `item` by the route its class really uses,
/// and returns the asset id the grid reported — or `None` for a class with no
/// in-place save at all.
async fn save(
    ctx: &mut TestContext,
    item: &InventoryItem,
    fixture: &SeededAsset,
) -> Result<Option<Uuid>, TestFailure> {
    let data = fixture.edited_body.clone();
    match fixture.save_path {
        SavePath::NewFileOnly => Ok(None),
        SavePath::UpdateCap(kind) => save_over_cap(ctx, item.item_id, kind, data).await.map(Some),
        SavePath::ScriptCap => save_script(ctx, item.item_id, data).await.map(Some),
        SavePath::UdpTransaction => save_over_transaction(ctx, item, fixture, data)
            .await
            .map(Some),
    }
}

/// The `Update*AgentInventory` two-stage capability.
async fn save_over_cap(
    ctx: &mut TestContext,
    item_id: InventoryKey,
    kind: UpdatableAssetType,
    data: Vec<u8>,
) -> Result<Uuid, TestFailure> {
    let session = ctx.primary();
    session
        .send(Command::UpdateInventoryAsset {
            location: AssetUpdateLocation::AgentInventory { item_id },
            asset_type: kind,
            data,
        })
        .await?;
    let outcome = session
        .wait_for(LONG_TIMEOUT, |event| match event {
            Event::AssetUploaded { new_asset, .. } => Some(Ok(*new_asset)),
            Event::AssetUploadFailed { reason } => Some(Err(reason.clone())),
            _other => None,
        })
        .await?;
    outcome.map_err(|reason| TestFailure::Assertion(format!("{} failed: {reason}", kind.cap())))
}

/// `UpdateScriptAgent`, whose completion also carries the compile result.
async fn save_script(
    ctx: &mut TestContext,
    item_id: InventoryKey,
    source: Vec<u8>,
) -> Result<Uuid, TestFailure> {
    let session = ctx.primary();
    session
        .send(Command::UploadScript {
            location: ScriptUploadLocation::AgentInventory { item_id },
            target: ScriptTarget::Mono,
            source,
        })
        .await?;
    let outcome = session
        .wait_for(LONG_TIMEOUT, |event| match event {
            Event::ScriptUploaded {
                new_asset,
                compiled,
                errors,
                ..
            } => Some(Ok((*new_asset, *compiled, errors.clone()))),
            Event::AssetUploadFailed { reason } => Some(Err(reason.clone())),
            _other => None,
        })
        .await?;
    let (new_asset, compiled, errors) = outcome
        .map_err(|reason| TestFailure::Assertion(format!("UpdateScriptAgent failed: {reason}")))?;
    check(compiled, &format!("the script did not compile: {errors:?}"))?;
    new_asset.ok_or_else(|| TestFailure::Assertion("the script upload named no asset".to_owned()))
}

/// The legacy UDP transaction upload — the wearable save, which has no
/// capability. The asset id is the client's own prediction
/// (`combine(transaction, secure session)`), and the item is rebound by the
/// `UpdateInventoryItem` the same call sends.
async fn save_over_transaction(
    ctx: &mut TestContext,
    item: &InventoryItem,
    fixture: &SeededAsset,
    data: Vec<u8>,
) -> Result<Uuid, TestFailure> {
    let session = ctx.primary();
    let transaction_id = TransactionId::from(Uuid::new_v4());
    session
        .send(Command::SaveInventoryAsset {
            item: Box::new(item.clone()),
            asset_type: fixture.asset_type,
            transaction_id,
            data,
        })
        .await?;
    let (asset_id, success) = session
        .wait_for(LONG_TIMEOUT, |event| match event {
            Event::InventoryAssetSaved { asset_id, success } => Some((*asset_id, *success)),
            _other => None,
        })
        .await?;
    check(
        success,
        &format!("{}: the simulator refused the wearable save", fixture.name),
    )?;
    Ok(asset_id)
}

/// The third store: an item inside a prim, and the half nothing could reach
/// before.
///
/// The prim is **rezzed here**, and the item is **dropped in here**, rather than
/// taken from a fixture — because that is the case that used to be
/// unanswerable. Task-item bytes were a `(task, item)` map stated up front, and
/// an item dropped into a prim is minted a fresh task item id, so no fixture
/// could ever have stated its bytes: the `TransferRequest` for it was refused,
/// and the one item whose contents serial a test had just watched advance was
/// the one item whose asset could not be read back.
///
/// So: rez a cube, drop the seeded notecard into it, learn the fresh item id
/// from the listing, and fetch its asset over the legacy UDP `TransferRequest`
/// — the only path a task item's asset is served on, on either grid. Then save
/// a new body over it through `UpdateNotecardTaskInventory` and fetch again, so
/// the write half is covered too. Returns the byte count read back.
///
/// The donor's bytes are re-read over `ViewerAsset` rather than assumed to be
/// the seeded body, and the body written below is picked to differ from *those*
/// bytes. The write half above already saved over the same item, so the seeded
/// body is not what it holds any more — an assertion against
/// [`SeededAsset::body`] here would only be testing the order the two halves
/// happen to run in.
async fn task_inventory_round_trip(ctx: &mut TestContext) -> Result<usize, TestFailure> {
    // The notecard, because it is the one class with both a task-inventory
    // update capability and a decoder.
    let fixture = sl_test_assets::inventory::seeded_assets()
        .map_err(|error| TestFailure::Assertion(error.to_string()))?
        .into_iter()
        .find(|fixture| matches!(fixture.asset_type, AssetType::Notecard))
        .ok_or_else(|| TestFailure::Assertion("no notecard fixture".to_owned()))?;
    let cap = ctx
        .primary()
        .cap("ViewerAsset")
        .ok_or_else(|| TestFailure::Assertion("no ViewerAsset capability offered".to_owned()))?;
    let held = crawl_inventory(ctx.primary()).await?;
    let donor = held
        .get(fixture.name)
        .ok_or_else(|| TestFailure::Assertion("the notecard fixture is not seeded".to_owned()))?
        .clone();
    let donor_body = fetch(&cap, AssetKey::from(donor.asset_id), AssetType::Notecard).await?;
    // Whichever of the fixture's two bodies the donor is *not*, so the save
    // below is always observable.
    let wanted = if donor_body == fixture.edited_body {
        fixture.body.clone()
    } else {
        fixture.edited_body.clone()
    };

    let (container_id, container) = rez_container(ctx).await?;

    // Drop the notecard in. The grid mints a *fresh* task item id for the copy,
    // which is the whole point — nothing could have stated its bytes ahead of
    // time.
    ctx.primary()
        .send(Command::UpdateTaskInventory {
            target: container_id,
            key: TaskInventoryKey::Item,
            item: Box::new(task_item(&donor)),
        })
        .await?;
    let dropped = fetch_task_item_id(ctx.primary(), container_id, container, &donor.name).await?;

    // The read half: the copy carries the donor's asset id across, so it
    // resolves to the donor's own bytes.
    let read_back = fetch_task_item(
        ctx.primary(),
        container,
        dropped,
        AssetKey::from(Uuid::nil()),
        AssetType::Notecard,
    )
    .await?;
    check(
        read_back == donor_body,
        &format!(
            "a notecard dropped into a prim does not resolve to the donor's bytes \
             ({} back, {} expected)",
            read_back.len(),
            donor_body.len()
        ),
    )?;

    // The write half: save a different body over the item inside the prim.
    ctx.primary()
        .send(Command::UpdateInventoryAsset {
            location: AssetUpdateLocation::TaskInventory {
                task_id: container,
                item_id: dropped,
            },
            asset_type: UpdatableAssetType::Notecard,
            data: wanted.clone(),
        })
        .await?;
    let _saved = ctx
        .primary()
        .wait_for(LONG_TIMEOUT, |event| match event {
            Event::AssetUploaded { new_asset, .. } => Some(Ok(*new_asset)),
            Event::AssetUploadFailed { reason } => Some(Err(reason.clone())),
            _other => None,
        })
        .await?
        .map_err(|reason| {
            TestFailure::Assertion(format!("UpdateNotecardTaskInventory failed: {reason}"))
        })?;

    // The asset id passed here is deliberately **nil**: the resolver must answer
    // from the item the region holds, not from what the client claims, which is
    // what makes the save observable at all.
    let after = fetch_task_item(
        ctx.primary(),
        container,
        dropped,
        AssetKey::from(Uuid::nil()),
        AssetType::Notecard,
    )
    .await?;
    check(
        after == wanted,
        &format!(
            "the task item's asset is not what was saved into it ({} back, {} written)",
            after.len(),
            wanted.len()
        ),
    )?;
    Ok(after.len())
}

/// Rezzes a cube to hold a task inventory, returning its scoped and full ids.
async fn rez_container(ctx: &mut TestContext) -> Result<(ScopedObjectId, ObjectKey), TestFailure> {
    let session = ctx.primary();
    let mut seen = HashSet::new();
    // Drain whatever the region already streamed, so the object rezzed below is
    // recognisable as the new one rather than as whichever update arrives next.
    while let Ok(object) = session
        .wait_for(SETTLE_IDLE, |event| match event {
            Event::ObjectAdded(object) => Some((**object).clone()),
            _other => None,
        })
        .await
    {
        let _added = seen.insert(object.scoped_id());
    }
    session
        .send(Command::RezObject {
            shape: PrimShape::cube(CONTAINER_POSITION),
            group_id: None,
        })
        .await?;
    let container = session
        .wait_for(LONG_TIMEOUT, |event| match event {
            Event::ObjectAdded(object) if !seen.contains(&object.scoped_id()) => {
                Some((**object).clone())
            }
            _other => None,
        })
        .await?;
    Ok((container.scoped_id(), container.full_id))
}

/// The fresh task-inventory item id the grid minted for the dropped item,
/// matched by name — a task copy is a new item, not the same one in two places.
async fn fetch_task_item_id(
    session: &mut Session,
    target: ScopedObjectId,
    task: ObjectKey,
    name: &str,
) -> Result<InventoryKey, TestFailure> {
    session.send(Command::FetchTaskInventory { target }).await?;
    let items = session
        .wait_for(LONG_TIMEOUT, |event| match event {
            Event::TaskInventoryContents {
                task: got, items, ..
            } if *got == task => Some(items.clone()),
            _other => None,
        })
        .await?;
    items
        .into_iter()
        .find(|entry| entry.name == name)
        .map(|entry| entry.item_id)
        .ok_or_else(|| {
            TestFailure::Assertion(format!("{name:?} is not in the prim's contents listing"))
        })
}

/// One task-item asset fetch over the legacy UDP `TransferRequest`.
async fn fetch_task_item(
    session: &mut Session,
    task: ObjectKey,
    item_id: InventoryKey,
    asset_id: AssetKey,
    asset_type: AssetType,
) -> Result<Vec<u8>, TestFailure> {
    session
        .send(Command::FetchTaskItemAsset {
            task,
            item_id,
            asset_id,
            asset_type,
        })
        .await?;
    let outcome = session
        .wait_for(LONG_TIMEOUT, |event| match event {
            Event::TaskItemAssetReceived { item, data, .. } if *item == item_id => {
                Some(Ok(data.clone()))
            }
            Event::TransferFailed {
                transfer_id,
                status,
            } => Some(Err((*transfer_id, *status))),
            _other => None,
        })
        .await?;
    outcome.map_err(|(transfer_id, status): (TransferId, _)| {
        TestFailure::Assertion(format!(
            "the task item's asset was refused ({status:?}, transfer {transfer_id:?})"
        ))
    })
}

/// The [`RestoreItem`] an `UpdateTaskInventory` drops into a prim. The
/// simulator resolves the item by id from the agent's own inventory rather than
/// trusting this copy, so only the id has to be right; the rest travels so the
/// message is well formed.
fn task_item(item: &InventoryItem) -> RestoreItem {
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

/// One asset fetch over the `ViewerAsset` capability.
///
/// Each fetch gets its **own** cache directory. A shared one would let a second
/// fetch of an id be answered from disk, which is fine — but the id under test
/// changes on every save, and a stale directory shared with an earlier run of
/// the case could answer a *pre-save* id with pre-save bytes and hide exactly
/// the failure this case exists to find.
async fn fetch(cap: &str, key: AssetKey, asset_type: AssetType) -> Result<Vec<u8>, TestFailure> {
    let dir = std::env::temp_dir().join(format!(
        "sl-conformance-round-trip-{}-{}",
        std::process::id(),
        key.uuid().simple()
    ));
    let _removed = fs_err::remove_dir_all(&dir);
    let fetcher = Arc::new(ReqwestAssetFetcher::with_default_client());
    fetcher.set_cap_url(Some(cap.to_owned()));
    let store = AssetStore::new(fetcher, Some(dir.clone()), AssetCacheLimits::default())
        .map_err(|error| TestFailure::Assertion(format!("open asset store: {error}")))?;
    let result = store.get(key, asset_type).await;
    let _removed = fs_err::remove_dir_all(&dir);
    let entry = result.map_err(|error| {
        TestFailure::Assertion(format!("fetching {key:?} as {asset_type:?}: {error}"))
    })?;
    entry
        .data()
        .map(|bytes| bytes.to_vec())
        .ok_or_else(|| TestFailure::Assertion(format!("{key:?} fetched no bytes")))
}
