//! Read the **Current Outfit Folder** (COF) — the modern, server-authoritative
//! record of what the avatar is wearing — and assert its outfit links resolve to
//! real wearables, including the mandatory body parts.
//!
//! This is the mirror of `wearables-request`. Under modern Second Life's
//! server-side baking the legacy `AgentWearablesUpdate` is transitional and may
//! omit body parts (so `wearables-request` is only `partial` on aditi); the
//! authoritative outfit instead lives in the `FT_CURRENT_OUTFIT` system folder,
//! whose contents are inventory **links** back to the worn wearables and
//! attachments. This case locates that folder — via
//! [`Command::QueryInventoryFolders`], the local query over the session's held
//! folder model (seeded from the login skeleton, so every folder's preferred type
//! is known before any contents fetch) — then fetches folder contents and follows
//! each link to its target item, asserting the four mandatory body parts (shape /
//! skin / hair / eyes) are all present, precisely the slots the legacy message
//! drops on modern SL.
//!
//! Finding the COF by its stored preferred type is deliberate: modern SL's
//! `FetchInventoryDescendents2` does not reliably echo a folder's `type_default`
//! in a descendents reply (the authoritative type lives in the login skeleton /
//! AIS3), so a recursive crawl that reads the type off the descendents reply
//! cannot identify the COF there — whereas the skeleton-seeded model always can.
//!
//! To resolve a link, the case fetches every agent-tree folder's contents into an
//! item map: a COF entry is an inventory link whose `asset_id` is the *linked
//! item's* id, so the target is looked up by that id among all fetched items — a
//! genuine dereference, not merely trusting the link's own mirrored metadata. The
//! fetch is driven off the skeleton's folder list rather than a root-down descent,
//! so it does not depend on any one descendents reply enumerating its sub-folders.
//!
//! **Grid divergence.** Expected `complete` on aditi, where the COF is the real
//! outfit. OpenSim keeps the legacy `AgentWearablesUpdate` authoritative and does
//! not necessarily populate the COF with body-part links for a stock avatar, so a
//! missing-or-unpopulated COF there is recorded `partial` (the mirror of
//! `wearables-request`, which is `partial` on aditi), not a failure.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use sl_client_tokio::{
    AssetType, Command, Event, FolderInfo, FolderType, InventoryFolderKey, InventoryItem, Throttle,
    Uuid, WearableType,
};

use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{check, is_opensim};

/// How long to wait for the region to become active.
const REGION_TIMEOUT: Duration = Duration::from_secs(60);

/// How long to wait for the local folder-snapshot query to answer.
const QUERY_TIMEOUT: Duration = Duration::from_secs(30);

/// How long to wait for any one folder's contents to arrive.
const FOLDER_TIMEOUT: Duration = Duration::from_secs(60);

/// Upper bound on the number of folders fetched, so a pathological tree cannot
/// stall the case unboundedly (a stock agent inventory holds a few dozen).
const MAX_FOLDERS: usize = 5_000;

/// The `AT_LINK` asset code for an item link.
const AT_LINK: i8 = 24;

/// The `AT_LINK_FOLDER` asset code for a folder link.
const AT_LINK_FOLDER: i8 = 25;

/// The four mandatory body-part slots every valid avatar wears exactly one of.
const BODY_PARTS: [WearableType; 4] = [
    WearableType::Shape,
    WearableType::Skin,
    WearableType::Hair,
    WearableType::Eyes,
];

/// The wearable slot ([`WearableType`]) encoded in a wearable item's flags: the
/// low byte of `flags` carries the `LLWearableType::EType` subtype for clothing
/// and body-part items.
fn wearable_slot(item: &InventoryItem) -> WearableType {
    let subtype = u8::try_from(item.flags & 0xFF).unwrap_or(0);
    WearableType::from_code(subtype)
}

/// Whether an item is an inventory link (item or folder link).
const fn is_link(item: &InventoryItem) -> bool {
    item.item_type == AT_LINK || item.item_type == AT_LINK_FOLDER
}

/// Reads the Current Outfit Folder and asserts its outfit links resolve,
/// including the mandatory body parts.
#[derive(Debug)]
pub struct CurrentOutfitFolder;

impl GridTest for CurrentOutfitFolder {
    fn name(&self) -> &'static str {
        "current-outfit-folder"
    }

    fn description(&self) -> &'static str {
        "Read the Current Outfit Folder and resolve its outfit links (CAPS or UDP)"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;
            session
                .send(Command::SetThrottle(Throttle::preset_1000()))
                .await?;

            // The whole agent folder structure, straight from the session's held
            // model (login skeleton) — every folder with its authoritative
            // preferred type, so the Current Outfit folder is identifiable without
            // relying on a descendents reply to carry `type_default`.
            session.send(Command::QueryInventoryFolders).await?;
            let folders: Vec<FolderInfo> = session
                .wait_for(QUERY_TIMEOUT, |event| match event {
                    Event::InventoryFolders(folders) => Some(folders.to_vec()),
                    _ => None,
                })
                .await?;

            let folder_summary: Vec<(String, FolderType)> = folders
                .iter()
                .map(|folder| (folder.name.clone(), folder.folder_type))
                .collect();
            tracing::info!(
                folders_total = folders.len(),
                ?folder_summary,
                "agent folder snapshot"
            );
            let folders_total = folders.len();

            // Locate the Current Outfit Folder by its stored preferred type.
            let cof = folders
                .iter()
                .find(|folder| folder.folder_type == FolderType::CurrentOutfit)
                .map(|folder| folder.folder_id);
            let Some(cof) = cof else {
                if is_opensim(grid) {
                    ctx.mark_partial(
                        "OpenSim does not expose a Current Outfit folder (FT_CURRENT_OUTFIT) — \
                         the legacy AgentWearablesUpdate stays authoritative there",
                    );
                    return Ok(());
                }
                return Err(TestFailure::Assertion(
                    "no Current Outfit folder (FT_CURRENT_OUTFIT) in the agent inventory"
                        .to_owned(),
                ));
            };

            // Fetch every agent folder's contents into a by-id item map so a COF
            // link's target can be dereferenced. Driven off the skeleton's folder
            // list (not a root-down descent), so it does not hinge on any one
            // descendents reply enumerating its sub-folders. The COF is fetched
            // among the rest, giving its own link entries.
            let mut items_by_id: HashMap<Uuid, InventoryItem> = HashMap::new();
            let fetch_list: Vec<InventoryFolderKey> = folders
                .iter()
                .take(MAX_FOLDERS)
                .map(|folder| folder.folder_id)
                .collect();
            let start = Instant::now();
            for folder in fetch_list {
                session.send(Command::RequestFolderContents(folder)).await?;
                let folder_items = session
                    .wait_for(FOLDER_TIMEOUT, |event| match event {
                        Event::InventoryDescendents {
                            folder_id, items, ..
                        } if *folder_id == folder => Some(items.clone()),
                        _ => None,
                    })
                    .await?;
                for item in folder_items {
                    items_by_id.insert(item.item_id.uuid(), item);
                }
            }
            let fetch_secs = start.elapsed().as_secs_f64();

            let metrics = ctx.metrics();
            metrics.set(
                "folders_total",
                i64::try_from(folders_total).unwrap_or(i64::MAX),
            );
            metrics.set_timing("fetch_secs", fetch_secs);
            metrics.set(
                "items_total",
                i64::try_from(items_by_id.len()).unwrap_or(i64::MAX),
            );

            // The COF's own entries: every item filed directly under it. Under
            // server-side baking these are inventory links back to the worn items.
            let cof_entries: Vec<&InventoryItem> = items_by_id
                .values()
                .filter(|item| item.folder_id == cof)
                .collect();
            let cof_links: Vec<&InventoryItem> =
                cof_entries.iter().copied().filter(|i| is_link(i)).collect();

            let metrics = ctx.metrics();
            metrics.set(
                "cof_entry_count",
                i64::try_from(cof_entries.len()).unwrap_or(i64::MAX),
            );
            metrics.set(
                "cof_link_count",
                i64::try_from(cof_links.len()).unwrap_or(i64::MAX),
            );

            if cof_links.is_empty() {
                if is_opensim(grid) {
                    ctx.mark_partial(&format!(
                        "OpenSim Current Outfit folder holds no outfit links ({} entr(ies)) — \
                         the legacy AgentWearablesUpdate stays authoritative there",
                        cof_entries.len()
                    ));
                    return Ok(());
                }
                return Err(TestFailure::Assertion(
                    "Current Outfit folder holds no outfit links".to_owned(),
                ));
            }

            // Dereference each link to its target item (the link's asset_id is the
            // linked item's id) and classify which body-part slots the resolved
            // targets cover.
            let mut resolved = 0_usize;
            let mut unresolved = 0_usize;
            let mut present_body_parts: HashSet<u8> = HashSet::new();
            for link in &cof_links {
                match items_by_id.get(&link.asset_id) {
                    Some(target) => {
                        resolved = resolved.saturating_add(1);
                        if AssetType::from_code(i32::from(target.item_type)) == AssetType::Bodypart
                        {
                            present_body_parts.insert(wearable_slot(target).to_code());
                        }
                    }
                    None => unresolved = unresolved.saturating_add(1),
                }
            }

            let missing: Vec<WearableType> = BODY_PARTS
                .into_iter()
                .filter(|part| !present_body_parts.contains(&part.to_code()))
                .collect();

            tracing::info!(
                cof_links = cof_links.len(),
                resolved,
                unresolved,
                body_parts = present_body_parts.len(),
                ?missing,
                "Current Outfit folder resolved"
            );

            let metrics = ctx.metrics();
            metrics.set(
                "resolved_links",
                i64::try_from(resolved).unwrap_or(i64::MAX),
            );
            metrics.set(
                "unresolved_links",
                i64::try_from(unresolved).unwrap_or(i64::MAX),
            );
            metrics.set(
                "body_part_count",
                i64::try_from(present_body_parts.len()).unwrap_or(i64::MAX),
            );

            // Every link must dereference: an outfit link with no target item is a
            // broken outfit, on either grid.
            check(
                unresolved == 0,
                &format!("{unresolved} COF outfit link(s) did not resolve to an inventory item"),
            )?;

            if !missing.is_empty() {
                if is_opensim(grid) {
                    ctx.mark_partial(&format!(
                        "OpenSim Current Outfit folder resolves {resolved} link(s) but not all \
                         body parts as links ({} missing: {missing:?}) — the legacy \
                         AgentWearablesUpdate stays authoritative there",
                        missing.len()
                    ));
                    return Ok(());
                }
                check(
                    false,
                    &format!("Current Outfit folder is missing body-part link(s): {missing:?}"),
                )?;
            }

            Ok(())
        })
    }
}
