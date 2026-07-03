//! Request the agent's own current wearables (`AgentWearablesRequest`) and
//! assert the simulator's authoritative reply.
//!
//! The simulator pushes an `AgentWearablesUpdate` at login and after every
//! wearable change, but a viewer can also ask for it on demand:
//! [`Command::RequestWearables`] queues an `AgentWearablesRequest`, and the sim
//! answers with a fresh `AgentWearablesUpdate` surfaced as
//! [`Event::AgentWearables`] — a serial number plus the list of worn
//! [`Wearable`]s (each an inventory item id, an asset id, and a
//! [`WearableType`] slot).
//!
//! A valid avatar always wears exactly one of each of the four mandatory *body
//! parts* — shape, skin, hair and eyes.
//!
//! **Grid divergence.** On OpenSim this legacy message carries the real outfit,
//! so the case asserts all four body parts are present, each worn once and
//! naming a real asset. On modern Second Life (aditi) the outfit is managed
//! server-side (central baking + the Current Outfit Folder over AIS3), and the
//! `AgentWearablesUpdate` message is *transitional/deprecated*: the simulator may
//! answer `AgentWearablesRequest` with fewer than four body parts (Firestorm's
//! `processAgentInitialWearablesUpdate` explicitly treats a sub-body-part update
//! as a dummy and ignores it, reading the true outfit from the COF instead). So
//! on aditi an incomplete reply is recorded `partial` rather than failing —
//! mirroring `server-appearance-bake` / `baked-texture-upload`.

use std::collections::BTreeSet;
use std::time::Instant;

use sl_client_tokio::{Command, Event, Throttle, Wearable, WearableType};

use crate::context::TestContext;
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{LONG_TIMEOUT, REGION_TIMEOUT, check, is_aditi};

/// The four mandatory body-part slots every valid avatar wears exactly one of.
const BODY_PARTS: [WearableType; 4] = [
    WearableType::Shape,
    WearableType::Skin,
    WearableType::Hair,
    WearableType::Eyes,
];

/// Requests the agent's own wearables over `AgentWearablesRequest`.
#[derive(Debug)]
pub struct WearablesRequest;

impl GridTest for WearablesRequest {
    fn name(&self) -> &'static str {
        "wearables-request"
    }

    fn description(&self) -> &'static str {
        "Request the agent's own current wearables (AgentWearablesRequest)"
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

            // Ask the simulator to (re-)send the agent's current wearables and
            // wait for the authoritative reply. Region-active drains any
            // login-time update, so this is a genuine reply to our request.
            let start = Instant::now();
            session.send(Command::RequestWearables).await?;
            let (serial, wearables) = session
                .wait_for(LONG_TIMEOUT, |event| match event {
                    Event::AgentWearables { serial, wearables } => {
                        Some((*serial, wearables.clone()))
                    }
                    _other => None,
                })
                .await?;
            let reply_secs = start.elapsed().as_secs_f64();

            // Classify the reply: which body-part slots are present, and how many
            // clothing layers ride alongside them.
            let present_body_parts: BTreeSet<u8> = wearables
                .iter()
                .filter(|wearable| wearable.wearable_type.is_body_part())
                .map(|wearable| wearable.wearable_type.to_code())
                .collect();
            let clothing_count = wearables
                .iter()
                .filter(|wearable| !wearable.wearable_type.is_body_part())
                .count();
            let distinct_slots: BTreeSet<u8> = wearables
                .iter()
                .map(|wearable| wearable.wearable_type.to_code())
                .collect();
            let missing: Vec<WearableType> = BODY_PARTS
                .into_iter()
                .filter(|part| !present_body_parts.contains(&part.to_code()))
                .collect();

            tracing::info!(
                serial,
                wearable_count = wearables.len(),
                body_parts = present_body_parts.len(),
                clothing = clothing_count,
                ?missing,
                "AgentWearablesUpdate reply"
            );

            let metrics = ctx.metrics();
            metrics.set_timing("reply_secs", reply_secs);
            metrics.set("serial", i64::from(serial));
            let wearable_count = i64::try_from(wearables.len()).unwrap_or(i64::MAX);
            metrics.set("wearable_count", wearable_count);
            let body_part_count = i64::try_from(present_body_parts.len()).unwrap_or(i64::MAX);
            metrics.set("body_part_count", body_part_count);
            metrics.set(
                "clothing_count",
                i64::try_from(clothing_count).unwrap_or(i64::MAX),
            );
            let distinct_slot_count = i64::try_from(distinct_slots.len()).unwrap_or(i64::MAX);
            metrics.set("distinct_slots", distinct_slot_count);

            check(
                !wearables.is_empty(),
                "AgentWearablesUpdate carried no wearables",
            )?;

            // Modern Second Life manages the outfit server-side (central baking +
            // COF/AIS3); its legacy AgentWearablesUpdate is transitional and may
            // omit body parts, which the reference viewer ignores as a dummy. So a
            // short body-part set on aditi is a grid-behaviour outcome, not a
            // client fault — record it partial with the missing slots.
            if !missing.is_empty() {
                if is_aditi(grid) {
                    ctx.mark_partial(&format!(
                        "modern SL AgentWearablesUpdate is transitional — {} body \
                         part(s) missing ({missing:?}); the real outfit lives in \
                         the Current Outfit Folder (AIS3)",
                        missing.len()
                    ));
                    return Ok(());
                }
                check(
                    false,
                    &format!("missing mandatory body part(s): {missing:?}"),
                )?;
            }

            // Full outfit present: every body part is worn exactly once and names a
            // real asset (a nil asset id would mean the sim could not resolve it).
            for part in BODY_PARTS {
                let worn: Vec<&Wearable> = wearables
                    .iter()
                    .filter(|wearable| wearable.wearable_type == part)
                    .collect();
                check(
                    worn.len() == 1,
                    &format!(
                        "body part {part:?} worn {} times (expected exactly one)",
                        worn.len()
                    ),
                )?;
                let asset_ok = worn
                    .first()
                    .and_then(|wearable| wearable.asset_id)
                    .is_some_and(|id| !id.is_nil());
                check(asset_ok, &format!("body part {part:?} has no asset id"))?;
            }

            Ok(())
        })
    }
}
