//! The in-memory experience fixture set the server-side experience
//! capabilities serve from.
//!
//! [`SimSession`](crate::SimSession) holds one of these as a
//! driver-populated serving store (the `display_names` stance): the
//! authoritative grid experience database is out of scope, but the three
//! mutating capabilities (`ExperiencePreferences`, `UpdateExperience`, the
//! `RegionExperiences` POST) **do** apply to this fixture, so a follow-up
//! read observes the preference / metadata edit / list replacement a test
//! (or the fake grid) just performed.
//!
//! Determinism: every collection is a `BTreeMap`/`BTreeSet`, listings sort
//! with an id tie-break, and ids are caller-supplied — no clock, no RNG.

use std::collections::{BTreeMap, BTreeSet};

use sl_types::key::ExperienceKey;
use sl_wire::{
    ExperienceInfo, ExperiencePermission, ExperienceProperties, ExperienceUpdate, PROPERTY_INVALID,
    SEARCH_PAGE_SIZE,
};
use uuid::Uuid;

/// The in-memory experience fixture set: metadata records plus the
/// agent-scoped, group-scoped and region-scoped id lists the twelve
/// experience capabilities serve.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SimExperiences {
    /// Every experience's metadata record, keyed by its public id
    /// (`GetExperienceInfo`, `FindExperienceByName`, `UpdateExperience`).
    records: BTreeMap<ExperienceKey, ExperienceInfo>,
    /// The experiences the agent has admitted (`GetExperiences`'
    /// `experiences` array; moved by `ExperiencePreferences`).
    allowed: BTreeSet<ExperienceKey>,
    /// The experiences the agent has blocked (`GetExperiences`' `blocked`
    /// array; moved by `ExperiencePreferences`).
    blocked: BTreeSet<ExperienceKey>,
    /// The experiences the agent owns (`AgentExperiences`).
    owned: BTreeSet<ExperienceKey>,
    /// The experiences the agent administers (`GetAdminExperiences`,
    /// `IsExperienceAdmin`).
    admin: BTreeSet<ExperienceKey>,
    /// The experiences the agent created — the reference viewer's
    /// "contributor" list (`GetCreatorExperiences`,
    /// `IsExperienceContributor`).
    creator: BTreeSet<ExperienceKey>,
    /// Group-owned experience lists, keyed by group id
    /// (`GroupExperiences`); an unknown group answers an empty list.
    groups: BTreeMap<Uuid, Vec<ExperienceKey>>,
    /// The region's allowed experiences (`RegionExperiences`).
    region_allowed: Vec<ExperienceKey>,
    /// The region's blocked experiences (`RegionExperiences`).
    region_blocked: Vec<ExperienceKey>,
    /// The region's trusted experiences (`RegionExperiences`).
    region_trusted: Vec<ExperienceKey>,
}

impl SimExperiences {
    /// Inserts (or replaces) an experience record, filed under its
    /// `public_id` — the driver/test population API.
    pub fn insert(&mut self, info: ExperienceInfo) {
        self.records.insert(info.public_id, info);
    }

    /// Replaces the agent's allowed / blocked preference lists — the
    /// driver/test population API (`ExperiencePreferences` moves single ids
    /// afterwards).
    pub fn set_agent_permissions(
        &mut self,
        allowed: Vec<ExperienceKey>,
        blocked: Vec<ExperienceKey>,
    ) {
        self.allowed = allowed.into_iter().collect();
        self.blocked = blocked.into_iter().collect();
    }

    /// Replaces the agent's owned-experience list (`AgentExperiences`).
    pub fn set_owned(&mut self, ids: Vec<ExperienceKey>) {
        self.owned = ids.into_iter().collect();
    }

    /// Replaces the agent's admin-experience list (`GetAdminExperiences`,
    /// `IsExperienceAdmin`).
    pub fn set_admin(&mut self, ids: Vec<ExperienceKey>) {
        self.admin = ids.into_iter().collect();
    }

    /// Replaces the agent's creator/contributor-experience list
    /// (`GetCreatorExperiences`, `IsExperienceContributor`).
    pub fn set_creator(&mut self, ids: Vec<ExperienceKey>) {
        self.creator = ids.into_iter().collect();
    }

    /// Replaces one group's experience list (`GroupExperiences`).
    pub fn set_group(&mut self, group_id: Uuid, ids: Vec<ExperienceKey>) {
        self.groups.insert(group_id, ids);
    }

    /// Replaces the region's allowed / blocked / trusted lists — the
    /// driver/test population API (the `RegionExperiences` POST replaces
    /// them wholesale afterwards).
    pub fn set_region_lists(
        &mut self,
        allowed: Vec<ExperienceKey>,
        blocked: Vec<ExperienceKey>,
        trusted: Vec<ExperienceKey>,
    ) {
        self.region_allowed = allowed;
        self.region_blocked = blocked;
        self.region_trusted = trusted;
    }

    /// Serves one `GetExperienceInfo` lookup: the stored record per
    /// requested id, in request order; an unknown id yields a
    /// [`missing`](ExperienceInfo::missing) placeholder (flagged
    /// [`PROPERTY_INVALID`]), which
    /// [`build_experience_infos_response`](sl_wire::build_experience_infos_response)
    /// routes to the reply's `error_ids` array.
    #[must_use]
    pub fn infos(&self, ids: &[ExperienceKey]) -> Vec<ExperienceInfo> {
        ids.iter()
            .map(|id| {
                self.records
                    .get(id)
                    .cloned()
                    .unwrap_or_else(|| ExperienceInfo {
                        public_id: *id,
                        properties: ExperienceProperties(PROPERTY_INVALID),
                        missing: true,
                        ..ExperienceInfo::default()
                    })
            })
            .collect()
    }

    /// Serves one `FindExperienceByName` page: records whose name contains
    /// `text` case-insensitively, excluding invalid and private experiences
    /// (the grid's search surface only lists public ones), sorted by name
    /// with an id tie-break, paged 1-based by [`SEARCH_PAGE_SIZE`] (the
    /// reference viewer's picker starts at page 1). A page below 1 answers
    /// empty.
    #[must_use]
    pub fn find(&self, text: &str, page: i32) -> Vec<ExperienceInfo> {
        if page < 1 {
            return Vec::new();
        }
        let needle = text.to_lowercase();
        let mut matches: Vec<&ExperienceInfo> = self
            .records
            .values()
            .filter(|info| !info.properties.is_invalid() && !info.properties.is_private())
            .filter(|info| info.name.to_lowercase().contains(&needle))
            .collect();
        matches.sort_by(|a, b| a.name.cmp(&b.name).then(a.public_id.cmp(&b.public_id)));
        let page_size = usize::try_from(SEARCH_PAGE_SIZE).unwrap_or_default();
        let skipped = usize::try_from(page.saturating_sub(1))
            .unwrap_or_default()
            .saturating_mul(page_size);
        matches
            .into_iter()
            .skip(skipped)
            .take(page_size)
            .cloned()
            .collect()
    }

    /// The agent's `(allowed, blocked)` preference lists, in id order — the
    /// `GetExperiences` / `ExperiencePreferences` reply payload.
    #[must_use]
    pub fn agent_permissions(&self) -> (Vec<ExperienceKey>, Vec<ExperienceKey>) {
        (
            self.allowed.iter().copied().collect(),
            self.blocked.iter().copied().collect(),
        )
    }

    /// The agent's owned experiences, in id order (`AgentExperiences`).
    #[must_use]
    pub fn owned(&self) -> Vec<ExperienceKey> {
        self.owned.iter().copied().collect()
    }

    /// The agent's admin experiences, in id order (`GetAdminExperiences`).
    #[must_use]
    pub fn admin(&self) -> Vec<ExperienceKey> {
        self.admin.iter().copied().collect()
    }

    /// The agent's creator/contributor experiences, in id order
    /// (`GetCreatorExperiences`).
    #[must_use]
    pub fn creator(&self) -> Vec<ExperienceKey> {
        self.creator.iter().copied().collect()
    }

    /// One group's experience list (`GroupExperiences`); an unknown group
    /// answers empty — the "no such group / no experiences" signal.
    #[must_use]
    pub fn group(&self, group_id: Uuid) -> Vec<ExperienceKey> {
        self.groups.get(&group_id).cloned().unwrap_or_default()
    }

    /// Whether the agent administers the experience (`IsExperienceAdmin`) —
    /// membership in the [`admin`](Self::set_admin) list; an unknown id is
    /// simply `false`, never an error.
    #[must_use]
    pub fn is_admin(&self, id: ExperienceKey) -> bool {
        self.admin.contains(&id)
    }

    /// Whether the agent contributes to the experience
    /// (`IsExperienceContributor`) — membership in the
    /// [`creator`](Self::set_creator) list (the reference viewer files the
    /// creator list under its "Contributor" tab); an unknown id is `false`.
    #[must_use]
    pub fn is_contributor(&self, id: ExperienceKey) -> bool {
        self.creator.contains(&id)
    }

    /// The region's `(allowed, blocked, trusted)` lists — the
    /// `RegionExperiences` reply payload.
    #[must_use]
    pub fn region_lists(&self) -> (Vec<ExperienceKey>, Vec<ExperienceKey>, Vec<ExperienceKey>) {
        (
            self.region_allowed.clone(),
            self.region_blocked.clone(),
            self.region_trusted.clone(),
        )
    }

    /// Applies one `ExperiencePreferences` mutation: `Allow` / `Block` move
    /// the id into the corresponding list (and out of the other), `Forget`
    /// removes it from both. Any id is accepted — a preference is the
    /// agent's own keyed entry, not a record lookup (viewers can block ids
    /// they have never resolved).
    pub(crate) fn set_preference(&mut self, id: ExperienceKey, permission: ExperiencePermission) {
        match permission {
            ExperiencePermission::Allow => {
                self.blocked.remove(&id);
                self.allowed.insert(id);
            }
            ExperiencePermission::Block => {
                self.allowed.remove(&id);
                self.blocked.insert(id);
            }
            // Forget — and, `ExperiencePermission` being
            // `#[non_exhaustive]`, any future variant, read
            // least-privilege — clears the id from both lists.
            _ => {
                self.allowed.remove(&id);
                self.blocked.remove(&id);
            }
        }
    }

    /// Applies one `UpdateExperience` edit to the stored record: the
    /// editable fields (name, description, maturity, properties, SLURL,
    /// extended metadata) are replaced; the server-controlled fields
    /// (owner, quota, expiration — the reference viewer strips them from
    /// the POST) are preserved. Returns the updated record for the reply,
    /// or `None` when no record carries the update's `public_id` (→ `404`).
    pub(crate) fn apply_update(&mut self, update: &ExperienceUpdate) -> Option<ExperienceInfo> {
        let record = self.records.get_mut(&update.public_id)?;
        record.name.clone_from(&update.name);
        record.description.clone_from(&update.description);
        record.maturity = update.maturity;
        record.properties = ExperienceProperties(update.properties);
        record.slurl.clone_from(&update.slurl);
        record
            .extended_metadata
            .clone_from(&update.extended_metadata);
        Some(record.clone())
    }

    /// Replaces the region's allowed / blocked / trusted lists wholesale —
    /// the `RegionExperiences` POST semantics. Returns the stored triple
    /// for the reply's echo.
    pub(crate) fn apply_region_lists(
        &mut self,
        allowed: Vec<ExperienceKey>,
        blocked: Vec<ExperienceKey>,
        trusted: Vec<ExperienceKey>,
    ) -> (Vec<ExperienceKey>, Vec<ExperienceKey>, Vec<ExperienceKey>) {
        self.region_allowed = allowed;
        self.region_blocked = blocked;
        self.region_trusted = trusted;
        self.region_lists()
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    /// A deterministic experience key for tests.
    fn key(n: u128) -> ExperienceKey {
        ExperienceKey::from(Uuid::from_u128(n))
    }

    /// A public record with the given name.
    fn record(n: u128, name: &str) -> ExperienceInfo {
        ExperienceInfo {
            public_id: key(n),
            name: name.to_owned(),
            ..ExperienceInfo::default()
        }
    }

    /// Unknown ids come back as `missing` placeholders flagged
    /// `PROPERTY_INVALID`, in request order alongside the known records.
    #[test]
    fn infos_serves_records_and_missing_placeholders() {
        let mut store = SimExperiences::default();
        store.insert(record(1, "Magic Quest"));
        let served = store.infos(&[key(2), key(1)]);
        assert_eq!(served.len(), 2);
        assert_eq!(
            served.first().map(|info| (info.public_id, info.missing)),
            Some((key(2), true))
        );
        assert_eq!(
            served.first().map(|info| info.properties.is_invalid()),
            Some(true)
        );
        assert_eq!(
            served.get(1).map(|info| (info.public_id, info.missing)),
            Some((key(1), false))
        );
    }

    /// Search matches case-insensitively, hides private and invalid
    /// records, and pages 1-based by [`SEARCH_PAGE_SIZE`].
    #[test]
    fn find_filters_and_pages() {
        let mut store = SimExperiences::default();
        store.insert(record(1, "Magic Quest"));
        store.insert(ExperienceInfo {
            properties: ExperienceProperties(sl_wire::PROPERTY_PRIVATE),
            ..record(2, "Magic Dungeon")
        });
        store.insert(ExperienceInfo {
            properties: ExperienceProperties(PROPERTY_INVALID),
            ..record(3, "Magic Ruin")
        });
        store.insert(record(4, "Tour Guide"));
        let hits = store.find("mAgIc", 1);
        assert_eq!(
            hits.iter().map(|info| info.public_id).collect::<Vec<_>>(),
            vec![key(1)]
        );
        assert_eq!(store.find("magic", 2), Vec::new());
        assert_eq!(store.find("magic", 0), Vec::new());
        assert_eq!(store.find("magic", -3), Vec::new());
    }

    /// A full first page spills the remainder onto page 2, sorted by name
    /// with an id tie-break.
    #[test]
    fn find_pages_past_the_page_size() {
        let mut store = SimExperiences::default();
        for n in 1..=31 {
            store.insert(record(n, &format!("Quest {n:02}")));
        }
        let page_size = usize::try_from(SEARCH_PAGE_SIZE).unwrap_or_default();
        let first = store.find("quest", 1);
        assert_eq!(first.len(), page_size);
        assert_eq!(first.first().map(|info| info.public_id), Some(key(1)));
        let second = store.find("quest", 2);
        assert_eq!(
            second.iter().map(|info| info.public_id).collect::<Vec<_>>(),
            vec![key(31)]
        );
    }

    /// Allow / Block move an id between the two lists; Forget removes it
    /// from both; ids without a stored record are accepted.
    #[test]
    fn set_preference_moves_between_lists() {
        let mut store = SimExperiences::default();
        store.set_agent_permissions(vec![key(1)], vec![key(2)]);
        store.set_preference(key(2), ExperiencePermission::Allow);
        assert_eq!(store.agent_permissions(), (vec![key(1), key(2)], vec![]));
        store.set_preference(key(1), ExperiencePermission::Block);
        assert_eq!(store.agent_permissions(), (vec![key(2)], vec![key(1)]));
        store.set_preference(key(1), ExperiencePermission::Forget);
        assert_eq!(store.agent_permissions(), (vec![key(2)], vec![]));
        // No record with this id exists anywhere — still accepted.
        store.set_preference(key(9), ExperiencePermission::Block);
        assert_eq!(store.agent_permissions(), (vec![key(2)], vec![key(9)]));
    }

    /// The update applies the editable fields, preserves the
    /// server-controlled ones, and answers `None` for an unknown id.
    #[test]
    fn apply_update_edits_known_records_only() {
        let mut store = SimExperiences::default();
        store.insert(ExperienceInfo {
            quota: 128,
            expiration: 86_400.0,
            ..record(1, "Magic Quest")
        });
        let update = ExperienceUpdate {
            public_id: key(1),
            name: "Magic Quest II".to_owned(),
            description: "Now with more magic".to_owned(),
            maturity: 21,
            properties: sl_wire::PROPERTY_GRID,
            slurl: None,
            extended_metadata: "<meta />".to_owned(),
        };
        let updated = store.apply_update(&update);
        assert_eq!(
            updated.as_ref().map(|info| info.name.as_str()),
            Some("Magic Quest II")
        );
        assert_eq!(updated.as_ref().map(|info| info.quota), Some(128));
        assert_eq!(
            store
                .infos(&[key(1)])
                .first()
                .map(|info| (info.maturity, info.properties)),
            Some((21, ExperienceProperties(sl_wire::PROPERTY_GRID)))
        );
        assert_eq!(
            store.apply_update(&ExperienceUpdate {
                public_id: key(9),
                ..update
            }),
            None
        );
    }

    /// The region-list replacement is wholesale and echoes the stored
    /// triple.
    #[test]
    fn apply_region_lists_replaces_wholesale() {
        let mut store = SimExperiences::default();
        store.set_region_lists(vec![key(1)], vec![key(2)], vec![key(3)]);
        let echoed = store.apply_region_lists(vec![key(4)], vec![], vec![key(5)]);
        assert_eq!(echoed, (vec![key(4)], vec![], vec![key(5)]));
        assert_eq!(store.region_lists(), echoed);
    }
}
