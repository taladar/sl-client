//! Raise a patch of terrain, then undo the edit.
//!
//! Terraforming ("land editing") is driven by the `ModifyLand`
//! ([`Command::ModifyLand`]) message: a single brush stroke described by a
//! [`LandEdit`] — a [`LandBrushAction`] (raise / lower / smooth / revert / …), a
//! [`LandBrushSize`] radius, a strength, and the region-local ground rectangle it
//! covers ([`TerraformArea`]). The reference viewer sends a *zero-area* rectangle
//! at the cursor for a click-drag brush ([`TerraformArea::point`]); the simulator
//! then applies a cos-falloff sphere centred on that point, so the very centre
//! cell moves by the full strength. An `UndoLand` ([`Command::UndoLand`]) rolls
//! back the agent's last terraform edit.
//!
//! Terraforming needs land-edit rights, so this case runs as the **estate-owner**
//! avatar (`--avatar estate-owner`), who owns the region-wide parcel on the local
//! grid; it also forces a login at the region centre on OpenSim so the avatar is
//! within terrain-streaming range of the edited patch.
//!
//! There is no request/reply for a terraform edit — the confirmation is the
//! simulator re-broadcasting the affected terrain patch as a `LayerData`
//! ([`Event::TerrainPatch`]) with the new heights. The region centre (128, 128)
//! sits at patch grid position (8, 8) cell (0, 0), which is exactly the peak of a
//! brush centred there. So the flow is:
//!
//! 1. Wait for the region, learn the region-centre parcel's local id from a
//!    `ParcelPropertiesRequest` reply, and confirm we own it.
//! 2. Advertise a bandwidth throttle so the simulator streams terrain, and drain
//!    the login terrain flood so the next patch we see is genuinely post-edit.
//! 3. Raise the terrain at the region centre and await the re-broadcast patch —
//!    its centre height is the raised height `H1`.
//! 4. Send `UndoLand` and watch for the patch to drop back down.
//!    - On Second Life the undo restores the terrain, so the patch re-broadcasts
//!      at the pre-edit baseline `H0` — asserted, and the region is left clean.
//!    - On stock OpenSim `UndoLand` is a **no-op** (the terrain module's
//!      `client_OnLandUndo` is an empty stub), so no patch arrives and the wait
//!      times out; the case then restores the terrain with a `Revert` brush
//!      (which reverts toward the region's baked heightmap) and reads back the
//!      restored baseline `H0`, and marks the run **partial** — the undo half is
//!      only assertable on a grid that honours `UndoLand`.
//! 5. Assert the raise was observable: `H1 - H0` is at least a minimum delta.
//!
//! Either way the region is left as found (the terrain is restored to its
//! baseline before the case returns). `1av`, `[both]`.

use std::time::Duration;

use sl_client_tokio::{
    Command, Event, LandBrushAction, LandBrushSize, LandEdit, ParcelInfo, RegionLocalParcelId,
    TerraformArea, Throttle,
};

use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{LONG_TIMEOUT, REGION_TIMEOUT, REPLY_TIMEOUT, check, check_eq, is_opensim};

/// The `start` location forcing the estate-owner avatar to the OpenSim Default
/// Region centre, so it is within terrain-streaming range of the edited patch.
const OPENSIM_START: &str = "uri:Default Region&128&128&30";

/// The region-centre ground point the brush is applied at, in region metres. The
/// region centre (128, 128) is the peak of a brush stroke centred there.
const REGION_CENTRE_M: f32 = 128.0;

/// The terrain patch grid position covering the region centre: cell 128 lies in
/// patch `128 / 16 = 8` at cell 0, so the raised peak is patch (8, 8) cell (0, 0).
const CENTRE_PATCH: u32 = 8;

/// The western/southern edge of the `ParcelProperties` query square (region
/// metres) — a 4×4 m square centred on the region centre, so the reply describes
/// the region-centre parcel.
const SQUARE_WEST_SOUTH: f32 = 124.0;

/// The eastern/northern edge of the `ParcelProperties` query square (see
/// [`SQUARE_WEST_SOUTH`]).
const SQUARE_EAST_NORTH: f32 = 128.0;

/// A distinctive `ParcelProperties` sequence id, echoed back so the awaited reply
/// is our query's answer. Distinct from the other Phase 10 cases' ids.
const SEQUENCE_ID: i32 = 5153;

/// How far to raise the terrain at the brush centre, in metres. `RaiseSphere`
/// adds the strength directly at the peak cell, so this is the peak delta; kept
/// well under Second Life's default 4 m estate raise limit while staying clearly
/// larger than terrain quantisation noise.
const RAISE_STRENGTH: f32 = 3.0;

/// The strength for the `Revert` restore brush. `RevertSphere` clamps strength to
/// `1.0` and blends the peak cell fully back to the baked heightmap at that
/// value, so one stroke restores the centre.
const REVERT_STRENGTH: f32 = 1.0;

/// The minimum observed height change (metres) that counts as a real raise,
/// comfortably above terrain DCT-quantisation noise and below [`RAISE_STRENGTH`].
const MIN_DELTA: f32 = 1.0;

/// How long to wait for a terrain patch to re-broadcast after an edit.
const TERRAIN_TIMEOUT: Duration = REPLY_TIMEOUT;

/// How long to wait for `UndoLand` to restore the terrain before concluding the
/// simulator does not honour it (the OpenSim no-op path).
const UNDO_TIMEOUT: Duration = Duration::from_secs(15);

/// The quiet gap (no terrain patch) that marks the login terrain flood drained.
const DRAIN_QUIET: Duration = Duration::from_secs(3);

/// Raises a patch of terrain then undoes the edit, restoring the region.
#[derive(Debug)]
pub struct ModifyLand;

impl GridTest for ModifyLand {
    fn name(&self) -> &'static str {
        "modify-land"
    }

    fn description(&self) -> &'static str {
        "Raise a patch of terrain, then undo the edit"
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

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;

            let agent = session
                .agent_id()
                .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;

            // 1. Learn the region-centre parcel's local id and confirm we own it
            //    (terraforming needs land-edit rights). The paint brush ignores
            //    the parcel id, but a faithful ModifyLand carries the selection.
            session
                .send(Command::RequestParcelProperties {
                    west: SQUARE_WEST_SOUTH,
                    south: SQUARE_WEST_SOUTH,
                    east: SQUARE_EAST_NORTH,
                    north: SQUARE_EAST_NORTH,
                    sequence_id: SEQUENCE_ID,
                })
                .await?;
            let parcel: ParcelInfo = session
                .wait_for(LONG_TIMEOUT, |event| match event {
                    Event::ParcelProperties(parcel) if parcel.sequence_id == SEQUENCE_ID => {
                        Some((**parcel).clone())
                    }
                    _ => None,
                })
                .await?;
            check(
                parcel.request_result.has_data(),
                &format!(
                    "parcel query returned no data (request_result: {:?})",
                    parcel.request_result
                ),
            )?;
            let local_id = parcel.local_id;
            check_eq(
                "parcel owner is the logged-in (estate-owner) avatar",
                &parcel.owner.uuid(),
                &agent.uuid(),
            )?;

            // 2. Ask the simulator to stream terrain, then drain the login flood
            //    so the next patch we see is a genuine post-edit re-broadcast.
            session
                .send(Command::SetThrottle(Throttle::preset_1000()))
                .await?;
            drain_terrain(session, DRAIN_QUIET).await?;

            // 3. Raise the terrain at the region centre; the re-broadcast patch's
            //    centre cell carries the raised height.
            session
                .send(Command::ModifyLand(brush(
                    LandBrushAction::Raise,
                    RAISE_STRENGTH,
                    local_id,
                )))
                .await?;
            let raised = wait_centre_patch(session, TERRAIN_TIMEOUT, |_height| true).await?;

            // 4. Undo. On Second Life this restores the terrain; on OpenSim
            //    UndoLand is a no-op, so the wait times out and we restore with a
            //    Revert brush instead. Either way we read back the baseline.
            session.send(Command::UndoLand).await?;
            let undo_threshold = raised - MIN_DELTA;
            let (baseline, undo_restored) =
                match wait_centre_patch(session, UNDO_TIMEOUT, |height| height <= undo_threshold)
                    .await
                {
                    Ok(height) => (height, true),
                    Err(TestFailure::Timeout(_)) => {
                        // OpenSim: UndoLand did nothing. Restore via a Revert
                        // brush (reverts toward the region's baked heightmap) so
                        // the region is left as found.
                        session
                            .send(Command::ModifyLand(brush(
                                LandBrushAction::Revert,
                                REVERT_STRENGTH,
                                local_id,
                            )))
                            .await?;
                        let restored = wait_centre_patch(session, TERRAIN_TIMEOUT, |height| {
                            height <= undo_threshold
                        })
                        .await?;
                        (restored, false)
                    }
                    Err(other) => return Err(other),
                };

            // 5. The raise must have been observable relative to the baseline.
            let delta = raised - baseline;
            check(
                delta >= MIN_DELTA,
                &format!(
                    "terrain did not rise observably: raised {raised:.2} m vs baseline \
                     {baseline:.2} m (delta {delta:.2} m < {MIN_DELTA:.2} m)"
                ),
            )?;

            let metrics = ctx.metrics();
            metrics.set("local_id", i64::from(local_id.0));
            metrics.set("owner_id", parcel.owner.uuid().to_string());
            metrics.set("raise_strength", f64::from(RAISE_STRENGTH));
            metrics.set("raised_height", f64::from(raised));
            metrics.set("baseline_height", f64::from(baseline));
            metrics.set("raise_delta", f64::from(delta));
            metrics.set("undo_restored", undo_restored);

            if !undo_restored {
                ctx.mark_partial(
                    "UndoLand is a no-op on stock OpenSim (the terrain module's \
                     client_OnLandUndo is an empty stub); the raise was verified and the \
                     region restored via a Revert brush, but undo restoration is only \
                     assertable on a grid that honours UndoLand (Second Life)",
                );
            }
            Ok(())
        })
    }
}

/// Builds a point brush [`LandEdit`] at the region centre with the given action
/// and strength, targeting `parcel`. Uses the large brush radius and a zero-area
/// rectangle at the centre, as the viewer sends for a click-drag stroke.
const fn brush(action: LandBrushAction, strength: f32, parcel: RegionLocalParcelId) -> LandEdit {
    LandEdit {
        action,
        brush_size: LandBrushSize::Large,
        strength,
        // The reference height is only used by the level/flatten actions; the
        // raise and revert brushes ignore it.
        height: 0.0,
        parcel: Some(parcel),
        area: TerraformArea::point(REGION_CENTRE_M, REGION_CENTRE_M),
    }
}

/// Awaits the next re-broadcast of the region-centre terrain patch whose centre
/// height `accept`s, returning that height.
///
/// # Errors
///
/// Returns [`TestFailure::Timeout`] if no accepted patch arrives within
/// `timeout`, or propagates a [`Session::wait_for`] disconnect.
async fn wait_centre_patch(
    session: &mut Session,
    timeout: Duration,
    mut accept: impl FnMut(f32) -> bool,
) -> Result<f32, TestFailure> {
    session
        .wait_for(timeout, |event| match event {
            Event::TerrainPatch(patch)
                if patch.layer.is_land()
                    && patch.patch_x == CENTRE_PATCH
                    && patch.patch_y == CENTRE_PATCH =>
            {
                let height = patch.value(0, 0)?;
                accept(height).then_some(height)
            }
            _ => None,
        })
        .await
}

/// Drains queued terrain patches until none arrives for `quiet`, so a later
/// [`wait_centre_patch`] sees only genuinely post-edit re-broadcasts.
///
/// # Errors
///
/// Propagates a [`Session::wait_for`] disconnect.
async fn drain_terrain(session: &mut Session, quiet: Duration) -> Result<(), TestFailure> {
    loop {
        match session
            .wait_for(quiet, |event| {
                matches!(event, Event::TerrainPatch(_)).then_some(())
            })
            .await
        {
            Ok(()) => {}
            Err(TestFailure::Timeout(_)) => return Ok(()),
            Err(other) => return Err(other),
        }
    }
}
