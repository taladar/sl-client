//! Edit an object's administrative and geometric facts across the whole build
//! surface, each change confirmed by the reply that carries it back.
//!
//! "Set name / desc / flags / shape / material / permissions / for-sale" needs a
//! single owned prim the client is free to mutate every way the build tools can.
//! Rather than depend on a pre-existing editable object, this case manufactures
//! one throwaway cube (as [`super::object_rez_derez`] does) and drives every edit
//! against it, split by the channel that confirms each change:
//!
//! * The **administrative** edits land in the object's extended properties, so
//!   they are confirmed by a fresh [`Event::ObjectProperties`] read at the end:
//!   * rename with [`Command::SetObjectName`],
//!   * re-describe with [`Command::SetObjectDescription`],
//!   * toggle the next-owner *copy* bit with [`Command::SetObjectPermissions`],
//!   * put it up for sale (a copy, priced) with [`Command::SetObjectForSale`].
//! * The **geometric / physical** edits re-broadcast the object on the region's
//!   interest-list stream, so each is confirmed by an [`Event::ObjectUpdated`]
//!   carrying the new value:
//!   * set the material to metal with [`Command::SetObjectMaterial`]
//!     ([`Object::material`]),
//!   * make it phantom with [`Command::SetObjectFlags`] (the `FLAGS_PHANTOM` bit
//!     of [`Object::update_flags`]),
//!   * hollow the box with [`Command::SetObjectShape`]
//!     ([`Object::shape`]`.profile_hollow`).
//!
//! The flow is: settle the initial scene (so the rezzed cube is recognised as
//! new and a reference primitive supplies a rez position), rez the cube, read its
//! **baseline** properties (a full-perm, not-for-sale prim named "Primitive"),
//! apply every edit, read its properties again and assert the administrative
//! facts changed as sent, then derez the cube to the Trash so the scene is left
//! as found.
//!
//! `1av`, `[both]`. On OpenSim the avatar is forced into the "Default Region",
//! which holds this workspace's rezzed test object as the placement reference, so
//! a primitive is guaranteed and its absence fails the case. On Second Life the
//! landing region's contents are uncontrolled; a region that streams no primitive
//! to place against within the window is recorded `partial` rather than failed.
//! The Trash cleanup leaves one item per run — inventory residue bounded and
//! acceptable on a throwaway grid. The aditi run is deferred with the rest of the
//! Aditi batch (no aditi record this session).

use std::collections::HashSet;
use std::time::{Duration, Instant};

use sl_client_tokio::{
    Command, DeRezDestination, Event, FolderType, InventoryFolder, InventoryFolderKey,
    LindenAmount, Material, Object, ObjectFlagSettings, ObjectProperties, PermissionField,
    Permissions, PrimShape, PrimShapeParams, SaleType, ScopedObjectId, TransactionId, Uuid, Vector,
    pcode,
};

use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, REPLY_TIMEOUT, check, check_eq, is_opensim, secs_metric};

/// The OpenSim start location: the "Default Region" (1000,1000), centred, where
/// this workspace's test object lives and serves as the rez placement reference.
/// On Second Life the avatar keeps `"last"` (a named OpenSim region is
/// meaningless there), and whatever region it lands in supplies the reference
/// primitive.
const OPENSIM_START: &str = "uri:Default Region&128&128&30";

/// The overall budget for settling the initial scene: collecting the region's
/// pre-existing object ids (so the freshly rezzed cube is recognised as new) and
/// a reference primitive to place it against.
const SETTLE_WINDOW: Duration = Duration::from_secs(15);

/// The idle gap that ends the settle: once no new [`Event::ObjectAdded`] has
/// arrived for this long the initial scene is considered fully streamed
/// (generous enough to span the `ObjectUpdateCached` cache-miss round trip a
/// digest triggers).
const SETTLE_IDLE: Duration = Duration::from_secs(5);

/// How long to wait for each step's confirming event: the rezzed cube appearing,
/// a re-broadcast carrying an edited geometric field, or the cube being removed.
/// Kept generous for Aditi network jitter.
const STEP_TIMEOUT: Duration = Duration::from_secs(30);

/// How far above the reference primitive to rez the cube, in metres — clear of
/// the reference so nothing coincides, well inside the same parcel so the rez
/// permission check passes.
const REZ_LIFT_M: f32 = 1.0;

/// The new name the rename applies (a recognisable, non-default label).
const NEW_NAME: &str = "SLClientEditTest";

/// The new description the re-describe applies.
const NEW_DESCRIPTION: &str = "edited by the object-edit conformance case";

/// The asking price, in L$, the for-sale edit sets.
const SALE_PRICE: u64 = 25;

/// The phantom flag (`FLAGS_PHANTOM`, `1 << 10`) as it appears in an
/// object-update's `UpdateFlags`. From the viewer's `object_flags.h`; OpenSim
/// carries the object's live physics/phantom flags through the update's flags,
/// so a set bit confirms the flag edit took.
const FLAGS_PHANTOM: u32 = 1 << 10;

/// The quantized `profile_hollow` the shape edit applies: a 25%-hollow box
/// (`0.25 / 0.00002`). The default cube has `profile_hollow == 0`, so a non-zero
/// value confirms the shape edit round-tripped.
const HOLLOW_25_PCT: u16 = 12_500;

/// Edits one owned prim across the whole build surface — name, description,
/// permissions, for-sale, material, flags, shape — confirming the
/// administrative edits via a fresh [`Event::ObjectProperties`] and the
/// geometric edits via the object's re-broadcast [`Event::ObjectUpdated`].
#[derive(Debug)]
pub struct ObjectEdit;

impl GridTest for ObjectEdit {
    fn name(&self) -> &'static str {
        "object-edit"
    }

    fn description(&self) -> &'static str {
        "Edit an object's name, description, flags, shape, material, permissions, and for-sale state"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn start_location(&self, grid: Grid) -> &'static str {
        if is_opensim(grid) {
            OPENSIM_START
        } else {
            "last"
        }
    }

    #[expect(
        clippy::too_many_lines,
        reason = "one linear flow: settle, rez, baseline, seven edits, verify, clean up"
    )]
    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();

            // The Trash folder (cleanup destination) comes from the login
            // inventory skeleton, emitted before the region is ready — capture it
            // first, before `wait_for_region` would discard it.
            let trash_folder = {
                let session = ctx.primary();
                let folders = session
                    .wait_for(REPLY_TIMEOUT, |event| match event {
                        Event::InventorySkeleton(folders) => Some(folders.clone()),
                        _ => None,
                    })
                    .await?;
                // Fall back to the inventory root if the Trash folder is absent:
                // OpenSim resolves the caller's own Trash for a Delete/Trash derez
                // regardless of the destination id.
                folder_of_type(&folders, FolderType::Trash)
                    .or_else(|| agent_root(&folders))
                    .ok_or_else(|| {
                        TestFailure::Assertion(
                            "inventory skeleton had neither a Trash nor a root folder".to_owned(),
                        )
                    })?
            };

            // Settle the initial scene: record every region-local id already
            // present (so the rezzed cube is recognisable as new) and keep the
            // first primitive as the placement reference.
            let (seen, reference) = {
                let session = ctx.primary();
                session.wait_for_region(REGION_TIMEOUT).await?;
                settle_scene(session).await?
            };

            let reference = match reference {
                Some(reference) => reference,
                None if is_opensim(grid) => {
                    return Err(TestFailure::Assertion(
                        "no primitive appeared in the Default Region object stream".to_owned(),
                    ));
                }
                None => {
                    ctx.mark_partial(
                        "landing region streamed no primitive to place against within the window",
                    );
                    return Ok(());
                }
            };

            let base = reference.motion.position.clone();
            let session = ctx.primary();

            // Rez the throwaway cube to edit, a metre above the reference prim.
            let position = Vector {
                x: base.x,
                y: base.y,
                z: base.z + REZ_LIFT_M,
            };
            session
                .send(Command::RezObject {
                    shape: PrimShape::cube(position),
                    group_id: None,
                })
                .await?;
            let object = wait_for_new_object(session, &seen).await?.ok_or_else(|| {
                TestFailure::Assertion(
                    "no new object appeared after RezObject (ObjectAdd)".to_owned(),
                )
            })?;
            let scoped = object.scoped_id();
            let object_id = object.full_id;

            // Baseline: select the cube to read its extended properties before any
            // edit. A fresh owner-rezzed prim is full-perm and not for sale.
            let baseline = request_properties(session, &object).await?;
            session
                .send(Command::DeselectObjects {
                    local_ids: vec![scoped],
                })
                .await?;
            check(
                !baseline.name.is_empty(),
                "baseline ObjectProperties carried an empty name — the object did not decode",
            )?;
            check(
                baseline.sale_type == SaleType::NotForSale.to_code(),
                "a freshly rezzed cube was unexpectedly already for sale",
            )?;
            // Flip the next-owner copy bit away from whatever the grid defaults
            // it to, so the round trip is observable either way. (OpenSim
            // defaults a new prim's next-owner mask to move+transfer, copy clear;
            // Second Life defaults to full, copy set.)
            let had_copy = baseline.permissions.next_owner.contains(Permissions::COPY);

            // --- Administrative edits (confirmed by the final ObjectProperties) ---
            session
                .send(Command::SetObjectName {
                    local_id: scoped,
                    name: NEW_NAME.to_owned(),
                })
                .await?;
            session
                .send(Command::SetObjectDescription {
                    local_id: scoped,
                    description: NEW_DESCRIPTION.to_owned(),
                })
                .await?;
            session
                .send(Command::SetObjectPermissions {
                    local_ids: vec![scoped],
                    field: PermissionField::NextOwner,
                    set: !had_copy,
                    mask: Permissions::COPY,
                })
                .await?;
            session
                .send(Command::SetObjectForSale {
                    local_id: scoped,
                    sale_type: SaleType::Copy,
                    sale_price: Some(LindenAmount(SALE_PRICE)),
                })
                .await?;

            // --- Geometric / physical edits (each confirmed by a re-broadcast) ---
            session
                .send(Command::SetObjectMaterial {
                    local_id: scoped,
                    material: Material::Metal,
                })
                .await?;
            let material_rtt = confirm_object_update(
                session,
                scoped,
                |object| object.material == Material::Metal.to_code(),
                "the metal material",
            )
            .await?;

            session
                .send(Command::SetObjectFlags {
                    local_id: scoped,
                    flags: ObjectFlagSettings {
                        is_phantom: true,
                        ..ObjectFlagSettings::default()
                    },
                })
                .await?;
            let flags_rtt = confirm_object_update(
                session,
                scoped,
                |object| object.update_flags & FLAGS_PHANTOM != 0,
                "the phantom flag",
            )
            .await?;

            session
                .send(Command::SetObjectShape {
                    local_id: scoped,
                    shape: hollow_box_shape(),
                })
                .await?;
            let shape_rtt = confirm_object_update(
                session,
                scoped,
                |object| object.shape.profile_hollow != 0,
                "a hollowed profile",
            )
            .await?;

            // Verify: re-read the extended properties and assert every
            // administrative edit landed.
            let edited = request_properties(session, &object).await?;
            session
                .send(Command::DeselectObjects {
                    local_ids: vec![scoped],
                })
                .await?;

            check_eq("edited name", &edited.name, &NEW_NAME.to_owned())?;
            check_eq(
                "edited description",
                &edited.description,
                &NEW_DESCRIPTION.to_owned(),
            )?;
            check_eq(
                "edited sale_type",
                &edited.sale_type,
                &SaleType::Copy.to_code(),
            )?;
            check_eq(
                "edited sale_price",
                &edited.sale_price,
                &Some(LindenAmount(SALE_PRICE)),
            )?;
            let has_copy = edited.permissions.next_owner.contains(Permissions::COPY);
            check(
                has_copy != had_copy,
                "the next-owner copy permission did not toggle after the permission edit",
            )?;

            // Clean up: derez the edited cube to Trash, confirmed by its
            // `KillObject`, leaving the scene as found.
            session
                .send(Command::DerezObjects {
                    local_ids: vec![scoped],
                    destination: DeRezDestination::Trash(trash_folder),
                    transaction_id: TransactionId::from(Uuid::new_v4()),
                    group_id: None,
                })
                .await?;
            let removed = wait_for_removed(session, [scoped].into_iter().collect()).await?;
            check(removed == 1, "the edited object was not removed on cleanup")?;

            let metrics = ctx.metrics();
            metrics.set("object_id", object_id.to_string());
            metrics.set("edited_name", edited.name.clone());
            metrics.set(
                "next_owner_before_hex",
                format!("{:#010x}", baseline.permissions.next_owner.bits()),
            );
            metrics.set(
                "next_owner_after_hex",
                format!("{:#010x}", edited.permissions.next_owner.bits()),
            );
            metrics.set("sale_type", i64::from(edited.sale_type));
            metrics.set(
                "sale_price",
                i64::try_from(edited.sale_price.map_or(0, |amount| amount.0)).unwrap_or(-1),
            );
            metrics.set_timing(&secs_metric("material_rtt"), material_rtt.as_secs_f64());
            metrics.set_timing(&secs_metric("flags_rtt"), flags_rtt.as_secs_f64());
            metrics.set_timing(&secs_metric("shape_rtt"), shape_rtt.as_secs_f64());
            Ok(())
        })
    }
}

/// The agent inventory root folder id from a login skeleton — the folder with no
/// parent (`None` if the skeleton is empty or rootless).
fn agent_root(folders: &[InventoryFolder]) -> Option<InventoryFolderKey> {
    folders
        .iter()
        .find(|folder| folder.parent_id.is_none())
        .map(|folder| folder.folder_id)
}

/// The id of the first folder of the given well-known type in a login skeleton
/// (`None` if absent).
fn folder_of_type(
    folders: &[InventoryFolder],
    folder_type: FolderType,
) -> Option<InventoryFolderKey> {
    folders
        .iter()
        .find(|folder| folder.folder_type == folder_type.to_code())
        .map(|folder| folder.folder_id)
}

/// The path/profile parameters of a 25%-hollow box: the default cube's geometry
/// (line path, square profile, full top-size) with [`HOLLOW_25_PCT`] hollowing.
const fn hollow_box_shape() -> PrimShapeParams {
    PrimShapeParams {
        // LL_PCODE_PATH_LINE / LL_PCODE_PROFILE_SQUARE — a box.
        path_curve: 0x10,
        profile_curve: 0x01,
        path_begin: 0,
        path_end: 0,
        // 200 - 1.0 / 0.01 = 100 (full top size on both axes).
        path_scale_x: 100,
        path_scale_y: 100,
        path_shear_x: 0,
        path_shear_y: 0,
        path_twist: 0,
        path_twist_begin: 0,
        path_radius_offset: 0,
        path_taper_x: 0,
        path_taper_y: 0,
        path_revolutions: 0,
        path_skew: 0,
        profile_begin: 0,
        profile_end: 0,
        profile_hollow: HOLLOW_25_PCT,
    }
}

/// Selects `object` (`ObjectSelect`) and returns the extended
/// [`ObjectProperties`] the simulator answers with for it. The caller is
/// responsible for the matching `DeselectObjects`.
async fn request_properties(
    session: &mut Session,
    object: &Object,
) -> Result<ObjectProperties, TestFailure> {
    let scoped = object.scoped_id();
    let object_id = object.full_id;
    session
        .send(Command::RequestObjectProperties {
            local_ids: vec![scoped],
        })
        .await?;
    session
        .wait_for(REPLY_TIMEOUT, |event| match event {
            Event::ObjectProperties(props) if props.object_id == object_id => {
                Some((**props).clone())
            }
            _ => None,
        })
        .await
}

/// Waits for the edited object to re-broadcast as an [`Event::ObjectUpdated`]
/// whose snapshot satisfies `matches` — the value the edit just set. Object
/// updates for other objects, and re-broadcasts that do not yet carry the new
/// value, are skipped. Returns how long the confirming update took, or an
/// assertion failure describing `what` was never observed within
/// [`STEP_TIMEOUT`].
async fn confirm_object_update(
    session: &mut Session,
    scoped: ScopedObjectId,
    matches: impl Fn(&Object) -> bool + Send + Sync,
    what: &str,
) -> Result<Duration, TestFailure> {
    let started = Instant::now();
    match session
        .wait_for(STEP_TIMEOUT, |event| match event {
            Event::ObjectUpdated(object) if object.scoped_id() == scoped && matches(object) => {
                Some(())
            }
            _ => None,
        })
        .await
    {
        Ok(()) => Ok(started.elapsed()),
        Err(TestFailure::Timeout(_)) => Err(TestFailure::Assertion(format!(
            "the object never re-broadcast with {what} within the step window"
        ))),
        Err(other) => Err(other),
    }
}

/// Drains the region's initial object-update burst, returning the set of every
/// region-local id sighted and the first primitive seen (the placement
/// reference, or `None` if the region streamed no primitive). The drain ends
/// once no new [`Event::ObjectAdded`] has arrived for [`SETTLE_IDLE`], or the
/// overall [`SETTLE_WINDOW`] elapses.
async fn settle_scene(
    session: &mut Session,
) -> Result<(HashSet<ScopedObjectId>, Option<Object>), TestFailure> {
    let mut seen = HashSet::new();
    let mut reference: Option<Object> = None;
    let started = Instant::now();
    loop {
        let remaining = SETTLE_WINDOW.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            break;
        }
        let cap = remaining.min(SETTLE_IDLE);
        match session
            .wait_for(cap, |event| match event {
                Event::ObjectAdded(object) => Some((**object).clone()),
                _ => None,
            })
            .await
        {
            Ok(object) => {
                if reference.is_none() && object.pcode == pcode::PRIMITIVE {
                    reference = Some(object.clone());
                }
                seen.insert(object.scoped_id());
            }
            // An idle gap (no new object for `cap`) means the scene has settled.
            Err(TestFailure::Timeout(_)) => break,
            Err(other) => return Err(other),
        }
    }
    Ok((seen, reference))
}

/// Waits for the next [`Event::ObjectAdded`] whose region-local id is not in
/// `seen` — the freshly rezzed cube. Returns `None` if none appears within
/// [`STEP_TIMEOUT`].
async fn wait_for_new_object(
    session: &mut Session,
    seen: &HashSet<ScopedObjectId>,
) -> Result<Option<Object>, TestFailure> {
    match session
        .wait_for(STEP_TIMEOUT, |event| match event {
            Event::ObjectAdded(object) if !seen.contains(&object.scoped_id()) => {
                Some((**object).clone())
            }
            _ => None,
        })
        .await
    {
        Ok(object) => Ok(Some(object)),
        Err(TestFailure::Timeout(_)) => Ok(None),
        Err(other) => Err(other),
    }
}

/// Waits until every id in `pending` has arrived as an [`Event::ObjectRemoved`]
/// (`KillObject`), returning how many were removed. Fails if the whole set has
/// not been removed within [`STEP_TIMEOUT`].
async fn wait_for_removed(
    session: &mut Session,
    mut pending: HashSet<ScopedObjectId>,
) -> Result<usize, TestFailure> {
    let total = pending.len();
    let started = Instant::now();
    while !pending.is_empty() {
        let remaining = STEP_TIMEOUT.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return Err(TestFailure::Assertion(format!(
                "{} object(s) were never removed on cleanup",
                pending.len()
            )));
        }
        match session
            .wait_for(remaining, |event| match event {
                Event::ObjectRemoved { local_id, .. } => Some(*local_id),
                _ => None,
            })
            .await
        {
            Ok(local_id) => {
                pending.remove(&local_id);
            }
            Err(TestFailure::Timeout(_)) => {}
            Err(other) => return Err(other),
        }
    }
    Ok(total)
}
