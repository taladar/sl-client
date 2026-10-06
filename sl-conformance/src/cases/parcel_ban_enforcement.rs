//! Ban a second avatar from a parcel and record what it meets at the parcel
//! line — the enforcement half of `gridspec-parcel-access-and-ban-lines`.
//!
//! [`super::parcel_access_list`] measures the lists as data. This case measures
//! what a grid *does* with them, from the side of the avatar they are about:
//! the **primary** owns the land (the estate owner on the local grid), the
//! **secondary** is the resident it bans. The secondary must hold no estate or
//! land rights — OpenSim never bans an estate manager, an administrator or the
//! parcel's owner — so on the local grid it is the dedicated `resident`
//! account (`--secondary resident`).
//!
//! The primary divides off everything east of a line `LINE_OFFSET_M` east of
//! the secondary — *the plot* — the way `parcel-crossing` does, and joins the
//! region back afterwards under the same cleanup guard. Then, one leg at a
//! time:
//!
//! 1. **Banned, flying in.** The primary puts the secondary on the plot's ban
//!    list; the secondary reads that list itself (does a stranger see it?) and
//!    flies at a point `PLOT_DEPTH_M` inside the plot for `APPROACH_BUDGET`.
//! 2. **Banned, teleporting in.** The secondary teleports to the same point.
//! 3. **Not banned.** The primary empties the list and the secondary flies in —
//!    the control: the same flight arrives.
//! 4. **Banned while inside.** The primary bans the secondary where it stands,
//!    and the secondary first stands still for `STAND_WATCH`, then nudges
//!    forward.
//! 5. **Not on the allow list.** The primary empties the ban list and gives the
//!    plot an allow list the secondary is not on; the secondary flies at it.
//!
//! Each leg records where the secondary ended up (`*_stopped`, as metres past
//! the line — negative is short of it), every parcel record pushed on the way
//! (`*_pushes`, each `sequence/local id/result/collision`), every alert
//! (`*_alerts`) and every teleport message (`*_teleports`).
//!
//! `2av`, OpenSim only, live only. Second Life needs land the primary owns
//! (`gridspec-aditi-test-land`); what a resident meets at somebody else's
//! closed parcel there is [`super::parcel_ban_line`]. The fake grid enforces nothing
//! (`server-fake-grid-parcel-access-enforcement`).

use std::time::Duration;

use sl_client_tokio::{
    Command, Event, ParcelAccessEntry, ParcelAccessFlags, ParcelAccessScope, ParcelCollision,
    ParcelInfo, RegionCoordinates, ScopedParcelId, Uuid, Vector,
};

use crate::cases::parcel_divide_join::{RestoreOnDrop, restore_single_parcel};
use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{
    Gait, LONG_TIMEOUT, REGION_TIMEOUT, REPLY_TIMEOUT, check, check_eq, steer_towards,
};

/// The start location of both avatars: the "Default Region" centre, so the
/// divide below has room on both sides of them.
const OPENSIM_START: &str = "uri:Default Region&128&128&30";

/// How far east of the secondary the divide draws its line, in metres (a
/// multiple of the 4 m parcel grid).
const LINE_OFFSET_M: f32 = 12.0;

/// How far inside the plot the secondary aims, in metres.
const PLOT_DEPTH_M: f32 = 10.0;

/// How long a flight at the plot may take. The line is [`LINE_OFFSET_M`] away
/// and a flying avatar covers that in a few seconds; the rest is for a grid
/// that pushes it back and lets it try again.
const APPROACH_BUDGET: Duration = Duration::from_secs(15);

/// How long a flight back to where the secondary started may take.
const RETURN_BUDGET: Duration = Duration::from_secs(40);

/// How long to keep watching after a flight or a teleport ends.
const SETTLE: Duration = Duration::from_secs(4);

/// How long the secondary stands still after being banned where it stands.
const STAND_WATCH: Duration = Duration::from_secs(8);

/// How long the secondary nudges forward after that.
const NUDGE_BUDGET: Duration = Duration::from_secs(4);

/// How long a list or parcel edit is given to take effect.
const EDIT_SETTLE: Duration = Duration::from_secs(2);

/// How long the teleport leg waits for the teleport to end one way or another.
const TELEPORT_WINDOW: Duration = Duration::from_secs(30);

/// The first sequence id of this case's parcel queries.
const SEQUENCE_BASE: i32 = 5460;

/// The one id on the allow list of the last leg: nobody.
const ALLOWED_NOBODY: Uuid = Uuid::from_u128(0x0000_0000_0000_4000_8000_0000_a110_3d00);

/// The alert OpenSim sends an avatar it turns back from a parcel that bans it
/// (`LandManagementModule.EnforceBans`).
const OPENSIM_BANNED_ALERT: &str = "You are banned from parcel";

/// The alert OpenSim sends an avatar it turns back from a parcel whose allow
/// list it is not on.
const OPENSIM_RESTRICTED_ALERT: &str = "You do not have access to the parcel";

/// Bans a second avatar from a parcel and records what the grid does to it.
#[derive(Debug)]
pub struct ParcelBanEnforcement;

impl GridTest for ParcelBanEnforcement {
    fn name(&self) -> &'static str {
        "parcel-ban-enforcement"
    }

    fn description(&self) -> &'static str {
        "Ban a second avatar from a parcel and record what it meets at the line"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim]
    }

    fn accounts(&self) -> u8 {
        2
    }

    fn start_location(&self, _grid: Grid) -> &'static str {
        OPENSIM_START
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            // The divide leaves the region split until it is joined back: the
            // awaited join for every path that returns, the drop guard for one
            // that never does.
            let mut guard = RestoreOnDrop {
                commander: ctx.primary().commander(),
                armed: true,
            };
            let outcome = enforce(ctx).await;
            let restored = restore_single_parcel(ctx.primary()).await;
            guard.armed = false;
            match (outcome, restored) {
                (Ok(()), restored) => restored,
                (Err(failure), _) => Err(failure),
            }
        })
    }
}

/// The legs themselves.
///
/// # Errors
///
/// Returns a [`TestFailure`] for a send or wait failure, a missing second
/// avatar, or an answer that differs from what OpenSim was measured to give.
async fn enforce(ctx: &mut TestContext) -> Result<(), TestFailure> {
    ctx.primary().wait_for_region(REGION_TIMEOUT).await?;
    let owner_circuit = ctx
        .primary()
        .circuit_id()
        .ok_or_else(|| TestFailure::Assertion("the owner has no root circuit id".to_owned()))?;
    let resident = secondary(ctx)?;
    resident.wait_for_region(REGION_TIMEOUT).await?;
    let resident_id = resident
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("the resident has no agent id".to_owned()))?
        .uuid();
    let resident_circuit = resident
        .circuit_id()
        .ok_or_else(|| TestFailure::Assertion("the resident has no root circuit id".to_owned()))?;
    let region_handle = resident
        .region_handle()
        .ok_or_else(|| TestFailure::Assertion("the resident has no region handle".to_owned()))?;
    let here = own_position(resident, resident_id).await?;

    // The plot: everything east of a line a few metres east of the resident.
    let owner = ctx.primary();
    restore_single_parcel(owner).await?;
    let line_x = ((here.x + LINE_OFFSET_M) / 4.0).round() * 4.0;
    owner
        .send(Command::DivideParcel {
            west: line_x,
            south: 0.0,
            east: 256.0,
            north: 256.0,
        })
        .await?;
    tokio::time::sleep(EDIT_SETTLE).await;
    let inside = Vector {
        x: line_x + PLOT_DEPTH_M,
        y: here.y,
        z: here.z,
    };
    let plot = read_parcel(owner, &inside, SEQUENCE_BASE).await?;
    let home = read_parcel(owner, &here, SEQUENCE_BASE.saturating_add(1)).await?;
    check(
        plot.request_result.has_data() && plot.local_id != home.local_id,
        "the divide did not make a second parcel east of the resident",
    )?;
    let owner_plot = ScopedParcelId::new(owner_circuit, plot.local_id);
    let resident_plot = ScopedParcelId::new(resident_circuit, plot.local_id);
    let banned = vec![ParcelAccessEntry {
        id: resident_id,
        time: 0,
        flags: ParcelAccessFlags::NONE,
    }];
    let course = Course {
        agent: resident_id,
        here,
        inside,
        line_x,
    };

    // 1. Banned, flying in.
    write_list(owner, owner_plot, ParcelAccessScope::Ban, banned.clone()).await?;
    let resident = secondary(ctx)?;
    resident
        .send(Command::RequestParcelAccessList {
            local_id: resident_plot,
            scope: ParcelAccessScope::Ban,
        })
        .await?;
    let stranger_sees = resident
        .wait_for(REPLY_TIMEOUT, |event| match event {
            Event::ParcelAccessList {
                local_id,
                scope: ParcelAccessScope::Ban,
                entries,
            } if local_id.id() == plot.local_id => Some(entries.len()),
            _ => None,
        })
        .await?;
    let banned_flight = course.approach(resident).await?;
    course.go_home(resident, &banned_flight).await?;

    // 2. Banned, teleporting in.
    let banned_teleport = course.teleport(resident, region_handle).await?;
    course.go_home(resident, &banned_teleport).await?;

    // 3. Not banned: the control.
    write_list(
        ctx.primary(),
        owner_plot,
        ParcelAccessScope::Ban,
        Vec::new(),
    )
    .await?;
    let resident = secondary(ctx)?;
    let free_flight = course.approach(resident).await?;

    // 4. Banned while standing inside.
    write_list(ctx.primary(), owner_plot, ParcelAccessScope::Ban, banned).await?;
    let resident = secondary(ctx)?;
    let standing = course.stand(resident, &free_flight).await?;
    let nudged = course.nudge(resident, &standing).await?;
    write_list(
        ctx.primary(),
        owner_plot,
        ParcelAccessScope::Ban,
        Vec::new(),
    )
    .await?;
    let resident = secondary(ctx)?;
    course.go_home(resident, &nudged).await?;

    // 5. Not on the allow list.
    write_list(
        ctx.primary(),
        owner_plot,
        ParcelAccessScope::Access,
        vec![ParcelAccessEntry {
            id: ALLOWED_NOBODY,
            time: 0,
            flags: ParcelAccessFlags::NONE,
        }],
    )
    .await?;
    let resident = secondary(ctx)?;
    let restricted_flight = course.approach(resident).await?;
    write_list(
        ctx.primary(),
        owner_plot,
        ParcelAccessScope::Access,
        Vec::new(),
    )
    .await?;

    let metrics = ctx.metrics();
    metrics.set("line_x", format!("{line_x:.0}"));
    metrics.set(
        "stranger_sees_ban_entries",
        i64::try_from(stranger_sees).unwrap_or(-1),
    );
    for (name, leg) in [
        ("banned_flight", &banned_flight),
        ("banned_teleport", &banned_teleport),
        ("free_flight", &free_flight),
        ("banned_standing", &standing),
        ("banned_nudged", &nudged),
        ("restricted_flight", &restricted_flight),
    ] {
        metrics.set(&format!("{name}_stopped"), course.describe_stop(leg));
        metrics.set(&format!("{name}_pushes"), leg.describe_pushes());
        metrics.set(&format!("{name}_alerts"), leg.describe_alerts());
        metrics.set(&format!("{name}_teleports"), leg.teleports.join(" | "));
    }

    // What OpenSim was measured to do (2026-10-05, book/src/gridspec/land.md).
    check_eq(
        "ban entries a stranger reads off the plot's list",
        &stranger_sees,
        &1,
    )?;
    check(
        !course.is_inside(&banned_flight),
        "a banned avatar flew into the plot",
    )?;
    check(
        banned_flight.collided(ParcelCollision::Banned),
        "a banned avatar was pushed no ban line for the plot",
    )?;
    check(
        banned_flight
            .alerts
            .iter()
            .any(|alert| alert == OPENSIM_BANNED_ALERT),
        "a banned avatar was not alerted at the plot's line",
    )?;
    check(
        !course.is_inside(&banned_teleport),
        "a banned avatar teleported into the plot",
    )?;
    check(
        course.is_inside(&free_flight),
        "an avatar nobody bans did not reach the plot",
    )?;
    check(
        !course.is_inside(&nudged),
        "an avatar banned where it stood was still on the plot after moving",
    )?;
    check(
        !course.is_inside(&restricted_flight),
        "an avatar off the allow list flew into the plot",
    )?;
    check(
        restricted_flight.collided(ParcelCollision::NotOnList),
        "an avatar off the allow list was pushed no ban line for the plot",
    )?;
    check(
        restricted_flight
            .alerts
            .iter()
            .any(|alert| alert == OPENSIM_RESTRICTED_ALERT),
        "an avatar off the allow list was not alerted at the plot's line",
    )
}

/// The second avatar's session.
///
/// # Errors
///
/// Returns [`TestFailure::Assertion`] when the run has no second avatar.
fn secondary(ctx: &mut TestContext) -> Result<&mut Session, TestFailure> {
    ctx.secondary().ok_or_else(|| {
        TestFailure::Assertion("parcel-ban-enforcement needs a second avatar".to_owned())
    })
}

/// Where the resident starts, where it aims, and the line between the two.
struct Course {
    /// The resident's agent id, to pick its own avatar out of the object
    /// stream.
    agent: Uuid,
    /// Where the resident started, west of the line.
    here: Vector,
    /// The point inside the plot the resident aims at.
    inside: Vector,
    /// The plot's western edge, in region metres.
    line_x: f32,
}

impl Course {
    /// Flies the resident at the point inside the plot, then keeps watching
    /// for [`SETTLE`].
    ///
    /// # Errors
    ///
    /// Propagates the flight's and the watch's failures.
    async fn approach(&self, session: &mut Session) -> Result<Leg, TestFailure> {
        let mut leg = Leg::starting_at(&self.here);
        let (stopped, _arrived) = steer_towards(
            session,
            self.here.clone(),
            &self.inside,
            Gait::FlyingLevel,
            APPROACH_BUDGET,
            |event| leg.note(event, self.agent),
        )
        .await?;
        leg.position = stopped;
        watch(session, SETTLE, &mut leg, self.agent).await?;
        Ok(leg)
    }

    /// Teleports the resident to the point inside the plot and records how the
    /// teleport ended and where the avatar is afterwards.
    ///
    /// # Errors
    ///
    /// Propagates the send's and the watches' failures.
    async fn teleport(
        &self,
        session: &mut Session,
        region_handle: sl_client_tokio::RegionHandle,
    ) -> Result<Leg, TestFailure> {
        let mut leg = Leg::starting_at(&self.here);
        session
            .send(Command::Teleport {
                region_handle,
                position: RegionCoordinates::new(self.inside.x, self.inside.y, self.inside.z),
                look_at: Vector {
                    x: 1.0,
                    y: 0.0,
                    z: 0.0,
                },
            })
            .await?;
        let agent = self.agent;
        let ended = session
            .wait_for(TELEPORT_WINDOW, |event| {
                leg.note(event, agent);
                matches!(
                    event,
                    Event::TeleportLocal { .. } | Event::TeleportFailed { .. }
                )
                .then_some(())
            })
            .await;
        match ended {
            Ok(()) | Err(TestFailure::Timeout(_)) => {}
            Err(other) => return Err(other),
        }
        watch(session, SETTLE, &mut leg, self.agent).await?;
        Ok(leg)
    }

    /// Leaves the resident standing where `before` ended for [`STAND_WATCH`].
    ///
    /// # Errors
    ///
    /// Propagates the watch's failures.
    async fn stand(&self, session: &mut Session, before: &Leg) -> Result<Leg, TestFailure> {
        let mut leg = Leg::starting_at(&before.position);
        watch(session, STAND_WATCH, &mut leg, self.agent).await?;
        Ok(leg)
    }

    /// Nudges the resident further into the plot from where `before` ended.
    ///
    /// # Errors
    ///
    /// Propagates the flight's and the watch's failures.
    async fn nudge(&self, session: &mut Session, before: &Leg) -> Result<Leg, TestFailure> {
        let mut leg = Leg::starting_at(&before.position);
        let further = Vector {
            x: before.position.x + PLOT_DEPTH_M,
            y: before.position.y,
            z: before.position.z,
        };
        let (stopped, _arrived) = steer_towards(
            session,
            before.position.clone(),
            &further,
            Gait::FlyingLevel,
            NUDGE_BUDGET,
            |event| leg.note(event, self.agent),
        )
        .await?;
        leg.position = stopped;
        watch(session, SETTLE, &mut leg, self.agent).await?;
        Ok(leg)
    }

    /// Flies the resident back to where it started from where `after` ended.
    ///
    /// # Errors
    ///
    /// Returns [`TestFailure::Assertion`] when the resident cannot get back —
    /// the next leg would then measure a different approach — and propagates
    /// the flight's failures.
    async fn go_home(&self, session: &mut Session, after: &Leg) -> Result<(), TestFailure> {
        let (stopped, arrived) = steer_towards(
            session,
            after.position.clone(),
            &self.here,
            Gait::FlyingLevel,
            RETURN_BUDGET,
            |_event| {},
        )
        .await?;
        check(
            arrived,
            &format!(
                "the resident could not fly back to its start (stuck at {:.0}/{:.0})",
                stopped.x, stopped.y
            ),
        )
    }

    /// Whether `leg` ended with the resident on the plot.
    fn is_inside(&self, leg: &Leg) -> bool {
        leg.position.x >= self.line_x
    }

    /// Where `leg` ended, as the record shows it: metres past the line
    /// (negative is short of it) and the height.
    fn describe_stop(&self, leg: &Leg) -> String {
        format!(
            "{:+.1} m past the line, z {:.1}",
            leg.position.x - self.line_x,
            leg.position.z
        )
    }
}

/// What one leg saw.
struct Leg {
    /// Where the resident's avatar last was.
    position: Vector,
    /// Every parcel record pushed during the leg.
    pushes: Vec<ParcelInfo>,
    /// Every alert's text.
    alerts: Vec<String>,
    /// Every teleport message, described.
    teleports: Vec<String>,
}

impl Leg {
    /// A leg that begins with the avatar at `position`.
    fn starting_at(position: &Vector) -> Self {
        Self {
            position: position.clone(),
            pushes: Vec::new(),
            alerts: Vec::new(),
            teleports: Vec::new(),
        }
    }

    /// Records what `event` says about the leg.
    fn note(&mut self, event: &Event, agent: Uuid) {
        match event {
            Event::ObjectUpdated(object) | Event::ObjectAdded(object)
                if object.full_id.uuid() == agent =>
            {
                self.position = object.motion.position.clone();
            }
            Event::ParcelProperties(parcel) => self.pushes.push((**parcel).clone()),
            Event::AlertMessage {
                message,
                alert_info,
                ..
            } => {
                // A keyed-only alert has an empty plain message; its first
                // structured id says what arrived.
                let text = if message.trim().is_empty() {
                    alert_info
                        .first()
                        .map(|info| info.message.clone())
                        .unwrap_or_default()
                } else {
                    message.clone()
                };
                self.alerts.push(text);
            }
            Event::AgentAlertMessage { message, .. } => self.alerts.push(message.clone()),
            Event::TeleportStarted => self.teleports.push("start".to_owned()),
            Event::TeleportLocal { position, .. } => {
                self.position = Vector {
                    x: position.x(),
                    y: position.y(),
                    z: position.z(),
                };
                self.teleports.push(format!(
                    "local to {:.0}/{:.0}/{:.0}",
                    position.x(),
                    position.y(),
                    position.z()
                ));
            }
            Event::TeleportFailed { reason, .. } => {
                self.teleports.push(format!("failed: {reason}"));
            }
            _ => {}
        }
    }

    /// Whether a ban line of `kind` was pushed during the leg.
    fn collided(&self, kind: ParcelCollision) -> bool {
        self.pushes
            .iter()
            .any(|parcel| parcel.collision() == Some(kind))
    }

    /// The leg's alerts as the record shows them, repeats folded into a
    /// count: `text ×n`.
    fn describe_alerts(&self) -> String {
        let mut folded: Vec<(&str, usize)> = Vec::new();
        for alert in &self.alerts {
            match folded.last_mut() {
                Some((last, count)) if *last == alert => *count = count.saturating_add(1),
                _ => folded.push((alert, 1)),
            }
        }
        folded
            .iter()
            .map(|(alert, count)| format!("{alert} ×{count}"))
            .collect::<Vec<_>>()
            .join(" | ")
    }

    /// The leg's pushes as the record shows them, repeats folded into a count:
    /// `sequence/local id/result/collision ×n`.
    fn describe_pushes(&self) -> String {
        let mut folded: Vec<(String, usize)> = Vec::new();
        for parcel in &self.pushes {
            let shown = format!(
                "{}/{}/{:?}/{}",
                parcel.sequence_id,
                parcel.local_id.0,
                parcel.request_result,
                parcel
                    .collision()
                    .map_or_else(|| "-".to_owned(), |kind| format!("{kind:?}"))
            );
            match folded.last_mut() {
                Some((last, count)) if *last == shown => *count = count.saturating_add(1),
                _ => folded.push((shown, 1)),
            }
        }
        folded
            .iter()
            .map(|(shown, count)| format!("{shown} ×{count}"))
            .collect::<Vec<_>>()
            .join(" | ")
    }
}

/// Shows every event of the next `window` to `leg`.
///
/// # Errors
///
/// Propagates the wait's failures other than its own expected timeout.
async fn watch(
    session: &mut Session,
    window: Duration,
    leg: &mut Leg,
    agent: Uuid,
) -> Result<(), TestFailure> {
    match session
        .wait_for(window, |event| {
            leg.note(event, agent);
            None::<()>
        })
        .await
    {
        Ok(()) | Err(TestFailure::Timeout(_)) => Ok(()),
        Err(other) => Err(other),
    }
}

/// Replaces the `scope` list of the plot and gives the region a moment to
/// take it.
///
/// # Errors
///
/// Propagates the send failure.
async fn write_list(
    session: &Session,
    plot: ScopedParcelId,
    scope: ParcelAccessScope,
    entries: Vec<ParcelAccessEntry>,
) -> Result<(), TestFailure> {
    session
        .send(Command::UpdateParcelAccessList {
            local_id: plot,
            scope,
            entries,
        })
        .await?;
    tokio::time::sleep(EDIT_SETTLE).await;
    Ok(())
}

/// Reads the parcel under the 4 m square containing `point`.
///
/// # Errors
///
/// Propagates the send and wait failures.
async fn read_parcel(
    session: &mut Session,
    point: &Vector,
    sequence_id: i32,
) -> Result<ParcelInfo, TestFailure> {
    let west = (point.x / 4.0).floor() * 4.0;
    let south = (point.y / 4.0).floor() * 4.0;
    session
        .send(Command::RequestParcelProperties {
            west,
            south,
            east: west + 4.0,
            north: south + 4.0,
            sequence_id,
            snap_selection: false,
        })
        .await?;
    session
        .wait_for(LONG_TIMEOUT, |event| match event {
            Event::ParcelProperties(parcel) if parcel.sequence_id == sequence_id => {
                Some((**parcel).clone())
            }
            _ => None,
        })
        .await
}

/// Waits for the avatar `agent` in the object stream and returns where it
/// stands.
///
/// # Errors
///
/// Propagates the wait's failures.
async fn own_position(session: &mut Session, agent: Uuid) -> Result<Vector, TestFailure> {
    session
        .wait_for(LONG_TIMEOUT, |event| match event {
            Event::ObjectAdded(object) | Event::ObjectUpdated(object)
                if object.full_id.uuid() == agent =>
            {
                Some(object.motion.position.clone())
            }
            _ => None,
        })
        .await
}
