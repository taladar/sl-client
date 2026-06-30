//! Drive a folder through its whole lifecycle — create, rename, move, purge,
//! remove — and confirm each step by re-fetching the affected parent from the
//! grid.
//!
//! Where `inventory-tree-crawl` proves the *read* side of inventory (the
//! recursive descent answers), this proves the *write* side: every structural
//! mutation a viewer performs on a folder. The protocol path differs per grid,
//! so the case gates on it — Second Life carries folder mutations over the modern
//! **AIS3** (`InventoryAPIv3`) CAPS REST endpoint, while OpenSim has no AIS3 and
//! uses the legacy UDP messages (`CreateInventoryFolder`,
//! `UpdateInventoryFolder`, `MoveInventoryFolder`, `PurgeInventoryDescendents`,
//! `RemoveInventoryFolder`). The UDP mutations are fire-and-forget (OpenSim sends
//! no reply and the client caches optimistically), so the *only* trustworthy
//! confirmation is to re-fetch the parent over
//! [`Command::RequestFolderContents`] and inspect the server's authoritative
//! [`Event::InventoryDescendents`] reply — which is exactly what this case
//! asserts against, on both grids, rather than trusting the optimistic local
//! cache.
//!
//! The lifecycle: create a destination folder and a subject folder under the
//! agent root; rename the subject; create a child inside it (so purge has
//! something to delete); move the subject under the destination; send the subject
//! to Trash; purge it (emptying it); then remove it. The Trash step is not
//! incidental — both grids only let a folder be purged or deleted once it lives
//! under Trash (the viewer's own delete = move-to-trash-then-empty flow; OpenSim
//! enforces it with an `onlyIfTrash` guard), so the case re-parents into Trash
//! between the move and the purge. Every step polls the relevant parent until the
//! grid reflects it (OpenSim processes some mutations — purge, the descendents
//! reply — on a fire-and-forget worker, so a short poll absorbs the lag). The
//! destination folder is sent to Trash and removed at the end so re-runs start
//! clean.

use std::time::{Duration, Instant};

use sl_client_tokio::{
    Command, Event, FolderType, InventoryFolder, InventoryFolderKey, Throttle, Uuid,
};

use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{is_opensim, send_then_wait};

/// How long to wait for the region to become active.
const REGION_TIMEOUT: Duration = Duration::from_secs(60);

/// How long to wait for the inventory root query to answer.
const ROOTS_TIMEOUT: Duration = Duration::from_secs(30);

/// How long to wait for any one folder's contents to arrive.
const FOLDER_TIMEOUT: Duration = Duration::from_secs(60);

/// How long to wait for an AIS3 mutation's `BulkUpdateInventory` reply.
const CAPS_TIMEOUT: Duration = Duration::from_secs(60);

/// How long a verification poll keeps re-fetching before giving up.
const VERIFY_TIMEOUT: Duration = Duration::from_secs(30);

/// How long to wait between verification re-fetches.
const VERIFY_POLL_INTERVAL: Duration = Duration::from_millis(500);

/// Fetch a folder's immediate sub-folders by issuing a fresh
/// [`Command::RequestFolderContents`] and returning the grid's authoritative
/// reply (rather than the optimistic local cache).
async fn fetch_subfolders(
    session: &mut Session,
    parent: InventoryFolderKey,
) -> Result<Vec<InventoryFolder>, TestFailure> {
    send_then_wait(
        session,
        Command::RequestFolderContents(parent),
        FOLDER_TIMEOUT,
        |event| match event {
            Event::InventoryDescendents {
                folder_id, folders, ..
            } if *folder_id == parent => Some(folders.clone()),
            _ => None,
        },
    )
    .await
}

/// Re-fetch `parent` until its sub-folder list satisfies `predicate`, or fail
/// with `description` once [`VERIFY_TIMEOUT`] elapses. Absorbs the brief lag from
/// OpenSim's fire-and-forget mutation/descendents workers.
async fn poll_subfolders<P>(
    session: &mut Session,
    parent: InventoryFolderKey,
    mut predicate: P,
    description: &str,
) -> Result<(), TestFailure>
where
    P: FnMut(&[InventoryFolder]) -> bool,
{
    let start = Instant::now();
    loop {
        let folders = fetch_subfolders(session, parent).await?;
        if predicate(&folders) {
            return Ok(());
        }
        if start.elapsed() >= VERIFY_TIMEOUT {
            return Err(TestFailure::Assertion(format!(
                "inventory never reached expected state: {description}"
            )));
        }
        tokio::time::sleep(VERIFY_POLL_INTERVAL).await;
    }
}

/// Create a folder named `name` under `parent`, returning its key. OpenSim takes
/// the UDP `CreateInventoryFolder` (the client mints the id); Second Life takes
/// the AIS3 create, where the grid allocates the id and echoes the new folder in
/// a `BulkUpdateInventory`, from which the key is read back by name.
async fn create_folder(
    session: &mut Session,
    grid: Grid,
    parent: InventoryFolderKey,
    name: &str,
) -> Result<InventoryFolderKey, TestFailure> {
    if is_opensim(grid) {
        let folder_id = InventoryFolderKey::from(Uuid::new_v4());
        session
            .send(Command::CreateInventoryFolder {
                folder_id,
                parent_id: parent,
                folder_type: FolderType::None,
                name: name.to_owned(),
            })
            .await?;
        Ok(folder_id)
    } else {
        let wanted = name.to_owned();
        send_then_wait(
            session,
            Command::Ais3CreateFolder {
                parent_id: parent,
                folder_type: i32::from(FolderType::None.to_code()),
                name: name.to_owned(),
            },
            CAPS_TIMEOUT,
            move |event| match event {
                Event::InventoryBulkUpdate { folders, .. } => folders
                    .iter()
                    .find(|folder| folder.name == wanted)
                    .map(|folder| folder.folder_id),
                _ => None,
            },
        )
        .await
    }
}

/// Rename `folder` (parent `parent`) to `name`. UDP `UpdateInventoryFolder` on
/// OpenSim (an all-fields overwrite, so the unchanged parent/type are restated);
/// AIS3 `PATCH /category` on Second Life.
async fn rename_folder(
    session: &Session,
    grid: Grid,
    folder: InventoryFolderKey,
    parent: InventoryFolderKey,
    name: &str,
) -> Result<(), TestFailure> {
    if is_opensim(grid) {
        session
            .send(Command::UpdateInventoryFolder {
                folder_id: folder,
                parent_id: parent,
                folder_type: FolderType::None,
                name: name.to_owned(),
            })
            .await
    } else {
        session
            .send(Command::Ais3RenameFolder {
                folder_id: folder,
                name: name.to_owned(),
            })
            .await
    }
}

/// Re-parent `folder` under `new_parent`. UDP `MoveInventoryFolder` on OpenSim;
/// AIS3 `PATCH /category` with a new `parent_id` on Second Life.
async fn move_folder(
    session: &Session,
    grid: Grid,
    folder: InventoryFolderKey,
    new_parent: InventoryFolderKey,
) -> Result<(), TestFailure> {
    if is_opensim(grid) {
        session
            .send(Command::MoveInventoryFolder {
                folder_id: folder,
                parent_id: new_parent,
            })
            .await
    } else {
        session
            .send(Command::Ais3MoveFolder {
                folder_id: folder,
                parent_id: new_parent,
            })
            .await
    }
}

/// Empty `folder` of its contents. UDP `PurgeInventoryDescendents` on OpenSim;
/// AIS3 `DELETE /category/<id>/children` on Second Life.
async fn purge_folder(
    session: &Session,
    grid: Grid,
    folder: InventoryFolderKey,
) -> Result<(), TestFailure> {
    if is_opensim(grid) {
        session
            .send(Command::PurgeInventoryDescendents(folder))
            .await
    } else {
        session.send(Command::Ais3PurgeFolder(folder)).await
    }
}

/// Delete `folder` itself. UDP `RemoveInventoryFolder` on OpenSim; AIS3
/// `DELETE /category/<id>` on Second Life.
async fn remove_folder(
    session: &Session,
    grid: Grid,
    folder: InventoryFolderKey,
) -> Result<(), TestFailure> {
    if is_opensim(grid) {
        session
            .send(Command::RemoveInventoryFolders(vec![folder]))
            .await
    } else {
        session.send(Command::Ais3RemoveFolder(folder)).await
    }
}

/// Whether `folders` holds a folder with this key and name.
fn has_named(folders: &[InventoryFolder], folder: InventoryFolderKey, name: &str) -> bool {
    folders
        .iter()
        .any(|entry| entry.folder_id == folder && entry.name == name)
}

/// Whether `folders` holds a folder with this key (any name).
fn has_key(folders: &[InventoryFolder], folder: InventoryFolderKey) -> bool {
    folders.iter().any(|entry| entry.folder_id == folder)
}

/// The key of the Trash folder among `folders` (matched by its `FT_TRASH`
/// preferred type), if present. Deleting or purging a folder on OpenSim only
/// takes effect once the folder lives under Trash (the `onlyIfTrash` guard in the
/// inventory service — the same rule a viewer follows: delete sends to Trash,
/// then purge/remove empties it), so the lifecycle re-parents the subject into
/// Trash before those two steps.
fn find_trash(folders: &[InventoryFolder]) -> Option<InventoryFolderKey> {
    let trash = FolderType::Trash.to_code();
    folders
        .iter()
        .find(|entry| entry.folder_type == trash)
        .map(|entry| entry.folder_id)
}

/// Exercises the create → rename → move → purge → remove folder lifecycle,
/// confirming each step against the grid's authoritative folder listing.
#[derive(Debug)]
pub struct Ais3FolderLifecycle;

impl GridTest for Ais3FolderLifecycle {
    fn name(&self) -> &'static str {
        "ais3-folder-lifecycle"
    }

    fn description(&self) -> &'static str {
        "Create, rename, move, purge, and remove a folder (AIS3 CAPS or UDP)"
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

            // The agent inventory root is the parent the subject and destination
            // folders are created under.
            session.send(Command::QueryInventoryRoots).await?;
            let root = session
                .wait_for(ROOTS_TIMEOUT, |event| match event {
                    Event::InventoryRoots { agent_root, .. } => *agent_root,
                    _ => None,
                })
                .await?;

            // The Trash folder: deleting/purging a folder only takes effect once
            // it lives under Trash (see `find_trash`), so the subject is moved
            // there before the purge and remove steps.
            let trash = find_trash(&fetch_subfolders(session, root).await?).ok_or_else(|| {
                TestFailure::Assertion("no Trash folder under the inventory root".to_owned())
            })?;

            // Distinct per-run names so a leftover folder from an aborted run
            // cannot be mistaken for this run's, and so concurrent runs do not
            // collide.
            let tag: String = Uuid::new_v4()
                .simple()
                .to_string()
                .chars()
                .take(8)
                .collect();
            let subject_name = format!("conf-subject-{tag}");
            let renamed_name = format!("conf-renamed-{tag}");
            let dest_name = format!("conf-dest-{tag}");
            let child_name = format!("conf-child-{tag}");

            let lifecycle_start = Instant::now();

            // --- create: the move destination, then the subject under the root.
            let dest = create_folder(session, grid, root, &dest_name).await?;
            poll_subfolders(
                session,
                root,
                |folders| has_named(folders, dest, &dest_name),
                "destination folder did not appear under the root",
            )
            .await?;

            let create_start = Instant::now();
            let subject = create_folder(session, grid, root, &subject_name).await?;
            poll_subfolders(
                session,
                root,
                |folders| has_named(folders, subject, &subject_name),
                "subject folder did not appear under the root",
            )
            .await?;
            let create_secs = create_start.elapsed().as_secs_f64();

            // --- rename: the subject keeps its key but changes name in place.
            let rename_start = Instant::now();
            rename_folder(session, grid, subject, root, &renamed_name).await?;
            poll_subfolders(
                session,
                root,
                |folders| has_named(folders, subject, &renamed_name),
                "subject folder did not take its new name",
            )
            .await?;
            let rename_secs = rename_start.elapsed().as_secs_f64();

            // --- a child inside the subject, so the later purge has something to
            // empty (and the purge can be observed as the child disappearing).
            let child = create_folder(session, grid, subject, &child_name).await?;
            poll_subfolders(
                session,
                subject,
                |folders| has_named(folders, child, &child_name),
                "child folder did not appear under the subject",
            )
            .await?;

            // --- move: the subject leaves the root and lands under the
            // destination. Both edges are asserted — present under the new parent,
            // gone from the old.
            let move_start = Instant::now();
            move_folder(session, grid, subject, dest).await?;
            poll_subfolders(
                session,
                dest,
                |folders| has_key(folders, subject),
                "subject folder did not appear under the destination after the move",
            )
            .await?;
            poll_subfolders(
                session,
                root,
                |folders| !has_key(folders, subject),
                "subject folder still under the root after the move",
            )
            .await?;
            let move_secs = move_start.elapsed().as_secs_f64();

            // --- to-trash: send the subject (still holding its child) to Trash,
            // the standard "delete folder" step a viewer performs before a folder
            // can be emptied or destroyed. Asserted as another re-parent: present
            // under Trash, gone from the destination.
            move_folder(session, grid, subject, trash).await?;
            poll_subfolders(
                session,
                trash,
                |folders| has_key(folders, subject),
                "subject folder did not appear under Trash after the delete-to-trash move",
            )
            .await?;
            poll_subfolders(
                session,
                dest,
                |folders| !has_key(folders, subject),
                "subject folder still under the destination after the delete-to-trash move",
            )
            .await?;

            // --- purge: the subject's contents go (the child disappears) but the
            // subject itself survives, still under Trash.
            let purge_start = Instant::now();
            purge_folder(session, grid, subject).await?;
            poll_subfolders(
                session,
                subject,
                |folders| !has_key(folders, child),
                "child folder survived the purge of the subject",
            )
            .await?;
            poll_subfolders(
                session,
                trash,
                |folders| has_key(folders, subject),
                "subject folder vanished from Trash on purge (purge should empty, not delete)",
            )
            .await?;
            let purge_secs = purge_start.elapsed().as_secs_f64();

            // --- remove: the (now empty) subject folder itself is deleted.
            let remove_start = Instant::now();
            remove_folder(session, grid, subject).await?;
            poll_subfolders(
                session,
                trash,
                |folders| !has_key(folders, subject),
                "subject folder still under Trash after removal",
            )
            .await?;
            let remove_secs = remove_start.elapsed().as_secs_f64();

            let lifecycle_secs = lifecycle_start.elapsed().as_secs_f64();

            // Clean up the now-empty destination so back-to-back runs start from a
            // clean inventory: send it to Trash, then remove it (the same
            // to-trash-then-remove path the subject took).
            move_folder(session, grid, dest, trash).await?;
            poll_subfolders(
                session,
                trash,
                |folders| has_key(folders, dest),
                "destination folder did not reach Trash during cleanup",
            )
            .await?;
            remove_folder(session, grid, dest).await?;
            poll_subfolders(
                session,
                trash,
                |folders| !has_key(folders, dest),
                "destination folder still under Trash after cleanup removal",
            )
            .await?;

            let path = if is_opensim(grid) { "udp" } else { "ais3" };
            let metrics = ctx.metrics();
            metrics.set("path", path);
            metrics.set_timing("create_secs", create_secs);
            metrics.set_timing("rename_secs", rename_secs);
            metrics.set_timing("move_secs", move_secs);
            metrics.set_timing("purge_secs", purge_secs);
            metrics.set_timing("remove_secs", remove_secs);
            metrics.set_timing("lifecycle_secs", lifecycle_secs);

            Ok(())
        })
    }
}
