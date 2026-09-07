//! What a grid actually stores for `AssetType::Object`, and whether a viewer
//! can see it at all.
//!
//! `sl-object-asset` implements the nested-block text a Second Life object
//! asset was captured as in 2005. Nothing offline can say whether a grid still
//! writes that, so this case asks all three, and what it found is the reason
//! the crate's own documentation is careful about the claim:
//!
//! | grid | asset id exposed to a viewer? | body |
//! | --- | --- | --- |
//! | OpenSim | yes — every object item names one | `<SceneObjectGroup>` XML |
//! | Second Life | **no** | unobservable |
//! | `FakeSl`, a take | **no**, Second Life's side | withheld |
//! | `FakeOpensim`, a take | yes | the Linden text form |
//! | either fake, its seeded `Fixture Object` | yes | the Linden text form |
//!
//! The fake grid's row was one row until this case measured the other two, and
//! it is now two grids: the fake grid imitates whichever live one it was asked
//! for (`sl_fake_grid::ImitatedGrid`). **This case declares both**, because it
//! is a survey of exactly what they disagree about — the `take_step` metric
//! below reads `item-created-nil-asset` on the Second Life side, matching
//! aditi, and `item-created-with-asset` on the OpenSim one — so running it on
//! one flavour alone would leave half the switch unexercised. What stays
//! fetchable on both is the grid's own seeded `Fixture Object`, which is what
//! this case samples offline.
//!
//! **Second Life does not tell a viewer where an object item's asset lives.**
//! Eleven of eleven object items in the test account came back with a nil
//! `asset_id` — in the AIS3 folder listing *and* in the per-item
//! `GET /item/<id>` — and all eleven grant their owner copy, modify and
//! transfer. So it is not the familiar "the grid withholds an asset id for
//! content you do not fully own" rule; the class is simply not fetchable, which
//! is consistent with neither reference viewer ever having carried a reader for
//! it. The `objects_full_perm_with_asset_id` metric is that finding in one
//! number.
//!
//! It is deliberately a **survey, not an assertion**: the case classifies what
//! comes back and records it, and a grid answering a shape nobody predicted is
//! a finding rather than a failure. The one thing it fails on is a grid that
//! names an asset it will not then serve — an id resolving to nothing is the
//! bug the whole `asset-round-trip` family exists to catch.
//!
//! Object items come from the agent's own inventory; only the fake grid rezzes
//! and takes one, to exercise that path where the worst case is a discarded
//! process. An earlier version ran the rez on live grids too and left a cube
//! standing in a sandbox when a take went unacknowledged. `1av`.
//!
//! Two things it cannot see, both recorded rather than glossed: the Library
//! root 404s, because library folders are served by `LibraryAPIv3` and this
//! workspace only ever asks `InventoryAPIv3`
//! ([[protocol-ais3-library-cap]]); and on Second Life the AIS3 walk goes one
//! level at a time, because the client parser reads only the top level of a
//! nested `_embedded` ([[protocol-ais3-nested-embedded]]).
use std::collections::HashSet;
use std::sync::Arc;

use sl_client_tokio::{
    AgentKey, AssetCacheLimits, AssetKey, AssetStore, AssetType, Command, DeRezDestination, Event,
    FolderType, InventoryFolderKey, InventoryItem, PrimShape, ReqwestAssetFetcher, ScopedObjectId,
    Throttle, TransactionId, Uuid, Vector, pcode,
};
use sl_object_asset::ObjectAsset;

use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{LONG_TIMEOUT, REGION_TIMEOUT, REPLY_TIMEOUT, check, is_aditi, is_fake};

/// How many object assets to pull. Enough for a shape to be more than one
/// sample, few enough that a live run stays short and polite.
const SAMPLE_LIMIT: usize = 3;

/// How many bytes of an unrecognised body to record, so a shape nobody here
/// predicted can still be identified by eye from the record.
const PREVIEW_BYTES: usize = 48;

/// Surveys what a grid stores for `AssetType::Object`.
#[derive(Debug)]
pub struct ObjectAssetFormat;

impl GridTest for ObjectAssetFormat {
    fn name(&self) -> &'static str {
        "object-asset-format"
    }

    fn description(&self) -> &'static str {
        "What a grid stores for AssetType::Object, and whether we can read it"
    }

    fn grids(&self) -> &'static [Grid] {
        // **Both** fake flavours, which no other case needs. This is a survey of
        // exactly the thing the two disagree about, so it answers differently on
        // each — `take_step` reads `item-created-nil-asset` on the Second Life
        // side and `item-created-with-asset` on the OpenSim one — and running it
        // on only one of them would leave half the switch unexercised.
        &[Grid::FakeSl, Grid::FakeOpensim, Grid::Opensim, Grid::Aditi]
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

            let grid = ctx.grid();
            // Taken from the *arrival burst*, before anything slow runs: the
            // region streams its scene once, and a case that goes looking for a
            // prim after a few minutes of inventory traffic finds the stream
            // long finished. (The first run of the take leg recorded exactly
            // that, as `take_step = no-reference-prim`.)
            let (placement, seen) = settle_scene(ctx.primary()).await;
            let survey = object_items(ctx.primary(), grid).await?;
            let mut survey = survey;
            // **The fake grid only.** This leg once ran on live grids too, to
            // ask whether an object the avatar took *itself* would carry an
            // asset id where other people's content did not. The cross-tab
            // below answered that without rezzing anything: on Second Life
            // eleven of eleven object items are full-perm to their owner and
            // none of them names an asset, so the withholding is not about
            // permissions and a self-taken object would tell us nothing new.
            // What the leg did do is leave a cube standing in a live region
            // when a take went unacknowledged. It stays here because the
            // fake grid exercises the rez/take/fetch path offline, where the
            // worst case is a discarded process.
            if is_fake(grid) {
                // Second Life withholds an item's asset id unless the asker
                // owns it outright, and every object in a real account's tree
                // is somebody else's content. An object *this* avatar rezzes
                // and takes is its own creation, full-perm to it, so it is the
                // one object whose asset id the grid has no reason to hide. On
                // Second Life that one came back nil too, so the class is
                // simply not fetchable by a viewer — and a fake grid imitating
                // Second Life now says the same, which is what `take_step`
                // records here (and the OpenSim-flavoured run records the
                // opposite, which is why this case declares both).
                let objects_folder = survey.objects_without_asset.first().map_or_else(
                    || InventoryFolderKey::from(Uuid::nil()),
                    |item| item.folder_id,
                );
                let (step, taken) =
                    take_an_object(ctx, objects_folder, survey.trash, placement.clone(), &seen)
                        .await?;
                ctx.metrics().set("take_step", step);
                if let Some(taken) = taken.filter(|item| !item.asset_id.is_nil()) {
                    survey.objects.push(taken);
                }
            }
            if survey.objects.is_empty() && !survey.objects_without_asset.is_empty() {
                // Every object item this account holds hides its asset id. The
                // per-item AIS3 fetch is the other place an id could come
                // from, so ask before concluding the grid never tells.
                let (step, revealed) = reveal_asset_id(ctx.primary(), &survey).await?;
                ctx.metrics().set("item_fetch_step", step);
                if let Some(revealed) = revealed {
                    survey.objects.push(revealed);
                }
            }
            let objects = survey.objects.clone();
            if objects.is_empty() {
                // Which of the two zeroes this is matters: an inventory that
                // answered with items but no objects is a fact about the
                // account, and one that answered with nothing is a fact about
                // the fetch path.
                let reason = if survey.items_seen == 0 {
                    "the agent's inventory answered no items at all"
                } else {
                    "the agent's inventory holds no AssetType::Object item"
                };
                ctx.mark_partial(reason);
                let metrics = ctx.metrics();
                metrics.set("objects_seen", 0_i64);
                metrics.set("items_seen", survey.items_seen);
                metrics.set("folders_walked", survey.folders_walked);
                metrics.set("item_classes_seen", survey.classes_seen());
                survey.tally.record(metrics);
                metrics.set(
                    "objects_without_asset_id",
                    i64::try_from(survey.objects_without_asset.len()).unwrap_or(-1),
                );
                return Ok(());
            }

            let mut shapes: Vec<String> = Vec::new();
            let mut refusals: Vec<String> = Vec::new();
            let mut fetched = 0_i64;
            let mut first_reading: Option<Reading> = None;
            for item in objects.iter().take(SAMPLE_LIMIT) {
                match fetch(&cap, AssetKey::from(item.asset_id)).await {
                    Ok(body) => {
                        fetched = fetched.saturating_add(1);
                        let reading = classify(&body);
                        shapes.push(reading.shape.to_owned());
                        if first_reading.is_none() {
                            first_reading = Some(reading);
                        }
                    }
                    Err(reason) => refusals.push(reason),
                }
            }

            let metrics = ctx.metrics();
            metrics.set("objects_seen", i64::try_from(objects.len()).unwrap_or(-1));
            metrics.set("items_seen", survey.items_seen);
            metrics.set("folders_walked", survey.folders_walked);
            metrics.set(
                "objects_without_asset_id",
                i64::try_from(survey.objects_without_asset.len()).unwrap_or(-1),
            );
            metrics.set("objects_fetched", fetched);
            metrics.set("shapes", shapes.join(","));
            survey.tally.record(metrics);
            if let Some(reading) = &first_reading {
                metrics.set("bytes", i64::try_from(reading.bytes).unwrap_or(-1));
                metrics.set("preview", reading.preview.clone());
                metrics.set("prims", reading.prims);
                metrics.set("unknown_keywords", reading.unknown_keywords.join(","));
            }
            if !refusals.is_empty() {
                metrics.set("refusals", refusals.join(" | "));
            }

            // The one real assertion: an object item the grid handed out must
            // resolve to *something*. Which shape it is, is the survey.
            check(
                fetched > 0,
                &format!(
                    "the grid serves no asset for any of its own object items: {}",
                    refusals.join(" | ")
                ),
            )
        })
    }
}

/// What one fetched object asset turned out to be.
#[derive(Debug, Clone)]
struct Reading {
    /// The classified container shape.
    shape: &'static str,
    /// How many bytes came back.
    bytes: usize,
    /// The head of the body, for a shape nobody here predicted.
    preview: String,
    /// How many prims the body holds, or `-1` when it is not a form this
    /// workspace can count prims in.
    prims: i64,
    /// The keywords `sl-object-asset` did not recognise — empty unless the
    /// grid served the Linden text form.
    unknown_keywords: Vec<String>,
}

/// Classifies an object asset body by its container, and — where it is the
/// Linden text form — by what this workspace's decoder makes of it.
fn classify(body: &[u8]) -> Reading {
    let preview: String = String::from_utf8_lossy(body.get(..PREVIEW_BYTES).unwrap_or(body))
        .chars()
        .map(|character| {
            if character.is_control() {
                '.'
            } else {
                character
            }
        })
        .collect();
    let head = body.get(..PREVIEW_BYTES).unwrap_or(body);
    let shape = if body.is_empty() {
        "empty"
    } else if head.starts_with(b"{'task_id':u") {
        "linden-text"
    } else if head.starts_with(b"<?llsd/binary?>") || head.starts_with(b"<? LLSD/Binary ?>") {
        "llsd-binary"
    } else if head.starts_with(b"<SceneObjectGroup") {
        // Measured on the local grid 2026-09-06: OpenSim's
        // `SceneObjectSerializer.ToOriginalXmlFormat`, which carries no `<?xml`
        // declaration, so it has to be recognised by its root element.
        "opensim-xml"
    } else if head.starts_with(b"<CoalescedObject") {
        "opensim-xml-coalesced"
    } else if head.starts_with(b"<?xml") || head.starts_with(b"<llsd>") {
        "xml"
    } else if head.starts_with(b"<") {
        "markup"
    } else {
        "unrecognised"
    };
    let decoded = ObjectAsset::decode(body).ok();
    Reading {
        shape,
        bytes: body.len(),
        preview,
        prims: decoded
            .as_ref()
            .and_then(|asset| i64::try_from(asset.prims.len()).ok())
            .unwrap_or(-1),
        unknown_keywords: decoded
            .as_ref()
            .map(|asset| {
                let mut seen: Vec<String> = asset
                    .prims
                    .iter()
                    .flat_map(|prim| prim.unknown.iter().map(|field| field.keyword.clone()))
                    .collect();
                seen.sort();
                seen.dedup();
                seen
            })
            .unwrap_or_default(),
    }
}

/// What the inventory walk found, so a zero result says *which* zero it is.
#[derive(Debug, Default)]
struct Survey {
    /// The object items to sample.
    objects: Vec<InventoryItem>,
    /// How many items of any class the walk saw.
    items_seen: i64,
    /// How many folders answered.
    folders_walked: i64,
    /// The distinct `LLAssetType` codes seen, so "no objects" can be told from
    /// "no inventory".
    classes: std::collections::BTreeSet<i32>,
    /// This avatar, so an item it created itself can be told from content it
    /// merely holds.
    agent: Option<AgentKey>,
    /// The agent's Trash folder, noted while listing the root — where anything
    /// this case rezzed and could not take is sent, so no run leaves an object
    /// standing in a live region.
    trash: Option<InventoryFolderKey>,
    /// Object-class items whose `asset_id` came back **nil**, and so cannot be
    /// fetched at all. Second Life withholds the asset id of an item the viewer
    /// has no business fetching; counting them is the difference between "this
    /// account owns no objects" and "this grid does not tell a viewer where its
    /// objects' assets are".
    objects_without_asset: Vec<InventoryItem>,
    /// Every object item seen, tallied against the two things that would
    /// explain a withheld asset id: who created it, and whether the owner may
    /// copy *and* transfer *and* modify it.
    tally: Tally,
}

impl Tally {
    /// Writes the cross-tab into the record.
    fn record(self, metrics: &mut crate::metrics::Metrics) {
        metrics.set("objects_total", self.objects);
        metrics.set("objects_with_asset_id", self.with_asset_id);
        metrics.set("objects_self_created", self.self_created);
        metrics.set(
            "objects_self_created_with_asset_id",
            self.self_created_with_asset_id,
        );
        metrics.set("objects_full_perm", self.full_perm);
        metrics.set(
            "objects_full_perm_with_asset_id",
            self.full_perm_with_asset_id,
        );
    }
}

/// The owner rights an item has to grant before Second Life could be expected
/// to name its asset: copy, modify and transfer together.
const FULL_PERM: sl_client_tokio::Permissions = sl_client_tokio::Permissions::ITEM_UNRESTRICTED;

/// The cross-tab that decides whether a nil asset id is about permissions or
/// about the class.
///
/// If Second Life withholds the id only for content the avatar does not fully
/// own, then an item this avatar created itself — or holds full-perm — should
/// carry one. If even those come back nil, the withholding is about the class,
/// and no viewer can ever read an object asset on that grid.
#[derive(Debug, Default, Clone, Copy)]
struct Tally {
    /// Object items seen.
    objects: i64,
    /// … of which carry a non-nil asset id.
    with_asset_id: i64,
    /// … of which name this avatar as creator.
    self_created: i64,
    /// … of which are self-created *and* carry an asset id.
    self_created_with_asset_id: i64,
    /// … of which grant the owner copy, modify and transfer.
    full_perm: i64,
    /// … of which are full-perm *and* carry an asset id.
    full_perm_with_asset_id: i64,
}

impl Survey {
    /// The classes seen, as the comma-joined codes a record can carry.
    fn classes_seen(&self) -> String {
        self.classes
            .iter()
            .map(i32::to_string)
            .collect::<Vec<String>>()
            .join(",")
    }

    /// Folds one folder's items in, keeping the object ones.
    fn absorb(&mut self, items: Vec<InventoryItem>) {
        self.folders_walked = self.folders_walked.saturating_add(1);
        for item in items {
            self.items_seen = self.items_seen.saturating_add(1);
            let _new = self.classes.insert(i32::from(item.item_type));
            if i32::from(item.item_type) == AssetType::Object.to_code() {
                let has_asset = !item.asset_id.is_nil();
                let mine = self.agent.is_some_and(|agent| item.creator_id == agent);
                let full_perm = item.permissions.owner.contains(FULL_PERM);
                self.tally.objects = self.tally.objects.saturating_add(1);
                self.tally.with_asset_id = self
                    .tally
                    .with_asset_id
                    .saturating_add(i64::from(has_asset));
                self.tally.self_created = self.tally.self_created.saturating_add(i64::from(mine));
                self.tally.self_created_with_asset_id = self
                    .tally
                    .self_created_with_asset_id
                    .saturating_add(i64::from(mine && has_asset));
                self.tally.full_perm = self.tally.full_perm.saturating_add(i64::from(full_perm));
                self.tally.full_perm_with_asset_id = self
                    .tally
                    .full_perm_with_asset_id
                    .saturating_add(i64::from(full_perm && has_asset));
                if item.asset_id.is_nil() {
                    if self.objects_without_asset.len() < SAMPLE_LIMIT {
                        self.objects_without_asset.push(item);
                    }
                } else if self.objects.len() < SAMPLE_LIMIT {
                    self.objects.push(item);
                }
            }
        }
    }
}

/// The agent's `AssetType::Object` inventory items.
///
/// **Two fetch paths, because the grids no longer share one.** Second Life
/// retired the UDP `FetchInventoryDescendents` path: a `RequestFolderContents`
/// there answers nothing at all, which is exactly how the first aditi run of
/// this case recorded zero items and read as "the account has no objects".
/// Inventory on SL is AIS3 — one HTTP `GET /category/<id>/children?depth=`
/// that returns the tree — so that is what is used there, and the UDP walk
/// stays for OpenSim and the fake grid, which serve it.
///
/// The whole tree is walked rather than the "Objects" system folder: an
/// attachment lives wherever its owner filed it, so a folder-scoped search
/// would find nothing on plenty of real accounts.
async fn object_items(session: &mut Session, grid: Grid) -> Result<Survey, TestFailure> {
    session.send(Command::QueryInventoryRoots).await?;
    let (root, library) = session
        .wait_for(LONG_TIMEOUT, |event| match event {
            Event::InventoryRoots {
                agent_root,
                library_root,
            } => Some((*agent_root, *library_root)),
            _other => None,
        })
        .await?;
    let root =
        root.ok_or_else(|| TestFailure::Assertion("the grid reported no agent root".to_owned()))?;

    if is_aditi(grid) {
        // The Library too: it is full-permission content by construction, so if
        // even a library object hides its asset id, the withholding cannot be
        // about what this avatar is allowed to do with the item.
        return ais3_walk(session, root, library).await;
    }

    let mut survey = Survey {
        agent: session.agent_id(),
        ..Survey::default()
    };
    let mut queue = vec![root];
    while let Some(folder) = queue.pop() {
        let (folders, items) = read_folder(session, folder, &mut queue).await?;
        if survey.trash.is_none() {
            survey.trash = folders
                .iter()
                .find(|folder| folder.folder_type == FolderType::Trash.to_code())
                .map(|folder| folder.folder_id);
        }
        survey.absorb(items);
        if survey.folders_walked >= AIS_FOLDER_BUDGET {
            break;
        }
    }
    Ok(survey)
}

/// The AIS3 walk: one `depth=1` fetch per folder, breadth-first.
///
/// **Not one deep fetch, deliberately.** The AIS service nests `_embedded`
/// recursively, one level per depth, and this workspace's parser
/// (`ais_inventory_update_from_llsd`) reads only the *top* level of it — a
/// divergence its own serializer documents, since the fake grid flattens the
/// subtree to compensate. Against the real service a `depth=50` fetch therefore
/// yields the root's subfolders and no items at all, which is precisely what
/// the second aditi run of this case recorded. Asking one level at a time keeps
/// every answer inside what the parser reads. See
/// [[protocol-ais3-nested-embedded]].
async fn ais3_walk(
    session: &mut Session,
    root: InventoryFolderKey,
    library: Option<InventoryFolderKey>,
) -> Result<Survey, TestFailure> {
    let mut survey = Survey {
        agent: session.agent_id(),
        ..Survey::default()
    };
    let mut queue = vec![root];
    queue.extend(library);
    while let Some(folder) = queue.pop() {
        session
            .send(Command::Ais3FetchFolderChildren {
                folder_id: folder,
                depth: AIS_FETCH_DEPTH,
            })
            .await?;
        // One fetch can answer in several bulk updates; take them until they
        // stop rather than assuming a count.
        while let Ok((folders, items)) = session
            .wait_for(AIS_IDLE, |event| match event {
                Event::InventoryBulkUpdate { folders, items, .. } => {
                    Some((folders.clone(), items.clone()))
                }
                _other => None,
            })
            .await
        {
            // Objects last, so the stack pops them first. A real account's tree
            // is far larger than the folder budget, and a blind walk samples
            // whichever 40 folders the queue happened to reach — two runs of
            // this case saw two disjoint populations (38 objects, then 722
            // links) for exactly that reason. The system "Objects" folder is
            // where the class this case is about actually lives.
            if survey.trash.is_none() {
                survey.trash = folders
                    .iter()
                    .find(|folder| folder.folder_type == FolderType::Trash.to_code())
                    .map(|folder| folder.folder_id);
            }
            let (objects, others): (Vec<_>, Vec<_>) = folders
                .iter()
                .partition(|folder| folder.folder_type == FolderType::Object.to_code());
            queue.extend(others.iter().map(|folder| folder.folder_id));
            queue.extend(objects.iter().map(|folder| folder.folder_id));
            survey.absorb(items);
        }
        // Not stopping at the first few: the question is whether *any* object
        // item on this account carries an asset id, and the answer is only
        // meaningful over the whole folder.
        if survey.folders_walked >= AIS_FOLDER_BUDGET || queue.is_empty() {
            break;
        }
    }
    Ok(survey)
}

/// How deep each AIS3 fetch recurses: one level, for the reason
/// [`ais3_walk`] gives.
const AIS_FETCH_DEPTH: i32 = 1;

/// How many folders the AIS3 walk will open before giving up. A real account's
/// tree is large and each level is an HTTP round trip; a survey wants a sample,
/// not the whole inventory.
const AIS_FOLDER_BUDGET: i64 = 12;

/// How long to wait for another AIS3 bulk update before calling the tree
/// delivered.
const AIS_IDLE: std::time::Duration = std::time::Duration::from_secs(3);

/// Rezzes a cube and takes it, returning the inventory item the grid minted.
///
/// The take is the one object this avatar fully owns, so it is the strongest
/// test of whether the grid will name an object's asset at all. Returns `None`
/// when the region gives nothing to place against or refuses the rez — a
/// parcel that denies rezzing is a fact about the parcel, not a failure of this
/// case.
async fn take_an_object(
    ctx: &mut TestContext,
    folder: InventoryFolderKey,
    trash: Option<InventoryFolderKey>,
    placement: Option<Vector>,
    seen: &HashSet<ScopedObjectId>,
) -> Result<(&'static str, Option<InventoryItem>), TestFailure> {
    // Something already in the region is the placement reference: a rez is
    // aimed by a ray, so it needs a surface, and the avatar's own position is
    // not one.
    let Some(reference) = placement else {
        return Ok(("no-reference-prim", None));
    };
    let session = ctx.primary();
    // Just above the avatar, so the cube lands on the parcel the avatar is
    // standing on and is easy for a human to see and clear if anything here
    // fails to clean up after itself.
    let position = Vector {
        x: reference.x,
        y: reference.y,
        z: reference.z + REZ_LIFT_M,
    };
    session
        .send(Command::RezObject {
            shape: PrimShape::cube(position),
            group_id: None,
        })
        .await?;
    let Ok(rezzed) = session
        .wait_for(LONG_TIMEOUT, |event| match event {
            Event::ObjectAdded(object)
                if object.pcode == pcode::PRIMITIVE && !seen.contains(&object.scoped_id()) =>
            {
                Some(object.scoped_id())
            }
            _other => None,
        })
        .await
    else {
        return Ok(("rez-not-echoed", None));
    };
    session
        .send(Command::DerezObjects {
            local_ids: vec![rezzed],
            destination: DeRezDestination::TakeIntoAgentInventory(folder),
            transaction_id: TransactionId::from(Uuid::new_v4()),
            group_id: None,
        })
        .await?;
    // **Two shapes of acknowledgement.** OpenSim answers a take with the legacy
    // `UpdateCreateInventoryItem` (`Event::InventoryItemCreated`); Second Life
    // moved inventory to AIS3, so the new item arrives as a bulk update
    // instead. Waiting only for the legacy one is how this leg recorded
    // `take-not-acknowledged` against a take that had very likely worked.
    let created = session
        .wait_for(LONG_TIMEOUT, |event| match event {
            Event::InventoryItemCreated { item, .. } => Some(item.clone()),
            Event::InventoryBulkUpdate { items, .. } => items
                .iter()
                .find(|item| i32::from(item.item_type) == AssetType::Object.to_code())
                .cloned(),
            _other => None,
        })
        .await
        .ok();
    // A take that was not acknowledged has left the cube standing in somebody
    // else's region. Nothing else will clear it — an abandoned object counts
    // against the parcel's prims until an estate manager returns it — so the
    // case takes its own litter back out of the world before reporting.
    let step = match &created {
        None => {
            if sweep_up(session, rezzed, trash).await? {
                "take-not-acknowledged-swept"
            } else {
                // The one outcome a human has to hear about: an object this
                // case put in somebody else's region and could not remove.
                "take-not-acknowledged-LITTER-LEFT"
            }
        }
        Some(item) if item.asset_id.is_nil() => "item-created-nil-asset",
        Some(_taken) => "item-created-with-asset",
    };
    Ok((step, created))
}

/// Removes an object this case rezzed and could not take, so no run leaves
/// something standing in a live region.
///
/// Deleting to trash rather than taking, because the point is that the object
/// stops existing in-world; the trash copy is inventory residue this case
/// already accepts (and the same residue `object-rez-derez` records).
async fn sweep_up(
    session: &mut Session,
    rezzed: ScopedObjectId,
    trash: Option<InventoryFolderKey>,
) -> Result<bool, TestFailure> {
    // Derez to Trash rather than `ObjectDelete`: the force-delete is a no-op on
    // stock OpenSim (`object-rez-derez` records that), so the derez is the
    // portable one. The trash copy is inventory residue, which is the lesser
    // mess of the two.
    match trash {
        Some(folder) => {
            session
                .send(Command::DerezObjects {
                    local_ids: vec![rezzed],
                    destination: DeRezDestination::Trash(folder),
                    transaction_id: TransactionId::from(Uuid::new_v4()),
                    group_id: None,
                })
                .await?;
        }
        None => {
            session
                .send(Command::DeleteObjects {
                    local_ids: vec![rezzed],
                })
                .await?;
        }
    }
    // Whether the object really went is the whole point: an unconfirmed sweep
    // is litter somebody else has to clear, so the answer is returned rather
    // than assumed.
    Ok(session
        .wait_for(REPLY_TIMEOUT, |event| match event {
            Event::ObjectRemoved { local_id, .. } if *local_id == rezzed => Some(*local_id),
            _other => None,
        })
        .await
        .is_ok())
}

/// Drains the region's arrival burst, returning the position of a primitive to
/// rez against and **every object id sighted**.
///
/// The id set is what makes the rez identifiable afterwards: a live region
/// streams other people's objects the whole time, so "the next `ObjectAdded`"
/// is not the object this case rezzed. Taking whatever arrives next is how the
/// take leg came to derez something it did not own, which the grid then
/// ignored (`take_step = take-not-acknowledged`).
async fn settle_scene(session: &mut Session) -> (Option<Vector>, HashSet<ScopedObjectId>) {
    let agent = session.agent_id();
    let mut seen = HashSet::new();
    let mut own_avatar = None;
    let mut any_prim = None;
    // Two budgets, not one. A quiet region goes idle and the drain ends on the
    // gap; a busy sandbox streams somebody's rezzing the whole time and never
    // does, so the overall window is what actually ends it there.
    let started = std::time::Instant::now();
    while let Ok(object) = session
        .wait_for(
            SETTLE_IDLE.min(SETTLE_WINDOW.saturating_sub(started.elapsed())),
            |event| match event {
                Event::ObjectAdded(object) => Some((**object).clone()),
                _other => None,
            },
        )
        .await
    {
        if agent.is_some_and(|agent| object.full_id.uuid() == agent.uuid()) {
            own_avatar = Some(object.motion.position.clone());
        } else if any_prim.is_none() && object.pcode == pcode::PRIMITIVE {
            any_prim = Some(object.motion.position.clone());
        }
        let _sighted = seen.insert(object.scoped_id());
    }
    // **The avatar's own position first.** A region is not uniformly rezzable —
    // a sandbox is usually one parcel of several — so the only spot this case
    // can reason about is the one the avatar is standing on. Rezzing at some
    // other prim's position aims at an arbitrary parcel, which is at best a
    // refused rez and at worst an object left somewhere it is not wanted. The
    // stray prim is kept only as a fallback for a region that never streamed
    // the agent its own avatar.
    (own_avatar.or(any_prim), seen)
}

/// How far above the avatar the test cube is rezzed: clear of the avatar
/// itself, still plainly next to it.
const REZ_LIFT_M: f32 = 2.0;

/// How long to wait for another object update before calling the region's
/// arrival burst finished.
const SETTLE_IDLE: std::time::Duration = std::time::Duration::from_secs(5);

/// The overall budget for that drain, however busy the region is.
const SETTLE_WINDOW: std::time::Duration = std::time::Duration::from_secs(15);

/// Asks AIS3 for one object item directly, in case the per-item fetch carries
/// an `asset_id` the folder listing withheld.
///
/// A nil asset id in a listing is not necessarily "there is no asset": it can
/// be a grid declining to say. `GET /item/<id>` is the other place to ask, and
/// the answer — either way — is the finding.
async fn reveal_asset_id(
    session: &mut Session,
    survey: &Survey,
) -> Result<(&'static str, Option<InventoryItem>), TestFailure> {
    let Some(candidate) = survey.objects_without_asset.first() else {
        return Ok(("no-candidate", None));
    };
    let wanted = candidate.item_id;
    session.send(Command::Ais3FetchItem(wanted)).await?;
    let answered = session
        .wait_for(AIS_IDLE, |event| match event {
            Event::InventoryBulkUpdate { items, .. } => {
                items.iter().find(|item| item.item_id == wanted).cloned()
            }
            _other => None,
        })
        .await
        .ok();
    Ok(match answered {
        None => ("item-fetch-unanswered", None),
        Some(item) if item.asset_id.is_nil() => ("item-fetch-nil-asset", None),
        Some(item) => ("item-fetch-carries-asset", Some(item)),
    })
}

/// One folder's items, pushing its subfolders onto `queue`.
async fn read_folder(
    session: &mut Session,
    folder: InventoryFolderKey,
    queue: &mut Vec<InventoryFolderKey>,
) -> Result<(Vec<sl_client_tokio::InventoryFolder>, Vec<InventoryItem>), TestFailure> {
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
    Ok((folders, items))
}

/// Fetches one asset over the `ViewerAsset` capability, returning the grid's
/// refusal as text rather than as a failure — a refusal is data here.
async fn fetch(cap: &str, key: AssetKey) -> Result<Vec<u8>, String> {
    let dir = std::env::temp_dir().join(format!(
        "sl-conformance-object-asset-{}-{}",
        std::process::id(),
        key.uuid().simple()
    ));
    let _removed = fs_err::remove_dir_all(&dir);
    let fetcher = Arc::new(ReqwestAssetFetcher::with_default_client());
    fetcher.set_cap_url(Some(cap.to_owned()));
    let store = AssetStore::new(fetcher, Some(dir.clone()), AssetCacheLimits::default())
        .map_err(|error| format!("open asset store: {error}"))?;
    let result = store.get(key, AssetType::Object).await;
    let _removed = fs_err::remove_dir_all(&dir);
    let entry = result.map_err(|error| format!("{key:?}: {error}"))?;
    entry
        .data()
        .map(|bytes| bytes.to_vec())
        .ok_or_else(|| format!("{key:?}: fetched no bytes"))
}

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;

    use super::classify;

    /// The classifier has to tell the two grids' forms apart from the head of
    /// the body alone, since that is all a record can carry.
    #[test]
    fn the_two_known_forms_are_told_apart() {
        let linden = b"{'task_id':u1fd77b79-a8e7-25a5-9454-02a4d948ba1c}\n{\n\tname\tObject|\n}\n";
        assert_eq!(classify(linden).shape, "linden-text");
        let xml = br#"<?xml version="1.0" encoding="utf-8"?><llsd>"#;
        assert_eq!(classify(xml).shape, "xml");
        // What OpenSim really serves, measured on the local grid: no `<?xml`
        // declaration, straight into the root element.
        let opensim = b"<SceneObjectGroup><RootPart><SceneObjectPart xmlns:xsi=";
        assert_eq!(classify(opensim).shape, "opensim-xml");
        assert_eq!(classify(b"").shape, "empty");
        assert_eq!(classify(b"\x00\x01\x02").shape, "unrecognised");
    }

    /// A body in the text form is counted and its unknown keywords listed —
    /// which is the measurement this case exists for.
    #[test]
    fn the_text_form_reports_its_prims_and_unknown_keywords() {
        let body = concat!(
            "{'task_id':u00000000-0000-0000-0000-000000000001}\n",
            "{\n",
            "\tname\tObject|\n",
            "\tsome_field_from_2026\t7\n",
            "}\n",
        );
        let reading = classify(body.as_bytes());
        assert_eq!(reading.prims, 1);
        assert_eq!(
            reading.unknown_keywords,
            vec!["some_field_from_2026".to_owned()]
        );
    }
}
