//! The agent's group memberships and the group it is wearing.
//!
//! Group id to display name, the active (worn) group, and a revision stamp
//! bumped on every change so a view rebuilds only when something moved.

use std::collections::BTreeMap;

use crate::short_id;
use bevy::prelude::*;
use sl_client_bevy::{Command, GroupKey, GroupMembership, SlCommand, TextureKey, Uuid};

/// The point in a frame at which [`GroupsModel`] is settled for the frame.
///
/// The model is filled by the People surface's group list, which sits well
/// above the world; the avatar name tags, which draw the active group's title,
/// sit inside it. Neither may name the other's systems, so the tag composer
/// orders itself after this set and the ingest declares itself a member of it.
/// A set is the only vocabulary the two share.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GroupsSystems {
    /// This frame's `SlEvent` group traffic — memberships, resolved names,
    /// notice preferences — has been folded into [`GroupsModel`].
    Ingested,
}

/// The pure groups model: the agent's group memberships keyed by group id (to its
/// display name), the active (worn) group, and a revision stamp bumped on every
/// change so the view rebuilds only when something actually moved. Fed solely from
/// the event stream. The list and its actions need only the name; the membership
/// record's powers / contribution belong to the (out-of-scope) profile.
#[derive(Resource, Debug, Default)]
pub struct GroupsModel {
    /// The agent's groups, by group id, mapped to the group's display name.
    groups: BTreeMap<GroupKey, String>,
    /// Names of **other** groups the agent is not a member of, resolved on
    /// demand (`UUIDGroupNameRequest` → [`SlSessionEvent::GroupNames`], or a
    /// group profile). Kept separate from [`groups`](Self::groups), which is the
    /// authoritative membership set; [`group_name`](Self::group_name) falls back
    /// to this so a group-owned parcel / object shows a name, not a UUID.
    resolved: BTreeMap<GroupKey, String>,
    /// Whether the agent accepts notices from each group — retained (unlike the
    /// display name, which the list needs) for the group profile floater's
    /// membership toggle, which has no other source for the login-time value.
    accept_notices: BTreeMap<GroupKey, bool>,
    /// Each member group's insignia (texture id), from the login-time
    /// `AgentGroupDataUpdate` — the source the group-notice toast
    /// (`group_notice`) reads the notice's group image from.
    insignia: BTreeMap<GroupKey, TextureKey>,
    /// The currently-active (worn) group, if any.
    active: Option<GroupKey>,
    /// The own agent's active group **title** (e.g. `"Officer"`), from the
    /// same `ActiveGroupChanged` push; `None` when no group is active or the
    /// title is empty.
    own_title: Option<String>,
    /// Bumped on each mutation; the view compares its last-built value to skip an
    /// unchanged rebuild.
    revision: u64,
}

impl GroupsModel {
    /// Bump the revision after a mutation.
    pub(crate) const fn touch(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    /// Replace the membership set from an `AgentGroupDataUpdate`
    /// ([`SlSessionEvent::GroupMemberships`](sl_client_bevy::SlSessionEvent::GroupMemberships)) — the wire message carries the
    /// agent's **full** group list, so it is authoritative and replaces the cache
    /// wholesale. The active group is left untouched (it is tracked separately from
    /// [`SlSessionEvent::ActiveGroupChanged`](sl_client_bevy::SlSessionEvent::ActiveGroupChanged)).
    pub fn apply_memberships(&mut self, memberships: &[GroupMembership]) {
        self.groups.clear();
        self.accept_notices.clear();
        self.insignia.clear();
        for membership in memberships {
            self.groups
                .insert(membership.group_id, membership.group_name.clone());
            self.accept_notices
                .insert(membership.group_id, membership.accept_notices);
            self.insignia
                .insert(membership.group_id, membership.group_insignia_id);
        }
        self.touch();
    }

    /// The insignia texture of a member `group`, if known — the group-notice toast
    /// (`group_notice`) reads it to show the notice's group image. A nil
    /// texture (a group with no insignia) is reported as `None`.
    #[must_use]
    pub fn group_insignia(&self, group: GroupKey) -> Option<TextureKey> {
        self.insignia
            .get(&group)
            .copied()
            .filter(|key| *key != TextureKey::from(Uuid::nil()))
    }

    /// Whether the agent accepts notices from `group`, if the agent is a member —
    /// the group profile floater's membership toggle seeds from this (the
    /// login-time value is not otherwise available to a floater opened later).
    #[must_use]
    pub fn accepts_notices(&self, group: GroupKey) -> Option<bool> {
        self.accept_notices.get(&group).copied()
    }

    /// The display name of `group` — the agent's own membership name, else a
    /// name resolved on demand ([`note_resolved_name`](Self::note_resolved_name)),
    /// else `None` (the caller falls back to the id and can request a resolve).
    pub fn group_name(&self, group: GroupKey) -> Option<&str> {
        self.groups
            .get(&group)
            .or_else(|| self.resolved.get(&group))
            .map(String::as_str)
    }

    /// Whether the agent is a member of `group` — a membership test that, unlike
    /// [`group_name`](Self::group_name), does **not** consider the on-demand
    /// resolved-name cache (a resolved non-member group must not read as a member).
    #[must_use]
    pub fn is_member(&self, group: GroupKey) -> bool {
        self.groups.contains_key(&group)
    }

    /// Request `group`'s name (`UUIDGroupNameRequest`) if it is not already known
    /// — the shared resolve path every group-name display site uses so a
    /// non-member group's name fills the cache instead of showing a UUID forever.
    /// Call at a discrete event (a floater open, a selection change), not per
    /// frame; the reply folds into the `resolved` cache.
    pub fn request_name(&self, group: GroupKey, commands: &mut MessageWriter<SlCommand>) {
        if self.group_name(group).is_none() {
            commands.write(SlCommand(Command::RequestGroupNames(vec![group])));
        }
    }

    /// Fold a resolved name for a non-member `group` into the on-demand cache.
    /// Public so any group-name display site can seed the shared cache from a
    /// name it learned (an IM session, a profile) rather than keeping its own.
    pub fn note_resolved_name(&mut self, group: GroupKey, name: &str) {
        if name.is_empty() || self.groups.contains_key(&group) {
            return;
        }
        if self.resolved.get(&group).map(String::as_str) != Some(name) {
            self.resolved.insert(group, name.to_owned());
            self.touch();
        }
    }

    /// The agent's group ids, in the map's stable id order — the build
    /// floater's set-group cycle walks these (with "none" between the wrap).
    #[must_use]
    pub fn group_ids(&self) -> Vec<GroupKey> {
        self.groups.keys().copied().collect()
    }

    /// Set the active (worn) group, bumping the revision only on a real change.
    pub fn set_active(&mut self, active: Option<GroupKey>, title: &str) {
        let title = if title.is_empty() {
            None
        } else {
            Some(title.to_owned())
        };
        if self.active != active || self.own_title != title {
            self.active = active;
            self.own_title = title;
            self.touch();
        }
    }

    /// The active (worn) group, if any. [`ordered`](Self::ordered) already
    /// marks it on each membership row; this is for a surface listing groups
    /// from somewhere *else* — the group picker's directory search, which must
    /// mark a result the same way whichever source found it.
    #[must_use]
    pub const fn active(&self) -> Option<GroupKey> {
        self.active
    }

    /// The list revision — a view stores the value it last built at and
    /// rebuilds when it advances.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// The own agent's active group title (from `ActiveGroupChanged`) — the
    /// freshest source for the own tag's title line (the NameValue `Title`
    /// only refreshes when the simulator re-streams the avatar object).
    #[must_use]
    pub fn own_title(&self) -> Option<&str> {
        self.own_title.as_deref()
    }

    /// Drop a group the agent is no longer in (left, ejected, or dissolved),
    /// clearing the active marker if it was the active group.
    pub fn remove(&mut self, group: GroupKey) {
        if self.groups.remove(&group).is_some() {
            self.accept_notices.remove(&group);
            if self.active == Some(group) {
                self.active = None;
            }
            self.touch();
        }
    }

    /// The ordered, render-ready row list: case-folded by group name, with a stable
    /// id tie-break so equal names keep a fixed order.
    ///
    /// A [`GroupChoice::NoGroup`] row leads the list whenever the agent is in any
    /// group at all — the only way to wear **no** group (and so no title), which
    /// activating a real group can never undo. The reference lists it the same
    /// way, and suppresses it for a member of nothing for the same reason: with
    /// no group to leave there is nothing for it to do. Its `name` is empty
    /// because it has none of its own; the label is a localised string the UI
    /// supplies, which is why it does not live here.
    #[must_use]
    pub fn ordered(&self) -> Vec<GroupRow> {
        let mut rows: Vec<GroupRow> = self
            .groups
            .iter()
            .map(|(id, group_name)| {
                let name = if group_name.is_empty() {
                    short_id(id.uuid())
                } else {
                    group_name.clone()
                };
                GroupRow {
                    group: GroupChoice::Group(*id),
                    name,
                    active: self.active == Some(*id),
                }
            })
            .collect();
        rows.sort_by(|left, right| {
            left.name
                .to_lowercase()
                .cmp(&right.name.to_lowercase())
                .then_with(|| left.group.key().cmp(&right.group.key()))
        });
        if !rows.is_empty() {
            rows.insert(
                0,
                GroupRow {
                    group: GroupChoice::NoGroup,
                    name: String::new(),
                    active: self.active.is_none(),
                },
            );
        }
        rows
    }

    /// The number of groups the agent is in — the count line under the list.
    #[must_use]
    pub fn len(&self) -> usize {
        self.groups.len()
    }

    /// Whether the agent is in no groups at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }

    /// The display name for a group, if known (for the leave-confirm prompt).
    pub fn name_of(&self, group: GroupKey) -> Option<&str> {
        self.groups.get(&group).map(String::as_str)
    }
}

/// What a row of the group list stands for: one of the agent's groups, or the
/// **no group** choice that wears none of them.
///
/// An enum rather than the reference's null-UUID sentinel, so "wear nothing" and
/// "wear the group whose id happens to be nil" cannot be confused, and so every
/// action that only makes sense for a real group (Info, IM, Leave) has to say so
/// in its own signature instead of remembering to test for a magic id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupChoice {
    /// Wear no group: no active group and no title. Sent as
    /// [`Command::ActivateGroup(None)`](sl_client_bevy::Command::ActivateGroup).
    NoGroup,
    /// One of the agent's groups.
    Group(GroupKey),
}

impl GroupChoice {
    /// The group id this choice names, or `None` for [`Self::NoGroup`] — the
    /// shape `Command::ActivateGroup` already takes, so activating a row is the
    /// same call either way.
    #[must_use]
    pub const fn key(self) -> Option<GroupKey> {
        match self {
            Self::NoGroup => None,
            Self::Group(group) => Some(group),
        }
    }
}

/// One render-ready group row: what its actions act on, the display name, and
/// whether it is the active (worn) choice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupRow {
    /// The group this row acts on, or the "no group" choice.
    pub group: GroupChoice,
    /// The display name (or a short-id placeholder for an unnamed group). Empty
    /// for [`GroupChoice::NoGroup`], whose label the UI localises.
    pub name: String,
    /// Whether this is the agent's active (worn) choice — for
    /// [`GroupChoice::NoGroup`], whether no group is worn.
    pub active: bool,
}
