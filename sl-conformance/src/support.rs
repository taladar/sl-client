//! Shared scaffolding so the concrete cases stay short and consistent.
//!
//! This is the "Phase 0" helper layer of the test roadmap (`TEST_ROADMAP.md`):
//!
//! - standard [timeout constants](self#constants) tuned for live grids,
//! - a [`send_then_wait`] send-then-await-matching-event combinator,
//! - [grid-gating helpers](is_opensim) for per-grid conditionals,
//! - [`check`] / [`check_eq`] assertion helpers that wrap
//!   [`TestFailure::Assertion`] with a clear message,
//! - [metric-name helpers](secs_metric) for the conventional `_secs` / `_count`
//!   suffixes,
//! - a [`fixtures`] module of well-known ids.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use sl_client_tokio::{
    Camera, Command, ControlFlags, CreateGroupParams, Diagnostic, Event, GroupKey, InventoryItem,
    InventoryKey, LindenAmount, Object, ObjectKey, RegionLocalObjectId, Rotation, ScopedObjectId,
    TaskInventoryItem, Uuid, Vector, XferListing, pcode, prim_flags,
};

use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;

/// Generous timeout for the initial region handshake; covers an aditi login,
/// MFA, and a slow region cross.
pub const REGION_TIMEOUT: Duration = Duration::from_secs(60);

/// Default timeout for a single request/reply round-trip over the circuit.
pub const REPLY_TIMEOUT: Duration = Duration::from_secs(30);

/// Longer timeout for replies that stream, page, or arrive over a CAPS/HTTP
/// path rather than a single UDP packet.
pub const LONG_TIMEOUT: Duration = Duration::from_secs(60);

/// Send `command`, then await the first event for which `predicate` returns
/// `Some`, up to `timeout`.
///
/// The common shape of almost every case: issue one command and wait for its
/// reply. Wraps [`Session::send`] + [`Session::wait_for`].
///
/// # Errors
///
/// Propagates [`Session::send`] and [`Session::wait_for`] errors (a closed
/// channel, a timeout, or an intervening disconnect).
pub async fn send_then_wait<T, P>(
    session: &mut Session,
    command: Command,
    timeout: Duration,
    predicate: P,
) -> Result<T, TestFailure>
where
    P: FnMut(&Event) -> Option<T>,
{
    session.send(command).await?;
    session.wait_for(timeout, predicate).await
}

/// Whether the test is running on the local OpenSim grid.
///
/// Cases that branch on grid (e.g. asserting an OpenSim-only field, or marking
/// partial on aditi) read more clearly with these than with a bare `match`.
#[must_use]
pub const fn is_opensim(grid: Grid) -> bool {
    matches!(grid, Grid::Opensim)
}

/// Whether the test is running on the Second Life beta (aditi) grid.
#[must_use]
pub const fn is_aditi(grid: Grid) -> bool {
    matches!(grid, Grid::Aditi)
}

/// Whether the test is running against the in-process fake grid, **whichever
/// live grid that one is imitating**.
///
/// Nearly every use of this is asking "is this a grid whose content and
/// policies this workspace wrote", which is true of both flavours. A case that
/// really does mean one of them compares against [`Grid::FakeSl`] or
/// [`Grid::FakeOpensim`] itself — and if it is doing that to decide *what to
/// assert*, it should be declaring the flavour it needs in
/// [`GridTest::grids`](crate::registry::GridTest::grids) instead.
#[must_use]
pub const fn is_fake(grid: Grid) -> bool {
    grid.is_fake()
}

/// Whether the region's contents are something this workspace declares, so a
/// case may *require* what it expects to find rather than record its absence.
///
/// True of OpenSim (whose Default Region holds this workspace's rezzed test
/// object) and of the fake grid (whose region is the fixture catalogue);
/// false of Second Life, where the landing region is whatever it is. The
/// distinction is what separates a case's `check` branch from its
/// [`TestContext::mark_partial`] one.
#[must_use]
pub const fn content_is_ours(grid: Grid) -> bool {
    is_opensim(grid) || is_fake(grid)
}

/// Assert `condition`, failing the test with `message` as a
/// [`TestFailure::Assertion`] when it does not hold.
///
/// # Errors
///
/// Returns [`TestFailure::Assertion`] when `condition` is false.
pub fn check(condition: bool, message: &str) -> Result<(), TestFailure> {
    if condition {
        Ok(())
    } else {
        Err(TestFailure::Assertion(message.to_owned()))
    }
}

/// Assert that `actual` equals `expected`, failing with a formatted
/// `field: expected … got …` message naming the field under test.
///
/// Prefer this over [`check`] when comparing an observed protocol field to a
/// known value, so the failure record says what was wrong, not just that
/// something was.
///
/// # Errors
///
/// Returns [`TestFailure::Assertion`] when `actual != expected`.
pub fn check_eq<T>(field: &str, actual: &T, expected: &T) -> Result<(), TestFailure>
where
    T: PartialEq + core::fmt::Debug,
{
    if actual == expected {
        Ok(())
    } else {
        Err(TestFailure::Assertion(format!(
            "{field}: expected {expected:?}, got {actual:?}"
        )))
    }
}

/// The conventional name for a timing metric: `<base>_secs`, which the reporter
/// renders as "lower is better".
#[must_use]
pub fn secs_metric(base: &str) -> String {
    format!("{base}_secs")
}

/// The conventional name for a count metric: `<base>_count`.
#[must_use]
pub fn count_metric(base: &str) -> String {
    format!("{base}_count")
}

/// The [`created_item_announcement`] value for the legacy UDP
/// `UpdateCreateInventoryItem`, which is what both live grids answer a take
/// with (`object-rez-derez`, 2026-10-10).
pub const ANNOUNCED_LEGACY: &str = "update-create-inventory-item";

/// The [`created_item_announcement`] value for the event-queue
/// `BulkUpdateInventory`: not what either live grid answers a take with, and
/// what a fake grid built to announce in bulk does.
pub const ANNOUNCED_BULK: &str = "bulk-update-inventory";

/// The [`UploadObservation::announced_for`] value for an upload the grid
/// completed **without announcing the item at all** — the HTTP completion, and
/// nothing else.
///
/// A real answer rather than a missing one: a viewer that waits for a push
/// after an upload waits forever against such a grid, so "nothing arrived" is
/// the measurement the fake grid has to be able to imitate.
pub const ANNOUNCED_NOTHING: &str = "none";

/// How long [`observe_upload`] keeps watching the event stream **after** the
/// upload's own completion, before concluding the grid announced nothing.
///
/// Long enough for a `BulkUpdateInventory` riding the event queue to arrive on
/// the next long-poll (the announcement's slow road — see
/// `sl_fake_grid::InventoryAnnouncement`), which is the shape a shorter window
/// would silently miss and record as [`ANNOUNCED_NOTHING`].
pub const UPLOAD_SETTLE: Duration = Duration::from_secs(15);

/// One inventory announcement seen around an upload: which message shape it
/// was, and the items it named.
#[derive(Debug, Clone)]
pub struct Announcement {
    /// [`ANNOUNCED_LEGACY`] or [`ANNOUNCED_BULK`].
    pub shape: &'static str,
    /// The items the message carried — several, for a bulk update.
    pub items: Vec<InventoryKey>,
}

/// What an upload's own completion said: the asset the grid stored, and the
/// inventory item it minted (`None` for a save onto an existing item, and for a
/// baked texture, which names no item at all).
#[derive(Debug, Clone, Copy)]
pub struct UploadCompletion {
    /// The stored asset's id.
    pub new_asset: Uuid,
    /// The item the upload created, when it created one.
    pub new_inventory_item: Option<Uuid>,
}

/// What a grid did around one upload completing: the completion itself, and
/// every inventory announcement that arrived with it.
#[derive(Debug, Clone)]
pub struct UploadObservation {
    /// The upload's own completion, or the grid's refusal.
    pub outcome: Result<UploadCompletion, String>,
    /// The announcements seen, in arrival order — before the completion as well
    /// as after it, because the two travel by different roads (a UDP push and
    /// an HTTP response) and neither order is guaranteed.
    pub announcements: Vec<Announcement>,
    /// How long the completion took to arrive.
    pub elapsed: Duration,
}

impl UploadObservation {
    /// The announcement shapes that named `item`, joined with `+`, or
    /// [`ANNOUNCED_NOTHING`] when the grid announced it in no shape at all —
    /// the value a case records as a metric.
    #[must_use]
    pub fn announced_for(&self, item: InventoryKey) -> String {
        let shapes: Vec<&str> = self
            .announcements
            .iter()
            .filter(|announcement| announcement.items.contains(&item))
            .map(|announcement| announcement.shape)
            .collect();
        if shapes.is_empty() {
            ANNOUNCED_NOTHING.to_owned()
        } else {
            shapes.join("+")
        }
    }
}

/// Sends nothing; waits for an upload to complete and **keeps watching** for
/// [`UPLOAD_SETTLE`] afterwards, reporting every inventory announcement the
/// grid pushed around it.
///
/// This is the measuring instrument for "what does a grid send after an upload,
/// besides the HTTP response". A plain [`Session::wait_for`] cannot answer it:
/// it discards every event its predicate rejects, so an announcement that
/// arrived *before* the completion — perfectly possible, the completion being
/// an HTTP response while a legacy announcement is a UDP push — would be eaten
/// on the way past and the grid recorded as silent.
///
/// The caller sends the upload command first; this waits for the
/// [`Event::AssetUploaded`] / [`Event::AssetUploadFailed`] that ends it. A
/// refusal is returned in [`UploadObservation::outcome`] rather than failing,
/// because a grid that declines an upload is a measurement too.
///
/// # Errors
///
/// Propagates [`Session::wait_for`]'s timeout when the completion never
/// arrives, and any intervening disconnect.
pub async fn observe_upload(
    session: &mut Session,
    timeout: Duration,
) -> Result<UploadObservation, TestFailure> {
    let started = Instant::now();
    let mut announcements = Vec::new();
    let outcome = session
        .wait_for(timeout, |event| {
            collect_announcement(&mut announcements, event);
            match event {
                // The item the client assembled from the completion is not
                // part of what an upload case measures — that is the *grid's*
                // announcements, and the client files this one precisely
                // because no grid announces it.
                Event::AssetUploaded {
                    new_asset,
                    new_inventory_item,
                    ..
                } => Some(Ok(UploadCompletion {
                    new_asset: *new_asset,
                    new_inventory_item: *new_inventory_item,
                })),
                Event::AssetUploadFailed { reason } => Some(Err(reason.clone())),
                _other => None,
            }
        })
        .await?;
    let elapsed = started.elapsed();

    announcements.extend(drain_announcements(session, UPLOAD_SETTLE).await?);

    Ok(UploadObservation {
        outcome,
        announcements,
        elapsed,
    })
}

/// Watches the event stream for `window`, discarding everything but the
/// inventory announcements, which it returns.
///
/// Two uses, both about attribution. As [`observe_upload`]'s settle it is what
/// gives a late announcement time to arrive; called between two steps of a case
/// it is what stops the *first* step's trailing announcement from being counted
/// as the second's — the same "drain to quiet before measuring" the terrain
/// cases do for their patch floods.
///
/// # Errors
///
/// Propagates an intervening disconnect; the window elapsing is the normal exit
/// and is not an error.
pub async fn drain_announcements(
    session: &mut Session,
    window: Duration,
) -> Result<Vec<Announcement>, TestFailure> {
    let mut seen = Vec::new();
    // A wait whose predicate never matches, so it always ends in its own
    // timeout — the point is the events it sees on the way.
    match session
        .wait_for(window, |event| {
            collect_announcement(&mut seen, event);
            Option::<()>::None
        })
        .await
    {
        Ok(()) | Err(TestFailure::Timeout(_)) => Ok(seen),
        Err(other) => Err(other),
    }
}

/// Records `event` in `seen` when it is one of the two inventory announcement
/// shapes, and ignores it otherwise.
fn collect_announcement(seen: &mut Vec<Announcement>, event: &Event) {
    match event {
        Event::InventoryItemCreated { item, .. } => seen.push(Announcement {
            shape: ANNOUNCED_LEGACY,
            items: vec![item.item_id],
        }),
        Event::InventoryBulkUpdate { items, .. } => seen.push(Announcement {
            shape: ANNOUNCED_BULK,
            items: items.iter().map(|item| item.item_id).collect(),
        }),
        _other => {}
    }
}

/// Waits for the grid to announce an inventory item it just created, whichever
/// of the **two shapes** it uses, and says which one arrived.
///
/// Both live grids send the legacy UDP `UpdateCreateInventoryItem`
/// ([`Event::InventoryItemCreated`]) for a take; a `BulkUpdateInventory` over
/// the event queue ([`Event::InventoryBulkUpdate`]) is how Second Life
/// announces other inventory changes and what a fake grid can be built to
/// announce a take with. A case that waits for only one of them reports a
/// take that worked as unacknowledged against a grid that uses the other,
/// which is why this is one helper rather than a `match` copied into every
/// case that takes something.
///
/// `item_type` is the `LLAssetType` code the item must carry, because a bulk
/// update announces every object it touched and only one of them is the item
/// the caller asked for.
///
/// # Errors
///
/// Propagates [`Session::wait_for`]'s timeout when neither shape arrives.
pub async fn created_item_announcement(
    session: &mut Session,
    timeout: Duration,
    item_type: i32,
) -> Result<(&'static str, InventoryItem), TestFailure> {
    session
        .wait_for(timeout, |event| match event {
            Event::InventoryItemCreated { item, .. } if i32::from(item.item_type) == item_type => {
                Some((ANNOUNCED_LEGACY, item.clone()))
            }
            Event::InventoryBulkUpdate { items, .. } => items
                .iter()
                .find(|item| i32::from(item.item_type) == item_type)
                .cloned()
                .map(|item| (ANNOUNCED_BULK, item)),
            _other => None,
        })
        .await
}

/// One round of the group-departure confirmation poll: how long to wait for
/// either the `AgentDropGroup` or a refreshed membership list before
/// re-requesting agent data (the overall wait is bounded by [`REPLY_TIMEOUT`]).
const GROUP_DROP_POLL: Duration = Duration::from_secs(5);

/// How long each group-creation attempt waits for a `CreateGroupReply` before
/// re-sending with a fresh per-attempt name suffix.
const GROUP_CREATE_ATTEMPT_WINDOW: Duration = Duration::from_secs(15);

/// How many creation attempts before concluding the grid genuinely refuses.
const GROUP_CREATE_ATTEMPTS: u32 = 3;

/// After a *retried* group creation answered, how long to keep watching for a
/// second `CreateGroupReply` — the late answer to an earlier attempt, which
/// means that attempt did create a group after all and nothing else will ever
/// use it.
///
/// Only entered when a retry actually happened, because the wait discards the
/// events that arrive during it (see [`dispose_of_orphan_groups`]).
const GROUP_CREATE_ORPHAN_WINDOW: Duration = Duration::from_secs(10);

/// Confirm `session`'s agent is no longer a member of `group_id`.
///
/// The membership-list confirmation differs per grid: OpenSim pushes an
/// `AgentDropGroup` ([`Event::DroppedFromGroup`])
/// after a leave or ejection, while Second Life sends no drop message for
/// either — the reference viewer re-requests agent data
/// (`sendAgentDataUpdateRequest`) and trusts the refreshed membership list.
/// Accept whichever arrives first: watch for the drop while re-requesting
/// ([`Command::RequestAgentDataUpdate`]) until the membership list no longer
/// contains the group.
///
/// # Errors
///
/// Propagates send/wait failures; times out with [`TestFailure::Timeout`]
/// when neither signal arrives within [`REPLY_TIMEOUT`].
pub async fn confirm_group_departure(
    session: &mut Session,
    group_id: sl_client_tokio::GroupKey,
) -> Result<(), TestFailure> {
    let started = Instant::now();
    loop {
        session.send(Command::RequestAgentDataUpdate).await?;
        match session
            .wait_for(GROUP_DROP_POLL, |event| match event {
                Event::DroppedFromGroup { group_id: dropped } if *dropped == group_id => Some(()),
                Event::GroupMemberships(groups)
                    if !groups.iter().any(|entry| entry.group_id == group_id) =>
                {
                    Some(())
                }
                _ => None,
            })
            .await
        {
            Ok(()) => return Ok(()),
            Err(TestFailure::Timeout(_)) if started.elapsed() < REPLY_TIMEOUT => {}
            Err(other) => return Err(other),
        }
    }
}

/// Waits for the next object **this avatar rezzed**: a root prim
/// ([`Object::parent_id`] zero) owned by the session's agent whose scoped id is
/// not in `seen` — the region's objects settled before the rez.
///
/// "The next object not seen before" is not enough on a live grid. A Second
/// Life sandbox streams other residents' rezzes the whole time — rezzers
/// spawning temporary prims every few seconds — so the first unseen
/// [`Event::ObjectAdded`] after our `RezObject` is often somebody else's. A case
/// that took it would then select, edit, derez or write the task inventory of
/// an object it does not own, which the grid drops without a word, while its
/// own cube sits untouched until the parcel's auto-return. That was the whole
/// "Second Life drops our `RezScript`" mystery.
///
/// Nor is "the next unseen object of ours": a neighbouring region streams its
/// objects down a child circuit that opens after the agent's own region has
/// settled, and one of those may be ours too — OpenSim's neighbours hold this
/// workspace's fixtures. The cases rez where the avatar stands, so only an
/// object of the agent's own region counts.
///
/// Ownership is read from the object's per-viewer
/// [`OBJECT_YOU_OWNER`](prim_flags::OBJECT_YOU_OWNER) flag as well as its
/// owner id: Second Life sends the owner id only for objects that carry a sound
/// or particles, so a plain cube arrives with a nil owner.
///
/// When no such object appears within `timeout`, the inner `Err` says why as
/// far as the grid told us: a refused rez is answered with an
/// [`Event::AlertMessage`] (no-build land, a full parcel), and its text is the
/// reason a case records.
///
/// # Errors
///
/// Propagates [`Session::wait_for`] failures other than the timeout, and fails
/// with [`TestFailure::Assertion`] when login reported no agent id.
pub async fn wait_for_own_new_object(
    session: &mut Session,
    seen: &HashSet<ScopedObjectId>,
    timeout: Duration,
) -> Result<Result<Object, String>, TestFailure> {
    /// One thing the wait can see: our object, or an alert worth reporting.
    enum Sighting {
        /// The object we rezzed.
        Own(Box<Object>),
        /// An alert the grid sent meanwhile — the likely refusal.
        Alert(String),
    }
    let owner = session
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?
        .uuid();
    let root = session.circuit_id();
    let started = Instant::now();
    let mut alert: Option<String> = None;
    loop {
        let remaining = timeout.saturating_sub(started.elapsed());
        let sighting = session
            .wait_for(remaining, |event| match event {
                Event::ObjectAdded(object)
                    if is_own(object, owner)
                        && root.is_none_or(|root| object.circuit == root)
                        && object.parent_id == RegionLocalObjectId(0)
                        && !seen.contains(&object.scoped_id()) =>
                {
                    Some(Sighting::Own(object.clone()))
                }
                Event::ObjectAdded(object)
                    if object.parent_id == RegionLocalObjectId(0)
                        && !seen.contains(&object.scoped_id()) =>
                {
                    tracing::debug!(
                        id = %object.full_id.uuid(),
                        owner = %object.owner_id,
                        flags = object.update_flags,
                        pcode = object.pcode,
                        x = object.motion.position.x,
                        y = object.motion.position.y,
                        z = object.motion.position.z,
                        "an unseen object that is not ours appeared while waiting for our rez"
                    );
                    None
                }
                Event::AlertMessage {
                    message,
                    alert_info,
                    ..
                } => Some(Sighting::Alert(
                    alert_info
                        .first()
                        .filter(|_| message.is_empty())
                        .map_or_else(|| message.clone(), |info| info.message.clone()),
                )),
                _ => None,
            })
            .await;
        match sighting {
            Ok(Sighting::Own(object)) => return Ok(Ok(*object)),
            Ok(Sighting::Alert(text)) => alert = Some(text),
            Err(TestFailure::Timeout(_)) => {
                return Ok(Err(alert.map_or_else(
                    || format!("no object of ours appeared within {}s", timeout.as_secs()),
                    |text| format!("no object of ours appeared; the grid said: {text}"),
                )));
            }
            Err(other) => return Err(other),
        }
    }
}

/// Drains the region's initial object-update burst before a case rezzes
/// anything, returning every scoped id sighted (so a later rez can be told
/// apart — see [`wait_for_own_new_object`]) and the **anchor** to rez against.
///
/// The burst ends once no new [`Event::ObjectAdded`] has arrived for `idle`,
/// or after `window` overall.
///
/// The anchor is grid-dependent. Where the region's contents are ours
/// ([`content_is_ours`]) it is the first primitive streamed — the workspace's
/// test object on OpenSim, a fixture on the fake grid — which is what the
/// cases have always placed against. On Second Life the first primitive is
/// *anybody's*, anywhere in the region or in a neighbour, so a rez placed
/// against it lands on whatever parcel that happens to be. There the anchor is
/// **our own avatar**, which the login put on a parcel that allows building
/// (`build_location` in the fixtures, see
/// [`GridTest::rezzes_objects`](crate::registry::GridTest::rezzes_objects)).
/// `None` when the burst held no such object.
///
/// A `build_position` (the fixtures' build location) overrides all of that:
/// the operator named the spot that allows building, and the avatar's arrival
/// point may not be it — a telehub or a landing point redirects a login.
///
/// # Errors
///
/// Propagates [`Session::wait_for`] failures other than the idle timeout.
pub async fn settle_scene(
    session: &mut Session,
    grid: Grid,
    build_position: Option<Vector>,
    window: Duration,
    idle: Duration,
) -> Result<(HashSet<ScopedObjectId>, Option<Vector>), TestFailure> {
    let settled = settle_scene_with_avatar(session, grid, build_position, window, idle).await?;
    Ok((settled.seen, settled.anchor))
}

/// What [`settle_scene_with_avatar`] found.
#[derive(Debug, Clone)]
pub struct SettledScene {
    /// Every scoped id sighted while the scene settled.
    pub seen: HashSet<ScopedObjectId>,
    /// The anchor to rez against (see [`settle_scene`]).
    pub anchor: Option<Vector>,
    /// Where our own avatar stands once the scene has settled — after the
    /// move to the build location, where there was one. `None` when the
    /// avatar never appeared in the object stream.
    pub avatar: Option<Vector>,
    /// A root prim streamed from a neighbouring region's child circuit, if
    /// one came into view: an object the agent can see and is not in the
    /// region of.
    pub neighbour_object: Option<ObjectKey>,
}

/// [`settle_scene`], also reporting where our own avatar ended up — for a
/// case that places something *beside the avatar* rather than against the
/// anchor.
///
/// # Errors
///
/// As [`settle_scene`].
pub async fn settle_scene_with_avatar(
    session: &mut Session,
    grid: Grid,
    build_position: Option<Vector>,
    window: Duration,
    idle: Duration,
) -> Result<SettledScene, TestFailure> {
    let own = session.agent_id().map(|agent| agent.uuid());
    let mut seen = HashSet::new();
    let mut anchor: Option<Vector> = None;
    let mut avatar: Option<Vector> = None;
    let root = session.circuit_id();
    let mut neighbour_object: Option<ObjectKey> = None;
    drain_scene(session, window, idle, &mut seen, |object| {
        if Some(object.full_id.uuid()) == own {
            avatar = Some(object.motion.position.clone());
        }
        if neighbour_object.is_none()
            && root.is_some_and(|root| root != object.circuit)
            && object.pcode == pcode::PRIMITIVE
            && object.parent_id == RegionLocalObjectId(0)
        {
            neighbour_object = Some(object.full_id);
        }
        let anchors = if content_is_ours(grid) {
            object.pcode == pcode::PRIMITIVE
        } else {
            object.pcode == pcode::AVATAR && Some(object.full_id.uuid()) == own
        };
        if anchor.is_none() && anchors {
            anchor = Some(object.motion.position.clone());
        }
    })
    .await?;
    let Some(build) = build_position else {
        return Ok(SettledScene {
            seen,
            anchor,
            avatar,
            neighbour_object,
        });
    };
    // The build location: rez right there. When the login did not land the
    // avatar on it (a telehub or a landing point redirects a login — Mauve's
    // sends every arrival to its hub), the avatar is moved there first: a
    // grid ignores a rez far from the avatar without a word (Second Life did
    // at about 70 m). The scene around it is then settled again so an object
    // of ours already standing there is not mistaken for the new rez.
    let from = avatar.ok_or_else(|| {
        TestFailure::Assertion("our own avatar never appeared in the object stream".to_owned())
    })?;
    let arrived = if within(&from, &build, ARRIVAL_RADIUS_M) {
        from
    } else {
        let arrived = walk_within_region(session, from, &build, |_event| {}).await?;
        // The flight ends in the air: where the avatar comes down is where
        // it stands.
        let mut landed = arrived;
        drain_scene(session, window, idle, &mut seen, |object| {
            if Some(object.full_id.uuid()) == own {
                landed = object.motion.position.clone();
            }
        })
        .await?;
        landed
    };
    // The named spot's x/y, at the avatar's own height: the fixture's height
    // is only a login hint.
    let spot = Vector {
        x: build.x,
        y: build.y,
        z: arrived.z,
    };
    // Look at it. A grid streams objects by where the agent's *camera* is, not
    // its avatar, and the camera is still where the login put it — at the
    // hub, out of range of the spot — so the rez would land and never be
    // seen. (That was the last of the "no object appeared" runs: the cubes
    // were all there.)
    session
        .send(Command::SetCamera(Camera::looking_at(
            Vector {
                x: spot.x - CAMERA_BACK_M,
                y: spot.y,
                z: spot.z + CAMERA_BACK_M,
            },
            spot.clone(),
        )))
        .await?;
    let mut standing = arrived;
    drain_scene(session, window, idle, &mut seen, |object| {
        if Some(object.full_id.uuid()) == own {
            standing = object.motion.position.clone();
        }
    })
    .await?;
    Ok(SettledScene {
        seen,
        anchor: Some(spot),
        avatar: Some(standing),
        neighbour_object,
    })
}

/// Records every [`Event::ObjectAdded`] into `seen` until none has arrived
/// for `idle`, or `window` has passed, showing `observe` each of them and
/// every [`Event::ObjectUpdated`] that arrives meanwhile.
async fn drain_scene(
    session: &mut Session,
    window: Duration,
    idle: Duration,
    seen: &mut HashSet<ScopedObjectId>,
    mut observe: impl FnMut(&Object),
) -> Result<(), TestFailure> {
    let started = Instant::now();
    loop {
        let remaining = window.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return Ok(());
        }
        match session
            .wait_for(remaining.min(idle), |event| match event {
                Event::ObjectAdded(object) => {
                    observe(object);
                    Some(object.scoped_id())
                }
                // An object already sighted that moved: shown, and not what
                // the idle gap waits for.
                Event::ObjectUpdated(object) => {
                    observe(object);
                    None
                }
                _ => None,
            })
            .await
        {
            Ok(sighted) => {
                seen.insert(sighted);
            }
            Err(TestFailure::Timeout(_)) => return Ok(()),
            Err(other) => return Err(other),
        }
    }
}

/// Moves the avatar from `from` towards `position` in its current region the
/// way a viewer's autopilot does, until it is within `ARRIVAL_RADIUS_M`, and
/// returns where it stopped, showing every event that arrives on the way to
/// `observe` (the steering's own waits would otherwise discard them): face
/// the target ([`Command::SetRotation`]), fly forwards
/// ([`Command::SetControls`]) — at a nudge's pace for the last
/// `SLOWDOWN_RADIUS_M`, or the flight overshoots between two updates —
/// re-aim on every update of our own avatar, and let go.
///
/// Not a teleport: a region with a telehub (Mauve on aditi) or a parcel with a
/// landing point redirects every teleport, in-region ones included, away from
/// the spot that allows building. Not the simulator's `autopilot` generic
/// message either: Second Life ignores it (the reference viewer's autopilot is
/// its own steering, `LLAgent::autoPilot`). Flying keeps the route clear of
/// whatever stands on the ground in between.
///
/// # Errors
///
/// Returns [`TestFailure::Assertion`] when the login reported no agent id or
/// the avatar is still short of `position` after the walk's time budget, and
/// propagates the sends' and waits' failures.
pub async fn walk_within_region(
    session: &mut Session,
    from: Vector,
    position: &Vector,
    observe: impl FnMut(&Event),
) -> Result<Vector, TestFailure> {
    let (stopped, arrived) =
        steer_towards(session, from, position, Gait::Flying, WALK_TIMEOUT, observe).await?;
    if arrived {
        Ok(stopped)
    } else {
        Err(TestFailure::Assertion(format!(
            "could not move the avatar to the build location {:.0}/{:.0} (stuck at {:.0}/{:.0})",
            position.x, position.y, stopped.x, stopped.y
        )))
    }
}

/// How [`steer_towards`] moves the avatar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gait {
    /// Fly, steering on the region plane only and letting the height be what
    /// it becomes. On Second Life a forward flight climbs several metres a
    /// second.
    Flying,
    /// Fly, pushing down when more than `HEIGHT_SLACK_M` above the target's
    /// height and up when as far below it — for a case whose answer depends on
    /// the height.
    FlyingLevel,
    /// Walk. Whatever stands on the ground in between is in the way, so this
    /// is for a short stretch a case has to cover on foot — a parcel line, say,
    /// which a grid may treat differently for an avatar on the ground.
    Walking,
}

/// How far from its target's height a [`Gait::FlyingLevel`] flight may drift
/// before it corrects.
const HEIGHT_SLACK_M: f32 = 1.5;

/// Moves the avatar from `from` towards `position` for at most `budget`,
/// steering the way [`walk_within_region`] does, and returns where it stopped
/// and whether that is within `ARRIVAL_RADIUS_M` of `position`.
///
/// Not arriving is an answer here rather than a failure: this is the journey
/// of a case that measures what stops an avatar — a ban line, a parcel that
/// turns it back — so it steers for the whole budget, lets go, and reports.
///
/// # Errors
///
/// Returns [`TestFailure::Assertion`] when the login reported no agent id, and
/// propagates the sends' and waits' failures.
pub async fn steer_towards(
    session: &mut Session,
    from: Vector,
    position: &Vector,
    gait: Gait,
    budget: Duration,
    mut observe: impl FnMut(&Event),
) -> Result<(Vector, bool), TestFailure> {
    let agent = session
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?
        .uuid();
    let started = Instant::now();
    tracing::info!(
        from_x = from.x,
        from_y = from.y,
        to_x = position.x,
        to_y = position.y,
        "steering the avatar"
    );
    // The last update of our own avatar, its velocity, and when it came. A
    // grid sends none while the velocity holds — Second Life went three
    // seconds without one at 14 m/s — so, as a viewer does, the position
    // between updates is reckoned from the last one.
    let mut reported = from;
    let mut velocity = Vector {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };
    let mut reported_at = Instant::now();
    loop {
        let since = reported_at.elapsed().as_secs_f32();
        let current = Vector {
            x: velocity.x.mul_add(since, reported.x),
            y: velocity.y.mul_add(since, reported.y),
            z: velocity.z.mul_add(since, reported.z),
        };
        let speed = velocity.x.hypot(velocity.y);
        let arrived = within(&current, position, ARRIVAL_RADIUS_M);
        if arrived || started.elapsed() >= budget {
            tracing::info!(
                x = current.x,
                y = current.y,
                z = current.z,
                arrived,
                secs = started.elapsed().as_secs_f32(),
                "the avatar's journey ended"
            );
            session
                .send(Command::SetControls(ControlFlags::empty()))
                .await?;
            return Ok((current, arrived));
        }
        let half_yaw = (position.y - current.y).atan2(position.x - current.x) / 2.0;
        let facing = Rotation {
            x: 0.0,
            y: 0.0,
            z: half_yaw.sin(),
            s: half_yaw.cos(),
        };
        session
            .send(Command::SetRotation {
                body: facing.clone(),
                head: facing,
            })
            .await?;
        // Near the target the avatar is nudged — and, while it is still
        // carrying the speed of the approach, not pushed at all: a flight
        // arrives at a dozen metres a second, a nudge on top of that does not
        // slow it, and it then crosses the arrival radius between two updates
        // and swings back and forth over the spot until the budget runs out.
        let forwards = if !within(&current, position, SLOWDOWN_RADIUS_M) {
            ControlFlags::AT_POS
        } else if speed > NUDGE_BELOW_M_PER_S {
            ControlFlags::empty()
        } else {
            ControlFlags::NUDGE_AT_POS
        };
        let off_height = current.z - position.z;
        let controls = match gait {
            Gait::Walking => forwards,
            Gait::FlyingLevel if off_height > HEIGHT_SLACK_M => {
                forwards | ControlFlags::FLY | ControlFlags::UP_NEG
            }
            Gait::FlyingLevel if off_height < -HEIGHT_SLACK_M => {
                forwards | ControlFlags::FLY | ControlFlags::UP_POS
            }
            Gait::FlyingLevel | Gait::Flying => forwards | ControlFlags::FLY,
        };
        session.send(Command::SetControls(controls)).await?;
        match session
            .wait_for(STEER_INTERVAL, |event| {
                observe(event);
                match event {
                    Event::ObjectUpdated(object) | Event::ObjectAdded(object)
                        if object.full_id.uuid() == agent =>
                    {
                        Some((
                            object.motion.position.clone(),
                            object.motion.velocity.clone(),
                        ))
                    }
                    _ => None,
                }
            })
            .await
        {
            Ok((moved, moving)) => {
                tracing::debug!(x = moved.x, y = moved.y, z = moved.z, "the avatar moved");
                reported = moved;
                velocity = moving;
                reported_at = Instant::now();
            }
            Err(TestFailure::Timeout(_)) => {}
            Err(other) => return Err(other),
        }
    }
}

/// How far behind and above the build spot the camera looks at it from.
const CAMERA_BACK_M: f32 = 6.0;

/// How often the walk re-aims when no update of our own avatar arrives.
const STEER_INTERVAL: Duration = Duration::from_millis(250);

/// How close to the build location the walk slows to a nudge.
const SLOWDOWN_RADIUS_M: f32 = 15.0;

/// The horizontal speed, in metres a second, above which an avatar inside the
/// slowdown radius is left to coast rather than nudged on.
const NUDGE_BELOW_M_PER_S: f32 = 2.0;

/// How close to the build location the walk has to bring the avatar before it
/// rezzes there.
const ARRIVAL_RADIUS_M: f32 = 4.0;

/// Whether `a` and `b` are within `radius` metres of each other horizontally.
fn within(a: &Vector, b: &Vector, radius: f32) -> bool {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    dx.mul_add(dx, dy * dy) <= radius * radius
}

/// How long the flight to the build location may take — a region's diagonal,
/// with room for a slow region.
const WALK_TIMEOUT: Duration = Duration::from_secs(120);

/// Whether the agent `owner` owns `object`, by its owner id or by the
/// per-viewer [`OBJECT_YOU_OWNER`](prim_flags::OBJECT_YOU_OWNER) flag.
#[must_use]
pub fn is_own(object: &Object, owner: Uuid) -> bool {
    object.owner_id == owner || object.update_flags & prim_flags::OBJECT_YOU_OWNER != 0
}

/// Where the group a membership/messaging case operates on came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupSource {
    /// A throwaway group created fresh for this run (the OpenSim default: free
    /// and disposable).
    Created,
    /// A pre-made group configured via [`crate::fixtures`] and reused across runs
    /// (the Second Life path: avoids the per-run L$100 group-creation fee and the
    /// founder group-slot churn).
    Premade,
}

impl GroupSource {
    /// The metric label recorded for this source.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Premade => "premade",
        }
    }
}

/// The group a membership/messaging case will operate on, plus where it came
/// from and (for a freshly created group) how long creation took.
#[derive(Clone, Copy, Debug)]
pub struct MembershipGroup {
    /// The group to drive the case against.
    pub group_id: GroupKey,
    /// Whether it was created for this run or reused from fixtures.
    pub source: GroupSource,
    /// The create round-trip time, present only when [`source`](Self::source) is
    /// [`GroupSource::Created`].
    pub create_rtt: Option<Duration>,
}

/// Resolve the `index`-th group a group case should operate on.
///
/// Prefers the [pre-made group](crate::fixtures) configured at `index` for the
/// grid — reusing stable groups avoids Second Life's per-run L$100
/// group-creation fee and the founder group-slot churn (an emptied SL group
/// purges only ~48 h after dropping below two members). When none is configured
/// at that position (the norm on the throwaway OpenSim grid), it creates a fresh
/// open-enrollment group with the given `name` and `charter`, leaving the primary
/// as founder/owner.
///
/// `index` lets a case that needs more than one distinct group take them by
/// position: the membership/messaging cases use `0`, while
/// [`super::cases::chat_invite_accept_decline`] uses `0` and `1`.
///
/// The returned group is one the **primary** owns or belongs to, so the primary
/// can drive group traffic on it; a secondary then joins it.
///
/// Keep `name` **between 4 and 35 characters** (the reference viewer's
/// `DB_GROUP_NAME_MIN_LEN`/`DB_GROUP_NAME_STR_LEN`): Second Life's server
/// silently discards a `CreateGroupRequest` whose name is over the limit — no
/// `CreateGroupReply` at all, observed live on aditi — while OpenSim accepts
/// any length. The cases use short `"slc <tag> <millis>"` names for this.
///
/// # Errors
///
/// Returns [`TestFailure`] if creating the group fails (channel closed, timeout,
/// disconnect, or the grid reporting failure).
pub async fn membership_group(
    ctx: &mut TestContext,
    index: usize,
    name: &str,
    charter: &str,
) -> Result<MembershipGroup, TestFailure> {
    if let Some(group_id) = ctx.premade_group(index) {
        return Ok(MembershipGroup {
            group_id,
            source: GroupSource::Premade,
            create_rtt: None,
        });
    }

    let session = ctx.primary();
    let created_at = Instant::now();
    // Second Life silently drops a `CreateGroupRequest` that arrives too soon
    // after another create by the same agent (observed live: a case needing
    // two groups back-to-back got only one `CreateGroupReply`), so retry with
    // a per-attempt name suffix. The wait accepts whichever attempt's reply
    // arrives first; a retry after a merely-slow first reply can still leave an
    // orphan single-member group, which the disposal below names and leaves.
    let mut attempt: u32 = 0;
    let (group_id, create_ok, create_message) = loop {
        attempt = attempt.saturating_add(1);
        let attempt_name = if attempt == 1 {
            name.to_owned()
        } else {
            format!("{name} a{attempt}")
        };
        session
            .send(Command::CreateGroup(CreateGroupParams {
                name: attempt_name,
                charter: charter.to_owned(),
                show_in_list: false,
                insignia_id: None,
                membership_fee: LindenAmount(0),
                open_enrollment: true,
                allow_publish: false,
                mature_publish: false,
            }))
            .await?;
        match session
            .wait_for(GROUP_CREATE_ATTEMPT_WINDOW, |event| match event {
                Event::CreateGroupResult {
                    group_id,
                    success,
                    message,
                } => Some((*group_id, *success, message.clone())),
                _ => None,
            })
            .await
        {
            Ok(reply) => break reply,
            Err(TestFailure::Timeout(_)) if attempt < GROUP_CREATE_ATTEMPTS => {}
            Err(other) => return Err(other),
        }
    };
    let create_rtt = created_at.elapsed();
    check(
        create_ok,
        &format!("group creation failed: {create_message}"),
    )?;

    // A retry that raced a merely-slow first reply leaves an orphan group behind
    // — on Second Life that is L$100 and a founder group slot per orphan. Give
    // the late reply a moment to arrive so the orphan is at least named, and ask
    // to leave it (a group its founder has left drops to zero members and the
    // grid purges it).
    if attempt > 1 {
        let orphans = dispose_of_orphan_groups(ctx.primary(), group_id).await?;
        if !orphans.is_empty() {
            let listed = orphans
                .iter()
                .map(|orphan| orphan.uuid().to_string())
                .collect::<Vec<_>>()
                .join(",");
            tracing::warn!(
                "group creation retried {attempt} times and left {} orphan group(s): {listed}",
                orphans.len()
            );
            let metrics = ctx.metrics();
            metrics.set(
                &count_metric("orphan_group"),
                i64::try_from(orphans.len()).unwrap_or(i64::MAX),
            );
            metrics.set("orphan_groups", listed);
        }
    }

    Ok(MembershipGroup {
        group_id,
        source: GroupSource::Created,
        create_rtt: Some(create_rtt),
    })
}

/// Collect the groups an earlier creation attempt created after all — every
/// `CreateGroupReply` other than `kept`'s that still arrives within
/// [`GROUP_CREATE_ORPHAN_WINDOW`] — and ask to leave each one.
///
/// The departure is issued but not awaited: the point is to stop owning the
/// orphan, and a second wait here would discard yet more of the caller's events.
/// The ids are returned so the caller can name them in the log and the record;
/// on a grid that refuses to let a lone owner leave, that log line is the only
/// trace an operator has to clean up by hand.
///
/// Note this *does* consume events: [`Session::wait_for`] drops what does not
/// match, so this runs only on the retry path, where an orphan is possible.
///
/// # Errors
///
/// Propagates a [`Session::send`] failure; a timeout is the expected, quiet
/// outcome (no late reply, hence no orphan).
async fn dispose_of_orphan_groups(
    session: &mut Session,
    kept: GroupKey,
) -> Result<Vec<GroupKey>, TestFailure> {
    let mut orphans: Vec<GroupKey> = Vec::new();
    let started = Instant::now();
    loop {
        let remaining = GROUP_CREATE_ORPHAN_WINDOW.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            break;
        }
        match session
            .wait_for(remaining, |event| match event {
                Event::CreateGroupResult {
                    group_id,
                    success,
                    message: _,
                } if *success && *group_id != kept => Some(*group_id),
                _ => None,
            })
            .await
        {
            Ok(orphan) => {
                if !orphans.contains(&orphan) {
                    orphans.push(orphan);
                }
            }
            Err(TestFailure::Timeout(_)) => break,
            Err(other) => return Err(other),
        }
    }
    for orphan in &orphans {
        session.send(Command::LeaveGroup(*orphan)).await?;
    }
    Ok(orphans)
}

/// Well-known ids and labels reused across cases.
pub mod fixtures {
    use sl_client_tokio::{AgentKey, TextureKey, Uuid};

    use crate::context::TestFailure;

    /// The standard SL/OpenSim "plywood" default texture, present on any stock
    /// grid; used by `asset-decode` as a guaranteed-fetchable asset. Taken from
    /// the protocol crate rather than restated, so a case fetches the id the
    /// renderer falls back to.
    pub const PLYWOOD_TEXTURE: Uuid = sl_client_tokio::DEFAULT_PRIM_TEXTURE;

    /// The local OpenSim "Default Region" UUID, from this workspace's
    /// `Regions/Regions.ini` (the region at grid location 1000,1000).
    ///
    /// OpenSim-only and specific to the local test grid; Second Life regions
    /// have their own ids.
    pub const OPENSIM_DEFAULT_REGION: &str = "11111111-2222-3333-4444-555555555555";

    /// The conventional credentials-file label for the estate-owner avatar that
    /// estate/land-edit cases log in as (`--avatar estate-owner`).
    pub const ESTATE_OWNER_LABEL: &str = "estate-owner";

    /// The local OpenSim secondary test avatar (`Friend Tester`), created with a
    /// fixed UUID on this workspace's grid. The `avatar-properties` case reads
    /// *this* avatar's profile as a known "other avatar" on OpenSim — the account
    /// exists (so the profile service answers) and need not be logged in. Second
    /// Life has no such built-in second avatar, so the aditi run reads the
    /// `other_avatar` configured in `fixtures.aditi.toml` instead.
    pub const OPENSIM_SECONDARY_AVATAR: &str = "bbbbbbbb-aaaa-cccc-dddd-000000000001";

    /// The OpenSim secondary test avatar as a typed [`AgentKey`].
    ///
    /// # Errors
    ///
    /// Returns [`TestFailure::Assertion`] if the constant is malformed.
    pub fn opensim_secondary_avatar() -> Result<AgentKey, TestFailure> {
        Ok(AgentKey::from(uuid(OPENSIM_SECONDARY_AVATAR)?))
    }

    /// Parse a well-known UUID literal, failing the test on a malformed value.
    ///
    /// # Errors
    ///
    /// Returns [`TestFailure::Assertion`] if `literal` is not a valid UUID.
    pub fn uuid(literal: &str) -> Result<Uuid, TestFailure> {
        literal
            .parse()
            .map_err(|_invalid| TestFailure::Assertion(format!("bad fixture uuid: {literal}")))
    }

    /// The plywood default texture as a typed [`TextureKey`].
    #[must_use]
    pub fn plywood_texture() -> TextureKey {
        TextureKey::from(PLYWOOD_TEXTURE)
    }
}

/// Waits for `task`'s parsed task-inventory listing — the answer to a
/// [`Command::FetchTaskInventory`] — and fails the step with the parse error
/// when the listing arrived but did not parse, rather than timing out waiting
/// for a listing the session will never surface.
///
/// # Errors
///
/// Returns [`TestFailure::Assertion`] when the listing failed to parse, and the
/// wait's own failure when neither answer arrives within `timeout`.
pub async fn wait_for_task_listing(
    session: &mut Session,
    task: ObjectKey,
    timeout: Duration,
) -> Result<Vec<TaskInventoryItem>, TestFailure> {
    session
        .wait_for(timeout, |event| match event {
            Event::TaskInventoryContents {
                task: got, items, ..
            } if *got == task => Some(Ok(items.clone())),
            Event::XferDecodeFailed {
                file: XferListing::TaskInventory { task: got, .. },
                error,
                ..
            } if *got == task => Some(Err(error.clone())),
            _other => None,
        })
        .await?
        .map_err(|error| {
            TestFailure::Assertion(format!(
                "the prim's task inventory listing failed to parse: {error}"
            ))
        })
}

#[cfg(test)]
mod tests {
    use super::{
        check, check_eq, content_is_ours, count_metric, fixtures, is_aditi, is_fake, is_opensim,
        secs_metric,
    };
    use crate::context::TestFailure;
    use crate::grid::Grid;
    use pretty_assertions::assert_eq;

    /// `check` passes a true condition and fails a false one with its message.
    #[test]
    fn check_reports_message() {
        assert!(matches!(check(true, "ok"), Ok(())));
        assert!(matches!(
            check(false, "boom"),
            Err(TestFailure::Assertion(message)) if message == "boom"
        ));
    }

    /// `check_eq` formats field, expected, and actual on mismatch.
    #[test]
    fn check_eq_formats_mismatch() {
        assert!(matches!(check_eq("n", &3_i32, &3_i32), Ok(())));
        assert!(matches!(
            check_eq("max_agents", &10_i32, &40_i32),
            Err(TestFailure::Assertion(message))
                if message == "max_agents: expected 40, got 10"
        ));
    }

    /// Metric-name helpers apply the conventional suffixes.
    #[test]
    fn metric_name_suffixes() {
        assert_eq!(secs_metric("region_info"), "region_info_secs");
        assert_eq!(count_metric("folders"), "folders_count");
    }

    /// Grid-gating predicates are mutually exclusive, and only the grid whose
    /// region is somebody else's is not ours to make claims about.
    #[test]
    fn grid_gating() {
        assert!(is_opensim(Grid::Opensim));
        assert!(!is_aditi(Grid::Opensim));
        assert!(is_aditi(Grid::Aditi));
        assert!(!is_opensim(Grid::Aditi));
        assert!(is_fake(Grid::FakeSl));
        assert!(!is_fake(Grid::Opensim));
        assert!(content_is_ours(Grid::Opensim));
        assert!(content_is_ours(Grid::FakeSl));
        assert!(!content_is_ours(Grid::Aditi));
    }

    /// The fixture UUID constants parse, and the typed accessor matches.
    #[test]
    fn fixtures_parse() -> Result<(), crate::context::TestFailure> {
        let _region = fixtures::uuid(fixtures::OPENSIM_DEFAULT_REGION)?;
        assert!(matches!(fixtures::uuid("not-a-uuid"), Err(_failure)));
        assert_eq!(
            fixtures::plywood_texture().uuid(),
            fixtures::PLYWOOD_TEXTURE
        );
        Ok(())
    }
}

/// How long to let the diagnostic channel settle after `LoggedOut` arrives, so
/// a `LogoutReply`-timeout diagnostic (recorded on a background task in the
/// same run-loop tick) is visible before it is read.
const DIAGNOSTIC_GRACE: Duration = Duration::from_millis(500);

/// What one logout looked like from the client.
#[derive(Debug, Clone, Copy)]
pub struct Logout {
    /// From the request to [`Event::LoggedOut`], in seconds.
    pub seconds: f64,
    /// Whether the grid sent a `LogoutReply` (rather than the client timing
    /// out).
    pub reply_received: bool,
}

/// Request a logout on `session` and watch it through to [`Event::LoggedOut`].
///
/// Both a real `LogoutReply` and the client's logout-timeout fallback surface
/// the same [`Event::LoggedOut`]; only a
/// [`Diagnostic::ExpectedReplyMissing`] for `"Logout"` distinguishes them, so
/// this reads the diagnostics the logout added to tell them apart.
///
/// The session's run loop has ended when this returns; the caller still owns
/// the session (the runner logs the primary out, which is then a no-op).
///
/// # Errors
///
/// Propagates the send's failure and [`Session::wait_for`]'s — a logout
/// answered by a bare disconnect, or by nothing within [`REPLY_TIMEOUT`].
pub async fn log_out(session: &mut Session) -> Result<Logout, TestFailure> {
    let already = session.diagnostics().len();
    let started = Instant::now();
    session.send(Command::Logout).await?;
    // `wait_for` treats an intervening `Disconnected` as a failure unless the
    // predicate consumes it, so a logout answered by a bare unsolicited
    // disconnect (rather than a clean `LoggedOut`) fails.
    session
        .wait_for(REPLY_TIMEOUT, |event| {
            matches!(event, Event::LoggedOut).then_some(())
        })
        .await?;
    let seconds = started.elapsed().as_secs_f64();
    // The timeout-fallback diagnostic is recorded just before `LoggedOut` on a
    // separate task; let it settle before reading.
    tokio::time::sleep(DIAGNOSTIC_GRACE).await;
    let reply_received = !session
        .diagnostics()
        .iter()
        .skip(already)
        .any(|diagnostic| {
            matches!(
                diagnostic,
                Diagnostic::ExpectedReplyMissing { request, .. }
                    if request == Diagnostic::LOGOUT_REQUEST
            )
        });
    Ok(Logout {
        seconds,
        reply_received,
    })
}
