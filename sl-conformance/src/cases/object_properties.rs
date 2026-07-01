//! Request an object's extended properties and its condensed family properties.
//!
//! A viewer learns an object's *administrative* facts — creator, owner, group,
//! permissions, sale info, name/description — through two distinct requests that
//! this case exercises back to back against one primitive:
//!
//! 1. **`ObjectProperties`** (the build-floater / "more info" view): the client
//!    *selects* the object with an `ObjectSelect`
//!    ([`Command::RequestObjectProperties`]) and the simulator answers with a
//!    full [`Event::ObjectProperties`] carrying every field (creator, last
//!    owner, creation date, the full permission block, task-inventory serial,
//!    texture ids, …).
//! 2. **`ObjectPropertiesFamily`** (the hover / pay / report summary): a
//!    selection-free [`Command::RequestObjectPropertiesFamily`] to which the
//!    simulator replies with the condensed [`Event::ObjectPropertiesFamily`] —
//!    just the owner/group/permissions/sale rollup a viewer shows on hover.
//!
//! The case first watches the region's object-update stream for a primitive to
//! query (the same interest-list stream `object-update-decode` decodes), then
//! issues both requests for that one object and asserts the two replies describe
//! the *same* object consistently: identical `object_id`, owner, group, sale
//! type, and base permission mask. That cross-check proves both the
//! selection-based full path and the selection-free family path decode into a
//! coherent view of the object.
//!
//! `1av`, `[both]`. On OpenSim the avatar is forced into the "Default Region",
//! which holds this workspace's rezzed test object, so a primitive is guaranteed
//! and its absence fails the case. On Second Life the landing region's contents
//! are uncontrolled; a region that streams no primitive within the window is
//! recorded `partial` rather than failed. The aditi run is deferred with the
//! rest of the Aditi batch (no aditi record this session).

use std::time::{Duration, Instant};

use sl_client_tokio::{Command, Event, Object, pcode};

use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, REPLY_TIMEOUT, check, check_eq, is_opensim, secs_metric};

/// The OpenSim start location: the "Default Region" (1000,1000), centred, where
/// this workspace's test object lives. On Second Life the avatar keeps `"last"`
/// (a named OpenSim region is meaningless there), and whatever region it lands
/// in supplies the object.
const OPENSIM_START: &str = "uri:Default Region&128&128&30";

/// How long to watch the object-update stream for a primitive to query.
///
/// Generous enough to span the `ObjectUpdateCached` cache-miss round trip (a
/// digest whose full update this client refetches with `RequestMultipleObjects`)
/// and Second Life's larger, more staggered scene streaming.
const FIND_WINDOW: Duration = Duration::from_secs(20);

/// Requests an object's extended [`Event::ObjectProperties`] and condensed
/// [`Event::ObjectPropertiesFamily`], asserting the two replies describe the
/// same object consistently.
#[derive(Debug)]
pub struct ObjectProperties;

impl GridTest for ObjectProperties {
    fn name(&self) -> &'static str {
        "object-properties"
    }

    fn description(&self) -> &'static str {
        "Request an object's properties and properties-family"
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
            let grid = ctx.grid();

            // Find a primitive in the region's interest-list stream to query.
            // `ObjectAdded` fires on the first sighting of each region-local id;
            // the first one whose `PCode` is a primitive (not the self avatar) is
            // the object this case asks about. A per-iteration timeout that
            // consumes the remaining window ends the search empty-handed.
            let object: Option<Object> = {
                let session = ctx.primary();
                session.wait_for_region(REGION_TIMEOUT).await?;
                match session
                    .wait_for(FIND_WINDOW, |event| match event {
                        Event::ObjectAdded(object) if object.pcode == pcode::PRIMITIVE => {
                            Some((**object).clone())
                        }
                        _ => None,
                    })
                    .await
                {
                    Ok(object) => Some(object),
                    // No primitive streamed inside the window; handled per grid
                    // below (a hard failure on OpenSim, `partial` on SL).
                    Err(TestFailure::Timeout(_)) => None,
                    Err(other) => return Err(other),
                }
            };

            let object = match object {
                Some(object) => object,
                None if is_opensim(grid) => {
                    return Err(TestFailure::Assertion(
                        "no primitive appeared in the Default Region object stream".to_owned(),
                    ));
                }
                None => {
                    // On Second Life the landing region's contents are
                    // uncontrolled; a region that streams no primitive in the
                    // window is a legitimately incomplete dataset.
                    ctx.mark_partial(
                        "landing region streamed no primitive to query within the window",
                    );
                    return Ok(());
                }
            };
            let target_id = object.full_id;
            let scoped = object.scoped_id();

            let session = ctx.primary();

            // 1. The full path: selecting the object (`ObjectSelect`) makes the
            //    simulator send its extended `ObjectProperties`.
            let started = Instant::now();
            session
                .send(Command::RequestObjectProperties {
                    local_ids: vec![scoped],
                })
                .await?;
            let props = session
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::ObjectProperties(props) if props.object_id == target_id => {
                        Some((**props).clone())
                    }
                    _ => None,
                })
                .await?;
            let properties_rtt = started.elapsed();

            // 2. The condensed path: a selection-free `RequestObjectPropertiesFamily`
            //    (plain info query, no request flags) yields the hover summary.
            let started = Instant::now();
            session
                .send(Command::RequestObjectPropertiesFamily {
                    request_flags: 0,
                    object_id: target_id,
                })
                .await?;
            let family = session
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::ObjectPropertiesFamily { properties }
                        if properties.object_id == target_id =>
                    {
                        Some(properties.clone())
                    }
                    _ => None,
                })
                .await?;
            let family_rtt = started.elapsed();

            // Release the selection so the case leaves the scene as it found it.
            session
                .send(Command::DeselectObjects {
                    local_ids: vec![scoped],
                })
                .await?;

            // Both replies describe the requested object.
            check_eq("object_properties object_id", &props.object_id, &target_id)?;
            check_eq(
                "object_properties_family object_id",
                &family.object_id,
                &target_id,
            )?;

            // The full and condensed views agree on the object's administrative
            // facts — owner, group, sale type, name, and the base permission
            // mask. A mismatch would mean one of the two decode paths is wrong.
            check_eq("owner", &family.owner, &props.owner)?;
            check_eq("group", &family.group, &props.group)?;
            check_eq("sale_type", &family.sale_type, &props.sale_type)?;
            check_eq("name", &family.name, &props.name)?;
            check_eq(
                "base permissions",
                &family.permissions.base,
                &props.permissions.base,
            )?;

            // A real object was decoded (a prim always carries a non-empty name,
            // "Object" by default), rather than an empty placeholder reply.
            check(
                !props.name.is_empty(),
                "ObjectProperties carried an empty name — the object did not decode",
            )?;

            let metrics = ctx.metrics();
            metrics.set_timing(&secs_metric("properties_rtt"), properties_rtt.as_secs_f64());
            metrics.set_timing(&secs_metric("family_rtt"), family_rtt.as_secs_f64());
            metrics.set("object_id", target_id.to_string());
            metrics.set("object_name", props.name.clone());
            metrics.set("owner", props.owner.to_string());
            metrics.set("creator", props.creator_id.to_string());
            metrics.set("group_set", props.group.is_some());
            metrics.set("sale_type", i64::from(props.sale_type));
            metrics.set(
                "perms_base_hex",
                format!("{:#010x}", props.permissions.base.bits()),
            );
            metrics.set(
                "texture_ids",
                i64::try_from(props.texture_ids.len()).unwrap_or(-1),
            );
            Ok(())
        })
    }
}
