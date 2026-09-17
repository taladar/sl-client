//! When a view that draws resolved names is worth rebuilding.
//!
//! Every floater here shows names the grid answers for later: a parcel's object
//! owners, an estate's managers and ban list, a landmark's region owner. A row
//! showing `(3f2a…)` has to become a name the moment the reply lands, so those
//! views need a signal that a name arrived.
//!
//! `Res<AvatarState>::is_changed()` is not that signal, though it looks like it.
//! [`AvatarState`](crate::world_api::AvatarState) is written by every avatar
//! that moves, streams in, changes appearance or is re-costed, so in a crowded
//! region it is changed on most frames — About Land rebuilt every owner and
//! access row, with a `translator.get()` and a `format!` apiece, many times a
//! second for the whole time it was open, and About Region and About Landmark
//! did the same (`viewer-audit-about-land-row-rebuild`).
//!
//! Both caches carry a revision of their own that moves only when a name does.
//! A view records the pair it resolved at and compares.

use crate::social::GroupsModel;
use crate::world_api::AvatarState;

/// The avatar and group name caches' revisions, as a view last read them.
///
/// `Default` is "never read anything", which is correct for a view built before
/// its first sync: both caches start at revision zero with nothing in them, so a
/// view that has resolved no names is not out of date until one arrives.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NameRevisions {
    /// [`AvatarState::names_revision()`].
    avatars: u64,
    /// [`GroupsModel::revision()`] — broader than names alone (it moves on any
    /// membership or active-group change too), but bounded and rare, where the
    /// resource's change tick is neither.
    groups: u64,
}

impl NameRevisions {
    /// The caches' revisions as they stand now.
    pub(crate) const fn read(avatars: &AvatarState, groups: &GroupsModel) -> Self {
        Self {
            avatars: avatars.names_revision(),
            groups: groups.revision(),
        }
    }

    /// Whether the caches have moved since this reading — taking the new one if
    /// they have, so the caller may use it as its own "already built" record.
    pub(crate) fn advance(&mut self, fresh: Self) -> bool {
        if *self == fresh {
            return false;
        }
        *self = fresh;
        true
    }
}

/// What a name-resolving view was last built from: the revision of the list it
/// draws, and the name caches' revisions when it resolved them.
///
/// Kept in a component **beside** the view rather than inside it, and nothing
/// binds on that component's change tick. Writing it must not mark the view
/// changed, because the case this exists to catch is exactly the one where a
/// name resolved, the caches moved, and *this* list's rows did not — a name for
/// somebody who appears in neither list, which is most of them. Recording the
/// reading inside the view would mark it changed and re-bind every row anyway,
/// which is the cost the gate is here to avoid.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ViewBuilt {
    /// The source list's revision.
    revision: u64,
    /// The name caches' revisions the rows were resolved at.
    names: NameRevisions,
}

impl ViewBuilt {
    /// The list revision the rows were built from.
    pub(crate) const fn revision(&self) -> u64 {
        self.revision
    }

    /// Whether the rows are worth **resolving** again — the list moved, or a
    /// name somewhere did — recording the new reading when they are.
    ///
    /// Answering `true` does not mean the rows will differ; that is settled by
    /// comparing the resolved labels, which is why this records regardless.
    pub(crate) fn due(&mut self, revision: u64, names: NameRevisions) -> bool {
        let fresh = Self { revision, names };
        if *self == fresh {
            return false;
        }
        *self = fresh;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::{NameRevisions, ViewBuilt};
    use pretty_assertions::assert_eq;

    /// A synthetic reading of the two caches.
    const fn revisions(avatars: u64, groups: u64) -> NameRevisions {
        NameRevisions { avatars, groups }
    }

    /// A reading is due once and then not again — the property that turns a
    /// per-frame rebuild into a per-event one.
    #[test]
    fn a_reading_is_due_once() {
        let mut built = ViewBuilt::default();
        assert!(built.due(1, revisions(0, 0)), "a list that moved is due");
        assert!(!built.due(1, revisions(0, 0)), "and is not due twice");

        assert!(
            built.due(1, revisions(2, 0)),
            "a resolved avatar name is due"
        );
        assert!(!built.due(1, revisions(2, 0)));
        assert!(built.due(1, revisions(2, 5)), "so is a resolved group name");
        assert!(!built.due(1, revisions(2, 5)));

        assert_eq!(built.revision(), 1, "the list revision is readable back");
        assert!(built.due(2, revisions(2, 5)));
        assert_eq!(built.revision(), 2);
    }

    /// Advancing is the same question without a list beside it, for the
    /// name-dependent values that are not a table.
    #[test]
    fn advancing_reports_only_a_move() {
        let mut seen = NameRevisions::default();
        assert!(!seen.advance(revisions(0, 0)), "nothing has happened yet");
        assert!(seen.advance(revisions(1, 0)));
        assert!(!seen.advance(revisions(1, 0)));
        assert!(seen.advance(revisions(1, 1)));
    }
}
