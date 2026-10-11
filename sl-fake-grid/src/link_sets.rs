//! Linksets: which prims hang off which root, and in which order.
//!
//! An [`Object`](sl_proto::Object) says what it hangs off
//! ([`parent_id`](sl_proto::Object::parent_id)) and nothing of *where among
//! its root's children* it stands. That order is the linkset's link numbers —
//! the root is 1, its children 2 and up — and it decides two things a viewer
//! can see without a script: the order the prims are sent in, and which prim
//! becomes the root when the root is delinked alone.
//!
//! Both live grids were measured (`object-link-delink`, 2026-10-11,
//! `book/src/gridspec/building.md` § Linking) and they disagree on all of it,
//! so the rules are a [`LinkPolicy`]:
//!
//! - Second Life numbers the prims a link brings in from 2 on, ahead of the
//!   ones the set had, and promotes number 2;
//! - OpenSim numbers them after the ones the set had, and promotes whichever
//!   prim its `SceneObjectGroup` lists first — a dictionary's order, which
//!   hands a new part the slot the part removed last gave up. [`Parts`] is
//!   that dictionary, and it has to be kept for every group OpenSim would
//!   have, a prim on its own included, because a slot freed by one delink is
//!   taken by a link much later.
//!
//! A set this module has never been told about — a fixture's, a rezzed
//! item's — is in the order its prims stand in the region's store, which is
//! the order they were stated in.
//!
//! Link numbers as a script reads them (`llGetLinkNumber` and the rest) are
//! `server-world-link-sets`; this is the order they will be read from.

use std::collections::BTreeMap;

use sl_proto::RegionLocalObjectId;
use sl_types::lsl::{Rotation, Vector};

use crate::imitates::{LinkPolicy, Newcomers, NoBuildAlert, Promoted, SentOrder};
use crate::object_edits::{into_frame, out_of_frame};
use crate::world::SceneFixtures;

/// The order each linkset keeps its prims in.
#[derive(Debug, Clone, Default)]
pub(crate) struct LinkSets {
    /// Each root's children in the order of their link numbers, number 2
    /// first.
    numbers: BTreeMap<RegionLocalObjectId, Vec<RegionLocalObjectId>>,
    /// Each root's part collection as OpenSim keeps it, the root included.
    parts: BTreeMap<RegionLocalObjectId, Parts>,
}

impl LinkSets {
    /// Forgets what was kept about `root`: the prim left the region, or it
    /// became a group of its own, which starts with a fresh collection.
    pub(crate) fn forget(&mut self, root: RegionLocalObjectId) {
        let _numbers = self.numbers.remove(&root);
        let _parts = self.parts.remove(&root);
    }
}

/// A .NET `Dictionary`'s slots, as far as its order of enumeration goes: an
/// added key takes the slot freed last, or a new one at the end, and the keys
/// are listed slot by slot.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Parts {
    /// The slots, an empty one where a part was removed.
    slots: Vec<Option<RegionLocalObjectId>>,
    /// The empty slots, the one freed last at the end.
    free: Vec<usize>,
}

impl Parts {
    /// A collection `parts` were added to in order.
    fn of(parts: impl IntoIterator<Item = RegionLocalObjectId>) -> Self {
        Self {
            slots: parts.into_iter().map(Some).collect(),
            free: Vec::new(),
        }
    }

    /// Adds `part`.
    fn insert(&mut self, part: RegionLocalObjectId) {
        if self.slots.contains(&Some(part)) {
            return;
        }
        let reused = self.free.pop().and_then(|slot| self.slots.get_mut(slot));
        match reused {
            Some(slot) => *slot = Some(part),
            None => self.slots.push(Some(part)),
        }
    }

    /// Removes `part`.
    fn remove(&mut self, part: RegionLocalObjectId) {
        if let Some(index) = self.slots.iter().position(|slot| *slot == Some(part))
            && let Some(slot) = self.slots.get_mut(index)
        {
            *slot = None;
            self.free.push(index);
        }
    }

    /// The parts, slot by slot.
    fn listed(&self) -> impl Iterator<Item = RegionLocalObjectId> + '_ {
        self.slots.iter().flatten().copied()
    }
}

/// Why a link was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LinkRefusal {
    /// One of the roots is not the owner's to modify.
    Unmodifiable,
    /// The set would have more prims than a linkset may.
    TooMany {
        /// How many objects the link named, the root among them.
        pieces: usize,
    },
    /// Two of the prims stand further apart than a linkset reaches.
    TooFar {
        /// How many objects the link named, the root among them.
        pieces: usize,
    },
}

impl LinkRefusal {
    /// The alert the linker is sent.
    pub(crate) const fn alert(self) -> NoBuildAlert {
        match self {
            Self::Unmodifiable => LinkPolicy::NO_EDIT_PERMISSION,
            Self::TooMany { .. } => LinkPolicy::TOO_MANY,
            Self::TooFar { .. } => LinkPolicy::TOO_FAR,
        }
    }

    /// What the alert's `AlertInfo` states beside its key. Second Life's
    /// `LinkFailed` counts the objects named and the ones it could have
    /// linked — one fewer, in every refusal measured — and numbers its
    /// reason: 1 for too far apart, 2 for too many.
    pub(crate) fn parameters(self) -> Vec<(&'static str, usize)> {
        let (reason, pieces) = match self {
            Self::Unmodifiable => return Vec::new(),
            Self::TooFar { pieces } => (1, pieces),
            Self::TooMany { pieces } => (2, pieces),
        };
        vec![
            ("linkable_pieces", pieces.saturating_sub(1)),
            ("reason", reason),
            ("total_pieces", pieces),
        ]
    }
}

/// The children of `root` in the order of their link numbers, number 2
/// first.
#[must_use]
pub(crate) fn children_in_order(
    world: &SceneFixtures,
    root: RegionLocalObjectId,
) -> Vec<RegionLocalObjectId> {
    let children: Vec<RegionLocalObjectId> = world
        .objects
        .iter()
        .filter(|object| object.parent_id == root)
        .map(|object| object.local_id)
        .collect();
    let mut ordered: Vec<RegionLocalObjectId> = world
        .links
        .numbers
        .get(&root)
        .into_iter()
        .flatten()
        .copied()
        .filter(|child| children.contains(child))
        .collect();
    // A child no link of this session's made: after the ones it did, in
    // the order the store states them.
    for child in children {
        if !ordered.contains(&child) {
            ordered.push(child);
        }
    }
    ordered
}

/// How many prims the linkset `root` is the root of has.
#[must_use]
pub(crate) fn prims_of(world: &SceneFixtures, root: RegionLocalObjectId) -> usize {
    children_in_order(world, root).len().saturating_add(1)
}

/// The children of `root` in the order OpenSim's part collection lists
/// them.
fn parts_in_order(world: &SceneFixtures, root: RegionLocalObjectId) -> Vec<RegionLocalObjectId> {
    let children = children_in_order(world, root);
    let mut ordered: Vec<RegionLocalObjectId> = world
        .links
        .parts
        .get(&root)
        .into_iter()
        .flat_map(Parts::listed)
        .filter(|part| children.contains(part))
        .collect();
    for child in children {
        if !ordered.contains(&child) {
            ordered.push(child);
        }
    }
    ordered
}

/// The part collection of the group `root` is the root of, made of the
/// root and its children where nothing was kept of it yet.
fn parts_mut(world: &mut SceneFixtures, root: RegionLocalObjectId) -> &mut Parts {
    if !world.links.parts.contains_key(&root) {
        let children = children_in_order(world, root);
        let _none = world
            .links
            .parts
            .insert(root, Parts::of(std::iter::once(root).chain(children)));
    }
    world.links.parts.entry(root).or_default()
}

/// Where `prim` stands and how it is turned in the region's own frame,
/// which for a child is not what its update states.
fn region_placement(
    world: &SceneFixtures,
    prim: RegionLocalObjectId,
) -> Option<(Vector, Rotation)> {
    let object = world
        .objects
        .iter()
        .find(|object| object.local_id == prim)?;
    let own = (
        object.motion.position.clone(),
        object.motion.rotation.clone(),
    );
    let root = world
        .objects
        .iter()
        .find(|root| object.parent_id.0 != 0 && root.local_id == object.parent_id);
    Some(root.map_or_else(
        || own.clone(),
        |root| out_of_frame(&root.motion.position, &root.motion.rotation, &own.0, &own.1),
    ))
}

/// Hangs `prim` off `root` — or off nothing, for a `root` of zero — and
/// states the region placement `placed` in that frame.
fn hang(
    world: &mut SceneFixtures,
    prim: RegionLocalObjectId,
    root: RegionLocalObjectId,
    placed: &(Vector, Rotation),
    frame: Option<&(Vector, Rotation)>,
) {
    let (position, rotation) = frame.map_or_else(
        || placed.clone(),
        |frame| into_frame(&frame.0, &frame.1, &placed.0, &placed.1),
    );
    if let Some(object) = world.object_mut(prim) {
        object.parent_id = root;
        object.motion.position = position;
        object.motion.rotation = rotation;
    }
}

/// Whether the grid lets its owner take `prim` out of the set it is in:
/// Second Life refuses where the owner mask of the set's root lacks
/// modify.
pub(crate) fn may_delink(world: &SceneFixtures, prim: RegionLocalObjectId) -> bool {
    world.linking.unmodifiable.is_none() || is_modifiable(world, world.root_of(prim))
}

/// Whether the owner of `prim` may modify it.
fn is_modifiable(world: &SceneFixtures, prim: RegionLocalObjectId) -> bool {
    world.properties_of(prim).is_none_or(|record| {
        record
            .permissions
            .owner
            .contains(sl_wire::Permissions::MODIFY)
    })
}

/// Links the sets `named` are in under the set of the first: an
/// `ObjectLink`. Returns the root of the set that made, or [`None`] where
/// the link had nothing to join.
///
/// The first prim named is the root, and a prim named that is a child
/// brings its whole set, on both live grids. Where the newcomers are
/// numbered is the grid's ([`LinkPolicy::newcomers`]), and so are the
/// links it refuses — each of them whole, nothing of it linked.
///
/// # Errors
///
/// The [`LinkRefusal`] the grid being imitated answers the link with.
pub(crate) fn link(
    world: &mut SceneFixtures,
    named: &[RegionLocalObjectId],
) -> Result<Option<RegionLocalObjectId>, LinkRefusal> {
    let policy = world.linking;
    let is_prim = |world: &SceneFixtures, prim: RegionLocalObjectId| {
        world.objects.iter().any(|object| object.local_id == prim)
    };
    let Some((first, rest)) = named.split_first() else {
        return Ok(None);
    };
    if !is_prim(world, *first) {
        tracing::debug!("a link named root {first:?}, which this region does not have");
        return Ok(None);
    }
    let root = world.root_of(*first);
    let mut joining: Vec<RegionLocalObjectId> = Vec::new();
    for prim in rest {
        if !is_prim(world, *prim) {
            continue;
        }
        let of = world.root_of(*prim);
        if of != root && !joining.contains(&of) {
            joining.push(of);
        }
    }
    if joining.is_empty() {
        return Ok(None);
    }
    if policy.unmodifiable.is_some()
        && std::iter::once(&root)
            .chain(&joining)
            .any(|prim| !is_modifiable(world, *prim))
    {
        return Err(LinkRefusal::Unmodifiable);
    }
    let had = children_in_order(world, root);
    let mut newcomers: Vec<RegionLocalObjectId> = Vec::new();
    for of in &joining {
        newcomers.push(*of);
        newcomers.extend(match policy.promotes {
            // OpenSim's `LinkToGroup` walks the joining group's parts.
            Promoted::FirstOfParts => parts_in_order(world, *of),
            Promoted::LinkNumberTwo => children_in_order(world, *of),
        });
    }
    if policy
        .most_prims
        .is_some_and(|most| had.len().saturating_add(newcomers.len()) >= most)
    {
        return Err(LinkRefusal::TooMany {
            pieces: joining.len().saturating_add(1),
        });
    }
    // Where everything stands in the region, read before any of it is
    // hung elsewhere: a joining root's children are stated in its frame.
    let placed: Vec<(RegionLocalObjectId, (Vector, Rotation))> = newcomers
        .iter()
        .filter_map(|prim| Some((*prim, region_placement(world, *prim)?)))
        .collect();
    let Some(frame) = region_placement(world, root) else {
        return Ok(None);
    };
    if let Some(reach) = policy.reach_m {
        let mut stands: Vec<Vector> = vec![frame.0.clone()];
        stands.extend(
            had.iter()
                .filter_map(|prim| region_placement(world, *prim))
                .map(|(position, _rotation)| position),
        );
        let old = stands.len();
        stands.extend(placed.iter().map(|(_prim, (position, _))| position.clone()));
        // The prims the set had were within reach of each other when they
        // were linked; what is asked is whether a newcomer is of all.
        let too_far = stands.iter().enumerate().any(|(index, one)| {
            stands
                .iter()
                .skip(index.saturating_add(1).max(old))
                .any(|other| distance(one, other) > reach)
        });
        if too_far {
            return Err(LinkRefusal::TooFar {
                pieces: joining.len().saturating_add(1),
            });
        }
    }
    for (prim, placement) in &placed {
        hang(world, *prim, root, placement, Some(&frame));
    }
    for of in &joining {
        world.links.forget(*of);
    }
    let numbered = match policy.newcomers {
        Newcomers::AfterTheRoot => newcomers.iter().chain(&had).copied().collect(),
        Newcomers::AtTheEnd => had.iter().chain(&newcomers).copied().collect(),
    };
    let _previous = world.links.numbers.insert(root, numbered);
    let parts = parts_mut(world, root);
    for prim in newcomers {
        parts.insert(prim);
    }
    Ok(Some(root))
}

/// Takes the prims `named` out of their sets: an `ObjectDelink`. Returns
/// the root of every set that left changed, the prims now on their own
/// among them.
///
/// A named child leaves its set and the rest stays linked. A named root
/// leaves the rest a set of its own, under the prim the grid promotes
/// ([`LinkPolicy::promotes`]); a viewer that unlinks a whole set names
/// every prim of it, so nothing is left to promote.
pub(crate) fn delink(
    world: &mut SceneFixtures,
    named: &[RegionLocalObjectId],
) -> Vec<RegionLocalObjectId> {
    let policy = world.linking;
    let mut touched: Vec<RegionLocalObjectId> = Vec::new();
    let mut roots: Vec<RegionLocalObjectId> = Vec::new();
    for prim in named {
        let Some(object) = world.objects.iter().find(|object| object.local_id == *prim) else {
            continue;
        };
        let root = world.root_of(*prim);
        if root == *prim || object.parent_id.0 == 0 {
            if !roots.contains(prim) {
                roots.push(*prim);
            }
            continue;
        }
        let Some(placement) = region_placement(world, *prim) else {
            continue;
        };
        hang(world, *prim, RegionLocalObjectId(0), &placement, None);
        parts_mut(world, root).remove(*prim);
        if let Some(numbers) = world.links.numbers.get_mut(&root) {
            numbers.retain(|numbered| numbered != prim);
        }
        // A prim on its own is a group of its own, with a collection
        // nothing was ever removed from.
        world.links.forget(*prim);
        touched.extend([root, *prim]);
    }
    for root in roots {
        let rest = match policy.promotes {
            Promoted::FirstOfParts => parts_in_order(world, root),
            Promoted::LinkNumberTwo => children_in_order(world, root),
        };
        let Some((promoted, others)) = rest.split_first() else {
            continue;
        };
        let placed: Vec<(RegionLocalObjectId, (Vector, Rotation))> = rest
            .iter()
            .filter_map(|prim| Some((*prim, region_placement(world, *prim)?)))
            .collect();
        let parts = parts_mut(world, root);
        for prim in &rest {
            parts.remove(*prim);
        }
        let _numbers = world.links.numbers.remove(&root);
        let frame = placed
            .iter()
            .find(|(prim, _placement)| prim == promoted)
            .map(|(_prim, placement)| placement.clone());
        for (prim, placement) in &placed {
            if prim == promoted {
                hang(world, *prim, RegionLocalObjectId(0), placement, None);
            } else {
                hang(world, *prim, *promoted, placement, frame.as_ref());
            }
        }
        world.links.forget(*promoted);
        if !others.is_empty() {
            let _none = world.links.numbers.insert(*promoted, others.to_vec());
            let _none = world
                .links
                .parts
                .insert(*promoted, Parts::of(rest.iter().copied()));
        }
        touched.extend([root, *promoted]);
    }
    let mut changed: Vec<RegionLocalObjectId> = Vec::new();
    for root in touched {
        if !changed.contains(&root) {
            changed.push(root);
        }
    }
    changed
}

/// The prims of the set `root` is the root of, in the order the grid
/// being imitated sends them ([`LinkPolicy::sent`]).
pub(crate) fn sent_in_order(
    world: &SceneFixtures,
    root: RegionLocalObjectId,
) -> Vec<RegionLocalObjectId> {
    let mut prims: Vec<RegionLocalObjectId> = std::iter::once(root)
        .chain(children_in_order(world, root))
        .collect();
    if world.linking.sent == SentOrder::RootLast {
        prims.reverse();
    }
    prims
}

/// Notes the order of a linkset that was copied: `children` under `copy`,
/// in the order of the original's.
pub(crate) fn note_copy(
    world: &mut SceneFixtures,
    copy: RegionLocalObjectId,
    children: Vec<RegionLocalObjectId>,
) {
    if children.is_empty() {
        return;
    }
    let _none = world.links.numbers.insert(copy, children);
}

/// How far apart two places are, in metres.
fn distance(one: &Vector, other: &Vector) -> f32 {
    let (x, y, z) = (one.x - other.x, one.y - other.y, one.z - other.z);
    z.mul_add(z, x.mul_add(x, y * y)).sqrt()
}

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;
    use sl_proto::RegionLocalObjectId;
    use sl_types::lsl::Vector;

    use super::{LinkRefusal, Parts, children_in_order, delink, link, region_placement};
    use crate::imitates::ImitatedGrid;
    use crate::world::{SceneFixtures, box_prim};

    /// The region-local ids of the four prims of [`world`], `A` to `D`.
    const A: RegionLocalObjectId = RegionLocalObjectId(10);
    /// See [`A`].
    const B: RegionLocalObjectId = RegionLocalObjectId(11);
    /// See [`A`].
    const C: RegionLocalObjectId = RegionLocalObjectId(12);
    /// See [`A`].
    const D: RegionLocalObjectId = RegionLocalObjectId(13);

    /// A region of four free cubes a metre and a half apart, on `grid`.
    fn world(grid: ImitatedGrid) -> SceneFixtures {
        let mut world = SceneFixtures::new();
        world.linking = grid.link_policy();
        world.edit = grid.edit_policy();
        for (index, local_id) in [A, B, C, D].into_iter().enumerate() {
            let along = f32::from(u8::try_from(index).unwrap_or(u8::MAX)) * 1.5;
            world.objects.push(box_prim(
                local_id,
                uuid::Uuid::from_u128(u128::from(local_id.0)).into(),
                uuid::Uuid::from_u128(1).into(),
                Vector {
                    x: 100.0 + along,
                    y: 100.0,
                    z: 30.0,
                },
                Vector {
                    x: 0.5,
                    y: 0.5,
                    z: 0.5,
                },
            ));
        }
        world
    }

    /// The set under `root`, root first, in the order of its link numbers.
    fn numbers(world: &SceneFixtures, root: RegionLocalObjectId) -> Vec<RegionLocalObjectId> {
        std::iter::once(root)
            .chain(children_in_order(world, root))
            .collect()
    }

    /// Delinks the root of a set again and again and returns the prims in
    /// the order they were promoted in, the first root first.
    fn peel(world: &mut SceneFixtures, root: RegionLocalObjectId) -> Vec<RegionLocalObjectId> {
        let mut order = vec![root];
        let mut root = root;
        loop {
            let rest = children_in_order(world, root);
            let _changed = delink(world, &[root]);
            let Some(promoted) = rest.into_iter().find(|prim| world.root_of(*prim) == *prim) else {
                return order;
            };
            order.push(promoted);
            root = promoted;
        }
    }

    /// The dictionary OpenSim keeps a group's parts in hands a new part the
    /// slot the part removed last gave up.
    #[test]
    fn a_part_takes_the_slot_freed_last() {
        let mut parts = Parts::of([A, B, C]);
        parts.remove(B);
        parts.remove(C);
        parts.insert(D);
        parts.insert(B);
        assert_eq!(parts.listed().collect::<Vec<_>>(), vec![A, B, D]);
    }

    /// Second Life, as measured: the prims a link brings are numbered from 2
    /// on ahead of the ones the set had, a set that joins keeps its own
    /// order, and a root delinked alone is replaced by number 2.
    #[test]
    fn second_life_numbers_newcomers_first_and_promotes_number_two() -> Result<(), LinkRefusal> {
        let mut world = world(ImitatedGrid::SecondLife);
        let _root = link(&mut world, &[A, B, C, D])?;
        assert_eq!(numbers(&world, A), vec![A, B, C, D]);
        assert_eq!(peel(&mut world, A), vec![A, B, C, D]);

        let _root = link(&mut world, &[A, B])?;
        let _root = link(&mut world, &[A, C])?;
        assert_eq!(numbers(&world, A), vec![A, C, B]);
        assert_eq!(peel(&mut world, A), vec![A, C, B]);

        let _root = link(&mut world, &[A, B])?;
        let _root = link(&mut world, &[C, D])?;
        let _root = link(&mut world, &[A, C])?;
        assert_eq!(numbers(&world, A), vec![A, C, D, B]);
        assert_eq!(peel(&mut world, A), vec![A, C, D, B]);
        Ok(())
    }

    /// OpenSim, as measured — the same three sets, after the link and the
    /// delink of two prims the census begins with: newcomers are numbered
    /// last, and the prim promoted is the first of a collection whose order
    /// is the history of its slots.
    #[test]
    fn opensim_numbers_newcomers_last_and_promotes_the_first_part() -> Result<(), LinkRefusal> {
        let mut world = world(ImitatedGrid::OpenSim);
        let _root = link(&mut world, &[A, B])?;
        let _changed = delink(&mut world, &[A, B]);

        let _root = link(&mut world, &[A, B, C, D])?;
        assert_eq!(numbers(&world, A), vec![A, B, C, D]);
        assert_eq!(peel(&mut world, A), vec![A, B, C, D]);

        let _root = link(&mut world, &[A, B])?;
        let _root = link(&mut world, &[A, C])?;
        assert_eq!(numbers(&world, A), vec![A, B, C]);
        assert_eq!(peel(&mut world, A), vec![A, C, B]);

        let _root = link(&mut world, &[A, B])?;
        let _root = link(&mut world, &[C, D])?;
        let _root = link(&mut world, &[A, C])?;
        assert_eq!(numbers(&world, A), vec![A, B, C, D]);
        assert_eq!(peel(&mut world, A), vec![A, D, C, B]);
        Ok(())
    }

    /// A link that names a child brings the child's whole set, and one that
    /// names a single prim does nothing.
    #[test]
    fn a_named_child_brings_its_set() -> Result<(), LinkRefusal> {
        let mut world = world(ImitatedGrid::SecondLife);
        let _root = link(&mut world, &[A, B])?;
        assert_eq!(link(&mut world, &[C, B])?, Some(C));
        assert_eq!(world.root_of(A), C);
        assert_eq!(world.root_of(B), C);
        assert_eq!(link(&mut world, &[D])?, None);
        Ok(())
    }

    /// A link and a delink leave every prim where it stood in the region,
    /// whatever it hung off in between.
    #[test]
    fn a_prim_stands_where_it_stood_once_free_again() -> Result<(), LinkRefusal> {
        let mut world = world(ImitatedGrid::SecondLife);
        let before: Vec<_> = [A, B, C, D]
            .into_iter()
            .map(|prim| region_placement(&world, prim))
            .collect();
        let _root = link(&mut world, &[A, B])?;
        let _root = link(&mut world, &[C, D])?;
        let _root = link(&mut world, &[A, C])?;
        let _order = peel(&mut world, A);
        let after: Vec<_> = [A, B, C, D]
            .into_iter()
            .map(|prim| region_placement(&world, prim))
            .collect();
        assert_eq!(after, before);
        assert!(world.objects.iter().all(|object| object.parent_id.0 == 0));
        Ok(())
    }

    /// Second Life refuses a link of prims more than 64 m apart whole,
    /// whichever of them is the root; OpenSim links at any distance.
    #[test]
    fn second_life_refuses_prims_too_far_apart() -> Result<(), LinkRefusal> {
        for (grid, refused) in [
            (ImitatedGrid::SecondLife, true),
            (ImitatedGrid::OpenSim, false),
        ] {
            let mut world = world(grid);
            // `A` lowest, `B` 48 m above it and `C` 48 m above that.
            for (prim, lift) in [(B, 48.0), (C, 96.0)] {
                if let Some(object) = world.object_mut(prim) {
                    object.motion.position = Vector {
                        x: 100.0,
                        y: 100.0,
                        z: 30.0 + lift,
                    };
                }
            }
            let from_the_middle = link(&mut world, &[B, A, C]);
            if refused {
                assert_eq!(from_the_middle, Err(LinkRefusal::TooFar { pieces: 3 }));
                assert_eq!(world.root_of(A), A);
                assert_eq!(world.root_of(C), C);
                assert_eq!(link(&mut world, &[A, B])?, Some(A));
            } else {
                assert_eq!(from_the_middle?, Some(B));
            }
        }
        Ok(())
    }
}
