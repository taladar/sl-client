//! The buddy list: who this agent is friends with, and on which terms.
//!
//! One entry per friend — the rights granted in both directions and the
//! last-known presence — plus the flattened row the friends list draws. Fed
//! from the event stream; every surface that badges a friend reads it.

use std::collections::BTreeMap;

use crate::short_id;
use bevy::prelude::*;
use sl_client_bevy::{AgentKey, Friend, FriendKey, FriendPresence, FriendRights, Uuid};

/// One friend's cached state: the friendship rights in both directions and the
/// last-known presence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FriendEntry {
    /// The rights this agent grants the friend.
    rights_granted: FriendRights,
    /// The rights the friend grants this agent.
    rights_received: FriendRights,
    /// Whether the friend is currently known-online (`false` is "offline or not
    /// visible", never provably offline).
    online: bool,
}

impl FriendEntry {
    /// A fresh entry from a login / snapshot [`Friend`] record, offline until a
    /// presence notification says otherwise.
    const fn new(friend: Friend, online: bool) -> Self {
        Self {
            rights_granted: friend.rights_granted,
            rights_received: friend.rights_received,
            online,
        }
    }
}

/// The pure friends model: the buddy cache keyed by friend id, the resolved name
/// cache, and a revision stamp bumped on every change so the view rebuilds only
/// when something actually moved. Fed solely from the event stream.
#[derive(Resource, Debug, Default)]
pub struct FriendsModel {
    /// The buddy list, by friend id.
    friends: BTreeMap<FriendKey, FriendEntry>,
    /// Last-seen legacy display name per agent, for the row labels.
    names: BTreeMap<AgentKey, String>,
    /// The name the user gave a friend instead, if any (already quoted, as the
    /// name cache shows it) — mirrored from the contact-set store by
    /// `contact_sets::apply_name_aliases`. Kept beside the resolved
    /// names rather than over them: a wire action still needs the real one.
    aliases: BTreeMap<AgentKey, String>,
    /// Bumped on each mutation; the view compares its last-built value to skip an
    /// unchanged rebuild.
    revision: u64,
}

impl FriendsModel {
    /// Bump the revision after a mutation.
    pub const fn touch(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    /// Merge a buddy-list record set (login `FriendList`), keeping any presence
    /// already learned for a friend that is being refreshed.
    pub fn note_friends(&mut self, friends: &[Friend]) {
        for friend in friends {
            let online = self
                .friends
                .get(&friend.id)
                .is_some_and(|entry| entry.online);
            self.friends
                .insert(friend.id, FriendEntry::new(*friend, online));
        }
        self.touch();
    }

    /// Replace the model from a presence snapshot (the [`sl_client_bevy::Command::QueryFriends`]
    /// reply): authoritative for both rights and the online flag.
    pub fn apply_snapshot(&mut self, presence: &[FriendPresence]) {
        self.friends.clear();
        for entry in presence {
            self.friends.insert(
                entry.friend.id,
                FriendEntry::new(entry.friend, entry.online),
            );
        }
        self.touch();
    }

    /// Set the online flag on a set of friends (an online / offline notification).
    pub fn set_online(&mut self, friends: &[FriendKey], online: bool) {
        let mut changed = false;
        for id in friends {
            if let Some(entry) = self.friends.get_mut(id)
                && entry.online != online
            {
                entry.online = online;
                changed = true;
            }
        }
        if changed {
            self.touch();
        }
    }

    /// Update one friend's rights from a [`SlSessionEvent::FriendRightsChanged`](sl_client_bevy::SlSessionEvent::FriendRightsChanged):
    /// `granted_to_us` distinguishes the rights the friend now grants us from a
    /// server echo of the rights we grant them.
    pub fn update_rights(&mut self, friend: FriendKey, rights: FriendRights, granted_to_us: bool) {
        if let Some(entry) = self.friends.get_mut(&friend) {
            if granted_to_us {
                entry.rights_received = rights;
            } else {
                entry.rights_granted = rights;
            }
            self.touch();
        }
    }

    /// Drop a friend (friendship terminated by either side).
    pub fn remove(&mut self, friend: FriendKey) {
        if self.friends.remove(&friend).is_some() {
            self.touch();
        }
    }

    /// Record a resolved legacy name for an agent (ignoring empties).
    pub fn note_name(&mut self, id: AgentKey, name: &str) {
        if !name.is_empty() && self.names.get(&id).map(String::as_str) != Some(name) {
            self.names.insert(id, name.to_owned());
            self.touch();
        }
    }

    /// The resolved name for an agent, if known — the **grid's** answer, which
    /// is what a wire action (a mute entry) has to carry.
    pub fn name_of(&self, id: AgentKey) -> Option<&str> {
        self.names.get(&id).map(String::as_str)
    }

    /// The name to **show** for an agent: the alias the user gave them, else the
    /// resolved name.
    ///
    /// Every surface that draws a friend's name wants this one;
    /// [`Self::name_of`] is the grid's answer, which is what a wire action
    /// (a mute entry naming the muted avatar) has to carry.
    #[must_use]
    pub fn shown_name_of(&self, id: AgentKey) -> Option<&str> {
        self.aliases
            .get(&id)
            .or_else(|| self.names.get(&id))
            .map(String::as_str)
    }

    /// Replace the mirrored aliases, rebuilding the list when they moved (an
    /// alias given now renames that friend in the list at once). The one way in;
    /// `contact_sets::apply_name_aliases` is the caller.
    pub fn set_name_aliases(&mut self, aliases: BTreeMap<AgentKey, String>) {
        if self.aliases == aliases {
            return;
        }
        self.aliases = aliases;
        self.touch();
    }

    /// The model revision — a consumer that mirrors the roster (the friends-only
    /// render filter, `derender`) compares its last-mirrored value to
    /// skip an unchanged rebuild.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Every friend's agent id. The friends-only render filter mirrors this by
    /// revision so its per-avatar gate — which runs for every streamed object at
    /// a crowded event — stays a single hash lookup.
    #[must_use]
    pub fn friend_ids(&self) -> std::collections::HashSet<Uuid> {
        self.friends
            .keys()
            .map(|id| AgentKey::from(*id).uuid())
            .collect()
    }

    /// Whether `agent` is already in the buddy cache — a friend.
    ///
    /// The avatar context menu reads this to disable "Add as Friend" for someone
    /// who already is one, matching the reference viewer's `on_enable`.
    #[must_use]
    pub fn is_friend(&self, agent: AgentKey) -> bool {
        self.friends.contains_key(&FriendKey::from(agent.uuid()))
    }

    /// Whether `agent` is a friend the grid last reported **online**. Someone
    /// who is not a friend at all is not online as far as this model knows — the
    /// buddy cache is the only presence the protocol gives us.
    #[must_use]
    pub fn is_online(&self, agent: AgentKey) -> bool {
        self.friends
            .get(&FriendKey::from(agent.uuid()))
            .is_some_and(|entry| entry.online)
    }

    /// The whole roster as `(agent, display label)` pairs, name order — the
    /// avatar picker's Friends tab reads this. A friend whose name has not
    /// resolved yet labels as a provisional id fragment.
    #[must_use]
    pub fn roster(&self) -> Vec<(AgentKey, String)> {
        let mut entries: Vec<(AgentKey, String)> = self
            .friends
            .keys()
            .map(|id| {
                let agent = AgentKey::from(*id);
                let label = self
                    .shown_name_of(agent)
                    .map_or_else(|| format!("({id})"), ToOwned::to_owned);
                (agent, label)
            })
            .collect();
        entries.sort_by_key(|entry| entry.1.to_lowercase());
        entries
    }

    /// The friends whose name is not yet resolved — the set to request names for.
    #[must_use]
    pub fn unnamed(&self) -> Vec<AgentKey> {
        self.friends
            .keys()
            .map(|id| AgentKey::from(*id))
            .filter(|agent| !self.names.contains_key(agent))
            .collect()
    }

    /// The render-ready row list, in map order. The table sorts it through
    /// its own `SortState`; the model has no opinion on
    /// display order.
    #[must_use]
    pub fn rows(&self) -> Vec<FriendRow> {
        self.friends
            .iter()
            .map(|(id, entry)| {
                let agent = AgentKey::from(*id);
                let name = self
                    .shown_name_of(agent)
                    .map_or_else(|| short_id(agent.uuid()), ToOwned::to_owned);
                FriendRow {
                    friend: *id,
                    agent,
                    name,
                    online: entry.online,
                    rights_granted: entry.rights_granted,
                    rights_received: entry.rights_received,
                }
            })
            .collect()
    }

    /// The rights this agent currently grants `friend`, if known.
    #[must_use]
    pub fn granted_rights(&self, friend: FriendKey) -> Option<FriendRights> {
        self.friends.get(&friend).map(|entry| entry.rights_granted)
    }

    /// Optimistically set the rights this agent grants `friend` (so a toggled
    /// checkbox flips immediately; the server echo re-confirms the same value).
    pub fn set_granted(&mut self, friend: FriendKey, rights: FriendRights) {
        if let Some(entry) = self.friends.get_mut(&friend)
            && entry.rights_granted != rights
        {
            entry.rights_granted = rights;
            self.touch();
        }
    }
}

/// One render-ready friend row: the ids the actions need, the display name, the
/// presence flag, and the friendship rights in both directions (the table's
/// permission columns).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FriendRow {
    /// The friend id (for remove / grant-rights, which take a [`FriendKey`]).
    pub friend: FriendKey,
    /// The agent id (for IM / teleport / mute, which take an [`AgentKey`]).
    pub agent: AgentKey,
    /// The display name (or a short-id placeholder until the name resolves).
    pub name: String,
    /// Whether the friend is currently known-online.
    pub online: bool,
    /// The rights this agent grants the friend (the "They can …" columns).
    pub rights_granted: FriendRights,
    /// The rights the friend grants this agent (the "You can …" columns).
    pub rights_received: FriendRights,
}
