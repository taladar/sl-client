//! What a case rezzed, kept where a cancelled case body cannot take it along.
//!
//! Every case that rezzes clears its objects away on the way out — but only on
//! the way out. [`run_isolated`](crate::isolate::run_isolated) drops a body
//! that overruns its overall timeout, or panics, wherever it stands, so the
//! derez at its end never runs and the prims stay: sandbox debris on Second
//! Life until auto-return, and on the local OpenSim for good, skewing every
//! later measurement at the build location every rezzing case shares.
//!
//! So the record of what a session rezzed is not the case body's to keep.
//! Each [`Session`](crate::context::Session) owns a [`RezLedger`]: its
//! [`send`](crate::context::Session::send) notes every command that asks the
//! grid for a new object ([`RezLedger::note_command`]), and the forwarder that
//! drains the run loop's events — a task of its own, which lives as long as the
//! session does, whatever happens to the case — matches the objects that then
//! appear against those requests ([`RezLedger::observe`]). Once the body has
//! returned, however it returned, the runner derezzes whatever is still
//! standing ([`Session::clear_rezzed`](crate::context::Session::clear_rezzed)),
//! before the sessions log out.
//!
//! An object counts as this session's when it is a root, it is the agent's
//! (`OBJECT_YOU_OWNER`), it is new to the session's scene, and it appears
//! within [`PENDING_WINDOW`] of a rez command, within [`MATCH_RADIUS`] of where
//! that command aimed. Each command accounts for as many objects as it asked
//! for. That excludes what was already standing in the region when the agent
//! arrived (it appears before any rez), objects a script rezzes (no command
//! asked for them) and attachments (never roots); and an object that is killed,
//! taken or handed to another owner is no longer counted.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use sl_client_tokio::{
    Command, Event, FolderType, InventoryFolder, InventoryFolderKey, Object, ObjectKey,
    RegionHandle, RegionLocalObjectId, ScopedObjectId, Vector, prim_flags,
};
use tokio::time::Instant;

/// How long after a rez command an object may still appear and be counted as
/// its answer. A rez is answered within a second or two on both grids; the
/// window is generous so a loaded aditi region is not missed, and bounded so a
/// refused rez does not leave an open slot that an unrelated object of the
/// agent's, streaming in late, could fill.
pub const PENDING_WINDOW: Duration = Duration::from_secs(60);

/// How far from where a rez command aimed its object may land and still be
/// counted as its answer, in metres. A rez lands where its ray meets a surface,
/// which is near the ray's end but not at it.
pub const MATCH_RADIUS: f64 = 32.0;

/// A point in global grid metres: a region's south-west corner plus a
/// region-local position, so a rez aimed in one region and landing just over
/// a border in the next is still compared like with like.
#[derive(Debug, Clone, Copy, PartialEq)]
struct GlobalPoint {
    /// Global x, metres.
    x: f64,
    /// Global y, metres.
    y: f64,
    /// Height, metres.
    z: f64,
}

impl GlobalPoint {
    /// The global point of `position`, local to the region `region`.
    fn of(region: RegionHandle, position: &Vector) -> Self {
        let (corner_x, corner_y) = region.global_coordinates();
        Self {
            x: f64::from(corner_x) + f64::from(position.x),
            y: f64::from(corner_y) + f64::from(position.y),
            z: f64::from(position.z),
        }
    }

    /// This point moved by `offset`.
    fn offset_by(self, offset: &Vector) -> Self {
        Self {
            x: self.x + f64::from(offset.x),
            y: self.y + f64::from(offset.y),
            z: self.z + f64::from(offset.z),
        }
    }

    /// The straight-line distance to `other`, metres.
    fn distance(self, other: Self) -> f64 {
        let (dx, dy, dz) = (self.x - other.x, self.y - other.y, self.z - other.z);
        dz.mul_add(dz, dx.mul_add(dx, dy * dy)).sqrt()
    }
}

/// A rez command waiting for the object it asked for.
#[derive(Debug, Clone, Copy)]
struct PendingRez {
    /// Where the command aimed the object, when it said: `None` for a restore
    /// to the item's last position, or a duplicate of an object whose position
    /// the ledger never saw.
    target: Option<GlobalPoint>,
    /// When the command went out.
    sent: Instant,
}

/// One object a rez command of this session brought into the world.
#[derive(Debug, Clone, Copy)]
struct Rezzed {
    /// The object's key, which survives a re-login or a region change.
    key: ObjectKey,
    /// The object's id in the scene the session holds now, or `None` while it
    /// is not in it (killed, or out of view after a teleport).
    scoped: Option<ScopedObjectId>,
}

/// The ledger's contents, behind the lock.
#[derive(Debug, Default)]
struct LedgerState {
    /// Rez commands still waiting for their objects, oldest first.
    pending: Vec<PendingRez>,
    /// Objects this session rezzed and still owns.
    rezzed: Vec<Rezzed>,
    /// Where each of the agent's root objects in the scene stands, so a
    /// duplicate of one can be aimed at its copy's place.
    owned: HashMap<ScopedObjectId, GlobalPoint>,
    /// The agent's Trash (or, lacking one, its inventory root), from the login
    /// skeleton: where a derez of the leftovers files them.
    trash: Option<InventoryFolderKey>,
}

/// What a session rezzed and is still standing in its scene, as
/// [`RezLedger::leftovers`] hands it to the sweep.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Leftovers {
    /// Each object, by key, with its id in the current scene.
    pub objects: Vec<(ObjectKey, ScopedObjectId)>,
    /// The folder to derez them into, when the login skeleton named one.
    pub trash: Option<InventoryFolderKey>,
}

/// A session's record of the objects its own rez commands brought into the
/// world. Cheap to clone; every clone is the same ledger.
#[derive(Debug, Clone, Default)]
pub struct RezLedger {
    /// The shared state: written by the session's `send` and its event
    /// forwarder, read by the sweep.
    state: Arc<Mutex<LedgerState>>,
}

impl RezLedger {
    /// Run `f` over the state under the lock. A panic while the lock was held
    /// leaves the state as it was written, which is still the best record
    /// there is of what to clear, so a poisoned lock is read through.
    fn with<R>(&self, f: impl FnOnce(&mut LedgerState) -> R) -> R {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        f(&mut state)
    }

    /// Note `command` as it is sent, `region` being the agent's region then:
    /// one pending rez for each object it asks the grid to create.
    pub fn note_command(&self, command: &Command, region: Option<RegionHandle>) {
        let aimed = |position: &Vector| region.map(|region| GlobalPoint::of(region, position));
        let now = Instant::now();
        self.with(|state| {
            let targets: Vec<Option<GlobalPoint>> = match command {
                Command::RezObject { shape, .. } => vec![aimed(&shape.position)],
                Command::RezObjectFromInventory { params } => vec![aimed(&params.ray_end)],
                Command::RezObjectFromNotecard { rez } => vec![aimed(&rez.ray_end)],
                Command::RezRestoreToWorld { .. } => vec![None],
                Command::DuplicateObjects {
                    local_ids, offset, ..
                } => local_ids
                    .iter()
                    .map(|id| state.owned.get(id).map(|from| from.offset_by(offset)))
                    .collect(),
                Command::DuplicateObjectsOnRay {
                    local_ids, ray_end, ..
                } => local_ids.iter().map(|_id| aimed(ray_end)).collect(),
                _other => Vec::new(),
            };
            state.pending.extend(
                targets
                    .into_iter()
                    .map(|target| PendingRez { target, sent: now }),
            );
        });
    }

    /// Bring the ledger up to date with `event`, one of the session's events
    /// as the forwarder passes it on.
    pub fn observe(&self, event: &Event) {
        match event {
            Event::ObjectAdded(object) => self.with(|state| state.added(object)),
            Event::ObjectUpdated(object) => self.with(|state| state.updated(object)),
            Event::ObjectRemoved { local_id, .. } => self.with(|state| state.removed(*local_id)),
            Event::InventorySkeleton(folders) => {
                let trash = trash_of(folders);
                self.with(|state| state.trash = trash.or(state.trash));
            }
            _other => {}
        }
    }

    /// What this session rezzed that still stands in its scene.
    #[must_use]
    pub fn leftovers(&self) -> Leftovers {
        self.with(|state| Leftovers {
            objects: state
                .rezzed
                .iter()
                .filter_map(|rezzed| rezzed.scoped.map(|scoped| (rezzed.key, scoped)))
                .collect(),
            trash: state.trash,
        })
    }
}

impl LedgerState {
    /// An object entered the session's scene.
    fn added(&mut self, object: &Object) {
        if !is_owned_root(object) {
            return;
        }
        let place = GlobalPoint::of(object.region_handle, &object.motion.position);
        let scoped = object.scoped_id();
        self.owned.insert(scoped, place);
        // Seen again (a re-login, a region re-entered): the same object under
        // a new scene id, not a new one.
        if let Some(known) = self
            .rezzed
            .iter_mut()
            .find(|rezzed| rezzed.key == object.full_id)
        {
            known.scoped = Some(scoped);
            return;
        }
        let now = Instant::now();
        self.pending
            .retain(|pending| now.saturating_duration_since(pending.sent) <= PENDING_WINDOW);
        if let Some(index) = self.answering(place) {
            self.pending.remove(index);
            self.rezzed.push(Rezzed {
                key: object.full_id,
                scoped: Some(scoped),
            });
        }
    }

    /// Which pending rez an object appearing at `place` answers: the nearest
    /// one aimed within [`MATCH_RADIUS`] of it, else the oldest that named no
    /// aim.
    fn answering(&self, place: GlobalPoint) -> Option<usize> {
        let aimed = self
            .pending
            .iter()
            .enumerate()
            .filter_map(|(index, pending)| {
                pending.target.map(|target| (index, target.distance(place)))
            })
            .filter(|(_index, distance)| *distance <= MATCH_RADIUS)
            .min_by(|(_a, left), (_b, right)| left.total_cmp(right))
            .map(|(index, _distance)| index);
        aimed.or_else(|| {
            self.pending
                .iter()
                .position(|pending| pending.target.is_none())
        })
    }

    /// An object the session holds changed: it may have moved, or passed to
    /// another owner, after which clearing it is no longer this session's to
    /// do.
    fn updated(&mut self, object: &Object) {
        let scoped = object.scoped_id();
        if is_owned_root(object) {
            self.owned.insert(
                scoped,
                GlobalPoint::of(object.region_handle, &object.motion.position),
            );
        } else {
            self.owned.remove(&scoped);
            if object.update_flags & prim_flags::OBJECT_YOU_OWNER == 0 {
                self.rezzed.retain(|rezzed| rezzed.key != object.full_id);
            }
        }
    }

    /// An object left the session's scene. Killed, or merely out of view
    /// with its region gone; either way it cannot be addressed until it is
    /// seen again, so it is kept by key without a scene id.
    fn removed(&mut self, local_id: ScopedObjectId) {
        self.owned.remove(&local_id);
        for rezzed in &mut self.rezzed {
            if rezzed.scoped == Some(local_id) {
                rezzed.scoped = None;
            }
        }
    }
}

/// Whether `object` is a root object (not a linked child, not an attachment)
/// of the agent's own.
fn is_owned_root(object: &Object) -> bool {
    object.parent_id == RegionLocalObjectId(0)
        && object.update_flags & prim_flags::OBJECT_YOU_OWNER != 0
}

/// The Trash of a login skeleton, or its root when it has none.
fn trash_of(folders: &[InventoryFolder]) -> Option<InventoryFolderKey> {
    folders
        .iter()
        .find(|folder| folder.folder_type == FolderType::Trash.to_code())
        .or_else(|| folders.iter().find(|folder| folder.parent_id.is_none()))
        .map(|folder| folder.folder_id)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex, PoisonError};
    use std::time::Duration;

    use pretty_assertions::assert_eq;
    use sl_client_tokio::{
        Command, Event, ObjectKey, PrimShape, RegionHandle, RegionLocalObjectId, Vector, prim_flags,
    };
    use tokio::time::Instant;

    use super::{GlobalPoint, LedgerState, MATCH_RADIUS, PendingRez};
    use crate::context::TestFailure;
    use crate::fake::{FakeGridHarness, START_REGION};
    use crate::grid::Grid;
    use crate::isolate::run_and_clear;
    use crate::registry::{GridTest, TestFuture};
    use crate::support::{REGION_TIMEOUT, REPLY_TIMEOUT};

    /// A point `x` metres east of the corner of the region at the origin.
    fn east(x: f32) -> GlobalPoint {
        GlobalPoint::of(RegionHandle(0), &Vector { x, y: 0.0, z: 0.0 })
    }

    /// A pending rez aimed at `target`, sent now.
    fn pending(target: Option<GlobalPoint>) -> PendingRez {
        PendingRez {
            target,
            sent: Instant::now(),
        }
    }

    /// An object answers the nearest aimed rez within reach, else the oldest
    /// unaimed one, and nothing when every aimed rez is out of reach and none
    /// is unaimed.
    #[tokio::test]
    async fn an_object_answers_the_nearest_aimed_rez() {
        let state = LedgerState {
            pending: vec![
                pending(Some(east(0.0))),
                pending(Some(east(10.0))),
                pending(None),
            ],
            ..LedgerState::default()
        };
        assert_eq!(state.answering(east(9.0)), Some(1));
        assert_eq!(state.answering(east(1.0)), Some(0));
        assert_eq!(state.answering(east(200.0)), Some(2));

        let aimed_only = LedgerState {
            pending: vec![pending(Some(east(0.0)))],
            ..LedgerState::default()
        };
        assert_eq!(aimed_only.answering(east(200.0)), None);
    }

    /// A rez aimed just short of a region's east edge is answered by an
    /// object that lands just over it, in the neighbour.
    #[test]
    fn a_rez_across_a_border_is_compared_in_global_metres() {
        let here = RegionHandle::from_global(256_000, 256_000);
        let there = RegionHandle::from_global(256_256, 256_000);
        let aimed = GlobalPoint::of(
            here,
            &Vector {
                x: 255.0,
                y: 10.0,
                z: 30.0,
            },
        );
        let landed = GlobalPoint::of(
            there,
            &Vector {
                x: 1.0,
                y: 10.0,
                z: 30.0,
            },
        );
        assert!(aimed.distance(landed) < MATCH_RADIUS);
    }

    /// Commands that create objects leave one pending rez per object, and
    /// every other command leaves none.
    #[test]
    fn rez_commands_leave_one_pending_rez_per_object() {
        let ledger = super::RezLedger::default();
        let at = Vector {
            x: 10.0,
            y: 20.0,
            z: 30.0,
        };
        ledger.note_command(
            &Command::RezObject {
                shape: PrimShape::cube(at.clone()),
                group_id: None,
            },
            Some(RegionHandle(0)),
        );
        ledger.note_command(&Command::Logout, Some(RegionHandle(0)));
        ledger.note_command(
            &Command::DuplicateObjects {
                local_ids: vec![
                    sl_client_tokio::ScopedObjectId::new(
                        sl_client_tokio::CircuitId::default(),
                        RegionLocalObjectId(7),
                    ),
                    sl_client_tokio::ScopedObjectId::new(
                        sl_client_tokio::CircuitId::default(),
                        RegionLocalObjectId(8),
                    ),
                ],
                offset: at.clone(),
                group_id: None,
            },
            Some(RegionHandle(0)),
        );
        let targets: Vec<Option<GlobalPoint>> =
            ledger.with(|state| state.pending.iter().map(|pending| pending.target).collect());
        // The duplicated objects were never seen, so their copies' places are
        // not known.
        assert_eq!(
            targets,
            vec![Some(GlobalPoint::of(RegionHandle(0), &at)), None, None]
        );
    }

    /// How the case below ends once its cube stands.
    #[derive(Debug, Clone, Copy)]
    enum Ending {
        /// It waits until the runner's overall timeout cancels it.
        Hangs,
        /// It panics.
        Panics,
    }

    /// A case that rezzes a cube and then never gets to its derez.
    #[derive(Debug)]
    struct RezThenStop {
        /// How the body ends.
        ending: Ending,
        /// The cube's key, once it stood.
        cube: Arc<Mutex<Option<ObjectKey>>>,
    }

    impl GridTest for RezThenStop {
        fn name(&self) -> &'static str {
            "rez-then-stop"
        }

        fn description(&self) -> &'static str {
            "rezzes a cube, then hangs or panics before its derez"
        }

        fn grids(&self) -> &'static [Grid] {
            &[Grid::FakeSl, Grid::FakeOpensim]
        }

        fn run<'a>(&'a self, ctx: &'a mut crate::context::TestContext) -> TestFuture<'a> {
            Box::pin(async move {
                let session = ctx.primary();
                session.wait_for_region(REGION_TIMEOUT).await?;
                let at = Vector {
                    x: 128.0,
                    y: 140.0,
                    z: 40.0,
                };
                session
                    .send(Command::RezObject {
                        shape: PrimShape::cube(at),
                        group_id: None,
                    })
                    .await?;
                let cube = session
                    .wait_for(REPLY_TIMEOUT, |event| match event {
                        Event::ObjectAdded(object)
                            if object.parent_id == RegionLocalObjectId(0)
                                && object.update_flags & prim_flags::OBJECT_YOU_OWNER != 0 =>
                        {
                            Some(object.full_id)
                        }
                        _other => None,
                    })
                    .await?;
                *self.cube.lock().unwrap_or_else(PoisonError::into_inner) = Some(cube);
                match self.ending {
                    Ending::Hangs => std::future::pending::<()>().await,
                    Ending::Panics => std::panic::resume_unwind(Box::new("stopped short")),
                }
                Ok(())
            })
        }
    }

    /// Run a [`RezThenStop`] ending `ending` on `flavour`, and check that its
    /// cube stood and that nothing of it is left once the case is over.
    async fn leaves_nothing(flavour: Grid, ending: Ending) -> Result<(), TestFailure> {
        let case = RezThenStop {
            ending,
            cube: Arc::new(Mutex::new(None)),
        };
        let harness = FakeGridHarness::start(flavour).await?;
        let mut ctx = harness.context(&case).await?;
        let outcome = run_and_clear(&case, &mut ctx, Duration::from_secs(20)).await;
        match (ending, &outcome) {
            (Ending::Hangs, Err(TestFailure::Timeout(_)))
            | (Ending::Panics, Err(TestFailure::Panic(_))) => {}
            (_ending, other) => {
                return Err(TestFailure::Assertion(format!(
                    "the case was meant to stop short as {ending:?}, and ended {other:?}"
                )));
            }
        }
        let cube = case
            .cube
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .ok_or_else(|| TestFailure::Assertion("the cube never stood".to_owned()))?;
        let standing = harness
            .grid()
            .with_region_world(START_REGION, |world| world.object_by_key(cube).is_some())
            .map_err(|error| TestFailure::State(error.to_string()))?;
        let (_metrics, _completeness, _note, primary, _secondary, _tertiary) = ctx.into_parts();
        primary.logout().await?;
        if standing {
            return Err(TestFailure::Assertion(format!(
                "{cube} still stands after the case on {flavour}"
            )));
        }
        Ok(())
    }

    /// A body cancelled by its overall timeout leaves no object behind, on
    /// the flavour whose derez of an object is a kill of every prim.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_timed_out_case_leaves_no_object() -> Result<(), TestFailure> {
        leaves_nothing(Grid::FakeSl, Ending::Hangs).await
    }

    /// A body that panics leaves no object behind, on the flavour that
    /// ignores an `ObjectDelete`, so it is the derez into the Trash that took
    /// it away.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_panicked_case_leaves_no_object() -> Result<(), TestFailure> {
        leaves_nothing(Grid::FakeOpensim, Ending::Panics).await
    }
}
